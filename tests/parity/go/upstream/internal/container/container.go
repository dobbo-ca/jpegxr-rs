// Ported from jxrlib JXRGlueJxr.c (BSD-2, see NOTICE).

// Package container parses the HD Photo (TIFF-like) container around
// a JPEG XR codestream.
package container

import (
	"encoding/binary"
	"errors"
)

// ErrFormat reports a malformed or unsupported container.
var ErrFormat = errors.New("jpegxr: bad container")

// Info holds the IFD entries the decoder needs.
type Info struct {
	PixelFormat    [16]byte // GUID bytes as stored
	ImageOffset    uint32
	ImageByteCount uint32
	AlphaOffset    uint32
	AlphaByteCount uint32
}

// Read parses the container header and first IFD, as ReadContainer does.
func Read(data []byte) (Info, error) {
	var in Info
	if len(data) < 8 || data[0] != 'I' || data[1] != 'I' {
		return in, ErrFormat
	}
	id := binary.LittleEndian.Uint16(data[2:])
	if id&0xff != 0xbc || (id>>8 != 0 && id>>8 != 1) {
		return in, ErrFormat
	}
	off := int64(binary.LittleEndian.Uint32(data[4:]))
	if off+2 > int64(len(data)) {
		return in, ErrFormat
	}
	n := int64(binary.LittleEndian.Uint16(data[off:]))
	if n == 0 || n == 0xffff {
		return in, ErrFormat
	}
	off += 2
	if off+12*n > int64(len(data)) {
		return in, ErrFormat
	}
	pf := false
	for i := int64(0); i < n; i++ {
		e := data[off+12*i:]
		tag := binary.LittleEndian.Uint16(e)
		count := binary.LittleEndian.Uint32(e[4:])
		value := binary.LittleEndian.Uint32(e[8:])
		switch tag {
		case 0xbc01: // pixel format
			if int64(value)+16 > int64(len(data)) {
				return in, ErrFormat
			}
			copy(in.PixelFormat[:], data[value:])
			pf = true
		case 0xbc02, 0xbcc0, 0xbcc1, 0xbcc2, 0xbcc3, 0xbc82, 0xbc83:
			if count != 1 {
				return in, ErrFormat
			}
			switch tag {
			case 0xbcc0:
				in.ImageOffset = value
			case 0xbcc1:
				in.ImageByteCount = value
			case 0xbcc2:
				in.AlphaOffset = value
			case 0xbcc3:
				in.AlphaByteCount = value
			}
		case 0xbc80, 0xbc81: // width, height
			if value == 0 {
				return in, ErrFormat
			}
		}
	}
	if !pf || int64(in.ImageOffset) >= int64(len(data)) {
		return in, ErrFormat
	}
	return in, nil
}
