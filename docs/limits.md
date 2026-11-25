# Resource limits

Every entry point takes a `Limits`. A call in a 2-CPU, 1 GiB pod, with a small memory budget:

```rust,no_run
use jpegxr::Limits;
use std::sync::atomic::AtomicBool;

let bytes = std::fs::read("tile.jxr")?;
let cancel = AtomicBool::new(false);
let limits = Limits { max_threads: 1, max_alloc: 64 << 20, ..Limits::default() };
let img = jpegxr::decode(&bytes, &limits, &cancel)?; // one thread, never above 64 MiB
# Ok::<(), Box<dyn std::error::Error>>(())
```

| field | default | unit | over it |
|---|---|---|---|
| `max_alloc` | 2 GiB | bytes, the peak of the call, all threads together | `AllocLimit` |
| `max_pixels` | 2^30 | pixels | `LimitExceeded("pixels")` (and `TooLarge` over 2^40, which no limit lifts) |
| `max_width` | 2^20 | pixels | `LimitExceeded("width")` |
| `max_height` | 2^20 | pixels | `LimitExceeded("height")` |
| `max_threads` | `available_parallelism` | threads, the calling thread included | none: the call uses fewer |

A zero field is a real cap, except `max_threads`, where 0 counts as 1. Go had no per-call caps (a
process-wide budget, default half the cgroup limit); the defaults are this crate's own.

- **`max_alloc` is the enforced peak** of one call, threads included. Go's estimate of the decode
  (`Decoder::estimate`, from the header: macroblock rows, coding contexts, index table, the input),
  plus the output for `decode` and `decode_component`, is checked first, which keeps the Go error
  class (`errBudgetExceeded`). Then every allocation that grows with the input is charged to a
  ledger before it is made (the macroblock buffers, offsets, contexts, the header's tile tables
  and their copies, the quantizers' upper bound, the output buffers, and the thread buffers). A
  charge stays booked until the decode returns, then all of that decode's charges are given back
  at once (so the `decode_component` fallback to the full decode starts with its whole budget);
  the thread buffers are given back when the pipeline ends. Buffers are not given back one by one
  as they drop. The borrowed input is not charged. A few fixed buffers (one bit reader,
  a 16-entry slice list per row) are slack. The tests (`tests/alloc.rs`) measure the heap peak with a
  counting allocator against `max_alloc` and against the ledger, for 1, 2 and 8 threads.
- **Kept buffers** (a `Scratch`, see [decoding.md](decoding.md)): a decode on a scratch makes
  the charges of a fresh decode, in the same order, so every limit gives a fresh decode's outcome.
  A buffer's capacity beyond what the call needs is booked once all of those charges are made
  (the buffers are then sized): if it fits in `max_alloc`, the call's peak includes it; if not,
  the buffers are cut to what the call needs, so a call never fails for what an earlier, larger
  decode left. Kept buffers the call does not use (a larger stream's, an alpha plane's) are not
  charged; they are the scratch owner's memory, which stays until the `Scratch` drops. A few small
  vectors (the quantizers of a tile, the ring's slots of the right size) keep their capacity
  without being counted again: the first is inside the quantizer bound charged per call, the
  second inside the pipeline's hold. The header's tile tables are cut to the size the stream needs
  when it is parsed. The transform half's copies of them (and the alpha copies) keep their
  capacity: the pipeline charges them by their length when it runs (the reserve), and they stay
  uncounted between pipelined decodes. The prediction vectors a call leaves empty stay uncounted.
- **Cancel** is a flag checked once per macroblock row (by both pipeline stages), and between 16
  MiB chunks of the output buffers' zero fill; a set flag returns `Cancelled` (a pipelined call on
  a busy pool returns it once a pool thread has picked up its queued worker job). `decode_config` and
  `Decoder::new` take no cancel flag: parsing a header is bounded by the length of the input. The
  header's tile tables (two copies with an alpha plane) are the one allocation there that grows with
  the input; they are capped by `max_alloc` (`AllocLimit`), and a decode then books them on its
  ledger.
- A panic anywhere in the decoder is caught and returned as `Error::Internal`, as Go's `recover`
  returned `ErrInternal`.
- **Odd strides** (a deviation from Go): with 16-bit samples (`Gray16`, `Bgr48`) jxrlib and Go
  take `stride / 2` samples a row, so an odd stride silently drops its last byte. `decode_into`
  returns `OddStride` instead, after the `DstTooSmall` check, and writes nothing.

One `Error` enum, one variant per Go error, with Go's message text:

| Rust | Go |
|---|---|
| `Internal(msg)` | `ErrInternal` |
| `PixelFormat` | `errFormat` (container pixel format not one of the five) |
| `TooLarge` | `errTooLarge` (over 2^40 pixels) |
| `Container` | `container.ErrFormat` |
| `Codestream` | `codec.ErrFormat` |
| `Unsupported` | `codec.ErrUnsupported` |
| `DstTooSmall` | `errDst` |
| `AllocLimit` | `errBudgetExceeded` |
| `Cancelled` | `context.Canceled` |
| `LimitExceeded`, `OutOfMemory`, `OddStride` | new |
