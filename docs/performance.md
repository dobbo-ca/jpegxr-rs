# Performance

How to build and call this crate for speed. The measured numbers are in this page's tables and in the `bench.yml` job summary.

## Build profile

A dependent crate's own release profile applies to this code, so the profile that counts is the
consumer's. For a worker that decodes tiles:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
panic = "abort"
```

**Caution:** use the v4 flags only when every machine that runs the binary has AVX-512; otherwise
use the default build.

```sh
RUSTFLAGS="-C target-cpu=x86-64-v4 -C target-feature=+false-deps-tzcnt,+false-deps-bls" cargo build --release
```

- `lto = "fat"` and `codegen-units = 1` let the kernels and the loops that call them inline across
  modules and across the crate boundary.
- `panic = "abort"` removes the unwinding tables. It also changes one thing here: a panic on a pool
  thread is re-raised on the calling thread (rayon), and with `abort` the process stops at the
  panic. The crate does not panic on any input (clippy denies `unwrap`, `expect` and `panic` in its
  non-test code), so this only matters for a bug.
- `-C target-cpu=x86-64-v4` lets the compiler use AVX-512 everywhere, not only in kernels (a CPU
  without it dies with `SIGILL`; the default build runs everywhere). This crate has an AVX2 tier and a NEON tier, no AVX-512
  tier, so the flag changes only the code the compiler generates, not the tier that runs.
- `-C target-feature=+false-deps-tzcnt,+false-deps-bls` goes with it. On Zen 4 and Zen 5 `tzcnt`,
  `blsr`, `blsi` and `blsmsk` wait for the last write to their output register, and LLVM breaks that
  wait only under `target-cpu=znver4`/`znver5` tuning, not under `x86-64-v4`. Other bit-twiddling
  Rust code has been seen to run 30 to 60% slower without the two features. Whether the decoder's
  bit loops gain is measured by the v4 column of the bench; the flags are in the recommendation so
  every crate of a binary is built the same way. rustc warns that it does not know the two features and passes them
  to LLVM, which does.
- aarch64: do not set any of these flags (they are x86 only). NEON is part of the aarch64 baseline,
  so the default target already uses it; `lto`, `codegen-units` and `panic` apply as above.

## SIMD dispatch at run time

The SIMD tier is chosen at run time, not by the build flags: AVX2 when the CPU has it (NEON on
aarch64), whatever `target-cpu` says ([safety.md](safety.md)). `JPEGXR_SIMD=scalar` forces the
scalar tier, which is the reference the SIMD kernels are tested against; the output is the same in
every tier. The `ci` stage also runs `cargo test --release --lib --test jpegxr` with the v4 flags,
so the v4 build is held to the same pinned hashes.

## Reusing a `Decoder`

Keep one `Scratch` per worker thread and make each tile's decoder with `Decoder::reusing`
(example and rules in [decoding.md](decoding.md#reusing-buffers)). With `max_threads` 1 the second
decode of the same stream makes no allocator call (`tests/reuse_alloc.rs`); a stream that is no
larger in any buffer dimension (width, height, colour format and channels, tiling, quantizers,
index-table entries, alpha) than one decoded before usually needs none, and a larger one allocates
what grows. With threads a few allocations remain (the rayon job of the pipeline worker, its injector block, the list of
finished rows), bounded and the same for any image size. The output and the limit outcomes are
those of a fresh decoder. `examples/bench.rs` decodes every stream on one `Scratch`, as Go's bench
reuses one `Decoder`.

## PGO and BOLT

Profile-guided optimisation and BOLT apply to the final binary, so the consumer does them (the
worker that links this crate); the library does not change. `pgo.yml` (dispatch only, no secrets)
measures how much they gain on the bench example: it builds the example with `cargo-pgo`, trains it
on the bench's own streams (`tests/data/synth` and the conformance subset of `bench.sh`, on one
thread and on the default threads; the BOLT profile on one thread only), then times the default, the
PGO and the PGO plus BOLT binaries with `tests/parity/bench.sh` and prints the tables in the job
summary. BOLT is optional there: if the package is not available the job reports PGO only.

Rust ms per MB of output (lower is better), an earlier `pgo.yml` run on a
Ryzen 7 9800X3D (the numbers are not comparable with a hosted runner's); the change is against the default build:

| input class | default (1 thread) | PGO | PGO + BOLT | default (default threads) | PGO | PGO + BOLT |
|---|---|---|---|---|---|---|
| Bgr24_lossless | 6.240 | 5.589 (-10.4%) | 5.552 (-11.0%) | 6.268 | 5.579 (-11.0%) | 5.553 (-11.4%) |
| Bgr24_lossy | 3.987 | 3.649 (-8.5%) | 3.607 (-9.5%) | 4.005 | 3.633 (-9.3%) | 3.607 (-9.9%) |
| Bgr48_lossless | 5.070 | 4.173 (-17.7%) | 4.161 (-17.9%) | 5.090 | 4.176 (-18.0%) | 4.161 (-18.3%) |
| Gray16_lossless | 4.366 | 3.656 (-16.3%) | 3.659 (-16.2%) | 4.396 | 3.655 (-16.9%) | 3.671 (-16.5%) |
| Gray8_lossless | 6.087 | 5.461 (-10.3%) | 5.471 (-10.1%) | 6.125 | 5.448 (-11.1%) | 5.485 (-10.4%) |
| Gray8_lossy | 5.243 | 4.769 (-9.0%) | 4.758 (-9.3%) | 5.240 | 4.750 (-9.4%) | 4.754 (-9.3%) |
| N3x16_lossless | 4.096 | 3.626 (-11.5%) | 3.609 (-11.9%) | 4.124 | 3.629 (-12.0%) | 3.610 (-12.5%) |
| N3x16_lossy | 3.467 | 3.167 (-8.7%) | 3.154 (-9.0%) | 3.487 | 3.166 (-9.2%) | 3.155 (-9.5%) |
| conformance | 3.829 | 3.504 (-8.5%) | 3.492 (-8.8%) | 2.846 | 2.605 (-8.5%) | 2.614 (-8.2%) |
| all | 3.850 | 3.516 (-8.7%) | 3.505 (-9.0%) | 2.927 | 2.671 (-8.7%) | 2.678 (-8.5%) |

Result: PGO gains about 9% on every class, single-threaded and threaded (Rust against Go goes from
0.95 to about 0.87 on one thread). BOLT adds at most 0.6 points over PGO, and nothing with threads.
The gain is an upper bound for another workload: the training input is the benchmark's own, so the
profile fits it more closely than it would fit a worker's real tile stream. A worker that wants PGO
trains on its own tiles.

## v4 build

The bench's v4 tables (Go `GOAMD64=v4`, Rust with the flags above) come from `bench.yml`.
An earlier run (after buffer reuse, on a Ryzen 7 9800X3D), all 29 streams, ms
per MB of output (lower is better):

| build | Go, default threads | Rust, default threads | Rust / Go | Go, 1 thread | Rust, 1 thread | Rust / Go |
|---|---|---|---|---|---|---|
| default | 4.026 | 2.890 | 0.72 | 4.038 | 3.817 | 0.95 |
| x86-64-v4 | 3.992 | 2.870 | 0.72 | 3.995 | 4.066 | 1.02 |

The v4 build does not help this crate: with threads it is the same as the default build, and on
one thread Rust is about 7% slower (4.066 against 3.817 ms/MB, the conformance streams most). The
cause is not analysed. A binary that links this crate together with others can still use the v4
flags for them; in the table above the v4 Rust build is within 1.10x of Go's v4 build.
