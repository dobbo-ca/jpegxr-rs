//go:build (arm64 || amd64) && !purego && kernelcheck

package codec

import (
	"fmt"
	"slices"
)

// Under -tags kernelcheck every kernel call also runs the Go body on a
// copy and panics at the first difference.
const kernelCheck = true

// KernelCalls counts checked calls: idct, post, outBGR8, outGray16, flexbits, highpass.
var KernelCalls [6]int

func kernelPanic(name string, got, want any) {
	panic(fmt.Sprintf("kernelcheck: %s: asm %v, Go %v", name, got, want))
}

func idctCheck(p []int32, o, m int) {
	KernelCalls[0]++
	q := slices.Clone(p[o : o+64*(m-1)+32])
	strIDCT4x4Stage1PairsGo(q, 0, m)
	idct4x4Stage1PairsAsm(&p[o], m)
	if i := firstDiff32(p[o:o+len(q)], q); i >= 0 {
		kernelPanic(fmt.Sprintf("idct4x4Stage1Pairs o=%d m=%d index %d", o, m, i), p[o+i], q[i])
	}
}

func postCheck(p []int32, p0, p1, iOffset, a, b, c, d int) {
	KernelCalls[1]++
	// Copy only a..b and c..d; rows can be far apart.
	n := b + 4 - a
	q := append(slices.Clone(p[a:b+4]), p[c:d+4]...)
	strPost4x4Stage1SplitAlternateGo(q, -12, n-4, iOffset)
	post4x4Stage1SplitAsm(&p[a], &p[b], &p[c], &p[d])
	for k, o := range [4][2]int{{a, 0}, {b, b - a}, {c, n}, {d, n + d - c}} {
		if !slices.Equal(p[o[0]:o[0]+4], q[o[1]:o[1]+4]) {
			kernelPanic(fmt.Sprintf("post4x4Stage1Split p0=%d p1=%d off=%d run %d", p0, p1, iOffset, k), p[o[0]:o[0]+4], q[o[1]:o[1]+4])
		}
	}
}

func outBGR8Check(out []byte, d int, y, u, v []int32, s, n int, bias int32, shift uint32, iR, iB int) {
	KernelCalls[2]++
	q := slices.Clone(out[d : d+48*n])
	outBGR8Go(q, 0, y, u, v, s, n, bias, shift, iR, iB)
	outBGR8Asm(&out[d], &y[s], &u[s], &v[s], n, bias, shift, 1-iR/2)
	if !slices.Equal(out[d:d+48*n], q) {
		kernelPanic("outBGR8", out[d:d+48*n], q)
	}
}

func outGray16Check(out []byte, d int, y []int32, s, n int, bias int32, shift, nLen uint32) {
	KernelCalls[3]++
	q := slices.Clone(out[d : d+32*n])
	outGray16Go(q, 0, y, s, n, bias, shift, nLen)
	outGray16Asm(&out[d], &y[s], n, bias, shift, nLen)
	if !slices.Equal(out[d:d+32*n], q) {
		kernelPanic("outGray16", out[d:d+32*n], q)
	}
}

func flexbitsCheck(coef []int32, order *[16]int, io *bitIO, f uint32, q int32) {
	KernelCalls[4]++
	gc, gio := slices.Clone(coef[:16]), *io
	flexbitsGo(gc, order, &gio, f, q)
	acc, nb, adv := flexbitsAsm(&coef[0], order, &io.buf[io.off], io.acc, io.nb, f, q)
	io.acc, io.nb, io.off = acc, nb, io.off+uint64(adv)
	if i := firstDiff32(coef[:16], gc); i >= 0 || io.acc != gio.acc || io.nb != gio.nb || io.off != gio.off {
		kernelPanic(fmt.Sprintf("flexbits f=%d q=%d", f, q),
			fmt.Sprint(coef[:16], io.acc, io.nb, io.off), fmt.Sprint(gc, gio.acc, gio.nb, gio.off))
	}
}

func firstDiff32(a, b []int32) int {
	for i := range a {
		if a[i] != b[i] {
			return i
		}
	}
	return -1
}
