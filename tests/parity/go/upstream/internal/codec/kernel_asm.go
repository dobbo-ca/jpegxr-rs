//go:build (arm64 || amd64) && !purego

package codec

import "os"

// JXR_NOSIMD forces the Go kernels.
var useAsm = cpuSIMD() && os.Getenv("JXR_NOSIMD") == ""

// idct4x4Stage1PairsAsm needs n a multiple of asmPairs.
//
//go:noescape
func idct4x4Stage1PairsAsm(p *int32, n int)

//go:noescape
func post4x4Stage1SplitAsm(a, b, c, d *int32)

//go:noescape
func outBGR8Asm(dst *byte, y, u, v *int32, n int, bias int32, shift uint32, rgb int)

//go:noescape
func outGray16Asm(dst *byte, y *int32, n int, bias int32, shift, nLen uint32)

func strIDCT4x4Stage1Pairs(p []int32, o, n int) {
	if useAsm && n >= asmPairs {
		m := n &^ (asmPairs - 1)
		_ = p[o+64*(m-1)+31]
		if kernelCheck {
			idctCheck(p, o, m)
		} else {
			idct4x4Stage1PairsAsm(&p[o], m)
		}
		o, n = o+64*m, n-m
	}
	strIDCT4x4Stage1PairsGo(p, o, n)
}

// The four runs never overlap: rows are 256+ apart.
func strPost4x4Stage1SplitAlternate(p []int32, p0, p1, iOffset int) {
	if !useAsm {
		strPost4x4Stage1SplitAlternateGo(p, p0, p1, iOffset)
		return
	}
	a, b, c, d := p0+12, p0+72-iOffset, p1+4, p1+64-iOffset
	_, _, _, _ = p[a+3], p[b+3], p[c+3], p[d+3]
	if kernelCheck {
		postCheck(p, p0, p1, iOffset, a, b, c, d)
		return
	}
	post4x4Stage1SplitAsm(&p[a], &p[b], &p[c], &p[d])
}

// NEON shift counts are one signed byte.
func outBGR8(out []byte, d int, y, u, v []int32, s, n int, bias int32, shift uint32, iR, iB int) {
	if !useAsm || shift > 31 {
		outBGR8Go(out, d, y, u, v, s, n, bias, shift, iR, iB)
		return
	}
	last := s + 256*(n-1) + 197
	_, _, _, _ = out[d+48*n-1], y[last], u[last], v[last]
	if kernelCheck {
		outBGR8Check(out, d, y, u, v, s, n, bias, shift, iR, iB)
		return
	}
	outBGR8Asm(&out[d], &y[s], &u[s], &v[s], n, bias, shift, 1-iR/2)
}

func outGray16(out []byte, d int, y []int32, s, n int, bias int32, shift, nLen uint32) {
	if !useAsm || shift > 31 || nLen > 31 {
		outGray16Go(out, d, y, s, n, bias, shift, nLen)
		return
	}
	_, _ = out[d+32*n-1], y[s+256*(n-1)+197]
	if kernelCheck {
		outGray16Check(out, d, y, s, n, bias, shift, nLen)
		return
	}
	outGray16Asm(&out[d], &y[s], n, bias, shift, nLen)
}

//go:noescape
func flexbitsAsm(coef *int32, order *[16]int, p *byte, acc uint64, nb, f uint32, q int32) (accOut uint64, nbOut uint32, adv uintptr)

// flexbits reports false when the Go loop must run.
// 48 bytes cover 15 coefficients of 16 bits plus a refill.
func flexbits(coef []int32, order *[16]int, io *bitIO, f uint32, q int32) bool {
	if !useAsm || uint64(len(io.buf)) < io.off+48 || len(coef) < 16 {
		return false
	}
	if kernelCheck {
		flexbitsCheck(coef, order, io, f, q)
		return true
	}
	acc, nb, adv := flexbitsAsm(&coef[0], order, &io.buf[io.off], io.acc, io.nb, f, q)
	io.acc, io.nb, io.off = acc, nb, io.off+uint64(adv)
	return true
}
