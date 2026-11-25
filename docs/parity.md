# Parity with Go

`tests/parity/go/upstream` is the vendored jpegxr-go snapshot; `tests/parity/go/oracle` and
`examples/oracle.rs` decode the tests/data streams and about 17,000 mutations of them with Go and
with this crate, and CI requires the output trees (pixels, error classes, partial output of a
failed `decode_into`) to be equal, on x86_64 (both SIMD tiers) and on aarch64 (a native arm64 runner). Inputs,
classes and the Go tests each part covers are in [parity.md](parity.md).

`tests/parity/go/upstream/` is the non-test source of github.com/dobbo-ca/jpegxr-go (with `go.mod`,
`LICENSE`, `NOTICE`), vendored so the oracle builds offline. Two files
in it are additions to the snapshot, not jpegxr-go's: `internal/codec/kernel_export.go` and
`cmd/kernelgen/` (see Kernels below). `tests/parity/go/oracle/` is a Go program with its own `go.mod` that
`replace`s the module with `../upstream`. The CI job `parity` (`.github/ci/Dockerfile`, stages
`go-oracle`, `go-run`, `parity`) first runs the kernel check (below), then:

1. `oracle -gen tests/data /inputs` writes the inputs.
2. `oracle /inputs /go-out` decodes them with jpegxr-go.
3. `cargo run --release --example oracle -- /inputs /rust-out` decodes them with this crate, once
   with the best SIMD tier and once with `JPEGXR_SIMD=scalar`.
4. `diff -rq /go-out /rust-out` must be empty (and the same for the scalar run). A difference is a
   bug in the Rust port, never in the oracle. The job fails if Go wrote fewer than 100,000 files.
   Each Rust output tree is deleted right after its diff: all of them together once filled the
   a runner's disk (`No space left on device`).

The `parity-arm64` job runs `examples/oracle.rs` on a native arm64 runner (`ubuntu-24.04-arm`) on a
subset (every input without `~` in its name, and every 20th mutation; Go decodes the subset
again), once with NEON and once with `JPEGXR_SIMD=scalar`, and diffs with Go.

## Inputs

The Go oracle only sees the public package, so the inputs are files and the outputs are what the
public API returns. Bases: the 21 `tests/data/synth` streams, the 2 `tests/data/alpha` streams and the
7 files of `tests/data/fuzz` (Go's 5 `FuzzDecode` corpus entries, `cd1a7499fdd5fb83` and
`crash-a6c20a23...`, both found by the Rust fuzzing): 30 streams, which is the "30 base streams" of
the CI log, written as `synth-<name>.jxr`, `alpha-<name>.jxr`, `fuzz-<hash>.jxr`. The ITU-T T.834 conformance streams (about 460, fetched in
the CI image with the sha256 of Go's `fetch.sh`; the BuildKit layer of the download is kept in the
Actions cache, and a blocked download fails the build unless the build arg `ALLOW_NO_CONFORMANCE=1`
is set, which loses the conformance coverage) are inputs too, written as they are as
`conf-<path>.jxr`, without mutations; the aarch64 subset leaves them out. Each other base is written
as it is and with these mutations
(`<base>~<tag>.jxr`), made from the oracle's own xorshift so the bytes do not depend on the Go
version:

| tag | mutation | per base |
|---|---|---|
| `~b<i>` | bit i of the first 32 bytes flipped (container, IFD, image header) | 256 |
| `~r<j>` | single bit flips at positions from the xorshift | 120 |
| `~t<n>` | truncated to n bytes: 1..32, 64 evenly spaced cuts, the last two | about 95 |
| `~m<j>` | `mutate` of `TestFuzzDiff` (`fuzzdiff_test.go`): 1 to 8 flipped bytes, a zeroed run, or a truncation, past the first eighth | 60 |
| `~s<i>_00`, `~s<i>_ff` | byte i of the first 48 set to 0x00 or 0xff (length and count fields) | 96 |
| `~c` | IFD entry count corrupted: the seed of `FuzzContainer` (`container_test.go`) | 1 |
| `~big` | an untiled short-header stream rewritten long, both sizes 0xffffffff (2^32 x 2^32; Go wraps the size products, class `alloc`); the oracles write `skipped` for `into`, `ints`, `short` | 1 per untiled base |
| `~h` | image header subversion bits flipped: the seed of `FuzzHeader` (`strdec_test.go`) | 1 |
| `~i` | byte 16 of the codestream corrupted: the seed of `FuzzIndexTable` | 1 |

### Go tests and what covers them

jpegxr-go has 32 `Test`, `Fuzz` and `Benchmark` functions.

| Go test | Here |
|---|---|
| `TestSynth`, `TestPinned`, `TestDecodeConfig` (decode of every tests/data stream, golden hashes) | ported (`tests/jpegxr.rs`); also the `dec` and `cfg` outputs of the 28 base streams, byte for byte against Go |
| `FuzzDecode` | seeds and corpus ported as tests; `tests/fuzz/fuzz_targets/decode.rs`; the 5 corpus entries are bases here |
| `FuzzContainer`, `FuzzHeader`, `FuzzIndexTable` | seeds ported (`src/container.rs`, `src/codec/tests.rs`); `~c`, `~h`, `~i` inputs; `tests/fuzz/fuzz_targets/decode_config.rs` (container and header) and `decode.rs` (index table) |
| `TestFuzzDiff` (mutants of the synth streams, vs jxrlib) | its `mutate` is the `~m` inputs, compared with Go (jxrlib is not available in CI) |
| `TestDecodeComponent`, `TestDecodeComponentPlane` | ported; the `c0` to `c3` outputs of every input; `tests/fuzz/fuzz_targets/decode_component.rs` |
| `TestDecodeIntoShortDst`, `TestEstimate` | ported (`tests/jpegxr.rs`, `tests/alloc.rs`); the `short`, `into` and `ints` outputs of every input |
| `TestDecoderReuse` | ported as `tests/reuse_alloc.rs` (a counting allocator: no call in the second decode of a stream on one `Scratch`) and `tests/reuse.rs` (a kept `Scratch` against fresh decoders); reuse is also compared with Go: the `reuse` op, below |
| `TestReadHeader`, `TestLossyAlpha`, conformance gates A and B | ported (`src/codec/tests.rs`, `tests/jpegxr.rs`) |
| `TestIDCT4x4Stage1PairsAsm`, `TestPost4x4Stage1SplitAsm`, `TestOutBGR8Asm`, `TestOutGray16Asm`, `TestFlexbitsAsm`, `TestHighpassAsm` (random data, asm against the generic Go) | ported as a generator, `cmd/kernelgen`: the Rust scalar kernels and the SIMD tier against the generic Go kernels on Go's own inputs (Kernels, below); also cross-tier tests in `src/codec/simd.rs` and the four `tests/fuzz/fuzz_targets` kernel targets |
| `TestReserve*` (the process-wide memory budget) | not ported: `Limits::max_alloc` replaces the budget |
| `TestCZITiles`, `TestJxrlib`, `TestDecodeComponentDat`, `BenchmarkDecode*` | need local corpora or jxrlib; not ported (the benchmark is `examples/bench.rs`) |

## Operations and output files

Per input the oracle runs `cfg` (`DecodeConfig`), `dec` (`Decode`), `c0` to `c3`
(`DecodeComponent`), `into` (`Decoder.Reset` and `DecodeInto` into an exact `stride * h` buffer),
`ints` (the same with 8 bytes of row padding, the buffer filled with 0xa5), `intodd` (the same with
3 bytes of padding, so an odd stride), `short`
(`DecodeInto` with a dst one byte short and with a stride one byte short) and `reuse`.

`reuse` is `into` on ONE Go `Decoder` that every earlier input went through, in the sorted order
of the directory: `Reset(prev)` (the previous input, its result ignored) and then `Reset(cur)` and
`DecodeInto`, so the Decoder starts from whatever the last input left, including after a failed
`Reset` (which leaves the old stream, size and a half-parsed header in place) and a failed or
cut-off `DecodeInto`. The Rust side does the same on ONE `Scratch` (`Decoder::reusing` of `prev`,
its result ignored, then of `cur`), so a decoder whose output depends on its history shows up as a
difference on either side.
The result of the first run is in "Reuse result" below. Each writes
`<input>.<op>.out` or `<input>.<op>.err` (the error class and a newline). `cfg` and `dec` write
`<w> <h> <model>` and a newline (`dec` and the components then the pixel bytes); `into` the size
and `Format`, then the buffer.

For an input with `~` in its name the bytes are replaced by FNV-1a 64 and the length (`%016x %d`),
to keep the output tree small; the base streams are compared byte for byte.

Where Go returns output together with an error, so does this crate: `DecodeInto` and
`decode_into` leave what they wrote in dst. An `into`, `intodd` or `ints` that fails also writes
`<input>.<op>.partial.out` (the dst) when any byte of it is non-zero; both sides write it and the
files must be equal.

An input whose header says more than 2^20 pixels (2^24 for a mutated input, whose output is a
digest, so the bytes are not written; the 512 MiB budget bounds the decoder either way) is not
decoded (a mutated width or height would make the oracle allocate and write gigabytes): `dec`, `c0`
to `c3`, `into`, `intodd`, `ints`, `short` and `reuse` write class `oversize` on both sides. The Go oracle sets `SetMemoryLimit(512 MiB)` and the Rust one
`Limits { max_alloc: 512 MiB, .. }` with no other cap, so a stream whose estimate passes it is
class `alloc` on both sides. `Decoder::estimate` uses Go's constants (`ctxBytes`, and the sizes of
`predInfo`, `bitIO` and `wmiTile`), so the same limit gives the same class; the unit test
`estimate_constants` checks that the Rust structs fit them. The unmutated inputs also run under 12
limits from 64 KiB to 1 MiB (`ORACLE_MEM`, in bytes, read by both oracles), around the estimates of
the streams, and the trees must be equal. Go's estimate does not count the quantizer arrays; the
Rust decoder charges an upper bound for them (at most 33 entries of 20 bytes a channel and tile
column), which stays inside the slack between Go's estimate and the Rust structs.

## Error classes

| class | Go | Rust |
|---|---|---|
| `internal` | `ErrInternal` | `Error::Internal` |
| `pixfmt` | `errFormat` | `Error::PixelFormat` |
| `toolarge` | `errTooLarge` | `Error::TooLarge` |
| `container` | `container.ErrFormat` | `Error::Container` |
| `codestream` | `codec.ErrFormat` | `Error::Codestream` |
| `unsupported` | `codec.ErrUnsupported` | `Error::Unsupported` |
| `dst` | `errDst` | `Error::DstTooSmall` |
| `oddstride` | (none: Go takes `stride / 2` samples a row) | `Error::OddStride` |
| `alloc` | `errBudgetExceeded` | `Error::AllocLimit` |
| `cancelled` | `context.Canceled` | `Error::Cancelled` |
| `oversize` | (the oracle's own cap, above) | the same |

`Error::LimitExceeded` and `OutOfMemory` have no Go counterpart; they print as `other:<message>`
and would show up as a diff. Go's `DecodeConfig` has no `errTooLarge` check (only `Reset` has);
`decode_config` applies it too, so the oracle writes `toolarge` for `cfg` when the header says
more than 2^40 pixels.

`oddstride` is a deliberate deviation: a 16-bit format with an odd stride is refused, after the
`dst` check. The `intodd` op (pad 3, buffer filled with 0xa5) has one for every format: the
Go oracle writes `oddstride` and the untouched dst as `intodd.partial.out` when the stride is odd
and the format is 16-bit, as the Rust decoder does, and for the 8-bit formats both sides decode
at the odd stride and the output is compared. `tests/jpegxr.rs` and the `decoder` fuzz target
cover it too.

`decode_into` deviation for a non-zero dst: a codestream with fewer channels than the container's
pixel format (an N_COMPONENT stream of 1 channel in a 48bpp3Channels container, say) has no
bytes for the rest of each pixel. jxrlib and Go leave them as they were in dst; this crate
writes zero there, so the output never depends on the old dst (found by an earlier fuzz run). On a dst
that starts zeroed, which is what `decode` and the oracles' fresh buffers give, the output is
equal to Go's; into a non-zero dst (the `ints` and `intodd` ops fill 0xa5) the bytes differ, but
only for such a stream. `short_n_component_writes_every_byte` covers it.

Threads: the Rust oracle reads `ORACLE_THREADS` (default: the machine's CPUs). The main run, the
scalar run and the `ORACLE_MEM` runs use the default; the checked (dev profile) run on the subset
uses `ORACLE_THREADS=1`. The output must not depend on it.

`ErrMemoryLimit` (a decode that waited for budget) cannot happen in a sequential oracle.

## Benchmark

`bench.sh` and `.github/workflows/bench.yml`: `oracle -bench N dir` and `examples/bench.rs` decode
each stream of `tests/data/synth` N times (after one warm-up) with one dst, as a caller that decodes many streams in a loop does,
and print the ns per decode (Go resets one `Decoder` each time; the Rust `Decoder` has no reset,
so the Rust loop parses the header again every time). A stream that decodes once and then fails
exits non-zero on both sides; a stream that fails the warm-up is skipped, and `bench.sh` prints how
many conformance streams each side skipped. `bench.sh` joins them into a
ms/MB table by input class (pixel type and lossless or lossy). Best of five runs per file and side.
There are two tables: Go at its default parallelism against Rust at the default `max_threads`
(`BENCH_THREADS` unset), and both on one thread (`GOMAXPROCS=1`, `BENCH_THREADS=1`).
The streams are the 21 64x64 synthetic ones and up to 30 conformance streams of 30 to 400 KB (class
`conformance`, N/50 decodes each); the CZI tiles of Go's own benchmark are not in the repo. A stream
that decodes on one side only fails the script.

## Kernels

Go's six random-input kernel tests (`internal/codec/kernel_test.go`) compare the assembly with the
generic Go kernels. Here the generic Go kernels give the expected bytes and the Rust scalar kernels
and the Rust SIMD tier (AVX2 on x86_64, NEON on aarch64) must equal them.

`cmd/kernelgen` (Go, inside `tests/parity/go/upstream`) writes `<kernel>.in`, the inputs of the Go test, and
`<kernel>.out`, one FNV-1a 64 digest per case of the generic Go kernel's output. Go's `internal`
rule keeps a package outside the module away from `internal/codec`, and the generic kernels are
unexported, so the generator lives in the `upstream` module (as `cmd/kernelgen`) and reaches the
kernels through `internal/codec/kernel_export.go`, which wraps them and nothing else. The other
choice, vendoring `kernel_generic.go` into the generator, would not have given `flexbits` and
`highpass` (they need `bitIO`, the tables and `decodeBlockAdaptive`). Both files are additions to
the snapshot; the rest of `upstream` is jpegxr-go. `oracle -kernels <in> <out>`
(`examples/oracle.rs`) reads the `.in` files, runs each case through the Rust kernels (scalar,
then the SIMD tier) and writes the same digests to `<out>/scalar` and `<out>/simd`; the job
`cmp`s each file with Go's (the first differing byte / 8 is the case number). `flexbits` and
`highpass` have no SIMD tier here, so both directories hold the scalar result. The Rust entries
are the doc-hidden `jpegxr::kernels::{idct_pairs, post_split, out_bgr8, out_gray16, flexbits,
highpass}`; the last two call `decode_block_adaptive` and `decode_block_highpass` of `segdec.rs`
through `codec::simd::{flexbits_case, highpass_case}`.

The inputs are Go's: `fills()` (the 7 edge values repeated, the MAX/MIN alternation and 200 random
buffers of `rand.NewSource(1)`, 208 buffers), shifts {0, 3, 31, 40}, `nLen` {0, 4, 31, 200}, the
`qp` edge values, `uTotal` near `MaxUint32`, and the seeds 2 to 5 of the other tests, in the same
loop order, so the random values are Go's.

| kernel | Go test | cases |
|---|---|---|
| `idct_pairs` | `TestIDCT4x4Stage1PairsAsm`: n in {4, 8, 5} x 208 fills | 624 |
| `post_split` | `TestPost4x4Stage1SplitAsm`: 4 geometries x 208 fills | 832 |
| `out_bgr8` | `TestOutBGR8Asm`: n in {1, 3} x 208 fills x 4 shifts x 2 channel orders | 3,328 |
| `out_gray16` | `TestOutGray16Asm`: n in {1, 3} x 208 fills x 4 shifts x 4 `nLen` | 6,656 |
| `flexbits` | `TestFlexbitsAsm`: all 200,000 iterations | 200,000 |
| `highpass` | `TestHighpassAsm`: the first 100,000 of its 300,000 iterations (about 700 bytes of input a case; a third keeps the inputs of this layer small) | 100,000 |
| | | 311,440 |

The job prints the count (`311440 kernel cases, equal for the scalar kernels and the SIMD tier`);
`parity-arm64` runs the same cases on aarch64 (NEON) and prints the same count.
`kernels::flexbits` and `kernels::highpass` return the canonical bytes themselves (the coefficients,
every `adaptiveHuffman` table, the scan state and the bit reader's `acc`, `nb` and `off`), because
the types are private to `codec`. The `kernels` wrappers refuse an index outside the slices with
`Error::LimitExceeded` before the kernel runs; the Go cases all pass that check.

## Reuse result

An early CI run (`reuse` with the partial output compared): the stdout, the error class
and the pixels of every successful `reuse` equal a fresh decode, on x86_64 (19,496 inputs) and
aarch64 (the 1,003-input subset). One difference: for a failed `DecodeInto` the PARTIAL dst
(what was decoded before the error) differs for 284 inputs on x86_64 (14 in the subset), all
mutants of `fuzz-56edf249b82d8cd5` (`~b<i>` for bit flips in bytes 24 to 30 of the file, and
some `~r`, `~m`, `~s`). A reused Go `Decoder` writes other bytes into dst before the failing
macroblock row than a fresh one, which matches the stale scratch the Go code keeps: `reuse` of
`mbBack` clears only `[:mbPad]` and `[off:]`, and the tiles keep their quantizer arrays
(`strdec.go`). Class difference: a Go decode that FAILS is not a function of the input alone but
of the Decoder's history, in the partial dst only; a Go decode that succeeds is, and so is the
error class. A fresh Rust decode (no scratch) is the reference, so the `reuse` op does not write
`reuse.partial.out` on either side; `into` and `ints` (fresh decoders) keep comparing the partial
dst. A typical caller (a successful decode, or an error whose dst is thrown away)
does not depend on it.

## Cancel at row N

Not done. Go checks `ctx.Err()` between macroblock rows, so a context whose `Err` counts calls would
cancel it after row N without touching upstream. The Rust side reads an `AtomicBool`, which cannot
count its loads, and the only other way to stop at row N is a second thread (a race, so not
byte-equal between runs) or a hook in `strdec.rs`, which this harness does not own and the oracles
cannot add without it. The two sides also check at slightly different places (Go also at the
start of `DecodeInto` and while waiting for budget), so "after N checks" is not "after row N". The
cancel flag stays always false in both oracles; `cancel_flag_stops_decode` (`tests/jpegxr.rs`) is the
only test of the `cancelled` class.

## Not compared: the threaded pipeline on corrupt input

The MB-row pipeline runs only for images of at least 16 macroblock columns and 256 macroblocks.
Every mutated input comes from a 64x64 base, so parity compares pipelined output with Go only on the
large unmutated conformance streams, and the dev-profile `ORACLE_THREADS=1` run covers `/sub` only.
Pipelined error and partial-output paths on corrupt input are checked against the serial Rust path
instead (`tests/threads.rs`: `truncated_streams_agree`, and the 2-thread differential in
`tests/fuzz/fuzz_targets/decoder.rs`), which parity in turn ties to Go. A parity pass that forces the
pipeline (`decode_into_probe(.., uncapped = true)`) over `/inputs` is deferred: it would add a third
full oracle run to the parity job on the shared runners.
