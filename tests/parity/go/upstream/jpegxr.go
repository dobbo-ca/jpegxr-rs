// Package jpegxr decodes JPEG XR (ITU-T T.832) images. The code lives in
// internal/jpegxr; this file re-exports it.
package jpegxr

import (
	"context"
	"image"
	"io"

	ijpegxr "github.com/dobbo-ca/jpegxr-go/internal/jpegxr"
)

var (
	// ErrInternal reports a decoder bug caught as a panic.
	ErrInternal = ijpegxr.ErrInternal
	// ErrMemoryLimit reports a decode gave up waiting for other decodes to release
	// memory budget. Transient: retry later.
	ErrMemoryLimit = ijpegxr.ErrMemoryLimit
)

type (
	// Decoder decodes JPEG XR streams one at a time. Reset parses a new
	// stream's headers; DecodeInto reuses the previous stream's buffers, so
	// a warm decode of the same shape makes no allocation.
	Decoder = ijpegxr.Decoder
	// Format is libCZI's pixel type for a decoded image.
	Format = ijpegxr.Format
)

const (
	Bgr24  = ijpegxr.Bgr24
	Bgr48  = ijpegxr.Bgr48
	Gray8  = ijpegxr.Gray8
	Gray16 = ijpegxr.Gray16
)

// DecodeConfig returns the size and colour model of a JPEG XR image.
func DecodeConfig(r io.Reader) (image.Config, error) {
	return ijpegxr.DecodeConfig(r)
}

// Decode reads a JPEG XR image in an HD Photo container from r.
// The image is an *image.RGBA for 24bppBGR, *image.RGBA64 for
// 48bppRGB and 48bpp3Channels (channel k in R, G, B), *image.Gray for
// 8bppGray and *image.Gray16 for 16bppGray.
func Decode(r io.Reader) (image.Image, error) {
	return ijpegxr.Decode(r)
}

// DecodeContext is Decode, checking ctx between macroblock rows.
func DecodeContext(ctx context.Context, r io.Reader) (image.Image, error) {
	return ijpegxr.DecodeContext(ctx, r)
}

// SetMemoryLimit sets the bytes all running decodes may reserve
// together and returns the previous value. n <= 0 means no limit.
func SetMemoryLimit(n int64) int64 {
	return ijpegxr.SetMemoryLimit(n)
}

// DecodeComponent decodes channel k of a 16-bit JPEG XR image as an
// *image.Gray16 equal to that channel of Decode's image: k 0 of 16bppGray,
// or k 0 to 2 (R, G, B) of 48bppRGB and 48bpp3Channels. 48bpp3Channels
// writes only channel k, skipping the other channels' output.
func DecodeComponent(ctx context.Context, r io.Reader, k int) (*image.Gray16, error) {
	return ijpegxr.DecodeComponent(ctx, r, k)
}
