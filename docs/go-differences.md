# Differences from Go

- Input is a `&[u8]`, not an `io.Reader` (Go read it all first), and there is no `image.Decode`
  registration.
- Go's process-wide memory budget (`SetMemoryLimit`, blocking in `reserve`, `ErrMemoryLimit`,
  default half the cgroup limit) is replaced by the caller's `Limits::max_alloc`. `ErrMemoryLimit` (retry later) has no equivalent: nothing waits.
- `Decoder::new(data)` replaces `Reset(data)` (`errNotReset` cannot happen). Go's reusable
  `Decoder` is a `Decoder` made with `Decoder::reusing` on a `Scratch` the caller keeps: Go's
  "warm decode makes no allocation" holds for the second decode of the same stream at
  `max_threads` 1 (`tests/reuse_alloc.rs`), and usually for a stream no larger in any buffer
  dimension than one decoded before; see [decoding.md](decoding.md). A `Decoder` from
  `new` allocates its buffers per decode, bounded by `Decoder::estimate`. Go's reused buffers
  keep stale bytes that make a failed decode's partial output depend on history; every `Scratch`
  buffer is made as a fresh one, so the partial output here does not.
- `Limits::max_threads` and the pipeline, `ThreadPool`, `Decoder::with_pool`, `OddStride` and the
  tile-table cap on `decode_config` are new (see [limits.md](limits.md) and [threading.md](threading.md)); Go decoded on one goroutine
  per call.
- 64-bit targets only (`compile_error!` otherwise): the header's 2^32 sizes are held in `usize`.
- `decode_component` takes `k: usize`, so a negative `k` cannot be asked for.
- Arithmetic that wraps in Go wraps here through `wrapping_*` calls and `codec::wrap::W`, so the
  result does not depend on the profile: Cargo ignores a dependency's `[profile]` tables, and a
  consumer's dev build has overflow checks on. CI runs the tests and the parity oracle in the dev
  profile for this. Shifts by a count of the type's width or more follow Go (everything
  shifted out), not Rust's masking.
- Of the six assembly kernels, four have SIMD tiers (see [safety.md](safety.md)); `flexbits` and `highpass` do not.
  The `kernelcheck`/`purego` build tags are replaced by the cross-tier tests and `JPEGXR_SIMD`.
  `GOMEMLIMIT` has no counterpart.
