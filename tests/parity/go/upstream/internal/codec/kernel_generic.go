//go:build !(arm64 || amd64) || purego

package codec

func strIDCT4x4Stage1Pairs(p []int32, o, n int) { strIDCT4x4Stage1PairsGo(p, o, n) }

func strPost4x4Stage1SplitAlternate(p []int32, p0, p1, iOffset int) {
	strPost4x4Stage1SplitAlternateGo(p, p0, p1, iOffset)
}

func outBGR8(out []byte, d int, y, u, v []int32, s, n int, bias int32, shift uint32, iR, iB int) {
	outBGR8Go(out, d, y, u, v, s, n, bias, shift, iR, iB)
}

func outGray16(out []byte, d int, y []int32, s, n int, bias int32, shift, nLen uint32) {
	outGray16Go(out, d, y, s, n, bias, shift, nLen)
}

func flexbits(coef []int32, order *[16]int, io *bitIO, f uint32, q int32) bool { return false }

func decodeBlockHighpass(chroma bool, ah *[numVLCTables]adaptiveHuffman, io *bitIO, iQP int32, coef []int32, scan *[16]adaptiveScan) int {
	return decodeBlockHighpassGo(chroma, ah, io, iQP, coef, scan)
}
