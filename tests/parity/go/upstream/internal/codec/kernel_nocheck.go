//go:build (arm64 || amd64) && !purego && !kernelcheck

package codec

// kernelCheck is true under -tags kernelcheck; see kernel_check.go.
const kernelCheck = false

func idctCheck([]int32, int, int)                          { panic("kernelcheck") }
func postCheck([]int32, int, int, int, int, int, int, int) { panic("kernelcheck") }
func outBGR8Check([]byte, int, []int32, []int32, []int32, int, int, int32, uint32, int, int) {
	panic("kernelcheck")
}
func outGray16Check([]byte, int, []int32, int, int, int32, uint32, uint32) { panic("kernelcheck") }
func flexbitsCheck([]int32, *[16]int, *bitIO, uint32, int32)               { panic("kernelcheck") }
