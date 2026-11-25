// Ported from jxrlib strcodec.c/.h, windowsmediaphoto.h (BSD-2, see NOTICE).

// Package codec decodes a JPEG XR codestream. It is a port of jxrlib's
// decode path; names follow the C source.
package codec

import (
	"encoding/binary"
	"errors"
)

// Errors returned by the decoder.
var (
	ErrFormat      = errors.New("jpegxr: bad codestream")
	ErrUnsupported = errors.New("jpegxr: unsupported codestream feature")
)

// Colour formats (COLORFORMAT).
const (
	YOnly      = 0
	YUV420     = 1
	YUV422     = 2
	YUV444     = 3
	CMYK       = 4
	NComponent = 6
	CFRGB      = 7
	CFRGBE     = 8
)

// Bit depths (BITDEPTH_BITS).
const (
	BD1    = 0
	BD8    = 1
	BD16   = 2
	BD16S  = 3
	BD16F  = 4
	BD32   = 5
	BD32S  = 6
	BD32F  = 7
	BD5    = 8
	BD10   = 9
	BD565  = 10
	BD1alt = 0xf
)

const (
	olNone = 0
	olOne  = 1
	olTwo  = 2

	spatial   = 0
	frequency = 1

	sbAll        = 0
	sbNoFlexbits = 1
	sbNoHighpass = 2
	sbDCOnly     = 3
	sbIsolated   = 4

	maxChannels = 16
	logMaxTiles = 12
	maxTiles    = 1 << logMaxTiles

	codecSubversion         = 0
	codecSubversionSoftTile = 1
	codecSubversionHardTile = 9

	contextX     = 8
	ctDC         = 5
	numVLCTables = 21

	shiftZero  = 1
	qpFracBits = 2

	bandDC = 1
	bandLP = 2
	bandAC = 3
)

var blkOffset = [16]int{0, 64, 16, 80, 128, 192, 144, 208, 32, 96, 48, 112, 160, 224, 176, 240}
var blkOffsetUV = [4]int{0, 32, 16, 48}
var blkOffsetUV422 = [8]int{0, 64, 16, 80, 32, 96, 48, 112}

var dctIndex = [3][16]int{
	{0, 5, 1, 6, 10, 12, 8, 14, 2, 4, 3, 7, 9, 13, 11, 15},
	{0, 5, 1, 6, 10, 12, 8, 14, 2, 4, 3, 7, 9, 13, 11, 15},
	{0, 128, 64, 208, 32, 240, 48, 224, 16, 192, 80, 144, 112, 176, 96, 160},
}

var idxCC = [16][16]uint8{
	{0x00, 0x01, 0x05, 0x04, 0x40, 0x41, 0x45, 0x44, 0x80, 0x81, 0x85, 0x84, 0xc0, 0xc1, 0xc5, 0xc4},
	{0x02, 0x03, 0x07, 0x06, 0x42, 0x43, 0x47, 0x46, 0x82, 0x83, 0x87, 0x86, 0xc2, 0xc3, 0xc7, 0xc6},
	{0x0a, 0x0b, 0x0f, 0x0e, 0x4a, 0x4b, 0x4f, 0x4e, 0x8a, 0x8b, 0x8f, 0x8e, 0xca, 0xcb, 0xcf, 0xce},
	{0x08, 0x09, 0x0d, 0x0c, 0x48, 0x49, 0x4d, 0x4c, 0x88, 0x89, 0x8d, 0x8c, 0xc8, 0xc9, 0xcd, 0xcc},
	{0x10, 0x11, 0x15, 0x14, 0x50, 0x51, 0x55, 0x54, 0x90, 0x91, 0x95, 0x94, 0xd0, 0xd1, 0xd5, 0xd4},
	{0x12, 0x13, 0x17, 0x16, 0x52, 0x53, 0x57, 0x56, 0x92, 0x93, 0x97, 0x96, 0xd2, 0xd3, 0xd7, 0xd6},
	{0x1a, 0x1b, 0x1f, 0x1e, 0x5a, 0x5b, 0x5f, 0x5e, 0x9a, 0x9b, 0x9f, 0x9e, 0xda, 0xdb, 0xdf, 0xde},
	{0x18, 0x19, 0x1d, 0x1c, 0x58, 0x59, 0x5d, 0x5c, 0x98, 0x99, 0x9d, 0x9c, 0xd8, 0xd9, 0xdd, 0xdc},
	{0x20, 0x21, 0x25, 0x24, 0x60, 0x61, 0x65, 0x64, 0xa0, 0xa1, 0xa5, 0xa4, 0xe0, 0xe1, 0xe5, 0xe4},
	{0x22, 0x23, 0x27, 0x26, 0x62, 0x63, 0x67, 0x66, 0xa2, 0xa3, 0xa7, 0xa6, 0xe2, 0xe3, 0xe7, 0xe6},
	{0x2a, 0x2b, 0x2f, 0x2e, 0x6a, 0x6b, 0x6f, 0x6e, 0xaa, 0xab, 0xaf, 0xae, 0xea, 0xeb, 0xef, 0xee},
	{0x28, 0x29, 0x2d, 0x2c, 0x68, 0x69, 0x6d, 0x6c, 0xa8, 0xa9, 0xad, 0xac, 0xe8, 0xe9, 0xed, 0xec},
	{0x30, 0x31, 0x35, 0x34, 0x70, 0x71, 0x75, 0x74, 0xb0, 0xb1, 0xb5, 0xb4, 0xf0, 0xf1, 0xf5, 0xf4},
	{0x32, 0x33, 0x37, 0x36, 0x72, 0x73, 0x77, 0x76, 0xb2, 0xb3, 0xb7, 0xb6, 0xf2, 0xf3, 0xf7, 0xf6},
	{0x3a, 0x3b, 0x3f, 0x3e, 0x7a, 0x7b, 0x7f, 0x7e, 0xba, 0xbb, 0xbf, 0xbe, 0xfa, 0xfb, 0xff, 0xfe},
	{0x38, 0x39, 0x3d, 0x3c, 0x78, 0x79, 0x7d, 0x7c, 0xb8, 0xb9, 0xbd, 0xbc, 0xf8, 0xf9, 0xfd, 0xfc},
}

var idxCC420 = [8][8]uint8{
	{0x00, 0x01, 0x05, 0x04, 0x20, 0x21, 0x25, 0x24},
	{0x02, 0x03, 0x07, 0x06, 0x22, 0x23, 0x27, 0x26},
	{0x0a, 0x0b, 0x0f, 0x0e, 0x2a, 0x2b, 0x2f, 0x2e},
	{0x08, 0x09, 0x0d, 0x0c, 0x28, 0x29, 0x2d, 0x2c},
	{0x10, 0x11, 0x15, 0x14, 0x30, 0x31, 0x35, 0x34},
	{0x12, 0x13, 0x17, 0x16, 0x32, 0x33, 0x37, 0x36},
	{0x1a, 0x1b, 0x1f, 0x1e, 0x3a, 0x3b, 0x3f, 0x3e},
	{0x18, 0x19, 0x1d, 0x1c, 0x38, 0x39, 0x3d, 0x3c},
}

var cblkChromas = [9]int{0, 4, 8, 16, 16, 16, 16, 0, 0}

//================================================================
// Bit I/O
//================================================================

// bitIO reads MSB first from buf. Reads past the end yield zero bits.
type bitIO struct {
	buf []byte
	acc uint64 // unread bits, MSB aligned
	nb  uint32 // valid bits in acc
	off uint64 // next byte to load into acc
	ok  bool   // attached
}

func (b *bitIO) attach(buf []byte, byteOff uint64) {
	b.buf = buf
	b.acc, b.nb, b.off = 0, 0, byteOff
	b.ok = true
}

// refill tops acc up to at least 56 bits.
func (b *bitIO) refill() {
	var v uint64
	if b.off+8 <= uint64(len(b.buf)) {
		v = binary.BigEndian.Uint64(b.buf[b.off:])
	} else {
		for i := uint64(0); i < 8; i++ {
			v <<= 8
			if b.off+i < uint64(len(b.buf)) {
				v |= uint64(b.buf[b.off+i])
			}
		}
	}
	b.acc |= v >> (b.nb & 63)
	b.off += uint64(63-b.nb) >> 3
	b.nb |= 56
}

// fill is refill for callers holding acc and nb in locals.
func (b *bitIO) fill(acc uint64, nb uint32) (uint64, uint32) {
	b.acc, b.nb = acc, nb
	b.refill()
	return b.acc, b.nb
}

// peekBit16 returns the next n (1..32) bits.
func (b *bitIO) peekBit16(n uint32) uint32 {
	if b.nb < n {
		b.refill()
	}
	return uint32(b.acc >> ((64 - n) & 63))
}

func (b *bitIO) flushBit16(n uint32) {
	b.acc <<= n & 63
	b.nb -= n
}

func (b *bitIO) getBit16(n uint32) uint32 {
	if n == 0 {
		return 0
	}
	v := b.peekBit16(n)
	b.flushBit16(n)
	return v
}

func (b *bitIO) getBit32(n uint32) uint32 { return b.getBit16(n) }

func (b *bitIO) getBool16() uint32 { return b.getBit16(1) }

// getSign returns 0 or -1.
func (b *bitIO) getSign() int32 { return -int32(b.getBit16(1)) }

// getBit16s is _getBit16s: a signed value, sign bit only if nonzero.
func (b *bitIO) getBit16s(n uint32) int32 {
	r := int32(b.peekBit16(n + 1))
	r = ((r >> 1) ^ (-(r & 1))) + (r & 1)
	adv := n
	if r != 0 { // compiles to CSINC, avoids mispredicts
		adv++
	}
	b.flushBit16(adv)
	return r
}

func (b *bitIO) flushToByte() { b.flushBit16(b.nb & 7) }

// bytePos is getPosRead.
func (b *bitIO) bytePos() uint64 { return b.off - uint64((b.nb+7)>>3) }

// simpleBitIO is jxrlib's SimpleBitIO, used for the image headers.
type simpleBitIO struct {
	buf    []byte
	cbRead int
	acc    uint8
	left   uint32
	err    bool
}

func (s *simpleBitIO) getBit32(cBits uint32) uint32 {
	var rc uint32
	for s.left < cBits {
		rc <<= s.left
		rc |= uint32(s.acc) >> (8 - s.left)
		cBits -= s.left
		if s.cbRead < len(s.buf) {
			s.acc = s.buf[s.cbRead]
		} else {
			s.err = true
			s.acc = 0
		}
		s.cbRead++
		s.left = 8
	}
	rc <<= cBits
	rc |= uint32(s.acc) >> (8 - cBits)
	s.acc <<= cBits
	s.left -= cBits
	return rc
}

func (s *simpleBitIO) flushToByte() { s.acc, s.left = 0, 0 }

//================================================================
// Structures
//================================================================

type quantizer struct {
	iIndex  uint8
	iQP     int32
	iOffset int32
	iMan    int32
	iExp    int32
}

type mbInfo struct {
	iBlockDC     [maxChannels][16]int32
	iOrientation int32
	iCBP         [maxChannels]int32
	iDiffCBP     [maxChannels]int32
	iQIndexLP    uint8
	iQIndexHP    uint8
}

type adaptiveScan struct {
	uTotal uint32
	uScan  uint32
}

type adaptiveModel struct {
	flcState [2]int32
	flcBits  [2]int32
	band     int32
}

type cbpModel struct {
	count0 [2]int32
	count1 [2]int32
	state  [2]int32
}

type codingContext struct {
	ioDC, ioLP, ioAC, ioFL *bitIO

	ahCBPCY  adaptiveHuffman
	ahCBPCY1 adaptiveHuffman
	ahExpt   [numVLCTables]adaptiveHuffman

	scanLowpass [16]adaptiveScan
	scanHoriz   [16]adaptiveScan
	scanVert    [16]adaptiveScan

	modelAC, modelLP, modelDC adaptiveModel

	cbpCountZero int32
	cbpCountMax  int32

	cbpModel cbpModel

	trimFlexBits int32
	inROI        bool
}

type predInfo struct {
	iQPIndex int32
	iCBP     int32
	iDC      int32
	iAD      [6]int32
}

type coreParameters struct {
	cVersion               uint32
	cSubVersion            uint32
	cfColorFormat          int
	bRBSwapped             bool
	bAlphaChannel          bool
	bScaledArith           bool
	bIndexTable            bool
	bTrimFlexbitsFlag      bool
	bUseHardTileBoundaries bool
	cNumChannels           int
	cExtraPixelsTop        int
	cExtraPixelsLeft       int
	cExtraPixelsBottom     int
	cExtraPixelsRight      int
	uQPMode                uint32
	uiQPIndexDC            [maxChannels]uint8
	uiQPIndexLP            [maxChannels]uint8
	uiQPIndexHP            [maxChannels]uint8
}

type wmiTile struct {
	qDC, qLP, qHP          [maxChannels][]quantizer
	qDCAll, qLPAll, qHPAll []quantizer // backing arrays, reused across decodes
	cNumQPLP               uint8
	cNumQPHP               uint8
	cBitsLP                uint8
	cBitsHP                uint8
	bUseDC                 bool
	bUseLP                 bool
	cChModeDC              uint8
	cChModeLP              [16]uint8
	cChModeHP              [16]uint8
}

// imageInfo is CWMImageInfo, reduced to what decoding uses.
type imageInfo struct {
	cWidth, cHeight   int
	cfColorFormat     int
	bdBitDepth        int
	cBitsPerUnit      int
	bRGB              bool
	cChromaCenteringX uint8
	cChromaCenteringY uint8
	cROILeftX         int
	cROIWidth         int
	cROITopY          int
	cROIHeight        int
	oOrientation      int
}

// codecParam is CWMIStrCodecParam, reduced.
type codecParam struct {
	cfColorFormat          int
	olOverlap              int
	bfBitstreamFormat      int
	cChannel               int
	sbSubband              int
	cNumOfSliceMinus1V     uint32
	uiTileX                []uint32 // len cNumOfSliceMinus1V+1
	cNumOfSliceMinus1H     uint32
	uiTileY                []uint32 // len cNumOfSliceMinus1H+1
	nLenMantissaOrShift    uint8
	nExpBias               int8
	bBlackWhite            bool
	bUseHardTileBoundaries bool
}

type decoderParameters struct {
	bDecodeFullFrame bool
	bDecodeFullWidth bool
	bSkipFlexbits    bool
	bDecodeHP        bool
	bDecodeLP        bool
	cROILeftX        int
	cROIRightX       int
	cROITopY         int
	cROIBottomY      int
	pOffsetX         []int
	pOffsetY         []int
}

const mbPad = 256 // jxrlib reads outside its malloc here

// strCodec is CWMImageStrCodec.
type strCodec struct {
	wmii   imageInfo
	wmiscp codecParam
	mb     mbInfo
	param  coreParameters
	dp     decoderParameters

	cSB                 int
	uvResolutionChange  bool
	plane               int // Format.Plane
	pResU, pResV        []int32
	indexTable          []uint64
	cTileRow            uint32
	cTileColumn         uint32
	ctxLeft, ctxTop     bool
	resetRGITotals      bool
	resetContext        bool
	tiles               []wmiTile
	bitIO               []*bitIO
	ioHeader            bitIO
	cHeaderSize         uint64
	contexts            []codingContext
	cRow, cColumn       int
	cmbWidth, cmbHeight int

	// hard tile state kept across macroblocks
	mbX, mbY                           int
	tileX, tileY                       uint32
	vertTileBoundary, horiTileBoundary bool
	oneMBLeftVertTB, oneMBRightVertTB  bool
	iPredBefore, iPredAfter            [2][2]int32

	mbBack       []int32              // mbPad, rows, mbPad
	mbBuf        [maxChannels][]int32 // two MB rows per channel
	a0Off, a1Off [maxChannels]int     // previous and current row
	predInfo     [maxChannels][]predInfo
	predInfoPrev [maxChannels][]predInfo
	stream       []byte
	out          []byte

	// Interleaved alpha (m_pNextSC): alpha is the Y_ONLY companion plane
	// decoded in lockstep with this one; secondary marks alpha itself,
	// whose empty-bitIO fallback (setBitIOPointers) reads primary's ioHeader.
	alpha     *strCodec
	primary   *strCodec
	secondary bool
	outStride int
	err       error
}

//================================================================
// Tile position and quantizers
//================================================================

func (sc *strCodec) getTilePos(mbX, mbY int) {
	p := &sc.wmiscp
	if mbX == 0 {
		sc.cTileColumn = 0
	} else if sc.cTileColumn < p.cNumOfSliceMinus1V && uint32(mbX) == p.uiTileX[sc.cTileColumn+1] {
		sc.cTileColumn++
	}
	if mbY == 0 {
		sc.cTileRow = 0
	} else if sc.cTileRow < p.cNumOfSliceMinus1H && uint32(mbY) == p.uiTileY[sc.cTileRow+1] {
		sc.cTileRow++
	}
	sc.ctxLeft = uint32(mbX) == p.uiTileX[sc.cTileColumn]
	sc.ctxTop = uint32(mbY) == p.uiTileY[sc.cTileRow]
	sc.resetContext = ((uint32(mbX) - p.uiTileX[sc.cTileColumn]) & 0xf) == 0
	sc.resetRGITotals = sc.resetContext
	if sc.cTileColumn == p.cNumOfSliceMinus1V {
		if mbX+1 == sc.cmbWidth {
			sc.resetContext = true
		}
	} else if uint32(mbX+1) == p.uiTileX[sc.cTileColumn+1] {
		sc.resetContext = true
	}
}

// allocateQuantizer fills q from all, reusing all's backing array so a
// warm decode of the same shape makes no allocation.
func allocateQuantizer(all *[]quantizer, q *[maxChannels][]quantizer, cChannel, cQP int) {
	*all = reuse(*all, cQP*cChannel)
	for i := 0; i < cChannel; i++ {
		q[i] = (*all)[i*cQP : (i+1)*cQP : (i+1)*cQP]
	}
}

// reuse resizes buf to n, reusing its backing array when it already has
// the capacity, so a warm decode of the same shape makes no allocation.
func reuse[T any](buf []T, n int) []T {
	if cap(buf) >= n {
		return buf[:n]
	}
	return make([]T, n)
}

func formatQuantizer(q *[maxChannels][]quantizer, cChMode uint8, cCh, iPos int, bShiftedUV, bScaledArith bool) {
	for iCh := 0; iCh < cCh; iCh++ {
		if iCh > 0 {
			switch cChMode {
			case 0:
				q[iCh][iPos] = q[0][iPos]
			case 1:
				q[iCh][iPos] = q[1][iPos]
			}
		}
		shift := int32(shiftZero)
		if iCh > 0 && bShiftedUV {
			shift = shiftZero - 1
		}
		remapQP(&q[iCh][iPos], shift, bScaledArith)
	}
}

func (sc *strCodec) setUniformQuantizer(sb int) {
	for iCh := 0; iCh < sc.param.cNumChannels; iCh++ {
		for iTile := 1; iTile <= int(sc.wmiscp.cNumOfSliceMinus1V); iTile++ {
			switch sb {
			case 0:
				sc.tiles[iTile].qDC[iCh] = sc.tiles[0].qDC[iCh]
			case 1:
				sc.tiles[iTile].qLP[iCh] = sc.tiles[0].qLP[iCh]
			default:
				sc.tiles[iTile].qHP[iCh] = sc.tiles[0].qHP[iCh]
			}
		}
	}
}

func (sc *strCodec) useDCQuantizer(iTile int) {
	for iCh := 0; iCh < sc.param.cNumChannels; iCh++ {
		sc.tiles[iTile].qLP[iCh][0] = sc.tiles[iTile].qDC[iCh][0]
	}
}

func (sc *strCodec) useLPQuantizer(cQP, iTile int) {
	for iCh := 0; iCh < sc.param.cNumChannels; iCh++ {
		for iQP := 0; iQP < cQP; iQP++ {
			sc.tiles[iTile].qHP[iCh][iQP] = sc.tiles[iTile].qLP[iCh][iQP]
		}
	}
}

func dquantBits(cQP uint8) uint8 {
	switch {
	case cQP < 2:
		return 0
	case cQP < 4:
		return 1
	case cQP < 6:
		return 2
	case cQP < 10:
		return 3
	}
	return 4
}

func (sc *strCodec) allocateBitIOInfo() error {
	switch sc.wmiscp.sbSubband {
	case sbDCOnly:
		sc.cSB = 1
	case sbNoHighpass:
		sc.cSB = 2
	case sbNoFlexbits:
		sc.cSB = 3
	default:
		sc.cSB = 4
	}
	var n int
	if !sc.param.bIndexTable {
		n = 0
	} else if sc.wmiscp.bfBitstreamFormat == spatial {
		n = int(sc.wmiscp.cNumOfSliceMinus1V) + 1
	} else {
		n = (int(sc.wmiscp.cNumOfSliceMinus1V) + 1) * sc.cSB
	}
	if n > maxTiles*4 {
		return ErrFormat
	}
	if n > 0 {
		if sc.wmiscp.cNumOfSliceMinus1H >= maxTiles {
			return ErrFormat
		}
		// Each entry takes at least one byte.
		if n*(int(sc.wmiscp.cNumOfSliceMinus1H)+1) > len(sc.stream) {
			return ErrFormat
		}
		sc.bitIO = reuse(sc.bitIO, n)
		for i := range sc.bitIO {
			if sc.bitIO[i] == nil {
				sc.bitIO[i] = &bitIO{}
			}
		}
		sc.indexTable = reuse(sc.indexTable, n*(int(sc.wmiscp.cNumOfSliceMinus1H)+1))
	} else {
		sc.bitIO = sc.bitIO[:0]
		sc.indexTable = sc.indexTable[:0]
	}
	return nil
}

func (sc *strCodec) setBitIOPointers() {
	if len(sc.bitIO) > 0 {
		for i := 0; i <= int(sc.wmiscp.cNumOfSliceMinus1V); i++ {
			c := &sc.contexts[i]
			if sc.wmiscp.bfBitstreamFormat == spatial {
				c.ioDC, c.ioLP, c.ioAC, c.ioFL = sc.bitIO[i], sc.bitIO[i], sc.bitIO[i], sc.bitIO[i]
			} else {
				j := sc.cSB
				c.ioDC = sc.bitIO[i*j]
				if j > 1 {
					c.ioLP = sc.bitIO[i*j+1]
				}
				if j > 2 {
					c.ioAC = sc.bitIO[i*j+2]
				}
				if j > 3 {
					c.ioFL = sc.bitIO[i*j+3]
				}
			}
		}
	} else {
		hdr := &sc.ioHeader
		if sc.primary != nil {
			hdr = &sc.primary.ioHeader
		}
		c := &sc.contexts[0]
		c.ioDC, c.ioLP, c.ioAC, c.ioFL = hdr, hdr, hdr, hdr
	}
}
