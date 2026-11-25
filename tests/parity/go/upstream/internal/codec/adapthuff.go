// Ported from jxrlib adapthuff.c (BSD-2, see NOTICE).

package codec

var g4HuffLookupTable = [hufTableLen]int16{
	19, 19, 19, 19, 27, 27, 27, 27, 10, 10, 10, 10, 10, 10, 10, 10,
	1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
	0, 0, 0, 0, 0, 0, 0, 0}

var g5HuffLookupTable = [2][hufTableLen]int16{{
	28, 28, 36, 36, 19, 19, 19, 19, 10, 10, 10, 10, 10, 10, 10, 10,
	1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, {
	11, 11, 11, 11, 19, 19, 19, 19, 27, 27, 27, 27, 35, 35, 35, 35,
	1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}

var g6HuffLookupTable = [4][hufTableLen]int16{{
	13, 29, 44, 44, 19, 19, 19, 19, 34, 34, 34, 34, 34, 34, 34, 34,
	1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, {
	12, 12, 28, 28, 43, 43, 43, 43, 2, 2, 2, 2, 2, 2, 2, 2,
	18, 18, 18, 18, 18, 18, 18, 18, 34, 34, 34, 34, 34, 34, 34, 34,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, {
	4, 4, 12, 12, 43, 43, 43, 43, 18, 18, 18, 18, 18, 18, 18, 18,
	26, 26, 26, 26, 26, 26, 26, 26, 34, 34, 34, 34, 34, 34, 34, 34,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, {
	5, 13, 36, 36, 43, 43, 43, 43, 18, 18, 18, 18, 18, 18, 18, 18,
	25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}

var g7HuffLookupTable = [2][hufTableLen]int16{{
	45, 53, 36, 36, 27, 27, 27, 27, 2, 2, 2, 2, 2, 2, 2, 2,
	10, 10, 10, 10, 10, 10, 10, 10, 18, 18, 18, 18, 18, 18, 18, 18,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, {
	-32736, 37, 28, 28, 19, 19, 19, 19, 10, 10, 10, 10, 10, 10, 10, 10,
	1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
	5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}

var g8HuffLookupTable = [2][hufTableLen]int16{{
	53, 21, 28, 28, 11, 11, 11, 11, 43, 43, 43, 43, 59, 59, 59, 59,
	2, 2, 2, 2, 2, 2, 2, 2, 34, 34, 34, 34, 34, 34, 34, 34,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, {
	52, 52, 20, 20, 3, 3, 3, 3, 11, 11, 11, 11, 27, 27, 27, 27,
	35, 35, 35, 35, 43, 43, 43, 43, 58, 58, 58, 58, 58, 58, 58, 58,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}

var g9HuffLookupTable = [2][hufTableLen]int16{{
	13, 29, 37, 61, 20, 20, 68, 68, 3, 3, 3, 3, 51, 51, 51, 51,
	41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0}, {
	-32736, 53, 28, 28, 11, 11, 11, 11, 19, 19, 19, 19, 43, 43, 43, 43,
	1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
	-32734, 4, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0}}

var g12HuffLookupTable = [5][hufTableLen]int16{{
	-32736, 5, 76, 76, 37, 53, 69, 85, 43, 43, 43, 43, 91, 91, 91, 91,
	57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57,
	-32734, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0}, {
	-32736, 85, 13, 53, 4, 4, 36, 36, 43, 43, 43, 43, 67, 67, 67, 67,
	75, 75, 75, 75, 91, 91, 91, 91, 58, 58, 58, 58, 58, 58, 58, 58,
	2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0}, {
	-32736, 37, 92, 92, 11, 11, 11, 11, 43, 43, 43, 43, 59, 59, 59, 59,
	67, 67, 67, 67, 75, 75, 75, 75, 2, 2, 2, 2, 2, 2, 2, 2,
	-32734, -32732, 2, 3, 6, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0}, {
	-32736, 29, 37, 69, 3, 3, 3, 3, 43, 43, 43, 43, 59, 59, 59, 59,
	75, 75, 75, 75, 91, 91, 91, 91, 10, 10, 10, 10, 10, 10, 10, 10,
	-32734, 10, 2, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0}, {
	-32736, 93, 28, 28, 60, 60, 76, 76, 3, 3, 3, 3, 43, 43, 43, 43,
	9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
	-32734, -32732, -32730, 2, 4, 8, 6, 10, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0}}

var g5DeltaTable = []int32{0, -1, 0, 1, 1}

var g6DeltaTable = []int32{
	-1, 1, 1, 1, 0, 1,
	-2, 0, 0, 2, 0, 0,
	-1, -1, 0, 1, -2, 0,
}

var g7DeltaTable = []int32{1, 0, -1, -1, -1, -1, -1}

var g9DeltaTable = []int32{2, 2, 1, 1, -1, -2, -2, -2, -3}

var g12DeltaTable = []int32{
	1, 1, 1, 1, 1, 0, 0, -1, 2, 1, 0, 0,
	2, 2, -1, -1, -1, 0, -2, -1, 0, 0, -2, -1,
	-1, 1, 0, 2, 0, 0, 0, 0, -2, 0, 1, 1,
	0, 1, 0, 1, -2, 0, -1, -1, -2, -1, -2, -2,
}

// Tables are padded to one size for copying.
const hufTableLen = 64

// adaptiveHuffman is CAdaptiveHuffman; only the decode tables are kept.
// The tables are copies, so a lookup is one load from the struct.
type adaptiveHuffman struct {
	hufDecTable   [hufTableLen]int16
	delta, delta1 [16]int32
	disc, disc1   int32
	nSymbols      int
	tableIndex    int
	initialize    bool
	upperBound    int32
	lowerBound    int32
}

// setDelta copies the tail of t, as the C pointer sees it.
func setDelta(d *[16]int32, t []int32) {
	*d = [16]int32{}
	copy(d[:], t)
}

const (
	threshold = 8
	memory    = 8
)

var gMaxTables = [13]int{0, 0, 0, 0, 1, 2, 4, 2, 2, 2, 0, 0, 5}
var gSecondDisc = [13]int{0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1}

func (a *adaptiveHuffman) adaptDiscriminant() {
	iSym := a.nSymbols
	change := false
	if !a.initialize {
		a.initialize = true
		a.disc, a.disc1 = 0, 0
		a.tableIndex = gSecondDisc[iSym]
	}
	dL, dH := a.disc, a.disc
	if gSecondDisc[iSym] != 0 {
		dH = a.disc1
	}
	if dL < a.lowerBound {
		a.tableIndex--
		change = true
	} else if dH > a.upperBound {
		a.tableIndex++
		change = true
	}
	if change {
		a.disc, a.disc1 = 0, 0
	}
	if a.disc < -threshold*memory {
		a.disc = -threshold * memory
	} else if a.disc > threshold*memory {
		a.disc = threshold * memory
	}
	if a.disc1 < -threshold*memory {
		a.disc1 = -threshold * memory
	} else if a.disc1 > threshold*memory {
		a.disc1 = threshold * memory
	}

	t := a.tableIndex
	if t == 0 {
		a.lowerBound = -1 << 31
	} else {
		a.lowerBound = -threshold
	}
	if t == gMaxTables[iSym]-1 {
		a.upperBound = 1 << 30
	} else {
		a.upperBound = threshold
	}

	b2i := func(b bool) int {
		if b {
			return 1
		}
		return 0
	}
	switch iSym {
	case 4:
		a.delta = [16]int32{}
		a.hufDecTable = g4HuffLookupTable
	case 5:
		setDelta(&a.delta, g5DeltaTable)
		a.hufDecTable = g5HuffLookupTable[t]
	case 6:
		setDelta(&a.delta1, g6DeltaTable[iSym*(t-b2i(t+1 == gMaxTables[iSym])):])
		setDelta(&a.delta, g6DeltaTable[(t-1+b2i(t == 0))*iSym:])
		a.hufDecTable = g6HuffLookupTable[t]
	case 7:
		setDelta(&a.delta, g7DeltaTable)
		a.hufDecTable = g7HuffLookupTable[t]
	case 8:
		a.delta = [16]int32{}
		a.hufDecTable = g8HuffLookupTable[0]
	case 9:
		setDelta(&a.delta, g9DeltaTable)
		a.hufDecTable = g9HuffLookupTable[t]
	case 12:
		setDelta(&a.delta1, g12DeltaTable[iSym*(t-b2i(t+1 == gMaxTables[iSym])):])
		setDelta(&a.delta, g12DeltaTable[(t-1+b2i(t == 0))*iSym:])
		a.hufDecTable = g12HuffLookupTable[t]
	}
}
