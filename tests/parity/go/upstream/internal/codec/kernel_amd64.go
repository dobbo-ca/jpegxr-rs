//go:build !purego

package codec

// Eight blocks per AVX2 pass.
const asmPairs = 4

func cpuid(eaxArg, ecxArg uint32) (eax, ebx, ecx, edx uint32)

func xgetbv() (eax uint32)

// cpuSIMD reports AVX2 with OS-saved YMM state.
func cpuSIMD() bool {
	maxID, _, _, _ := cpuid(0, 0)
	if maxID < 7 {
		return false
	}
	_, _, ecx1, _ := cpuid(1, 0)
	const osxsave, avx = 1 << 27, 1 << 28
	if ecx1&osxsave == 0 || ecx1&avx == 0 || xgetbv()&6 != 6 {
		return false
	}
	_, ebx7, _, _ := cpuid(7, 0)
	return ebx7&(1<<5) != 0
}

func decodeBlockHighpass(chroma bool, ah *[numVLCTables]adaptiveHuffman, io *bitIO, iQP int32, coef []int32, scan *[16]adaptiveScan) int {
	return decodeBlockHighpassGo(chroma, ah, io, iQP, coef, scan)
}
