# jpegxr-rs

A pure Rust JPEG XR (ITU-T T.832) decoder, built for the JPEG XR tiles inside Zeiss CZI slides.
Decode only. A port of the Go library [jpegxr-go](https://github.com/dobbo-ca/jpegxr-go), which is
itself a Go port of the decode path of [jxrlib](https://github.com/4creators/jxrlib). Pure Rust,
one dependency (`rayon`), `unsafe` only in the SIMD tiers. Apache-2.0 (see `LICENSE` and `NOTICE`).

```toml
jpegxr = { git = "https://github.com/dobbo-ca/jpegxr-rs" }
```

## Quick start

```rust,no_run
use jpegxr::{decode, Limits};
use std::sync::atomic::AtomicBool;

let bytes = std::fs::read("tile.jxr")?;
let cancel = AtomicBool::new(false);
let limits = Limits { max_threads: 2, max_alloc: 256 << 20, ..Limits::default() };
let img = decode(&bytes, &limits, &cancel)?; // jpegxr::Image
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Resource limits

Every entry point takes a `Limits`; there is no unlimited call.

| field | default | unit |
|---|---|---|
| `max_alloc` | 2 GiB | bytes, the enforced peak of one call, all threads together |
| `max_pixels` | 2^30 | pixels |
| `max_width` | 2^20 | pixels |
| `max_height` | 2^20 | pixels |
| `max_threads` | `available_parallelism` | threads, the calling thread included |

`max_threads = 1` spawns no thread and never touches a pool. A call without a caller's pool uses a
lazy shared pool inside the crate; a pool passed with `Decoder::with_pool` always wins. A set cancel
flag returns `Cancelled`. Errors per limit, pools and cancel: [limits](docs/limits.md),
[threading](docs/threading.md).

## Docs

- [Decoding API](docs/decoding.md) and [differences from Go](docs/go-differences.md)
- [Resource limits](docs/limits.md), [threading](docs/threading.md), [unsafe and SIMD](docs/safety.md)
- [Parity with Go](docs/parity.md), [fuzzing](docs/fuzzing.md), [benchmarks](docs/benchmarks.md)
- [Tests and CI](docs/tests.md), [dependencies](docs/dependencies.md)
- [Performance](docs/performance.md): build profile and flags

This crate is a Rust port of github.com/dobbo-ca/jpegxr-go.
