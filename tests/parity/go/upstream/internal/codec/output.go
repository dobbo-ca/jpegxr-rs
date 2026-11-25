// Ported from jxrlib strdec.c outputMBRow and helpers (BSD-2, see NOTICE).

package codec

import (
	"encoding/binary"
	"math"
)

func clip8(v int32) byte {
	if v < 0 {
		return 0
	}
	if v > 255 {
		return 255
	}
	return byte(v)
}

func clipU16(v int32) uint16 {
	if v < 0 {
		return 0
	}
	if v > 65535 {
		return 65535
	}
	return uint16(v)
}

// clip16 is _CLIP16: clamp to a signed 16-bit range.
func clip16(v int32) int16 {
	if v < -32768 {
		return -32768
	}
	if v > 32767 {
		return 32767
	}
	return int16(v)
}

// clip2 is _CLIP2: clamp v to [lo, hi].
func clip2(lo, v, hi int32) int32 {
	if v < lo {
		return lo
	}
	if v > hi {
		return hi
	}
	return v
}

// icc is _ICC: inverse colour conversion to r, g, b.
func icc(y, u, v int32) (r, g, b int32) {
	g, r, b = y, -u, v
	g -= r >> 1
	r -= ((b + 1) >> 1) - g
	b += r
	return
}

// iccCMYK is _ICC_CMYK: inverse colour conversion, strdec.c:405.
func iccCMYK(c, m, y, k int32) (int32, int32, int32, int32) {
	k -= (m + 1) >> 1
	m -= (c >> 1) - k
	c -= ((y + 1) >> 1) - m
	y += c
	return c, m, y, k
}

// backwardHalf converts an internal sample to Half-16, strdec.c:504-510.
func backwardHalf(h int32) uint16 {
	s := h >> 31
	h = ((h & 0x7fff) ^ s) - s
	return uint16(h)
}

// pixel2float is pixel2float, strdec.c:464-501.
func pixel2float(h int32, expBias int8, lm uint8) float32 {
	lmshift := int32(1) << lm
	s := h >> 31
	h = (h ^ s) - s // abs
	e := int32(uint32(h) >> lm)
	m := (h & (lmshift - 1)) | lmshift
	if e == 0 {
		m ^= lmshift
		e = 1
	}
	e += 127 - int32(expBias)
	for m < lmshift && e > 1 && m > 0 {
		e--
		m <<= 1
	}
	if m < lmshift {
		e = 0
	} else {
		m ^= lmshift
	}
	m <<= 23 - int32(lm)
	bits := (uint32(s) & 0x80000000) | uint32(e)<<23 | uint32(m)
	return math.Float32frombits(bits)
}

// inverseConvert is inverseConvert, strdec.c:415-430.
func inverseConvert(f int32) (rgb, e byte) {
	switch {
	case f <= 0:
		return 0, 0
	case (f >> 7) > 1:
		return byte(f&0x7f) | 0x80, byte(f >> 7)
	default:
		return byte(f), 1
	}
}

// inverseConvertRGBE is inverseConvertRGBE, strdec.c:436-460.
func inverseConvertRGBE(fr, fg, fb int32) (r, g, b, e byte) {
	var re, ge, be byte
	r, re = inverseConvert(fr)
	g, ge = inverseConvert(fg)
	b, be = inverseConvert(fb)
	e = re
	if ge > e {
		e = ge
	}
	if be > e {
		e = be
	}
	if e > re {
		r = byte((int32(r)*2 + 1) >> (uint32(e-re) + 1))
	}
	if e > ge {
		g = byte((int32(g)*2 + 1) >> (uint32(e-ge) + 1))
	}
	if e > be {
		b = byte((int32(b)*2 + 1) >> (uint32(e-be) + 1))
	}
	return
}

// Pixel c of an MB row sits at idxCC[row][0] + grpOff[c].
var grpOff = [16]int{0, 1, 5, 4, 64, 65, 69, 68, 128, 129, 133, 132, 192, 193, 197, 196}

// fullMBs splits columns [c, e) at whole macroblocks [c0, c0+16n).
func fullMBs(c, e int) (c0, n int) {
	c0 = (c + 15) &^ 15
	if c0 >= e {
		return e, 0
	}
	return c0, (e - c0) >> 4
}

// outBGR8Go writes n MBs of one row from s, 3 bytes a pixel at d.
func outBGR8Go(out []byte, d int, y, u, v []int32, s, n int, bias int32, shift uint32, iR, iB int) {
	for m := 0; m < n; m++ {
		for c := 0; c < 16; c++ {
			idx := s + 256*m + grpOff[c]
			r, g, b := icc(y[idx]+bias, u[idx], v[idx])
			o := d + 48*m + 3*c
			out[o+iR], out[o+1], out[o+iB] = clip8(r>>shift), clip8(g>>shift), clip8(b>>shift)
		}
	}
}

// outGray16Go writes n MBs of one row from s, 2 bytes a pixel at d.
func outGray16Go(out []byte, d int, y []int32, s, n int, bias int32, shift, nLen uint32) {
	for m := 0; m < n; m++ {
		for c := 0; c < 16; c++ {
			p := ((y[s+256*m+grpOff[c]] + bias) >> shift) << nLen
			binary.LittleEndian.PutUint16(out[d+32*m+2*c:], clipU16(p))
		}
	}
}

// outGray8 writes n MBs of one row from s, 1 byte a pixel at d.
func outGray8(out []byte, d int, y []int32, s, n int, bias int32, shift uint32) {
	for m := 0; m < n; m++ {
		o := out[d+16*m : d+16*m+16]
		for c := range o {
			o[c] = byte(min(max((y[s+256*m+grpOff[c]]+bias)>>shift, 0), 255))
		}
	}
}

func (sc *strCodec) interpolateUV() {
	cfExt := sc.wmii.cfColorFormat
	cWidth := sc.cmbWidth * 16
	srcU := sc.mbBuf[1][sc.a0Off[1]:]
	srcV := sc.mbBuf[2][sc.a0Off[2]:]
	dstU, dstV := sc.pResU, sc.pResV

	if sc.param.cfColorFormat == YUV422 {
		for row := 0; row < 16; row++ {
			var idxD, col int
			for col = 0; col < cWidth; col += 2 {
				idxS := ((col >> 4) << 7) + int(idxCC[row][(col>>1)&7])
				idxD = ((col >> 4) << 8) + int(idxCC[row][col&15])
				dstU[idxD] = srcU[idxS]
				dstV[idxD] = srcV[idxS]
				if col > 0 {
					l := col - 2
					idxL := ((l >> 4) << 8) + int(idxCC[row][l&15])
					c := col - 1
					idxC := ((c >> 4) << 8) + int(idxCC[row][c&15])
					dstU[idxC] = (dstU[idxL] + dstU[idxD] + 1) >> 1
					dstV[idxC] = (dstV[idxL] + dstV[idxD] + 1) >> 1
				}
			}
			idxS := (((col - 1) >> 4) << 8) + int(idxCC[row][(col-1)&15])
			dstU[idxS] = dstU[idxD]
			dstV[idxS] = dstV[idxD]
		}
		return
	}

	cShift := 4
	if cfExt == YUV422 {
		cShift = 3
	}
	nextU := sc.mbBuf[1][sc.a1Off[1]:]
	nextV := sc.mbBuf[2][sc.a1Off[2]:]
	for col := 0; col < cWidth; col += 2 {
		cMB := (col >> 4) << (4 + cShift)
		cPix := (col >> (4 - cShift)) & ((1 << cShift) - 1)
		var idxD int
		for row := 0; row < 16; row += 2 {
			idxS := ((col >> 4) << 6) + int(idxCC420[row>>1][(col>>1)&7])
			idxD = cMB + int(idxCC[row][cPix])
			dstU[idxD] = srcU[idxS]
			dstV[idxD] = srcV[idxS]
			if row > 0 {
				idxT := cMB + int(idxCC[row-2][cPix])
				idxC := cMB + int(idxCC[row-1][cPix])
				dstU[idxC] = (dstU[idxT] + dstU[idxD] + 1) >> 1
				dstV[idxC] = (dstV[idxT] + dstV[idxD] + 1) >> 1
			}
		}
		idxS := cMB + int(idxCC[15][cPix])
		if sc.cRow == sc.cmbHeight {
			dstU[idxS] = dstU[idxD]
			dstV[idxS] = dstV[idxD]
		} else {
			idxB := ((col >> 4) << 6) + int(idxCC420[0][(col>>1)&7])
			dstU[idxS] = (nextU[idxB] + dstU[idxD] + 1) >> 1
			dstV[idxS] = (nextV[idxB] + dstV[idxD] + 1) >> 1
		}
	}

	if cfExt != YUV422 {
		for row := 0; row < 16; row++ {
			var idxS int
			for col := 1; col < cWidth-2; col += 2 {
				idxL := (((col - 1) >> 4) << 8) + int(idxCC[row][(col-1)&15])
				idxD := ((col >> 4) << 8) + int(idxCC[row][col&15])
				idxS = (((col + 1) >> 4) << 8) + int(idxCC[row][(col+1)&15])
				dstU[idxD] = (dstU[idxS] + dstU[idxL] + 1) >> 1
				dstV[idxD] = (dstV[idxS] + dstV[idxL] + 1) >> 1
			}
			idxD := (((cWidth - 1) >> 4) << 8) + int(idxCC[row][(cWidth-1)&15])
			dstU[idxD] = dstU[idxS]
			dstV[idxD] = dstV[idxS]
		}
	}
}

func (sc *strCodec) outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight int, iShift uint32, iBias int32) {
	cChannel := sc.wmiscp.cChannel
	if sc.wmii.cfColorFormat == YOnly {
		cChannel = 1
	}
	nLen := uint32(sc.wmiscp.nLenMantissaOrShift)
	nExpBias := sc.wmiscp.nExpBias
	var ch [maxChannels][]int32
	for i := 0; i < cChannel; i++ {
		ch[i] = sc.mbBuf[i][sc.a0Off[i]:]
	}
	if sc.uvResolutionChange {
		ch[1], ch[2] = sc.pResU, sc.pResV
	}
	offX := sc.dp.pOffsetX
	offY := sc.dp.pOffsetY[(sc.cRow-1)*16:]
	out := sc.out

	switch sc.wmii.bdBitDepth {
	case BD8:
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			c0, n := cWidth, 0
			if cChannel == 1 && sc.wmii.cBitsPerUnit == 8 {
				c0, n = fullMBs(iFirstColumn, cWidth)
			}
			if n > 0 {
				outGray8(out, iY+offX[c0], ch[0], (c0>>4)<<8+int(idxCC[row][0]), n, iBias, iShift)
			}
			for _, cr := range [2][2]int{{iFirstColumn, c0}, {c0 + 16*n, cWidth}} {
				for col := cr[0]; col < cr[1]; col++ {
					d := iY + offX[col]
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					for c := 0; c < cChannel; c++ {
						out[d+c] = clip8((ch[c][idx] + iBias) >> iShift)
					}
				}
			}
		}
	case BD16:
		chs := ch[:cChannel]
		if sc.plane > 0 {
			chs = ch[sc.plane-1 : sc.plane]
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			c0, n := cWidth, 0
			if len(chs) == 1 && (sc.plane > 0 || sc.wmii.cBitsPerUnit == 16) {
				c0, n = fullMBs(iFirstColumn, cWidth)
			}
			if n > 0 {
				outGray16(out, 2*(iY+offX[c0]), chs[0], (c0>>4)<<8+int(idxCC[row][0]), n, iBias, iShift, nLen)
			}
			for _, cr := range [2][2]int{{iFirstColumn, c0}, {c0 + 16*n, cWidth}} {
				for col := cr[0]; col < cr[1]; col++ {
					d := 2 * (iY + offX[col])
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					for c, v := range chs {
						p := ((v[idx] + iBias) >> iShift) << nLen
						binary.LittleEndian.PutUint16(out[d+2*c:], clipU16(p))
					}
				}
			}
		}
	case BD16S:
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				d := 2 * (iY + offX[col])
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				for c := 0; c < cChannel; c++ {
					p := ((ch[c][idx] + iBias) >> iShift) << nLen
					binary.LittleEndian.PutUint16(out[d+2*c:], uint16(clip16(p)))
				}
			}
		}
	case BD16F:
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				d := 2 * (iY + offX[col])
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				for c := 0; c < cChannel; c++ {
					p := (ch[c][idx] + iBias) >> iShift
					binary.LittleEndian.PutUint16(out[d+2*c:], backwardHalf(p))
				}
			}
		}
	case BD32S:
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				d := 4 * (iY + offX[col])
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				for c := 0; c < cChannel; c++ {
					p := ((ch[c][idx] + iBias) >> iShift) << nLen
					binary.LittleEndian.PutUint32(out[d+4*c:], uint32(p))
				}
			}
		}
	case BD32F:
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				d := 4 * (iY + offX[col])
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				for c := 0; c < cChannel; c++ {
					p := (ch[c][idx] + iBias) >> iShift
					binary.LittleEndian.PutUint32(out[d+4*c:], math.Float32bits(pixel2float(p, nExpBias, uint8(nLen))))
				}
			}
		}
	}
}

func (sc *strCodec) outputMBRow() error {
	cfExt := sc.wmii.cfColorFormat
	if sc.param.cfColorFormat == YOnly {
		cfExt = YOnly
	}
	bd := sc.wmii.bdBitDepth
	var iShift uint32
	if sc.param.bScaledArith {
		iShift = shiftZero + qpFracBits
	}
	dp := &sc.dp
	cHeight := dp.cROIBottomY + 1 - (sc.cRow-1)*16
	if cHeight > 16 {
		cHeight = 16
	}
	cWidth := dp.cROIRightX + 1
	iFirstRow := 0
	if (sc.cRow-1)*16 <= dp.cROITopY {
		iFirstRow = dp.cROITopY & 0xf
	}
	iFirstColumn := dp.cROILeftX
	nLen := uint32(sc.wmiscp.nLenMantissaOrShift)
	nExpBias := sc.wmiscp.nExpBias

	if sc.uvResolutionChange {
		sc.interpolateUV()
	}
	pY := sc.mbBuf[0][sc.a0Off[0]:]
	var pU, pV []int32
	if sc.param.cNumChannels >= 3 {
		pU = sc.mbBuf[1][sc.a0Off[1]:]
		pV = sc.mbBuf[2][sc.a0Off[2]:]
	}
	if sc.uvResolutionChange {
		pU, pV = sc.pResU, sc.pResV
	}
	iB, iR := 0, 2
	if sc.wmii.bRGB {
		iB, iR = 2, 0
	}
	offX := dp.pOffsetX
	offY := dp.pOffsetY[(sc.cRow-1)*16:]
	out := sc.out

	switch bd {
	case BD8:
		iBias1 := int32(128) << iShift
		var iBias2 int32
		if sc.param.bScaledArith {
			iBias2 = (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		iBias := iBias1 + iBias2
		switch cfExt {
		case CFRGB:
			if sc.alpha == nil {
				for row := iFirstRow; row < cHeight; row++ {
					iY := offY[row]
					c0, n := cWidth, 0
					if sc.wmii.cBitsPerUnit == 24 {
						c0, n = fullMBs(iFirstColumn, cWidth)
					}
					if n > 0 {
						outBGR8(out, offX[c0]+iY, pY, pU, pV, (c0>>4)<<8+int(idxCC[row][0]), n, iBias, iShift, iR, iB)
					}
					for _, cr := range [2][2]int{{iFirstColumn, c0}, {c0 + 16*n, cWidth}} {
						for col := cr[0]; col < cr[1]; col++ {
							idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
							r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
							r, g, b = r>>iShift, g>>iShift, b>>iShift
							d := offX[col] + iY
							out[d+iR], out[d+1], out[d+iB] = clip8(r), clip8(g), clip8(b)
						}
					}
				}
			} else {
				// BD8 RGB inline alpha (strdec.c:993-1036): the alpha
				// channel writes out[d+3], sharing the RGB bias/shift.
				pA := sc.alpha.mbBuf[0][sc.alpha.a0Off[0]:]
				for row := iFirstRow; row < cHeight; row++ {
					iY := offY[row]
					for col := iFirstColumn; col < cWidth; col++ {
						idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
						r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
						a := pA[idx] + iBias
						r, g, b, a = r>>iShift, g>>iShift, b>>iShift, a>>iShift
						d := offX[col] + iY
						out[d+iR], out[d+1], out[d+iB], out[d+3] = clip8(r), clip8(g), clip8(b), clip8(a)
					}
				}
			}
		case CFRGBE:
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					r, g, b := icc(pY[idx]+iBias2, pU[idx], pV[idx])
					d := offX[col] + iY
					out[d], out[d+1], out[d+2], out[d+3] = inverseConvertRGBE(r>>iShift, g>>iShift, b>>iShift)
				}
			}
		case CMYK:
			pK := sc.mbBuf[3][sc.a0Off[3]:]
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					m, c, y, k := -pY[idx]+iBias1, pU[idx], -pV[idx], pK[idx]+iBias2
					c, m, y, k = iccCMYK(c, m, y, k)
					c, m, y, k = c>>iShift, m>>iShift, y>>iShift, k>>iShift
					d := offX[col] + iY
					out[d], out[d+1], out[d+2], out[d+3] = clip8(c), clip8(m), clip8(y), clip8(k)
				}
			}
		case YOnly, YUV444, NComponent:
			sc.outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight, iShift, iBias)
		default:
			return ErrUnsupported
		}
	case BD16:
		iBias := (int32(1<<15) >> nLen) << iShift
		if iShift != 0 {
			iBias += 1 << (iShift - 1)
		}
		switch cfExt {
		case CFRGB:
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
					r, g, b = (r>>iShift)<<nLen, (g>>iShift)<<nLen, (b>>iShift)<<nLen
					d := 2 * (offX[col] + iY)
					binary.LittleEndian.PutUint16(out[d:], clipU16(r))
					binary.LittleEndian.PutUint16(out[d+2:], clipU16(g))
					binary.LittleEndian.PutUint16(out[d+4:], clipU16(b))
				}
			}
		case CMYK:
			iBias1 := (int32(32768) >> nLen) << iShift
			iBias2 := iBias - iBias1
			pK := sc.mbBuf[3][sc.a0Off[3]:]
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					m, c, y, k := -pY[idx]+iBias1, pU[idx], -pV[idx], pK[idx]+iBias2
					c, m, y, k = iccCMYK(c, m, y, k)
					c, m, y, k = (c>>iShift)<<nLen, (m>>iShift)<<nLen, (y>>iShift)<<nLen, (k>>iShift)<<nLen
					d := 2 * (offX[col] + iY)
					binary.LittleEndian.PutUint16(out[d:], clipU16(c))
					binary.LittleEndian.PutUint16(out[d+2:], clipU16(m))
					binary.LittleEndian.PutUint16(out[d+4:], clipU16(y))
					binary.LittleEndian.PutUint16(out[d+6:], clipU16(k))
				}
			}
		case YOnly, YUV444, NComponent:
			sc.outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight, iShift, iBias)
		default:
			return ErrUnsupported
		}
	case BD16S:
		var iBias int32
		if sc.param.bScaledArith {
			iBias = (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		switch cfExt {
		case CFRGB:
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
					r, g, b = (r>>iShift)<<nLen, (g>>iShift)<<nLen, (b>>iShift)<<nLen
					d := 2 * (offX[col] + iY)
					binary.LittleEndian.PutUint16(out[d:], uint16(clip16(r)))
					binary.LittleEndian.PutUint16(out[d+2:], uint16(clip16(g)))
					binary.LittleEndian.PutUint16(out[d+4:], uint16(clip16(b)))
				}
			}
		case CMYK:
			pK := sc.mbBuf[3][sc.a0Off[3]:]
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					m, c, y, k := -pY[idx], pU[idx], -pV[idx], pK[idx]+iBias
					c, m, y, k = iccCMYK(c, m, y, k)
					c, m, y, k = (c>>iShift)<<nLen, (m>>iShift)<<nLen, (y>>iShift)<<nLen, (k>>iShift)<<nLen
					d := 2 * (offX[col] + iY)
					binary.LittleEndian.PutUint16(out[d:], uint16(int16(c)))
					binary.LittleEndian.PutUint16(out[d+2:], uint16(int16(m)))
					binary.LittleEndian.PutUint16(out[d+4:], uint16(int16(y)))
					binary.LittleEndian.PutUint16(out[d+6:], uint16(int16(k)))
				}
			}
		case YOnly, YUV444, NComponent:
			sc.outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight, iShift, iBias)
		default:
			return ErrUnsupported
		}
	case BD16F:
		var iBias int32
		if sc.param.bScaledArith {
			iBias = (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		switch cfExt {
		case CFRGB:
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
					d := 2 * (offX[col] + iY)
					binary.LittleEndian.PutUint16(out[d:], backwardHalf(r>>iShift))
					binary.LittleEndian.PutUint16(out[d+2:], backwardHalf(g>>iShift))
					binary.LittleEndian.PutUint16(out[d+4:], backwardHalf(b>>iShift))
				}
			}
		case YOnly, YUV444, NComponent:
			sc.outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight, iShift, iBias)
		default:
			return ErrUnsupported
		}
	case BD32S:
		var iBias int32
		if sc.param.bScaledArith {
			iBias = (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		switch cfExt {
		case CFRGB:
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
					d := 4 * (offX[col] + iY)
					binary.LittleEndian.PutUint32(out[d:], uint32((r>>iShift)<<nLen))
					binary.LittleEndian.PutUint32(out[d+4:], uint32((g>>iShift)<<nLen))
					binary.LittleEndian.PutUint32(out[d+8:], uint32((b>>iShift)<<nLen))
				}
			}
		case YOnly, YUV444, NComponent:
			sc.outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight, iShift, iBias)
		default:
			return ErrUnsupported
		}
	case BD32F:
		var iBias int32
		if sc.param.bScaledArith {
			iBias = (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		switch cfExt {
		case CFRGB:
			for row := iFirstRow; row < cHeight; row++ {
				iY := offY[row]
				for col := iFirstColumn; col < cWidth; col++ {
					idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
					r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
					d := 4 * (offX[col] + iY)
					binary.LittleEndian.PutUint32(out[d:], math.Float32bits(pixel2float(r>>iShift, nExpBias, uint8(nLen))))
					binary.LittleEndian.PutUint32(out[d+4:], math.Float32bits(pixel2float(g>>iShift, nExpBias, uint8(nLen))))
					binary.LittleEndian.PutUint32(out[d+8:], math.Float32bits(pixel2float(b>>iShift, nExpBias, uint8(nLen))))
				}
			}
		case YOnly, YUV444, NComponent:
			sc.outputNChannel(iFirstRow, iFirstColumn, cWidth, cHeight, iShift, iBias)
		default:
			return ErrUnsupported
		}
	case BD5:
		iBias := int32(16) << iShift
		if sc.param.bScaledArith {
			iBias += (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
				r, g, b = r>>iShift, g>>iShift, b>>iShift
				d := 2 * (offX[col] + iY)
				var v uint16
				if sc.param.bRBSwapped {
					v = uint16(clip2(0, b, 31)) | uint16(clip2(0, g, 31))<<5 | uint16(clip2(0, r, 31))<<10
				} else {
					v = uint16(clip2(0, r, 31)) | uint16(clip2(0, g, 31))<<5 | uint16(clip2(0, b, 31))<<10
				}
				binary.LittleEndian.PutUint16(out[d:], v)
			}
		}
	case BD565:
		iBias := int32(32) << iShift
		if sc.param.bScaledArith {
			iBias += (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
				g, b, r = g>>iShift, b>>(iShift+1), r>>(iShift+1)
				d := 2 * (offX[col] + iY)
				var v uint16
				if sc.param.bRBSwapped {
					v = uint16(clip2(0, b, 31)) | uint16(clip2(0, g, 63))<<5 | uint16(clip2(0, r, 31))<<11
				} else {
					v = uint16(clip2(0, r, 31)) | uint16(clip2(0, g, 63))<<5 | uint16(clip2(0, b, 31))<<11
				}
				binary.LittleEndian.PutUint16(out[d:], v)
			}
		}
	case BD10:
		iBias := int32(512) << iShift
		if sc.param.bScaledArith {
			iBias += (1 << (shiftZero + qpFracBits - 1)) - 1
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				r, g, b := icc(pY[idx]+iBias, pU[idx], pV[idx])
				r, g, b = r>>iShift, g>>iShift, b>>iShift
				d := 4 * (offX[col] + iY)
				var v uint32
				if sc.param.bRBSwapped {
					v = uint32(clip2(0, b, 1023)) | uint32(clip2(0, g, 1023))<<10 | uint32(clip2(0, r, 1023))<<20
				} else {
					v = uint32(clip2(0, r, 1023)) | uint32(clip2(0, g, 1023))<<10 | uint32(clip2(0, b, 1023))<<20
				}
				binary.LittleEndian.PutUint32(out[d:], v)
			}
		}
	case BD1:
		iTh := int32(1)
		if iShift > 0 {
			iTh = 1 << (iShift - 1)
		}
		var bw int32
		if sc.wmiscp.bBlackWhite {
			bw = 1
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				byteIdx := (offX[col] >> 3) + iY
				cByte := out[byteIdx]
				shift := uint(7 - (offX[col] & 7))
				var bit int32
				if pY[idx] >= iTh {
					bit = 1
				}
				out[byteIdx] ^= byte((bw+bit+int32(cByte>>shift))&1) << shift
			}
		}
	default:
		return ErrUnsupported
	}

	// REENTRANT_MODE fixup: grey to RGB.
	if sc.wmii.cfColorFormat == CFRGB && sc.param.cfColorFormat == YOnly {
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				if bd == BD8 {
					d := iY + offX[col]
					out[d+1], out[d+2] = out[d], out[d]
				} else {
					d := 2 * (iY + offX[col])
					copy(out[d+2:d+4], out[d:d+2])
					copy(out[d+4:d+6], out[d:d+2])
				}
			}
		}
	}
	if sc.alpha != nil {
		if err := sc.outputAlpha(iFirstRow, iFirstColumn, cWidth, cHeight, iShift); err != nil {
			return err
		}
	}
	return nil
}

// outputAlpha is outputMBRowAlpha, strdec.c:791-871: composite an
// interleaved alpha plane into the trailing channel of each output
// pixel.
func (sc *strCodec) outputAlpha(iFirstRow, iFirstColumn, cWidth, cHeight int, iShift uint32) error {
	bd := sc.wmii.bdBitDepth
	// jxrlib tests internal format: never true.
	if bd == BD8 && sc.wmiscp.cfColorFormat == CFRGB {
		return nil
	}
	iAlphaPos := 3
	if sc.wmii.cfColorFormat == CMYK {
		iAlphaPos = 4
	} else if sc.wmii.cfColorFormat != CFRGB {
		return ErrUnsupported
	}
	nLen := uint32(sc.wmiscp.nLenMantissaOrShift)
	nExpBias := sc.wmiscp.nExpBias
	pA := sc.alpha.mbBuf[0][sc.alpha.a0Off[0]:]
	offX := sc.dp.pOffsetX
	offY := sc.dp.pOffsetY[(sc.cRow-1)*16:]
	out := sc.out

	switch bd {
	case BD8:
		iBias := int32(1) << (iShift + 7)
		if iShift != 0 {
			iBias += 1 << (iShift - 1)
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				a := (pA[idx] + iBias) >> iShift
				out[offX[col]+iY+iAlphaPos] = clip8(a)
			}
		}
	case BD16:
		iBias := int32(1) << (iShift + 15)
		if iShift != 0 {
			iBias += 1 << (iShift - 1)
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				a := ((pA[idx] + iBias) >> iShift) << nLen
				d := 2*(offX[col]+iY) + 2*iAlphaPos
				binary.LittleEndian.PutUint16(out[d:], clipU16(a))
			}
		}
	case BD16S:
		var iBias int32
		if iShift != 0 {
			iBias = 1 << (iShift - 1)
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				a := ((pA[idx] + iBias) >> iShift) << nLen
				d := 2*(offX[col]+iY) + 2*iAlphaPos
				binary.LittleEndian.PutUint16(out[d:], uint16(clip16(a)))
			}
		}
	case BD16F:
		var iBias int32
		if iShift != 0 {
			iBias = 1 << (iShift - 1)
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				a := (pA[idx] + iBias) >> iShift
				d := 2*(offX[col]+iY) + 2*iAlphaPos
				binary.LittleEndian.PutUint16(out[d:], backwardHalf(a))
			}
		}
	case BD32S:
		var iBias int32
		if iShift != 0 {
			iBias = 1 << (iShift - 1)
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				a := ((pA[idx] + iBias) >> iShift) << nLen
				d := 4*(offX[col]+iY) + 4*iAlphaPos
				binary.LittleEndian.PutUint32(out[d:], uint32(a))
			}
		}
	case BD32F:
		var iBias int32
		if iShift != 0 {
			iBias = 1 << (iShift - 1)
		}
		for row := iFirstRow; row < cHeight; row++ {
			iY := offY[row]
			for col := iFirstColumn; col < cWidth; col++ {
				idx := ((col >> 4) << 8) + int(idxCC[row][col&15])
				a := (pA[idx] + iBias) >> iShift
				d := 4*(offX[col]+iY) + 4*iAlphaPos
				binary.LittleEndian.PutUint32(out[d:], math.Float32bits(pixel2float(a, nExpBias, uint8(nLen))))
			}
		}
	default:
		return ErrUnsupported
	}
	return nil
}
