# Dependencies

Third-party dependencies: one, `rayon` (pinned `=1.12.0`, default features), with what it pulls in
(`rayon-core`, `crossbeam-deque`, `crossbeam-epoch`, `crossbeam-utils`, `either`; exact versions in
`Cargo.lock`). All are pure Rust: no C code, no `-sys` crate, no build script that compiles anything.
It is here only for `ThreadPool`: safe Rust cannot reuse a worker thread for jobs that borrow
short-lived buffers (`std::thread::scope` starts and joins its threads on every call, tens of
microseconds each, which made small tiles slower than Go), and rayon's scope can. It backs the
caller's pool and the lazy shared pool (see [limits.md](limits.md) and [threading.md](threading.md)); `src/par.rs` is the only module that
uses it, and the CI image fetches it in a layer before the source is copied.
`libfuzzer-sys` lives only in the separate `tests/fuzz/` crate.
