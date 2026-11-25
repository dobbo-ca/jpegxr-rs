//go:build !purego

package codec

// Four blocks per NEON pass.
const asmPairs = 2

func cpuSIMD() bool { return true }

//go:noescape
func highpassAsm(ah *[numVLCTables]adaptiveHuffman, ah1 *adaptiveHuffman, io *bitIO, coef *int32, scan *[16]adaptiveScan, iQP int32) int

// runTab[t][iMaxRun][peek] is decodeSignificantRun on g5 table t, run
// table ah[0]: bits 0-7 code plus fixed length, 8-15 code length,
// 16-23 runRemap, 24-31 63 minus fixed length.
var runTab = func() (r [2][15][32]uint32) {
	for t := range r {
		for m := 5; m < 15; m++ {
			for p := range 32 {
				sym := uint32(g5HuffLookupTable[t][p])
				ri := sym>>3 + uint32(gSignificantRunBin[m])*5
				flc := gSignificantRunFixedLength[ri]
				r[t][m][p] = sym&7 + flc | sym&7<<8 | uint32(runRemap[ri])<<16 | (63-flc)<<24
			}
		}
	}
	return r
}()

// Only highpassAsm reads runTab.
var _ = &runTab

// 160 bytes cover 17 coefficients of at most 61 bits plus a refill.
func decodeBlockHighpass(chroma bool, ah *[numVLCTables]adaptiveHuffman, io *bitIO, iQP int32, coef []int32, scan *[16]adaptiveScan) int {
	if !useAsm || uint64(len(io.buf)) < io.off+160 || len(coef) < 16 {
		return decodeBlockHighpassGo(chroma, ah, io, iQP, coef, scan)
	}
	if kernelCheck {
		return highpassCheck(chroma, ah, io, iQP, coef, scan)
	}
	ah1 := &ah[ctDC+contextX]
	if chroma {
		ah1 = &ah[ctDC+contextX+3]
	}
	return highpassAsm(ah, ah1, io, &coef[0], scan, iQP)
}
