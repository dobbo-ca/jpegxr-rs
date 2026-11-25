# Tests


Status: the whole library (container, codestream, every colour format and bit depth jxrlib
decodes, interleaved alpha, tiles, the CZI public API), with the Go tests ported, AVX2 and NEON
tiers for four kernels, the Go parity harness, fuzz targets and a benchmark. CI runs `fmt`,
`clippy` (x86_64 and aarch64) and the tests (also with `JPEGXR_SIMD=scalar`), and an aarch64
release build (`test` job); the `arm64` job runs the tests on a native arm64 runner; the `parity` and
`parity-arm64` jobs diff the decoder's output with jpegxr-go's (see [parity.md](parity.md)).

`tests/jpegxr.rs` ports the public-API tests: `TestSynth` (21 synthetic streams against libCZI
hashes), `TestDecodeConfig`, `TestPinned` (every tests/data stream against `tests/data/pinned.txt`,
with Go's error text), `TestDecodeComponent`, `TestDecodeIntoShortDst`, both examples, the
`FuzzDecode` corpus and seeds as regression tests, and gate A of the T.834 conformance suite
(skipped without `corpus/conformance`, which is never in git). `tests/alloc.rs` is `TestEstimate`
with a counting allocator, and the heap-peak and ledger tests of `max_alloc` for 1, 2 and 8
threads. `tests/threads.rs` requires the same output, partial output and error for 2, 3, 4 and 8
threads as for one, and the thread peak under `max_threads`; `tests/pool.rs` covers the shared and
the caller's pool; `src/par.rs` has the pipeline's unit tests, and `src/codec/tests.rs` decodes
every gate B stream (alpha and hard tiles included) serially and through the pipeline and compares. `src/codec/tests.rs` ports `TestReadHeader`, the header and index
table fuzz seeds, `TestLossyAlpha` and conformance gate B; `src/tests.rs` is
`TestDecodeComponentPlane`; `src/container.rs` has the container seeds. The budget tests become
the `max_alloc` and size-limit tests; Go's fuzzdiff, jxrlib (`JXR_REF_DIR`), CZI-tile and
`JXR_DAT_FILES` tests need local corpora and external binaries and are not ported. Go's asm
kernel tests (`kernel_test.go`) become the cross-tier tests above. The golden hashes are Go's
(`tests/data/*/golden.txt`, `tests/data/pinned.txt`); the fixtures are Go's `tests/data/synth` and
`tests/data/alpha`. `tests/common/sha256.rs` is a SHA-256 for them, since std has none.
