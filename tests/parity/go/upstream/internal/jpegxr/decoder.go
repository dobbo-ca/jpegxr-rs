package jpegxr

import (
	"context"
	"errors"
	"fmt"

	"github.com/dobbo-ca/jpegxr-go/internal/codec"
)

// Format is libCZI's pixel type for a decoded image.
type Format int

const (
	Bgr24 Format = iota
	Bgr48
	Gray8
	Gray16
)

// errNotReset reports DecodeInto called before a successful Reset.
var errNotReset = errors.New("jpegxr: DecodeInto before Reset")

// errDst reports a dst or stride too small for the image.
var errDst = errors.New("jpegxr: dst too small for the image")

// Decoder decodes JPEG XR streams one at a time. Reset parses a new
// stream's headers; DecodeInto reuses the previous stream's buffers, so
// a warm decode of the same shape makes no allocation.
type Decoder struct {
	cf     codec.Format
	format Format
	w, h   int
	stream []byte
	state  *codec.State
	hdr    codec.Header
}

func pixelFormat(f codec.Format) Format {
	switch {
	case f.ColorFormat == codec.YOnly && f.BitDepth == codec.BD8:
		return Gray8
	case f.ColorFormat == codec.YOnly:
		return Gray16
	case f.BitDepth == codec.BD8:
		return Bgr24
	default:
		return Bgr48
	}
}

// Reset parses data's container and image header for a following
// DecodeInto.
func (d *Decoder) Reset(data []byte) error {
	f, stream, err := parse(data)
	if err != nil {
		return err
	}
	h := &d.hdr
	if err := h.Parse(stream); err != nil {
		return err
	}
	if uint64(h.Width)*uint64(h.Height) > maxPixels {
		return errTooLarge
	}
	d.cf, d.format, d.w, d.h, d.stream = f, pixelFormat(f), h.Width, h.Height, stream
	if d.state == nil {
		d.state = codec.NewState()
	}
	return nil
}

// Config returns the size and libCZI pixel type set by Reset.
func (d *Decoder) Config() (w, h int, f Format) {
	return d.w, d.h, d.format
}

// estimate is the decoder's peak heap for the stream from Reset, from
// its header, plus the input.
func (d *Decoder) estimate() int64 {
	return d.hdr.Estimate(len(d.stream)) + int64(len(d.stream))
}

// DecodeInto decodes the stream from the last Reset into dst, laid out
// as libCZI expects for Config's format. It charges the memory budget
// for the decode's estimated peak and releases it on return. ctx is
// checked between macroblock rows and while waiting for budget.
func (d *Decoder) DecodeInto(ctx context.Context, dst []byte, stride int) error {
	if d.stream == nil {
		return errNotReset
	}
	est := d.estimate()
	if err := reserve(ctx, est); err != nil {
		return err
	}
	defer release(est)
	return d.decode(ctx, dst, stride)
}

// decode is DecodeInto without the budget charge.
func (d *Decoder) decode(ctx context.Context, dst []byte, stride int) (err error) {
	row := strideFor(d.w, d.format)
	if stride < row || d.h > 0 && len(dst) < stride*(d.h-1)+row {
		return errDst
	}
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("%w: %v", ErrInternal, p)
		}
	}()
	return d.state.Decode(ctx, d.stream, d.cf, dst, stride)
}

// decodePlane decodes channel k alone into dst, one 16-bit sample a
// pixel; Reset's image must be Gray16 (k 0) or 48bpp3Channels.
func (d *Decoder) decodePlane(ctx context.Context, k int, dst []byte, stride int) (err error) {
	if stride < 2*d.w || d.h > 0 && len(dst) < stride*(d.h-1)+2*d.w {
		return errDst
	}
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("%w: %v", ErrInternal, p)
		}
	}()
	cf := d.cf
	if d.format != Gray16 {
		cf.Plane = k + 1
	}
	return d.state.Decode(ctx, d.stream, cf, dst, stride)
}
