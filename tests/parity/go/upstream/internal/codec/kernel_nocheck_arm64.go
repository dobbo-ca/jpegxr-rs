//go:build !purego && !kernelcheck

package codec

func highpassCheck(bool, *[numVLCTables]adaptiveHuffman, *bitIO, int32, []int32, *[16]adaptiveScan) int {
	panic("kernelcheck")
}
