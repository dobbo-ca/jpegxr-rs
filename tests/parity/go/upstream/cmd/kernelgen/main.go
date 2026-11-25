// Command kernelgen is the Go side of the kernel parity check (see parity/README.md): the six
// random-input kernel tests of internal/codec/kernel_test.go (TestIDCT4x4Stage1PairsAsm,
// TestPost4x4Stage1SplitAsm, TestOutBGR8Asm, TestOutGray16Asm, TestFlexbitsAsm, TestHighpassAsm)
// turned into a generator. For each kernel it writes <dir>/<kernel>.in, the test's inputs, and
// <out>/<kernel>.out, one FNV-1a 64 digest (8 bytes, little endian) per case of the GENERIC Go
// kernel's output. `oracle -kernels` of the Rust crate reads the .in files and writes the same
// .out files for its scalar kernels and for every SIMD tier.
//
// This is an addition to the jpegxr-go snapshot: it lives inside the module only so that it may
// import internal/codec.
//
//	kernelgen <in dir> <out dir>
//
// File format: u64 case count, then the cases. A case is a list of values: an integer is a
// little-endian u64 (negative ones as two's complement), an int32 slice a u64 length and 4 bytes
// an element, a byte slice a u64 length and the bytes.
package main

import (
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"slices"

	"github.com/dobbo-ca/jpegxr-go/internal/codec"
)

// fills is fills() of kernel_test.go: edge and random buffers of length n.
func fills(n int) [][]int32 {
	edges := []int32{0, -1, 1, math.MaxInt32, math.MinInt32, math.MaxInt32 - 3, math.MinInt32 + 3}
	var out [][]int32
	for _, e := range edges {
		out = append(out, slices.Repeat([]int32{e}, n))
	}
	alt := make([]int32, n)
	for i := range alt {
		alt[i] = edges[3+i%2]
	}
	out = append(out, alt)
	r := rand.New(rand.NewSource(1))
	for k := 0; k < 200; k++ {
		b := make([]int32, n)
		for i := range b {
			switch k % 3 {
			case 0:
				b[i] = int32(r.Uint32())
			case 1:
				b[i] = int32(r.Intn(1<<16) - 1<<15)
			default:
				b[i] = edges[r.Intn(len(edges))]
			}
		}
		out = append(out, b)
	}
	return out
}

// file is one kernel: the inputs being written and the digests of the outputs.
type file struct {
	in      []byte
	digests []byte
	cases   uint64
}

func (f *file) u64(x uint64) {
	f.in = append(f.in, byte(x), byte(x>>8), byte(x>>16), byte(x>>24), byte(x>>32), byte(x>>40), byte(x>>48), byte(x>>56))
}
func (f *file) int(x int) { f.u64(uint64(int64(x))) }
func (f *file) i32s(s []int32) {
	f.u64(uint64(len(s)))
	for _, x := range s {
		f.in = append(f.in, byte(x), byte(x>>8), byte(x>>16), byte(x>>24))
	}
}
func (f *file) bytes(b []byte) {
	f.u64(uint64(len(b)))
	f.in = append(f.in, b...)
}

// fnv is FNV-1a 64, as the oracle's digest.
func fnv(b []byte) uint64 {
	h := uint64(0xcbf29ce484222325)
	for _, c := range b {
		h = (h ^ uint64(c)) * 0x100000001b3
	}
	return h
}

func (f *file) done(out []byte) {
	h := fnv(out)
	f.digests = append(f.digests, byte(h), byte(h>>8), byte(h>>16), byte(h>>24), byte(h>>32), byte(h>>40), byte(h>>48), byte(h>>56))
	f.cases++
}

func i32le(s []int32) []byte {
	b := make([]byte, 0, 4*len(s))
	for _, x := range s {
		b = append(b, byte(x), byte(x>>8), byte(x>>16), byte(x>>24))
	}
	return b
}

func (f *file) write(name, in, out string) error {
	hdr := []byte{byte(f.cases), byte(f.cases >> 8), byte(f.cases >> 16), byte(f.cases >> 24), 0, 0, 0, 0}
	if err := os.WriteFile(filepath.Join(in, name+".in"), append(hdr, f.in...), 0o644); err != nil {
		return err
	}
	fmt.Printf("%s: %d cases\n", name, f.cases)
	return os.WriteFile(filepath.Join(out, name+".out"), f.digests, 0o644)
}

// idct: TestIDCT4x4Stage1PairsAsm. Case: o, n, buf.
func idct() *file {
	f := new(file)
	for _, n := range []int{codec.KernelAsmPairs, 2 * codec.KernelAsmPairs, codec.KernelAsmPairs + 1} {
		const o = 8
		for _, in := range fills(o + 64*n + 8) {
			f.int(o)
			f.int(n)
			f.i32s(in)
			want := slices.Clone(in)
			codec.KernelIDCT(want, o, n)
			f.done(i32le(want))
		}
	}
	return f
}

// post: TestPost4x4Stage1SplitAsm. Case: p0, p1, iOffset, buf.
func post() *file {
	f := new(file)
	cases := []struct{ p0, p1, iOffset int }{
		{0, 16, 0},     // strPost4x4Stage1Alternate
		{0, 16, 32},    // 420 chroma
		{48, 1024, 0},  // split across rows
		{16, 2000, 32}, // split, 420 chroma
	}
	for _, c := range cases {
		for _, in := range fills(2200) {
			f.int(c.p0)
			f.int(c.p1)
			f.int(c.iOffset)
			f.i32s(in)
			want := slices.Clone(in)
			codec.KernelPost(want, c.p0, c.p1, c.iOffset)
			f.done(i32le(want))
		}
	}
	return f
}

// bgr8: TestOutBGR8Asm. Case: d, s, n, bias, shift, iR, iB, len(out), y, u, v.
func bgr8() *file {
	f := new(file)
	r := rand.New(rand.NewSource(2))
	for _, n := range []int{1, 3} {
		size := 256*n + 64
		fs := fills(size)
		for fi, y := range fs {
			u, v := fs[(fi+1)%len(fs)], fs[(fi+5)%len(fs)]
			row := fi % 16
			s := 256*(fi%2) + codec.KernelIdxCC0(row)
			if s+256*(n-1)+197 >= size {
				s = codec.KernelIdxCC0(row)
			}
			for _, sh := range []uint32{0, 3, 31, 40} {
				bias := int32(r.Uint32())
				if fi%2 == 0 {
					bias = 128 << (sh % 32)
				}
				for _, iR := range []int{0, 2} {
					want := make([]byte, 48*n+10)
					f.int(5)
					f.int(s)
					f.int(n)
					f.int(int(bias))
					f.int(int(sh))
					f.int(iR)
					f.int(2 - iR)
					f.int(len(want))
					f.i32s(y)
					f.i32s(u)
					f.i32s(v)
					codec.KernelOutBGR8(want, 5, y, u, v, s, n, bias, sh, iR, 2-iR)
					f.done(want)
				}
			}
		}
	}
	return f
}

// gray16: TestOutGray16Asm. Case: d, s, n, bias, shift, nLen, len(out), y.
func gray16() *file {
	f := new(file)
	r := rand.New(rand.NewSource(3))
	for _, n := range []int{1, 3} {
		size := 256*n + 64
		for fi, y := range fills(size) {
			s := codec.KernelIdxCC0(fi % 16)
			for _, sh := range []uint32{0, 3, 31, 40} {
				for _, nLen := range []uint32{0, 4, 31, 200} {
					bias := int32(r.Uint32())
					if fi%2 == 0 {
						bias = 1 << 15 >> (nLen % 32)
					}
					want := make([]byte, 32*n+10)
					f.int(3)
					f.int(s)
					f.int(n)
					f.int(int(bias))
					f.int(int(sh))
					f.int(int(nLen))
					f.int(len(want))
					f.i32s(y)
					codec.KernelOutGray16(want, 3, y, s, n, bias, sh, nLen)
					f.done(want)
				}
			}
		}
	}
	return f
}

// flexbits: TestFlexbitsAsm, flexbitsIters of its 200000 iterations. Case: skip, model, trim,
// qp, coef, buf.
func flexbits(iters int) *file {
	f := new(file)
	r := rand.New(rand.NewSource(4))
	for it := 0; it < iters; it++ {
		coef := make([]int32, 16+r.Intn(4))
		for i := range coef {
			switch r.Intn(4) {
			case 0, 1:
			case 2:
				coef[i] = int32(r.Intn(64) - 32)
			default:
				coef[i] = int32(r.Uint32())
			}
		}
		buf := make([]byte, r.Intn(200))
		for i := range buf {
			switch it % 3 {
			case 0:
				buf[i] = byte(r.Uint32())
			case 1:
				buf[i] = 0xff
			}
		}
		model := int32(1 + r.Intn(15))
		trim := int32(r.Intn(int(model) + 1))
		qp := []int32{1, 0, -1, 2, math.MaxInt32, math.MinInt32, int32(r.Uint32())}[r.Intn(7)]
		skip := int32(r.Intn(512))
		f.int(int(skip))
		f.int(int(model))
		f.int(int(trim))
		f.int(int(qp))
		f.i32s(coef)
		f.bytes(buf)
		f.done(codec.KernelFlexbits(coef, buf, skip, model, trim, qp))
	}
	return f
}

// highpass: TestHighpassAsm, `iters` of its 300000 iterations. Case: chroma, skip, qp, 21 tables
// of (nSymbols, tableIndex, disc, disc1), 16 scans of (uTotal, uScan), coef, buf.
func highpass(iters int) *file {
	f := new(file)
	r := rand.New(rand.NewSource(5))
	alphabet := codec.KernelAlphabet()
	for it := 0; it < iters; it++ {
		var tables [codec.KernelNumVLCTables]codec.KernelTable
		for k := range tables {
			t := &tables[k]
			t.NSymbols = alphabet[k]
			t.TableIndex = r.Intn(max(codec.KernelMaxTables(t.NSymbols), 1))
			t.Disc, t.Disc1 = int32(r.Intn(129)-64), int32(r.Intn(129)-64)
		}
		var scans [16]codec.KernelScan
		for i, p := range r.Perm(16) {
			scans[i] = codec.KernelScan{UTotal: uint32(r.Intn(4)), UScan: uint32(p)}
			if it%5 == 0 {
				scans[i].UTotal = math.MaxUint32 - uint32(r.Intn(2))
			}
		}
		coef := make([]int32, 16+r.Intn(4))
		buf := make([]byte, 150+r.Intn(250))
		for i := range buf {
			switch it % 4 {
			case 0, 1:
				buf[i] = byte(r.Uint32())
			case 2:
				x, y := byte(r.Uint32()), byte(r.Uint32())
				buf[i] = x & y // long codes
			}
		}
		qp := []int32{1, -1, 7, math.MaxInt32, math.MinInt32, int32(r.Uint32())}[r.Intn(6)]
		chroma := r.Intn(2) == 0
		skip := uint32(r.Intn(64))
		c := 0
		if chroma {
			c = 1
		}
		f.int(c)
		f.int(int(skip))
		f.int(int(qp))
		for _, t := range tables {
			f.int(t.NSymbols)
			f.int(t.TableIndex)
			f.int(int(t.Disc))
			f.int(int(t.Disc1))
		}
		for _, s := range scans {
			f.u64(uint64(s.UTotal))
			f.u64(uint64(s.UScan))
		}
		f.i32s(coef)
		f.bytes(buf)
		f.done(codec.KernelHighpass(chroma, skip, qp, &tables, &scans, buf, coef))
	}
	return f
}

const (
	flexbitsIters = 200000 // all of TestFlexbitsAsm
	highpassIters = 100000 // a third of TestHighpassAsm's 300000: its inputs are 650 bytes a case
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: kernelgen <in dir> <out dir>")
		os.Exit(2)
	}
	in, out := os.Args[1], os.Args[2]
	for _, d := range []string{in, out} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
	for _, k := range []struct {
		name string
		gen  func() *file
	}{
		{"idct_pairs", idct},
		{"post_split", post},
		{"out_bgr8", bgr8},
		{"out_gray16", gray16},
		{"flexbits", func() *file { return flexbits(flexbitsIters) }},
		{"highpass", func() *file { return highpass(highpassIters) }},
	} {
		if err := k.gen().write(k.name, in, out); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
}
