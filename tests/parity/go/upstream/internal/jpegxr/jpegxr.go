// Package jpegxr implements github.com/dobbo-ca/jpegxr-go, which re-exports its API.
package jpegxr

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"image"
	"image/color"
	"io"
	"sync"

	"github.com/dobbo-ca/jpegxr-go/internal/codec"
	"github.com/dobbo-ca/jpegxr-go/internal/container"
)

// ErrInternal reports a decoder bug caught as a panic.
var ErrInternal = errors.New("jpegxr: internal error")

var (
	errFormat   = errors.New("jpegxr: unsupported pixel format")
	errTooLarge = errors.New("jpegxr: image too large")
)

// maxPixels keeps the output buffer size within an int.
const maxPixels = 1 << 40

func init() {
	image.RegisterFormat("jpegxr", "II\xbc?", Decode, DecodeConfig)
}

// pixfmtPrefix is the stored GUID 24C3DD6F-034E-FE4B-B185-3D77768DC9xx.
var pixfmtPrefix = []byte{0x24, 0xc3, 0xdd, 0x6f, 0x03, 0x4e, 0xfe, 0x4b, 0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9}

// Pixel formats by last GUID byte (JXRGlue.c).
var formats = map[byte]codec.Format{
	0x0c: {ColorFormat: codec.CFRGB, BitDepth: codec.BD8, BitsPerUnit: 24},                  // 24bppBGR
	0x15: {ColorFormat: codec.CFRGB, BitDepth: codec.BD16, BitsPerUnit: 48, RGB: true},      // 48bppRGB
	0x08: {ColorFormat: codec.YOnly, BitDepth: codec.BD8, BitsPerUnit: 8},                   // 8bppGray
	0x0b: {ColorFormat: codec.YOnly, BitDepth: codec.BD16, BitsPerUnit: 16},                 // 16bppGray
	0x26: {ColorFormat: codec.NComponent, BitDepth: codec.BD16, BitsPerUnit: 48, RGB: true}, // 48bpp3Channels
}

func parse(data []byte) (codec.Format, []byte, error) {
	in, err := container.Read(data)
	if err != nil {
		return codec.Format{}, nil, err
	}
	f, ok := formats[in.PixelFormat[15]]
	if !ok || !bytes.Equal(in.PixelFormat[:15], pixfmtPrefix) {
		return codec.Format{}, nil, errFormat
	}
	return f, data[in.ImageOffset:], nil
}

// DecodeConfig returns the size and colour model of a JPEG XR image.
func DecodeConfig(r io.Reader) (image.Config, error) {
	data, err := io.ReadAll(r)
	if err != nil {
		return image.Config{}, err
	}
	f, stream, err := parse(data)
	if err != nil {
		return image.Config{}, err
	}
	h, err := codec.ReadHeader(stream)
	if err != nil {
		return image.Config{}, err
	}
	var m color.Model
	switch {
	case f.ColorFormat == codec.YOnly && f.BitDepth == codec.BD8:
		m = color.GrayModel
	case f.ColorFormat == codec.YOnly:
		m = color.Gray16Model
	case f.BitDepth == codec.BD8:
		m = color.RGBAModel
	default:
		m = color.RGBA64Model
	}
	return image.Config{ColorModel: m, Width: h.Width, Height: h.Height}, nil
}

var decoderPool = sync.Pool{New: func() any { return &Decoder{} }}

// Decode reads a JPEG XR image in an HD Photo container from r.
// The image is an *image.RGBA for 24bppBGR, *image.RGBA64 for
// 48bppRGB and 48bpp3Channels (channel k in R, G, B), *image.Gray for
// 8bppGray and *image.Gray16 for 16bppGray.
func Decode(r io.Reader) (image.Image, error) {
	return DecodeContext(context.Background(), r)
}

// DecodeContext is Decode, checking ctx between macroblock rows.
func DecodeContext(ctx context.Context, r io.Reader) (image.Image, error) {
	var hint int
	if l, ok := r.(interface{ Len() int }); ok { // bytes.Reader: one allocation
		hint = l.Len()
	}
	buf := bytes.NewBuffer(make([]byte, 0, hint+bytes.MinRead))
	if _, err := buf.ReadFrom(r); err != nil {
		return nil, err
	}
	data := buf.Bytes()
	d := decoderPool.Get().(*Decoder)
	defer decoderPool.Put(d)
	if err := d.Reset(data); err != nil {
		return nil, err
	}
	w, ht, format := d.Config()
	stride := strideFor(w, format)
	// raw plus the converted image; Gray16 decodes in place
	out := int64(stride*ht) + int64(w*ht)*[...]int64{Bgr24: 4, Bgr48: 8, Gray8: 1, Gray16: 0}[format]
	est := d.estimate() + out
	if err := reserve(ctx, est); err != nil {
		return nil, err
	}
	defer release(est)
	if format == Gray16 { // decode in place, skip a copy
		m := image.NewGray16(image.Rect(0, 0, w, ht))
		if err := d.decode(ctx, m.Pix, m.Stride); err != nil {
			return nil, err
		}
		swap16(m.Pix)
		return m, nil
	}
	raw := make([]byte, stride*ht)
	if err := d.decode(ctx, raw, stride); err != nil {
		return nil, err
	}
	return toImage(w, ht, format, raw), nil
}

// strideFor is the row length in bytes libCZI expects for format.
func strideFor(w int, format Format) int {
	switch format {
	case Gray8:
		return w
	case Gray16:
		return w * 2
	case Bgr24:
		return w * 3
	default: // Bgr48
		return w * 6
	}
}

// swap16 turns little-endian samples big-endian in place.
func swap16(p []byte) {
	const lo = 0x00ff00ff00ff00ff
	i := 0
	for ; i+8 <= len(p); i += 8 {
		v := binary.LittleEndian.Uint64(p[i:])
		binary.LittleEndian.PutUint64(p[i:], (v>>8)&lo|(v&lo)<<8)
	}
	for ; i+1 < len(p); i += 2 {
		p[i], p[i+1] = p[i+1], p[i]
	}
}

// toImage lays out raw, jxrlib's output, into a standard library image.
func toImage(w, ht int, format Format, raw []byte) image.Image {
	rect := image.Rect(0, 0, w, ht)
	switch format {
	case Gray8:
		m := image.NewGray(rect)
		copy(m.Pix, raw)
		return m
	case Bgr24:
		m := image.NewRGBA(rect)
		for i, j := 0, 0; i < len(raw); i, j = i+3, j+4 {
			m.Pix[j], m.Pix[j+1], m.Pix[j+2], m.Pix[j+3] = raw[i+2], raw[i+1], raw[i], 0xff
		}
		return m
	default: // Bgr48
		m := image.NewRGBA64(rect)
		for i, j := 0, 0; i < len(raw); i, j = i+6, j+8 {
			for c := 0; c < 3; c++ {
				binary.BigEndian.PutUint16(m.Pix[j+2*c:], binary.LittleEndian.Uint16(raw[i+2*c:]))
			}
			m.Pix[j+6], m.Pix[j+7] = 0xff, 0xff
		}
		return m
	}
}

// DecodeComponent decodes channel k of a 16-bit JPEG XR image as an
// *image.Gray16 equal to that channel of Decode's image: k 0 of 16bppGray,
// or k 0 to 2 (R, G, B) of 48bppRGB and 48bpp3Channels. 48bpp3Channels
// writes only channel k, skipping the other channels' output.
func DecodeComponent(ctx context.Context, r io.Reader, k int) (*image.Gray16, error) {
	var hint int
	if l, ok := r.(interface{ Len() int }); ok {
		hint = l.Len()
	}
	buf := bytes.NewBuffer(make([]byte, 0, hint+bytes.MinRead))
	if _, err := buf.ReadFrom(r); err != nil {
		return nil, err
	}
	d := decoderPool.Get().(*Decoder)
	defer decoderPool.Put(d)
	if err := d.Reset(buf.Bytes()); err != nil {
		return nil, err
	}
	w, ht, format := d.Config()
	if format == Gray16 && k == 0 || format == Bgr48 && d.cf.ColorFormat == codec.NComponent && k >= 0 && k < 3 {
		est := d.estimate() + int64(2*w*ht)
		if err := reserve(ctx, est); err != nil {
			return nil, err
		}
		m := image.NewGray16(image.Rect(0, 0, w, ht))
		err := d.decodePlane(ctx, k, m.Pix, m.Stride)
		release(est)
		if err == nil {
			swap16(m.Pix)
			return m, nil
		}
		if !errors.Is(err, codec.ErrUnsupported) || format == Gray16 {
			return nil, err
		}
		// e.g. fewer than 3 stored channels: take the full path
	}
	if format != Bgr48 || k < 0 || k > 2 {
		return nil, errFormat
	}
	est := d.estimate() + int64(8*w*ht) // raw and the plane
	if err := reserve(ctx, est); err != nil {
		return nil, err
	}
	defer release(est)
	raw := make([]byte, 6*w*ht)
	if err := d.decode(ctx, raw, 6*w); err != nil {
		return nil, err
	}
	m := image.NewGray16(image.Rect(0, 0, w, ht))
	for i := range w * ht {
		binary.BigEndian.PutUint16(m.Pix[2*i:], binary.LittleEndian.Uint16(raw[6*i+2*k:]))
	}
	return m, nil
}
