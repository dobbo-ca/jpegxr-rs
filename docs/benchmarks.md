# Benchmarks

`bench.yml` runs `examples/bench.rs` and `oracle -bench` on the same streams in one job and prints
the ratio per input class in the job summary, in two tables: Go at its default parallelism against
Rust at the default `max_threads`, and both on one thread (`GOMAXPROCS=1`, `BENCH_THREADS=1`).

The same two tables follow for the x86-64-v4 builds: Go built with `GOAMD64=v4`, Rust with
`-C target-cpu=x86-64-v4` and the false-deps features ([performance.md](performance.md)). The script
prints them when `oracle-v4` and `bench-v4` are on `PATH` and the CPU has AVX-512 (`avx512bw` in
`/proc/cpuinfo`; hosted x86_64 runners may not); `BENCH_NO_V4=1` skips them.

`bench.yml` runs on a GitHub-hosted `ubuntu-24.04` runner. Timings there are noisy and are not
comparable with numbers from other hardware.

## Hosted runner baseline

Bench run 45719955852 (2026-11-25): `ubuntu-24.04`, 2 CPUs, AVX-512 present (the v4 tables are
real v4 builds). Rust / Go, ms per MB of output:

| class | default | one thread | v4 default | v4 one thread |
|---|---|---|---|---|
| conformance (8 streams, 6.39 MB) | 0.85 | 0.95 | 0.87 | 1.05 |
| synthetic classes | 0.77 to 0.92 | 0.77 to 0.93 | 0.79 to 0.99 | 0.80 to 0.99 |
| all | 0.84 | 0.94 | 0.87 | 1.03 |

The Rust v4 build is slower than the default build here (all, one thread: 6.06 against 5.60 ms
per MB), so no v4 flags are recommended.
