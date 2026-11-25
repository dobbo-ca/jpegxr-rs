//go:build !purego && kernelcheck

package codec

import (
	"fmt"
	"slices"
)

func highpassCheck(chroma bool, ah *[numVLCTables]adaptiveHuffman, io *bitIO, iQP int32, coef []int32, scan *[16]adaptiveScan) int {
	KernelCalls[5]++
	gah, gio, gc, gs := *ah, *io, slices.Clone(coef[:16]), *scan
	want := decodeBlockHighpassGo(chroma, &gah, &gio, iQP, gc, &gs)
	ah1 := &ah[ctDC+contextX]
	if chroma {
		ah1 = &ah[ctDC+contextX+3]
	}
	n := highpassAsm(ah, ah1, io, &coef[0], scan, iQP)
	if i := firstDiff32(coef[:16], gc); i >= 0 || n != want || *ah != gah || *scan != gs ||
		io.acc != gio.acc || io.nb != gio.nb || io.off != gio.off {
		kernelPanic(fmt.Sprintf("highpass chroma=%v qp=%d", chroma, iQP),
			fmt.Sprint(n, coef[:16], *scan, io.acc, io.nb, io.off), fmt.Sprint(want, gc, gs, gio.acc, gio.nb, gio.off))
	}
	return n
}
