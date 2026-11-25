//go:build (arm64 || amd64) && !purego

// Not part of jpegxr-go: an addition to the snapshot (see parity/README.md). The generic kernels
// are unexported and Go's internal rule keeps other modules out, so cmd/kernelgen (inside this
// module) reaches them through these wrappers. Every wrapper runs the GENERIC Go kernel, never the
// assembly.

package codec

import "slices"

// KernelAsmPairs is the number of block pairs one assembly pass takes on this architecture.
const KernelAsmPairs = asmPairs

// KernelIdxCC0 is idxCC[row][0].
func KernelIdxCC0(row int) int { return int(idxCC[row][0]) }

func KernelIDCT(p []int32, o, n int)            { strIDCT4x4Stage1PairsGo(p, o, n) }
func KernelPost(p []int32, p0, p1, iOffset int) { strPost4x4Stage1SplitAlternateGo(p, p0, p1, iOffset) }
func KernelOutBGR8(out []byte, d int, y, u, v []int32, s, n int, bias int32, shift uint32, iR, iB int) {
	outBGR8Go(out, d, y, u, v, s, n, bias, shift, iR, iB)
}
func KernelOutGray16(out []byte, d int, y []int32, s, n int, bias int32, shift, nLen uint32) {
	outGray16Go(out, d, y, s, n, bias, shift, nLen)
}

// KernelAlphabet is aAlphabet: the symbol count of each of the 21 VLC tables.
func KernelAlphabet() [numVLCTables]int { return aAlphabet }

// KernelMaxTables is gMaxTables[nSymbols].
func KernelMaxTables(nSymbols int) int { return gMaxTables[nSymbols] }

const KernelNumVLCTables = numVLCTables

// canon is the byte form both sides hash: little endian, in the order of the Rust side
// (src/codec/simd.rs, flexbits_case and highpass_case).
type canon []byte

func (c *canon) u64(x uint64) {
	*c = append(*c, byte(x), byte(x>>8), byte(x>>16), byte(x>>24), byte(x>>32), byte(x>>40), byte(x>>48), byte(x>>56))
}
func (c *canon) u32(x uint32) { *c = append(*c, byte(x), byte(x>>8), byte(x>>16), byte(x>>24)) }
func (c *canon) i32s(s []int32) {
	for _, x := range s {
		c.u32(uint32(x))
	}
}
func (c *canon) io(io *bitIO) {
	c.u64(io.acc)
	c.u64(uint64(io.nb))
	c.u64(io.off)
}

// KernelFlexbits is TestFlexbitsAsm's run with the kernel off: flexbitsGo through
// decodeBlockAdaptive, three blocks on one reader that skipped `skip` bits. It returns coef and
// the reader's state.
func KernelFlexbits(coef []int32, buf []byte, skip, model, trim, qp int32) []byte {
	old := useAsm
	useAsm = false
	defer func() { useAsm = old }()
	c := slices.Clone(coef)
	var io bitIO
	io.attach(buf, 0)
	io.getBit16(uint32(skip % 16))
	io.getBit32(uint32(skip / 16 % 32))
	for i := 0; i < 3; i++ {
		decodeBlockAdaptive(false, false, nil, nil, &io, c, nil, model, trim, qp, &dctIndex[0], false)
	}
	var out canon
	out.i32s(c)
	out.io(&io)
	return out
}

// KernelTable is the state TestHighpassAsm gives table k: its symbol count and table index, and
// the two discriminants (set after adaptDiscriminant).
type KernelTable struct {
	NSymbols, TableIndex int
	Disc, Disc1          int32
}

// KernelScan is an adaptiveScan.
type KernelScan struct{ UTotal, UScan uint32 }

// KernelHighpass is TestHighpassAsm's decodeBlockHighpassGo: the tables are built as the test
// builds them, then it decodes one block. It returns the count, coef, every table, the scan and
// the reader's state.
func KernelHighpass(chroma bool, skip uint32, qp int32, tables *[numVLCTables]KernelTable,
	scans *[16]KernelScan, buf []byte, coef []int32) []byte {
	var ah [numVLCTables]adaptiveHuffman
	for k := range ah {
		a, t := &ah[k], tables[k]
		a.nSymbols = t.NSymbols
		a.initialize = true
		a.tableIndex = t.TableIndex
		a.lowerBound, a.upperBound = -1<<31, 1<<30
		a.adaptDiscriminant()
		a.disc, a.disc1 = t.Disc, t.Disc1
	}
	var scan [16]adaptiveScan
	for i, s := range scans {
		scan[i] = adaptiveScan{uTotal: s.UTotal, uScan: s.UScan}
	}
	var io bitIO
	io.attach(buf, 0)
	io.getBit16(skip % 16)
	io.getBit32(skip / 16)
	c := slices.Clone(coef)
	n := decodeBlockHighpassGo(chroma, &ah, &io, qp, c, &scan)
	var out canon
	out.u64(uint64(n))
	out.i32s(c)
	for k := range ah {
		a := &ah[k]
		for _, x := range a.hufDecTable {
			out = append(out, byte(x), byte(uint16(x)>>8))
		}
		out.i32s(a.delta[:])
		out.i32s(a.delta1[:])
		out.u32(uint32(a.disc))
		out.u32(uint32(a.disc1))
		out.u64(uint64(a.nSymbols))
		out.u64(uint64(a.tableIndex))
		if a.initialize {
			out = append(out, 1)
		} else {
			out = append(out, 0)
		}
		out.u32(uint32(a.upperBound))
		out.u32(uint32(a.lowerBound))
	}
	for _, s := range scan {
		out.u32(s.uTotal)
		out.u32(s.uScan)
	}
	out.io(&io)
	return out
}
