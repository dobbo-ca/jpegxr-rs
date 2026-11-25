// Command oracle is the Go side of the parity check with the Rust crate (see docs/parity.md).
//
//	oracle <inputs> <outputs>      decode every input, write <input>.<op>.out or .err (and .partial.out)
//	oracle -gen <testdata> <dir>   write the inputs: the tests/data streams and their mutations
//	oracle -bench <n> <dir>        decode each stream n times, print name, output bytes, ns per decode
//	oracle -ppm <inputs> <outputs> decode every input, write <stem>.ppm or .pgm//
// The Rust side is examples/oracle.rs; both must write the same files.
package main

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"image"
	"image/color"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	jpegxr "github.com/dobbo-ca/jpegxr-go"
)

const (
	// memLimit bounds the decoder's own estimate (jpegxr.SetMemoryLimit), as Limits.max_alloc does
	// on the Rust side.
	memLimit = 512 << 20
	// maxPixels: an input whose header says more is not decoded (both sides write `oversize`), so
	// a mutated width or height cannot make the oracle allocate or write gigabytes. The mutated
	// inputs (digest mode, `~` in the name) get the higher cap: their output is a digest and the
	// 512 MiB budget bounds the decoder; the base streams and the conformance streams are written
	// whole, so they keep the low one.
	maxPixels       = 1 << 20
	maxPixelsDigest = 1 << 24
	// goMaxPixels is the decoder's own cap (errTooLarge).
	goMaxPixels = 1 << 40
)

func main() {
	limit := int64(memLimit)
	if v, err := strconv.ParseInt(os.Getenv("ORACLE_MEM"), 10, 64); err == nil {
		limit = v // a small limit compares the alloc class with Rust's (see docs/parity.md)
	}
	jpegxr.SetMemoryLimit(limit)
	a := os.Args[1:]
	var err error
	switch {
	case len(a) == 3 && a[0] == "-gen":
		err = gen(a[1], a[2], "")
	case len(a) == 4 && a[0] == "-gen":
		err = gen(a[1], a[2], a[3])
	case len(a) == 3 && a[0] == "-bench":
		n, _ := strconv.Atoi(a[1])
		err = bench(max(n, 1), a[2])
	case len(a) == 3 && a[0] == "-ppm":
		err = ppm(a[1], a[2])
	case len(a) == 2:
		err = run(a[0], a[1])
	default:
		err = errors.New("usage: oracle <inputs> <outputs> | oracle -gen <testdata> <dir> [<conformance dir>] | oracle -bench <n> <dir> | oracle -ppm <inputs> <outputs>")
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

// class is the error class: a short stable token. The table is in docs/parity.md.
func class(err error) string {
	s := err.Error()
	switch {
	case errors.Is(err, jpegxr.ErrInternal):
		return "internal"
	case errors.Is(err, jpegxr.ErrMemoryLimit):
		return "memlimit"
	case strings.HasSuffix(s, "unsupported pixel format"):
		return "pixfmt"
	case strings.HasSuffix(s, "image too large"):
		return "toolarge"
	case strings.HasSuffix(s, "bad container"):
		return "container"
	case strings.HasSuffix(s, "bad codestream"):
		return "codestream"
	case strings.HasSuffix(s, "unsupported codestream feature"):
		return "unsupported"
	case strings.HasSuffix(s, "dst too small for the image"):
		return "dst"
	case strings.HasSuffix(s, "byte memory limit"):
		return "alloc"
	case errors.Is(err, context.Canceled):
		return "cancelled"
	}
	return "other:" + s
}

// digest is FNV-1a 64 and the length of b, as a line.
func digest(b []byte) []byte {
	h := uint64(0xcbf29ce484222325)
	for _, c := range b {
		h = (h ^ uint64(c)) * 0x100000001b3
	}
	return []byte(fmt.Sprintf("%016x %d\n", h, len(b)))
}

type outDir struct {
	dir, name string
	digests   bool // a mutated input: write digests of the bytes, not the bytes
}

func (o outDir) write(op, ext string, b []byte) {
	if err := os.WriteFile(filepath.Join(o.dir, o.name+"."+op+"."+ext), b, 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

// bytes writes an operation's output (or its digest).
func (o outDir) bytes(op, ext string, b []byte) {
	if o.digests {
		b = digest(b)
	}
	o.write(op, ext, b)
}

func (o outDir) fail(op string, err error) { o.write(op, "err", []byte(class(err)+"\n")) }
func (o outDir) failClass(op, c string)    { o.write(op, "err", []byte(c+"\n")) }

func rowBytes(w int, f jpegxr.Format) int {
	switch f {
	case jpegxr.Gray8:
		return w
	case jpegxr.Gray16:
		return 2 * w
	case jpegxr.Bgr24:
		return 3 * w
	}
	return 6 * w
}

func formatName(f jpegxr.Format) string {
	return [...]string{jpegxr.Bgr24: "Bgr24", jpegxr.Bgr48: "Bgr48", jpegxr.Gray8: "Gray8", jpegxr.Gray16: "Gray16"}[f]
}

func modelName(c image.Config) string {
	switch c.ColorModel {
	case color.GrayModel:
		return "Gray"
	case color.Gray16Model:
		return "Gray16"
	case color.RGBAModel:
		return "RGBA"
	}
	return "RGBA64"
}

// pix is the image as "<w> <h> <model>\n" and its pixel bytes.
func pix(img image.Image) []byte {
	b := img.Bounds()
	var model string
	var p []byte
	switch m := img.(type) {
	case *image.Gray:
		model, p = "Gray", m.Pix
	case *image.Gray16:
		model, p = "Gray16", m.Pix
	case *image.RGBA:
		model, p = "RGBA", m.Pix
	case *image.RGBA64:
		model, p = "RGBA64", m.Pix
	default:
		model = fmt.Sprintf("%T", img)
	}
	return append([]byte(fmt.Sprintf("%d %d %s\n", b.Dx(), b.Dy(), model)), p...)
}

// partial writes dst as the partial output of a failed operation, if any byte of it is non-zero
// (a zeroed dst is what an operation that wrote nothing leaves).
func (o outDir) partial(op string, dst []byte) {
	for _, c := range dst {
		if c != 0 {
			o.bytes(op, "partial.out", dst)
			return
		}
	}
}

// into is Decoder.Reset and DecodeInto into a dst whose rows are padded by pad bytes (0xa5 where
// the decoder writes nothing). The header line and dst, and the error with whatever dst holds.
func into(o outDir, op string, d *jpegxr.Decoder, data []byte, pad int) {
	if err := d.Reset(data); err != nil {
		o.fail(op, err)
		return
	}
	w, h, f := d.Config()
	stride := rowBytes(w, f) + pad
	dst := make([]byte, stride*h)
	if pad > 0 {
		for i := range dst {
			dst[i] = 0xa5
		}
	}
	if stride%2 == 1 && (f == jpegxr.Gray16 || f == jpegxr.Bgr48) {
		// Go takes stride / 2 samples a row; the Rust decoder refuses (class oddstride, dst
		// untouched), after the memory check. An empty dst gets Go's memory check and nothing else.
		if err := d.DecodeInto(context.Background(), dst[:0], stride); err != nil && class(err) == "alloc" {
			o.fail(op, err)
		} else {
			o.failClass(op, "oddstride")
		}
		o.partial(op, dst)
		return
	}
	if err := d.DecodeInto(context.Background(), dst, stride); err != nil {
		o.fail(op, err)
		if op != "reuse" { // the partial dst of a reused Decoder depends on its history (docs/parity.md)
			o.partial(op, dst)
		}
		return
	}
	o.bytes(op, "out", append([]byte(fmt.Sprintf("%d %d %s\n", w, h, formatName(f))), dst...))
}

// short is DecodeInto with a dst one byte short and a stride one byte short.
func short(o outDir, data []byte) {
	var d jpegxr.Decoder
	if err := d.Reset(data); err != nil {
		o.fail("short", err)
		return
	}
	w, h, f := d.Config()
	s := rowBytes(w, f)
	dst := make([]byte, s*h)
	r := func(err error) string {
		if err == nil {
			return "ok"
		}
		return class(err)
	}
	var e1, e2 error
	if len(dst) > 0 {
		e1 = d.DecodeInto(context.Background(), dst[:len(dst)-1], s)
	}
	if s > 0 {
		e2 = d.DecodeInto(context.Background(), dst, s-1)
	}
	o.write("short", "out", []byte(fmt.Sprintf("dst %s\nstride %s\n", r(e1), r(e2))))
}

func process(o outDir, data []byte) {
	cfg, err := jpegxr.DecodeConfig(bytes.NewReader(data))
	ops := []string{"dec", "c0", "c1", "c2", "c3", "into", "ints", "intodd", "short", "reuse"}
	limit := uint64(maxPixels)
	if o.digests {
		limit = maxPixelsDigest
	}
	if err == nil {
		px := uint64(cfg.Width) * uint64(cfg.Height)
		if px > goMaxPixels {
			// Go's DecodeConfig has no cap, Reset has; the Rust decode_config applies it too.
			o.failClass("cfg", "toolarge")
		} else {
			o.write("cfg", "out", []byte(fmt.Sprintf("%d %d %s\n", cfg.Width, cfg.Height, modelName(cfg))))
		}
		if px > limit {
			for _, op := range ops {
				o.failClass(op, "oversize")
			}
			return
		}
	} else {
		o.fail("cfg", err)
	}
	if img, err := jpegxr.Decode(bytes.NewReader(data)); err != nil {
		o.fail("dec", err)
	} else {
		o.bytes("dec", "out", pix(img))
	}
	for k := range 4 {
		op := "c" + strconv.Itoa(k)
		if m, err := jpegxr.DecodeComponent(context.Background(), bytes.NewReader(data), k); err != nil {
			o.fail(op, err)
		} else {
			o.bytes(op, "out", pix(m))
		}
	}
	// A 2^32 x 2^32 header: the three ops below would allocate a wrapped stride * h (32 GiB).
	if cfg.Width > 0 && uint64(cfg.Width)*uint64(cfg.Height)/uint64(cfg.Width) != uint64(cfg.Height) {
		for _, op := range []string{"into", "ints", "intodd", "short", "reuse"} {
			o.failClass(op, "skipped")
		}
		return
	}
	into(o, "into", new(jpegxr.Decoder), data, 0)
	into(o, "ints", new(jpegxr.Decoder), data, 8)
	into(o, "intodd", new(jpegxr.Decoder), data, 3)
	short(o, data)
	reuse(o, data)
}

// shared is the one Decoder that `reuse` runs every input through, and prev the input before.
var (
	shared jpegxr.Decoder
	prev   []byte
)

// reuse is `into` on the shared Decoder: Reset(prev) first, so the Reset that follows starts from
// whatever the last input left (a failed Reset, a failed or cut-off DecodeInto), then Reset(data)
// and DecodeInto. The Rust side writes what a fresh decoder gives, so a Go decoder that depends on
// its history shows up as a difference (see docs/parity.md).
func reuse(o outDir, data []byte) {
	if prev != nil {
		_ = shared.Reset(prev)
	}
	prev = data
	into(o, "reuse", &shared, data, 0)
}

func run(in, out string) error {
	ents, err := os.ReadDir(in)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		return err
	}
	for _, e := range ents {
		if e.IsDir() || filepath.Ext(e.Name()) != ".jxr" {
			continue
		}
		data, err := os.ReadFile(filepath.Join(in, e.Name()))
		if err != nil {
			return err
		}
		process(outDir{out, e.Name(), strings.Contains(e.Name(), "~")}, data)
	}
	return nil
}

// ppm decodes every stream of in into a fresh dst in the libCZI layout and writes <stem>.ppm (Bgr24,
// Bgr48) or <stem>.pgm (Gray8, Gray16); a stream that does not decode writes nothing. A 16-bit
// sample is two bytes of the row, as decoded, so Gray16 is a P5 of 2*w pixels and Bgr48 a P6 of 2*w
// pixels (6 bytes a pixel is 2 PPM pixels): the exact byte compare covers them.
func ppm(in, out string) error {
	files, err := filepath.Glob(filepath.Join(in, "*.jxr"))
	if err != nil {
		return err
	}
	sort.Strings(files)
	if err := os.MkdirAll(out, 0o755); err != nil {
		return err
	}
	var d jpegxr.Decoder
	for _, f := range files {
		data, err := os.ReadFile(f)
		if err != nil {
			return err
		}
		if d.Reset(data) != nil {
			continue
		}
		w, h, fm := d.Config()
		stride := rowBytes(w, fm)
		dst := make([]byte, stride*h)
		if d.DecodeInto(context.Background(), dst, stride) != nil {
			continue
		}
		magic, ext := "P5", ".pgm"
		if fm == jpegxr.Bgr24 || fm == jpegxr.Bgr48 {
			magic, ext = "P6", ".ppm"
		}
		if fm == jpegxr.Bgr24 {
			for i := 0; i+2 < len(dst); i += 3 {
				dst[i], dst[i+2] = dst[i+2], dst[i]
			}
		}
		pw := w
		if fm == jpegxr.Gray16 || fm == jpegxr.Bgr48 {
			pw = 2 * w
		}
		b := append([]byte(fmt.Sprintf("%s\n%d %d\n255\n", magic, pw, h)), dst...)
		name := strings.TrimSuffix(filepath.Base(f), ".jxr") + ext
		if err := os.WriteFile(filepath.Join(out, name), b, 0o644); err != nil {
			return err
		}
	}
	return nil
}

// bench decodes each stream n times with one Decoder and one dst, as a caller that decodes in a loop does.
func bench(n int, dir string) error {
	files, err := filepath.Glob(filepath.Join(dir, "*.jxr"))
	if err != nil {
		return err
	}
	sort.Strings(files)
	var d jpegxr.Decoder
	var dst []byte
	for _, f := range files {
		data, err := os.ReadFile(f)
		if err != nil {
			return err
		}
		decode := func() error {
			if err := d.Reset(data); err != nil {
				return err
			}
			w, h, fm := d.Config()
			stride := rowBytes(w, fm)
			if len(dst) < stride*h {
				dst = make([]byte, stride*h)
			}
			return d.DecodeInto(context.Background(), dst[:stride*h], stride)
		}
		if decode() != nil {
			continue
		}
		w, h, fm := d.Config()
		t := time.Now()
		for range n {
			if err := decode(); err != nil {
				return err
			}
		}
		fmt.Printf("%s\t%d\t%d\n", filepath.Base(f), rowBytes(w, fm)*h, time.Since(t).Nanoseconds()/int64(n))
	}
	return nil
}

// rng is the xorshift64 the Rust tests use; the generated bytes do not depend on the Go version.
type rng uint64

func (r *rng) next() uint64 {
	x := uint64(*r)
	x ^= x << 13
	x ^= x >> 7
	x ^= x << 17
	*r = rng(x)
	return x
}

func (r *rng) n(n int) int { return int(r.next() % uint64(n)) }

// mutate is `mutate` of jpegxr-go's fuzzdiff_test.go: flips bytes, zeroes a run or truncates,
// past the first eighth.
func mutate(r *rng, b []byte) []byte {
	m := bytes.Clone(b)
	lo := len(m) / 8
	switch r.n(4) {
	case 0, 1:
		for k := 1 + r.n(8); k > 0; k-- {
			m[lo+r.n(len(m)-lo)] ^= byte(1 + r.n(255))
		}
	case 2:
		i := lo + r.n(len(m)-lo)
		clear(m[i:min(len(m), i+1+r.n(64))])
	default:
		m = m[:len(m)/2+r.n(len(m)/2)]
	}
	return m
}

// imageOffset is the codestream's offset, from the container's IFD (tag 0xbcc0), or -1.
func imageOffset(b []byte) int {
	if len(b) < 8 {
		return -1
	}
	off := int(binary.LittleEndian.Uint32(b[4:]))
	if off+2 > len(b) {
		return -1
	}
	n := int(binary.LittleEndian.Uint16(b[off:]))
	for i := range n {
		e := off + 2 + 12*i
		if e+12 > len(b) {
			return -1
		}
		if binary.LittleEndian.Uint16(b[e:]) == 0xbcc0 {
			return int(binary.LittleEndian.Uint32(b[e+8:]))
		}
	}
	return -1
}

type base struct {
	name string
	data []byte
}

// gen writes the inputs. Each base stream (testdata/synth and alpha, and the FuzzDecode entries of
// testdata/fuzz, which are raw bytes here) is written as it is, and with:
//
//	~b<i>   bit i of the first 32 bytes flipped (the container and the image header)
//	~r<j>   120 bit flips at positions from the xorshift
//	~t<n>   truncated to n bytes
//	~m<j>   60 `mutate` calls of fuzzdiff_test.go (TestFuzzDiff)
//	~s<i>_00, ~s<i>_ff   byte i of the first 48 set to 0x00 or 0xff (length and count fields)
//	~c, ~h, ~i   the seeds of FuzzContainer (IFD count), FuzzHeader (header bits of the codestream)
//	             and FuzzIndexTable (byte 16 of the codestream)
//
// Every .jxr under conf (the T.834 conformance suite, fetched in CI) is written as it is, without
// mutations: the suite is 460 streams, and carries the formats and tilings the 64x64 test data does
// not.
func gen(testdata, out, conf string) error {
	var bases []base
	for _, d := range []string{"synth", "alpha"} {
		files, _ := filepath.Glob(filepath.Join(testdata, d, "*.jxr"))
		sort.Strings(files)
		for _, f := range files {
			b, err := os.ReadFile(f)
			if err != nil {
				return err
			}
			bases = append(bases, base{d + "-" + filepath.Base(f), b})
		}
	}
	files, _ := filepath.Glob(filepath.Join(testdata, "fuzz", "*"))
	sort.Strings(files)
	for _, f := range files {
		b, err := os.ReadFile(f)
		if err != nil {
			return err
		}
		bases = append(bases, base{"fuzz-" + filepath.Base(f) + ".jxr", b})
	}
	if len(bases) == 0 {
		return errors.New("no base streams under " + testdata)
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		return err
	}
	var werr error
	count := 0
	if conf != "" {
		n := 0
		err := filepath.WalkDir(conf, func(p string, d fs.DirEntry, err error) error {
			if err != nil || d.IsDir() || !strings.HasSuffix(strings.ToLower(p), ".jxr") {
				return err
			}
			b, err := os.ReadFile(p)
			if err != nil {
				return err
			}
			rel, _ := filepath.Rel(conf, p)
			name := "conf-" + strings.NewReplacer("/", "_", " ", "_", ".jxr", "", ".JXR", "").Replace(filepath.ToSlash(rel)) + ".jxr"
			n++
			count++
			return os.WriteFile(filepath.Join(out, name), b, 0o644)
		})
		if err != nil {
			return err
		}
		if n > 0 && n < 400 { // none: the CI image could not fetch the suite
			return fmt.Errorf("only %d conformance streams under %s", n, conf)
		}
	}
	for _, b := range bases {
		stem := strings.TrimSuffix(b.name, ".jxr")
		w := func(tag string, data []byte) {
			if werr == nil {
				werr = os.WriteFile(filepath.Join(out, stem+tag+".jxr"), data, 0o644)
				count++
			}
		}
		w("", b.data)
		L := len(b.data)
		flip := func(m []byte, i int) []byte {
			m = bytes.Clone(m)
			m[i/8] ^= 1 << (i % 8)
			return m
		}
		for i := 0; i < 256 && i/8 < L; i++ {
			w("~b"+strconv.Itoa(i), flip(b.data, i))
		}
		seed := rng(0xcbf29ce484222325)
		for _, c := range []byte(b.name) {
			seed = (seed ^ rng(c)) * 0x100000001b3
		}
		r := seed | 1
		for j := range 120 {
			w("~r"+strconv.Itoa(j), flip(b.data, r.n(8*L)))
		}
		cuts := map[int]bool{L - 1: true, L - 2: true}
		for k := range 64 {
			cuts[L*k/64] = true
		}
		for k := 1; k <= 32; k++ {
			cuts[k] = true
		}
		for n := range cuts {
			if n >= 0 && n < L {
				w("~t"+strconv.Itoa(n), b.data[:n])
			}
		}
		for j := range 60 {
			w("~m"+strconv.Itoa(j), mutate(&r, b.data))
		}
		for i := 0; i < 48 && i < L; i++ {
			for _, v := range []byte{0x00, 0xff} {
				m := bytes.Clone(b.data)
				m[i] = v
				w(fmt.Sprintf("~s%d_%02x", i, v), m)
			}
		}
		// ~big: the short header (16-bit sizes) of an untiled stream rewritten long, both sizes
		// 0xffffffff: a 2^32 x 2^32 image, whose size products wrap in Go.
		if off := imageOffset(b.data); off >= 0 && off+16 < L && b.data[off+10]&0x80 != 0 && b.data[off+9]&0x80 == 0 {
			m := bytes.Clone(b.data[:off+12])
			m[off+10] &^= 0x80
			for range 8 {
				m = append(m, 0xff)
			}
			w("~big", append(m, b.data[off+16:]...))
		}
		if L > 10 {
			m := bytes.Clone(b.data)
			m[8] ^= 0xff
			m[9] ^= 0xff
			w("~c", m)
		}
		if off := imageOffset(b.data); off >= 0 && off+17 < L {
			h := bytes.Clone(b.data)
			h[off+8] ^= 0x0f
			w("~h", h)
			i := bytes.Clone(b.data)
			i[off+16] ^= 0xff
			w("~i", i)
		}
	}
	fmt.Fprintf(os.Stderr, "%d base streams, %d inputs\n", len(bases), count)
	return werr
}
