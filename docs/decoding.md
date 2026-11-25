# Decoding

`decode` returns an `Image` whose `pix` has the byte layout of the Go `image` type named by its
`ColorModel`: `Rgba` (24bppBGR), `Rgba64` (48bppRGB, or 48bpp3Channels with channel k in R, G, B;
big-endian 16-bit samples), `Gray` (8bppGray) or `Gray16` (16bppGray, big endian).
`decode_config` returns the size and colour model only. `decode_component(data, k, ..)` returns
channel k of a 16-bit image as `Gray16`; for 48bpp3Channels it writes only that channel.

Many tiles of one slide: `Decoder::new(data, &limits)` parses the container and header;
`config()` gives the size and libCZI `Format` (`Bgr24`, `Bgr48`, `Gray8`, `Gray16`), and
`decode_into(&mut dst, stride, &cancel)` writes libCZI's layout (B, G, R for `Bgr24`, 16-bit
samples little endian) into the caller's buffer. `DstTooSmall` is returned, and nothing is
written, when `stride` is under one row or `dst` under `stride * (h - 1)` plus one row.

## Reusing buffers

A worker that decodes many tiles keeps one `Scratch` (per thread) and makes each decoder with
`Decoder::reusing`. The scratch keeps the header's tile tables and everything a decode works in:
the macroblock rows, prediction state, tile quantizers, coding contexts, bit readers, index table,
output offsets and the pipeline's ring of rows. The decoder borrows the scratch until it drops.

```rust,no_run
use jpegxr::{Decoder, Format, Limits, Scratch};
use std::sync::atomic::AtomicBool;

let limits = Limits { max_threads: 1, ..Limits::default() };
let cancel = AtomicBool::new(false);
let mut scratch = Scratch::default();
let mut dst = Vec::new();
for path in ["a.jxr", "b.jxr"] {
    let data = std::fs::read(path)?;
    let d = Decoder::reusing(&data, &limits, None, &mut scratch)?;
    let (w, h, f) = d.config();
    let stride = match f {
        Format::Gray8 => w,
        Format::Gray16 => 2 * w,
        Format::Bgr24 => 3 * w,
        Format::Bgr48 => 6 * w,
    };
    dst.resize(stride * h, 0);
    d.decode_into(&mut dst, stride, &cancel)?;
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

- With `max_threads` 1, the second decode of the same stream makes no allocator call
  (`tests/reuse_alloc.rs`). A stream that is no larger in any buffer dimension (width, height,
  colour format and channels, tiling, quantizers, index-table entries, alpha) than one decoded
  before usually needs none; a larger one allocates what grows.
- With threads, a small bounded number of allocations remains, the same for any image size: the
  rayon job that starts the pipeline's worker, its injector block (one in 63 jobs) and the list of
  finished rows in `par.rs`. The test allows five.
- The result does not depend on the history of the scratch. Every buffer is cleared and refilled
  as a new one at the start of each decode, so the sizes, formats, tiling, errors and cancels of
  earlier decodes change nothing: a decode is byte for byte what a fresh `Decoder::new` gives,
  `dst` after an error included (`tests/reuse.rs`, the fuzz target `decoder`, and the parity
  oracle's `reuse` op, which runs every input through one scratch).
- `Limits::max_alloc` is enforced as for a fresh decode: the same charges are made, in the same
  order, so every limit gives the same outcome. See [limits.md](limits.md) for how kept buffers
  count.
- The memory stays with the `Scratch` until it drops: the buffers of the largest stream it has
  seen. Drop it, or use a new one, to give it back.
- `decode` and `decode_component` return an `Image` and still allocate their output (and use a
  scratch of their own, per call), by design.

