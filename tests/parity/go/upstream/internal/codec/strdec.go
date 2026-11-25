// Ported from jxrlib strdec.c (BSD-2, see NOTICE).

package codec

import (
	"context"
	"unsafe"
)

// Format is the output pixel layout, from the container pixel format.
type Format struct {
	ColorFormat int // CFRGB or YOnly
	BitDepth    int // BD8 or BD16
	BitsPerUnit int // bits per pixel
	RGB         bool
	Alpha       bool // uAlphaMode > 0: decode an interleaved alpha plane
	// Plane > 0 writes only channel Plane-1 of an NComponent BD16 image,
	// one 16-bit sample a pixel.
	Plane int
}

// Header holds the image header fields callers need.
type Header struct {
	Width, Height int  // inscribed image size
	ColorFormat   int  // source colour format field
	BitDepth      int  // source bit depth field
	Alpha         bool // bAlphaChannel: an interleaved alpha plane follows the primary

	param coreParameters
	ii    imageInfo
	scp   codecParam
	size  int // header bytes, up to the index table

	// Interleaved alpha's own plane header, read straight after the
	// primary's (strdec.c:3346-3350); zero when !Alpha.
	aParam coreParameters
	aScp   codecParam
}

func readQuantizerSB(q *[maxChannels]uint8, sb *simpleBitIO, cChannel int) uint8 {
	var mode uint8
	if cChannel >= maxChannels {
		return 0
	}
	if cChannel > 1 {
		mode = uint8(sb.getBit32(2))
	}
	q[0] = uint8(sb.getBit32(8))
	if mode == 1 {
		q[1] = uint8(sb.getBit32(8))
	} else if mode > 0 {
		for i := 1; i < cChannel; i++ {
			q[i] = uint8(sb.getBit32(8))
		}
	}
	return mode
}

func readImagePlaneHeader(ii *imageInfo, scp *codecParam, p *coreParameters, sb *simpleBitIO) error {
	p.cfColorFormat = int(sb.getBit32(3))
	if p.cfColorFormat > NComponent || p.cfColorFormat == 5 {
		return ErrFormat
	}
	scp.cfColorFormat = p.cfColorFormat
	p.bScaledArith = sb.getBit32(1) != 0
	scp.sbSubband = int(sb.getBit32(4))

	switch p.cfColorFormat {
	case YOnly:
		p.cNumChannels = 1
	case YUV420:
		p.cNumChannels = 3
		sb.getBit32(1)
		ii.cChromaCenteringX = uint8(sb.getBit32(3))
		sb.getBit32(1)
		ii.cChromaCenteringY = uint8(sb.getBit32(3))
	case YUV422:
		p.cNumChannels = 3
		sb.getBit32(1)
		ii.cChromaCenteringX = uint8(sb.getBit32(3))
		sb.getBit32(4)
	case YUV444:
		p.cNumChannels = 3
		sb.getBit32(4)
		sb.getBit32(4)
	case NComponent:
		p.cNumChannels = int(sb.getBit32(4)) + 1
		sb.getBit32(4)
	case CMYK:
		p.cNumChannels = 4
	}

	switch ii.bdBitDepth {
	case BD16, BD16S, BD32, BD32S:
		scp.nLenMantissaOrShift = uint8(sb.getBit32(8))
	case BD32F:
		scp.nLenMantissaOrShift = uint8(sb.getBit32(8))
		scp.nExpBias = int8(sb.getBit32(8))
	}

	p.uQPMode = 0
	if sb.getBit32(1) == 1 {
		p.uQPMode += uint32(readQuantizerSB(&p.uiQPIndexDC, sb, p.cNumChannels)) << 3
	} else {
		p.uQPMode++
	}
	if scp.sbSubband != sbDCOnly {
		if sb.getBit32(1) == 0 {
			p.uQPMode += 0x200
			if sb.getBit32(1) == 1 {
				p.uQPMode += uint32(readQuantizerSB(&p.uiQPIndexLP, sb, p.cNumChannels)) << 5
			} else {
				p.uQPMode += 2
			}
		} else {
			p.uQPMode += ((p.uQPMode & 1) << 1) + ((p.uQPMode & 0x18) << 2)
		}
		if scp.sbSubband != sbNoHighpass {
			if sb.getBit32(1) == 0 {
				p.uQPMode += 0x400
				if sb.getBit32(1) == 1 {
					p.uQPMode += uint32(readQuantizerSB(&p.uiQPIndexHP, sb, p.cNumChannels)) << 7
				} else {
					p.uQPMode += 4
				}
			} else {
				p.uQPMode += ((p.uQPMode & 2) << 1) + ((p.uQPMode & 0x60) << 2)
			}
		}
	}
	switch scp.sbSubband {
	case sbDCOnly:
		p.uQPMode |= 0x200
	case sbNoHighpass:
		p.uQPMode |= 0x400
	}
	if p.uQPMode&0x600 == 0 {
		return ErrFormat
	}
	sb.flushToByte()
	return nil
}

// ReadHeader parses the image header and first plane header (ReadWMIHeader).
func ReadHeader(stream []byte) (Header, error) {
	var h Header
	err := h.Parse(stream)
	return h, err
}

// Parse is ReadHeader into h, reusing h's tile storage.
func (h *Header) Parse(stream []byte) error {
	tiles := h.scp.uiTileX[:cap(h.scp.uiTileX)]
	*h = Header{}
	if len(stream) < 8 || string(stream[:7]) != "WMPHOTO" {
		return ErrFormat
	}
	sb := &simpleBitIO{buf: stream[8:]}
	p := &h.param
	ii := &h.ii
	scp := &h.scp

	if sb.getBit32(4) != 1 {
		return ErrFormat
	}
	p.cVersion = 1
	i := sb.getBit32(4)
	if i != codecSubversion && i != codecSubversionSoftTile && i != codecSubversionHardTile {
		return ErrFormat
	}
	p.cSubVersion = i
	p.bUseHardTileBoundaries = i == codecSubversionHardTile
	scp.bUseHardTileBoundaries = p.bUseHardTileBoundaries

	tiling := sb.getBit32(1) != 0
	scp.bfBitstreamFormat = int(sb.getBit32(1))
	ii.oOrientation = int(sb.getBit32(3))
	p.bIndexTable = sb.getBit32(1) != 0
	i = sb.getBit32(2)
	if i == 3 {
		return ErrFormat
	}
	scp.olOverlap = int(i)

	short := sb.getBit32(1) != 0
	sb.getBit32(1) // long word flag
	inscribed := sb.getBit32(1) != 0
	p.bTrimFlexbitsFlag = sb.getBit32(1) != 0
	tileStretch := sb.getBit32(1) != 0
	p.bRBSwapped = sb.getBit32(1) != 0
	sb.getBit32(1)
	p.bAlphaChannel = sb.getBit32(1) != 0

	ii.cfColorFormat = int(sb.getBit32(4))
	ii.bdBitDepth = int(sb.getBit32(4))
	if ii.bdBitDepth == BD1alt {
		ii.bdBitDepth = BD1
		scp.bBlackWhite = true
	}

	sizeBits := uint32(32)
	tileBits := uint32(16)
	if short {
		sizeBits, tileBits = 16, 8
	}
	ii.cWidth = int(sb.getBit32(sizeBits)) + 1
	ii.cHeight = int(sb.getBit32(sizeBits)) + 1
	if !inscribed && ii.cWidth&0xf != 0 {
		p.cExtraPixelsRight = 0x10 - ii.cWidth&0xf
	}
	if !inscribed && ii.cHeight&0xf != 0 {
		p.cExtraPixelsBottom = 0x10 - ii.cHeight&0xf
	}

	if tiling {
		scp.cNumOfSliceMinus1V = sb.getBit32(logMaxTiles)
		scp.cNumOfSliceMinus1H = sb.getBit32(logMaxTiles)
	}
	if !p.bIndexTable && (scp.bfBitstreamFormat == frequency || scp.cNumOfSliceMinus1V+scp.cNumOfSliceMinus1H > 0) {
		return ErrFormat
	}
	// Slices: headers are copied per decode.
	t := tiles
	if n := int(scp.cNumOfSliceMinus1V + scp.cNumOfSliceMinus1H + 2); cap(t) >= n {
		t = t[:n]
		clear(t)
	} else {
		t = make([]uint32, n)
	}
	scp.uiTileX, scp.uiTileY = t[:scp.cNumOfSliceMinus1V+1], t[scp.cNumOfSliceMinus1V+1:]
	for i := uint32(0); i < scp.cNumOfSliceMinus1V; i++ {
		scp.uiTileX[i+1] = sb.getBit32(tileBits) + scp.uiTileX[i]
	}
	for i := uint32(0); i < scp.cNumOfSliceMinus1H; i++ {
		scp.uiTileY[i+1] = sb.getBit32(tileBits) + scp.uiTileY[i]
	}
	if tileStretch {
		for i := uint32(0); i < (scp.cNumOfSliceMinus1V+1)*(scp.cNumOfSliceMinus1H+1); i++ {
			sb.getBit32(8)
		}
	}
	if inscribed {
		p.cExtraPixelsTop = int(sb.getBit32(6))
		p.cExtraPixelsLeft = int(sb.getBit32(6))
		p.cExtraPixelsBottom = int(sb.getBit32(6))
		p.cExtraPixelsRight = int(sb.getBit32(6))
	}
	if (ii.cWidth+p.cExtraPixelsLeft+p.cExtraPixelsRight)&0xf+(ii.cHeight+p.cExtraPixelsTop+p.cExtraPixelsBottom)&0xf != 0 {
		if ii.cWidth&0xf+ii.cHeight&0xf+p.cExtraPixelsLeft+p.cExtraPixelsTop != 0 {
			return ErrFormat
		}
		if ii.cWidth <= p.cExtraPixelsRight || ii.cHeight <= p.cExtraPixelsBottom {
			return ErrFormat
		}
		ii.cWidth -= p.cExtraPixelsRight
		ii.cHeight -= p.cExtraPixelsBottom
	}
	sb.flushToByte()

	if err := readImagePlaneHeader(ii, scp, p, sb); err != nil {
		return err
	}
	if sb.err {
		return ErrFormat
	}
	scp.cChannel = p.cNumChannels
	if (ii.bdBitDepth == BD5 || ii.bdBitDepth == BD10 || ii.bdBitDepth == BD565) &&
		scp.cfColorFormat != YUV444 && scp.cfColorFormat != YUV422 && scp.cfColorFormat != YUV420 && scp.cfColorFormat != YOnly {
		return ErrFormat
	}
	if p.bAlphaChannel {
		// Alpha's plane header follows the primary's in the same
		// bitstream (strdec.c:3346-3350); its own errors are ignored
		// like jxrlib does, but a truncated stream still fails below.
		aIi := *ii
		h.aParam = *p
		h.aScp = *scp
		_ = readImagePlaneHeader(&aIi, &h.aScp, &h.aParam, sb)
		h.aParam.cfColorFormat = YOnly
		h.aParam.cNumChannels = 1
		h.aScp.cChannel = 1
	}
	if sb.err {
		return ErrFormat
	}
	h.size = 8 + sb.cbRead
	h.Width, h.Height = ii.cWidth, ii.cHeight
	h.ColorFormat, h.BitDepth = ii.cfColorFormat, ii.bdBitDepth
	h.Alpha = p.bAlphaChannel
	return nil
}

func (sc *strCodec) readQuantizer(q *[maxChannels][]quantizer, io *bitIO, cChannel, iPos int) uint8 {
	var mode uint8
	if cChannel > 1 {
		mode = uint8(io.getBit16(2))
	}
	q[0][iPos].iIndex = uint8(io.getBit16(8))
	if mode == 1 {
		q[1][iPos].iIndex = uint8(io.getBit16(8))
	} else if mode > 0 {
		for i := 1; i < cChannel; i++ {
			q[i][iPos].iIndex = uint8(io.getBit16(8))
		}
	}
	return mode
}

func readPacketHeader(io *bitIO) bool {
	if io.getBit16(8) != 0 {
		return false
	}
	if io.getBit16(8) != 0 {
		return false
	}
	if io.getBit16(8) != 1 {
		return false
	}
	io.getBit16(8)
	return true
}

func (sc *strCodec) readTileHeaderDC(io *bitIO) {
	if sc.param.uQPMode&1 != 0 {
		t := &sc.tiles[sc.cTileColumn]
		if sc.cTileRow+sc.cTileColumn == 0 {
			for i := range sc.tiles {
				allocateQuantizer(&sc.tiles[i].qDCAll, &sc.tiles[i].qDC, sc.param.cNumChannels, 1)
			}
		}
		t.cChModeDC = sc.readQuantizer(&t.qDC, io, sc.param.cNumChannels, 0)
		formatQuantizer(&t.qDC, t.cChModeDC, sc.param.cNumChannels, 0, true, sc.param.bScaledArith)
	}
}

func (sc *strCodec) readTileHeaderLP(io *bitIO) {
	if sc.wmiscp.sbSubband != sbDCOnly && sc.param.uQPMode&2 != 0 {
		t := &sc.tiles[sc.cTileColumn]
		t.bUseDC = io.getBit16(1) == 1
		t.cBitsLP = 0
		t.cNumQPLP = 1
		if t.bUseDC {
			allocateQuantizer(&t.qLPAll, &t.qLP, sc.param.cNumChannels, int(t.cNumQPLP))
			sc.useDCQuantizer(int(sc.cTileColumn))
		} else {
			t.cNumQPLP = uint8(io.getBit16(4)) + 1
			t.cBitsLP = dquantBits(t.cNumQPLP)
			allocateQuantizer(&t.qLPAll, &t.qLP, sc.param.cNumChannels, int(t.cNumQPLP))
			for i := 0; i < int(t.cNumQPLP); i++ {
				t.cChModeLP[i] = sc.readQuantizer(&t.qLP, io, sc.param.cNumChannels, i)
				formatQuantizer(&t.qLP, t.cChModeLP[i], sc.param.cNumChannels, i, true, sc.param.bScaledArith)
			}
		}
	}
}

func (sc *strCodec) readTileHeaderHP(io *bitIO) {
	if sc.wmiscp.sbSubband != sbDCOnly && sc.wmiscp.sbSubband != sbNoHighpass && sc.param.uQPMode&4 != 0 {
		t := &sc.tiles[sc.cTileColumn]
		t.bUseLP = io.getBit16(1) == 1
		t.cBitsHP = 0
		t.cNumQPHP = 1
		if t.bUseLP {
			t.cNumQPHP = t.cNumQPLP
			allocateQuantizer(&t.qHPAll, &t.qHP, sc.param.cNumChannels, int(t.cNumQPHP))
			sc.useLPQuantizer(int(t.cNumQPHP), int(sc.cTileColumn))
		} else {
			t.cNumQPHP = uint8(io.getBit16(4)) + 1
			t.cBitsHP = dquantBits(t.cNumQPHP)
			allocateQuantizer(&t.qHPAll, &t.qHP, sc.param.cNumChannels, int(t.cNumQPHP))
			for i := 0; i < int(t.cNumQPHP); i++ {
				t.cChModeHP[i] = sc.readQuantizer(&t.qHP, io, sc.param.cNumChannels, i)
				formatQuantizer(&t.qHP, t.cChModeHP[i], sc.param.cNumChannels, i, false, sc.param.bScaledArith)
			}
		}
	}
}

func (sc *strCodec) readPackets() error {
	w := &sc.wmiscp
	if sc.cColumn == 0 && uint32(sc.cRow) == w.uiTileY[sc.cTileRow] {
		if sc.secondary {
			// The alpha plane shares the primary's packet/tile headers
			// (strdec.c:206-216); only its coding contexts reset here.
			if len(sc.bitIO) > 0 {
				for k := 0; k <= int(w.cNumOfSliceMinus1V); k++ {
					resetCodingContextDec(&sc.contexts[k])
				}
			} else {
				resetCodingContextDec(&sc.contexts[0])
			}
		} else {
			n := len(sc.bitIO)
			for k := 0; k < n; k++ {
				off := sc.indexTable[n*int(sc.cTileRow)+k] + sc.cHeaderSize
				sc.bitIO[k].attach(sc.stream, off)
			}
			if n == 0 {
				sc.ioHeader.attach(sc.stream, sc.cHeaderSize)
			}
			for k := 0; k <= int(w.cNumOfSliceMinus1V); k++ {
				if w.bfBitstreamFormat == spatial {
					io := &sc.ioHeader
					if n != 0 {
						io = sc.bitIO[k]
					}
					if !readPacketHeader(io) {
						return ErrFormat
					}
					sc.contexts[k].trimFlexBits = 0
					if sc.param.bTrimFlexbitsFlag {
						sc.contexts[k].trimFlexBits = int32(io.getBit16(4))
					}
				} else {
					if !readPacketHeader(sc.bitIO[k*sc.cSB+0]) {
						return ErrFormat
					}
					if sc.cSB > 1 && !readPacketHeader(sc.bitIO[k*sc.cSB+1]) {
						return ErrFormat
					}
					if sc.cSB > 2 && !readPacketHeader(sc.bitIO[k*sc.cSB+2]) {
						return ErrFormat
					}
					if sc.cSB > 3 {
						io := sc.bitIO[k*sc.cSB+3]
						readPacketHeader(io) // jxrlib ignores a bad flexbits header
						sc.contexts[k].trimFlexBits = 0
						if sc.param.bTrimFlexbitsFlag {
							sc.contexts[k].trimFlexBits = int32(io.getBit16(4))
						}
					}
				}
				resetCodingContextDec(&sc.contexts[k])
			}
		}
	}

	if sc.ctxLeft && sc.ctxTop && !sc.secondary {
		c := &sc.contexts[sc.cTileColumn]
		sc.readTileHeaderDC(c.ioDC)
		if sc.alpha != nil {
			sc.alpha.readTileHeaderDC(c.ioDC)
		}
		if sc.cSB > 1 {
			sc.readTileHeaderLP(c.ioLP)
			if sc.alpha != nil {
				sc.alpha.readTileHeaderLP(c.ioLP)
			}
		}
		if sc.cSB > 2 {
			sc.readTileHeaderHP(c.ioAC)
			if sc.alpha != nil {
				sc.alpha.readTileHeaderHP(c.ioAC)
			}
		}
	}
	return nil
}

// processMacroblockDec steps this plane's macroblock, then, when an
// interleaved alpha plane is present, steps it too (strdec.c:309-393:
// the for(j=0;j<=jend;j++) loop, unrolled since jend is at most 1 here).
func (sc *strCodec) processMacroblockDec() error {
	if err := sc.mbStep(); err != nil {
		return err
	}
	if sc.alpha != nil {
		sc.alpha.cRow, sc.alpha.cColumn = sc.cRow, sc.cColumn
		if err := sc.alpha.mbStep(); err != nil {
			return err
		}
	}
	return nil
}

func (sc *strCodec) mbStep() error {
	ol := sc.wmiscp.olOverlap
	bottom := sc.cRow == sc.cmbHeight
	bottomORright := bottom || sc.cColumn == sc.cmbWidth
	dp := &sc.dp

	if !bottomORright {
		sc.getTilePos(sc.cColumn, sc.cRow)
		ctx := &sc.contexts[sc.cTileColumn]
		if err := sc.readPackets(); err != nil {
			return err
		}
		w := &sc.wmiscp
		if !dp.bDecodeFullFrame && uint32(sc.cColumn) == w.uiTileX[sc.cTileColumn] {
			rLeft, rRight := dp.cROILeftX, dp.cROIRightX
			rTop, rBottom := dp.cROITopY, dp.cROIBottomY
			rExt := 0
			switch ol {
			case olOne:
				rExt = 2
			case olTwo:
				rExt = 10
			}
			tLeft := sc.cColumn * 16
			tTop := int(w.uiTileY[sc.cTileRow]) * 16
			tRight := sc.cmbWidth * 16
			if sc.cTileColumn != w.cNumOfSliceMinus1V {
				tRight = int(w.uiTileX[sc.cTileColumn+1]) * 16
			}
			tBottom := sc.cmbHeight * 16
			if sc.cTileRow != w.cNumOfSliceMinus1H {
				tBottom = int(w.uiTileY[sc.cTileRow+1]) * 16
			}
			ctx.inROI = rLeft < tRight+rExt && rTop < tBottom+rExt && tLeft <= rRight+rExt &&
				tTop <= rBottom+rExt && sc.cRow*16 <= rBottom+rExt
		}

		if dp.bDecodeFullFrame || ctx.inROI {
			if err := sc.decodeMacroblockDC(ctx); err != nil {
				return err
			}
			if dp.bDecodeLP {
				if err := sc.decodeMacroblockLowpass(ctx); err != nil {
					return err
				}
			}
			sc.predDCACDec()
			sc.dequantizeMacroblock()
			if dp.bDecodeHP {
				if err := sc.decodeMacroblockHighpass(ctx); err != nil {
					return err
				}
				sc.predACDec()
			}
			sc.updatePredInfo(sc.cColumn, sc.param.cfColorFormat)
		}
	}

	if !dp.bDecodeFullFrame && (sc.cColumn*16 > dp.cROIRightX+25 || sc.cColumn*16+25 < dp.cROILeftX ||
		sc.cRow*16 > dp.cROIBottomY+25 || sc.cRow*16+25 < dp.cROITopY) {
		return nil
	}
	sc.invTransformMacroblockAlteredOperatorsHard()
	return nil
}

// getVLWordEsc reads a variable length byte aligned integer.
func getVLWordEsc(io *bitIO) uint64 {
	s := uint64(io.getBit32(8))
	if s == 0xfd || s == 0xfe || s == 0xff {
		return 0
	}
	if s < 0xfb {
		return s<<8 | uint64(io.getBit32(8))
	}
	s -= 0xfb
	if s != 0 {
		s = uint64(io.getBit32(16)) << 16
		s = (s | uint64(io.getBit32(16))) << 16
		s <<= 16
	}
	s |= uint64(io.getBit32(16)) << 16
	s |= uint64(io.getBit32(16))
	return s
}

func (sc *strCodec) readIndexTable() error {
	io := &sc.ioHeader
	if n := len(sc.bitIO); n > 0 {
		if io.getBit32(16) != 1 {
			return ErrFormat
		}
		for i := range sc.indexTable {
			sc.indexTable[i] = getVLWordEsc(io)
		}
	}
	sc.cHeaderSize = getVLWordEsc(io)
	io.flushToByte()
	sc.cHeaderSize += io.bytePos()
	if sc.cHeaderSize > uint64(len(sc.stream)) {
		return ErrFormat
	}
	for _, v := range sc.indexTable {
		if v > uint64(len(sc.stream)) {
			return ErrFormat
		}
	}
	return nil
}

func (sc *strCodec) setROI() {
	ii := &sc.wmii
	dp := &sc.dp
	p := &sc.param
	sb := sc.wmiscp.sbSubband
	dp.bSkipFlexbits = sb == sbNoFlexbits
	dp.bDecodeHP = sb == sbAll || sb == sbNoFlexbits
	dp.bDecodeLP = sb != sbDCOnly

	// ii holds the padded size here.
	dp.cROILeftX = ii.cROILeftX + p.cExtraPixelsLeft
	dp.cROIRightX = dp.cROILeftX + ii.cROIWidth - 1
	dp.cROITopY = ii.cROITopY + p.cExtraPixelsTop
	dp.cROIBottomY = dp.cROITopY + ii.cROIHeight - 1
	if dp.cROIRightX >= ii.cWidth {
		dp.cROIRightX = ii.cWidth - 1
	}
	if dp.cROIBottomY >= ii.cHeight {
		dp.cROIBottomY = ii.cHeight - 1
	}
	dp.bDecodeFullFrame = dp.cROILeftX+dp.cROITopY == 0 &&
		(dp.cROIRightX+15)/16 >= (ii.cWidth+14)/16 && (dp.cROIBottomY+15)/16 >= (ii.cHeight+14)/16
	dp.bDecodeFullWidth = dp.cROILeftX == 0 && (dp.cROIRightX+15)/16 >= (ii.cWidth+14)/16

	ii.cWidth -= p.cExtraPixelsLeft + p.cExtraPixelsRight
	ii.cHeight -= p.cExtraPixelsTop + p.cExtraPixelsBottom
	sc.cTileColumn, sc.cTileRow = 0, 0
}

var cbChannels = [11]int{1, 1, 2, 2, 2, 4, 4, 4, 0, 0, 0}

// initLookupTables builds pixel offsets, in output units, for O_NONE.
func (sc *strCodec) initLookupTables() {
	ii := &sc.wmii
	dp := &sc.dp
	w := ii.cWidth + dp.cROILeftX
	h := ii.cHeight + dp.cROITopY
	strideY := sc.outStride
	switch ii.bdBitDepth {
	case BD16, BD16S, BD5, BD565, BD16F:
		strideY /= 2
	case BD32, BD32S, BD32F, BD10:
		strideY /= 4
	}
	var strideX int
	switch ii.bdBitDepth {
	case BD1, BD5, BD10, BD565:
		strideX = 1
	default:
		strideX = (ii.cBitsPerUnit >> 3) / cbChannels[ii.bdBitDepth]
	}
	if sc.plane > 0 {
		strideX = 1
	}
	firstX, firstY := 0, 0
	if !dp.bDecodeFullFrame {
		firstX, firstY = dp.cROILeftX, dp.cROITopY
	}
	dp.pOffsetX = reuse(dp.pOffsetX, w)
	clear(dp.pOffsetX[:firstX])
	for i := 0; i+firstX < w; i++ {
		dp.pOffsetX[i+firstX] = i * strideX
	}
	dp.pOffsetY = reuse(dp.pOffsetY, h)
	clear(dp.pOffsetY[:firstY])
	for i := 0; i+firstY < h; i++ {
		dp.pOffsetY[i+firstY] = i * strideY
	}
}

func (sc *strCodec) mbStride(i int) int {
	if i == 0 {
		return 256
	}
	return cblkChromas[sc.param.cfColorFormat] * 16
}

func (sc *strCodec) p0(i int) int { return sc.a0Off[i] + sc.cColumn*sc.mbStride(i) }
func (sc *strCodec) p1(i int) int { return sc.a1Off[i] + sc.cColumn*sc.mbStride(i) }

// State is a decoder's working buffers, reused across Decode calls so a
// warm decode of the same shape makes no allocation.
type State struct {
	sc  strCodec
	asc strCodec // interleaved alpha companion, warm across decodes
	hdr Header
}

// NewState returns an empty State.
func NewState() *State { return &State{} }

// Decode decodes stream (the codestream from WMPHOTO to end of file)
// into dst, laid out as jxrlib writes format f.
func Decode(stream []byte, f Format, dst []byte, stride int) error {
	return NewState().Decode(context.Background(), stream, f, dst, stride)
}

// Decode decodes stream into dst, reusing s's buffers when their shape
// still fits. ctx is checked between macroblock rows.
func (s *State) Decode(ctx context.Context, stream []byte, f Format, dst []byte, stride int) error {
	h := &s.hdr
	if err := h.Parse(stream); err != nil {
		return err
	}
	if h.param.cSubVersion == codecSubversion || (h.param.bAlphaChannel && !f.Alpha) || h.scp.sbSubband == sbIsolated {
		return ErrUnsupported
	}
	if h.ii.bdBitDepth != f.BitDepth {
		return ErrUnsupported
	}

	// Keep the previous decode's buffers; everything else starts fresh.
	sc := &s.sc
	mbBack, mbBuf, predInfo, predInfoPrev := sc.mbBack, sc.mbBuf, sc.predInfo, sc.predInfoPrev
	tiles, contexts, bitIO := sc.tiles, sc.contexts, sc.bitIO
	indexTable, pResU, pResV := sc.indexTable, sc.pResU, sc.pResV
	pOffsetX, pOffsetY := sc.dp.pOffsetX, sc.dp.pOffsetY
	*sc = strCodec{stream: stream, out: dst, outStride: stride}
	sc.mbBack, sc.mbBuf, sc.predInfo, sc.predInfoPrev = mbBack, mbBuf, predInfo, predInfoPrev
	sc.tiles, sc.contexts, sc.bitIO = tiles, contexts, bitIO
	sc.indexTable, sc.pResU, sc.pResV = indexTable, pResU, pResV
	sc.dp.pOffsetX, sc.dp.pOffsetY = pOffsetX, pOffsetY

	sc.param = h.param
	sc.wmiscp = h.scp
	sc.wmii = h.ii

	// WMPhotoValidate: output colour format.
	ext := f.ColorFormat
	internal := sc.param.cfColorFormat
	if internal == NComponent {
		ext = NComponent
	}
	if internal == CMYK && ext != YOnly && ext != CFRGB {
		ext = CMYK
	}
	if h.ii.cfColorFormat == CFRGB && ext != YOnly && ext != NComponent {
		ext = CFRGB
	}
	if h.ii.cfColorFormat == CFRGBE {
		if f.ColorFormat != CFRGBE { // writer needs 32-bit pixels
			return ErrUnsupported
		}
		ext = CFRGBE
	}
	// jxrlib writes past the pixel here.
	if ext == NComponent && sc.param.cNumChannels*cbChannels[f.BitDepth]*8 > f.BitsPerUnit ||
		ext == CMYK && 4*cbChannels[f.BitDepth]*8 > f.BitsPerUnit {
		return ErrUnsupported
	}
	if f.Plane > 0 && (ext != NComponent || f.BitDepth != BD16 || f.Alpha || f.Plane > sc.param.cNumChannels) {
		return ErrUnsupported
	}
	sc.plane = f.Plane
	sc.wmii.cfColorFormat = ext
	sc.wmii.bRGB = f.RGB
	sc.wmii.cBitsPerUnit = f.BitsPerUnit
	sc.wmii.oOrientation = 0
	sc.wmii.cROILeftX, sc.wmii.cROITopY = 0, 0
	sc.wmii.cROIWidth, sc.wmii.cROIHeight = h.Width, h.Height

	// ImageStrDecInit: padded size.
	sc.wmii.cWidth += sc.param.cExtraPixelsLeft + sc.param.cExtraPixelsRight
	sc.wmii.cHeight += sc.param.cExtraPixelsTop + sc.param.cExtraPixelsBottom
	sc.cmbWidth = (sc.wmii.cWidth + 15) / 16
	sc.cmbHeight = (sc.wmii.cHeight + 15) / 16
	// jxrlib layout (strdec.c:3320): edge reads cross channels.
	total := 2 * mbPad
	for i := 0; i < sc.param.cNumChannels; i++ {
		total += 2 * sc.mbStride(i) * sc.cmbWidth
	}
	sc.mbBack = reuse(sc.mbBack, total)
	clear(sc.mbBack[:mbPad])
	off := mbPad
	for i := 0; i < sc.param.cNumChannels; i++ {
		n := sc.mbStride(i) * sc.cmbWidth
		sc.mbBuf[i] = sc.mbBack // absolute offsets reach neighbours
		sc.a0Off[i], sc.a1Off[i] = off, off+n
		off += 2 * n
	}
	clear(sc.mbBack[off:])

	// StrIODecInit
	if err := sc.allocateBitIOInfo(); err != nil {
		return err
	}
	sc.ioHeader.attach(stream, uint64(h.size))
	if err := sc.readIndexTable(); err != nil {
		return err
	}

	if h.param.bAlphaChannel {
		if err := sc.setUpAlpha(&s.asc, h); err != nil {
			return err
		}
	}

	if err := sc.strDecInit(); err != nil {
		return err
	}
	if sc.alpha != nil {
		sc.alpha.bitIO, sc.alpha.cSB = sc.bitIO, sc.cSB
		if err := sc.alpha.strDecInit(); err != nil {
			return err
		}
	}

	// ImageStrDecDecode
	sc.setROI()
	if sc.alpha != nil {
		sc.alpha.setROI()
	}
	cMBRow := sc.cmbHeight
	if !sc.dp.bDecodeFullFrame {
		cMBRow = (sc.dp.cROIBottomY + 16) >> 4
	}
	sc.initLookupTables()

	nch := sc.param.cNumChannels
	for sc.cRow = 0; sc.cRow <= cMBRow; sc.cRow++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		for i := 0; i < nch; i++ {
			clear(sc.mbBuf[i][sc.a1Off[i] : sc.a1Off[i]+sc.mbStride(i)*sc.cmbWidth])
		}
		if sc.alpha != nil {
			clear(sc.alpha.mbBuf[0][sc.alpha.a1Off[0] : sc.alpha.a1Off[0]+256*sc.alpha.cmbWidth])
		}
		for sc.cColumn = 0; sc.cColumn <= sc.cmbWidth; sc.cColumn++ {
			if err := sc.processMacroblockDec(); err != nil {
				return err
			}
		}
		if sc.cRow != 0 && (sc.dp.bDecodeFullFrame ||
			(sc.cRow*16 > sc.dp.cROITopY && sc.cRow*16 <= sc.dp.cROIBottomY+16)) {
			if err := sc.outputMBRow(); err != nil {
				return err
			}
		}
		for i := 0; i < nch; i++ {
			sc.predInfo[i], sc.predInfoPrev[i] = sc.predInfoPrev[i], sc.predInfo[i]
			sc.a0Off[i], sc.a1Off[i] = sc.a1Off[i], sc.a0Off[i]
		}
		if sc.alpha != nil {
			sc.alpha.predInfo[0], sc.alpha.predInfoPrev[0] = sc.alpha.predInfoPrev[0], sc.alpha.predInfo[0]
			sc.alpha.a0Off[0], sc.alpha.a1Off[0] = sc.alpha.a1Off[0], sc.alpha.a0Off[0]
		}
	}
	return nil
}

// strDecInit is StrDecInit: per-tile quantizers, coding contexts and
// bit-reader pointers. sc.wmii.cfColorFormat must already hold the
// validated external colour format.
func (sc *strCodec) strDecInit() error {
	cfInt := sc.param.cfColorFormat
	ext := sc.wmii.cfColorFormat
	sc.uvResolutionChange = ext != YOnly && ((cfInt == YUV420 && ext != YUV420) || (cfInt == YUV422 && ext != YUV422))
	if sc.uvResolutionChange {
		sc.pResU = reuse(sc.pResU, 256*sc.cmbWidth)
		sc.pResV = reuse(sc.pResV, 256*sc.cmbWidth)
	}
	sc.allocatePredInfo()
	if sc.wmiscp.cNumOfSliceMinus1V >= maxTiles {
		return ErrFormat
	}
	sc.tiles = reuse(sc.tiles, int(sc.wmiscp.cNumOfSliceMinus1V)+1)
	for i := range sc.tiles {
		// Reset to a fresh tile's zero value; only qDCAll etc. carry over.
		sc.tiles[i].cNumQPLP, sc.tiles[i].cNumQPHP = 1, 1
		sc.tiles[i].cBitsLP, sc.tiles[i].cBitsHP = 0, 0
		sc.tiles[i].bUseDC, sc.tiles[i].bUseLP = false, false
	}
	nch := sc.param.cNumChannels
	if sc.param.uQPMode&1 == 0 {
		allocateQuantizer(&sc.tiles[0].qDCAll, &sc.tiles[0].qDC, nch, 1)
		sc.setUniformQuantizer(0)
		for i := 0; i < nch; i++ {
			sc.tiles[0].qDC[i][0].iIndex = sc.param.uiQPIndexDC[i]
		}
		formatQuantizer(&sc.tiles[0].qDC, uint8((sc.param.uQPMode>>3)&3), nch, 0, true, sc.param.bScaledArith)
	}
	if sc.wmiscp.sbSubband != sbDCOnly {
		if sc.param.uQPMode&2 == 0 {
			allocateQuantizer(&sc.tiles[0].qLPAll, &sc.tiles[0].qLP, nch, 1)
			sc.setUniformQuantizer(1)
			if sc.param.uQPMode&0x200 == 0 {
				sc.useDCQuantizer(0)
			} else {
				for i := 0; i < nch; i++ {
					sc.tiles[0].qLP[i][0].iIndex = sc.param.uiQPIndexLP[i]
				}
				formatQuantizer(&sc.tiles[0].qLP, uint8((sc.param.uQPMode>>5)&3), nch, 0, true, sc.param.bScaledArith)
			}
		}
		if sc.wmiscp.sbSubband != sbNoHighpass && sc.param.uQPMode&4 == 0 {
			allocateQuantizer(&sc.tiles[0].qHPAll, &sc.tiles[0].qHP, nch, 1)
			sc.setUniformQuantizer(2)
			if sc.param.uQPMode&0x400 == 0 {
				sc.useLPQuantizer(1, 0)
			} else {
				for i := 0; i < nch; i++ {
					sc.tiles[0].qHP[i][0].iIndex = sc.param.uiQPIndexHP[i]
				}
				formatQuantizer(&sc.tiles[0].qHP, uint8((sc.param.uQPMode>>7)&3), nch, 0, false, sc.param.bScaledArith)
			}
		}
	}
	if err := sc.allocateCodingContextDec(int(sc.wmiscp.cNumOfSliceMinus1V) + 1); err != nil {
		return err
	}
	sc.setBitIOPointers()
	return nil
}

// setUpAlpha configures asc as sc's interleaved alpha companion
// (strdec.c:3330-3390): a Y_ONLY, 1-channel plane sharing sc's tile
// grid, decoded in lockstep and composited by outputMBRow/outputAlpha.
func (sc *strCodec) setUpAlpha(asc *strCodec, h *Header) error {
	mbBuf, predInfo, predInfoPrev := asc.mbBuf, asc.predInfo, asc.predInfoPrev
	tiles, contexts := asc.tiles, asc.contexts
	*asc = strCodec{stream: sc.stream, secondary: true, primary: sc}
	asc.mbBuf, asc.predInfo, asc.predInfoPrev = mbBuf, predInfo, predInfoPrev
	asc.tiles, asc.contexts = tiles, contexts
	asc.param = h.aParam
	asc.wmiscp = h.aScp
	asc.wmii = sc.wmii
	asc.wmii.cfColorFormat = YOnly
	asc.cmbWidth, asc.cmbHeight = sc.cmbWidth, sc.cmbHeight
	n := 256 * asc.cmbWidth
	asc.mbBuf[0] = reuse(asc.mbBuf[0], 2*n+2*mbPad)
	clear(asc.mbBuf[0][:mbPad])
	clear(asc.mbBuf[0][mbPad+2*n:])
	asc.a0Off[0], asc.a1Off[0] = mbPad, mbPad+n
	if asc.wmiscp.cNumOfSliceMinus1V >= maxTiles {
		return ErrFormat
	}
	sc.alpha = asc
	return nil
}

// Estimate is an upper bound on the heap a decode of h allocates,
// following the buffer sizes Decode reuses: macroblock rows, the
// per-tile-column contexts and bit readers, and the index table,
// which the stream length bounds.
func (h *Header) Estimate(streamLen int) int64 {
	const ctxBytes = 16 << 10 // codingContext plus its Huffman tables
	p := &h.param
	w := int64(h.ii.cWidth + p.cExtraPixelsLeft + p.cExtraPixelsRight)
	mbw := (w+15)/16 + 1
	ch := int64(p.cNumChannels)
	if p.bAlphaChannel {
		ch++
	}
	cols := int64(h.scp.cNumOfSliceMinus1V) + 1
	rows := ch*mbw*(2*256*4+2*int64(unsafe.Sizeof(predInfo{}))) + 2*256*4*mbw
	offsets := 8 * (w + int64(h.ii.cHeight+p.cExtraPixelsTop+p.cExtraPixelsBottom) + 64)
	tiles := cols * (ctxBytes + 4*int64(unsafe.Sizeof(bitIO{})+8) + int64(unsafe.Sizeof(wmiTile{})))
	if p.bAlphaChannel {
		tiles += cols * (ctxBytes + int64(unsafe.Sizeof(wmiTile{})))
	}
	return rows + offsets + tiles + 8*int64(streamLen) + ctxBytes
}
