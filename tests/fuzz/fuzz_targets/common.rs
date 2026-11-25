// Shared by the targets (`include!`d; not a target itself).

use jpegxr::{Error, Limits};
use std::sync::atomic::AtomicBool;

/// Go's FuzzDecode skips images over 1<<22 pixels; 1<<20 here, so an execution stays fast. The
/// decoder's estimated peak (max_alloc) stays under -malloc_limit_mb=512, so an allocation that
/// the limits did not charge is a crash.
fn limits() -> Limits {
    Limits {
        max_alloc: 256 << 20,
        max_pixels: 1 << 20,
        max_width: 1 << 20,
        max_height: 1 << 20,
        // One thread: the fuzzer's own process is the only one, and the threaded path has its
        // differential in `decoder.rs`.
        max_threads: 1,
    }
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

/// A panic caught by the decoder is `Error::Internal`: the decoder has a bug.
fn no_internal<T>(r: &Result<T, Error>) {
    if let Err(Error::Internal(m)) = r {
        panic!("internal error: {m}");
    }
}

/// An error that depends on how much the call charges, not on the stream.
fn is_limit(e: &Error) -> bool {
    matches!(
        e,
        Error::AllocLimit { .. } | Error::LimitExceeded(_) | Error::OutOfMemory
    )
}

/// A runner without the SIMD tier would fuzz only the scalar one: fail instead.
#[allow(dead_code)]
fn check_tier() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let t = jpegxr::kernels::tier_name(&limits());
        assert_ne!(t, "scalar", "no SIMD tier: the scalar tier only");
        eprintln!("kernel tier fuzzed against scalar: {t}");
    });
}

/// `n` values of about `bits` bits, either sign, cycled from `rest` (zero if `rest` is empty).
#[allow(dead_code)]
fn values(rest: &[u8], n: usize, bits: u32) -> Vec<i32> {
    let b = |i: usize| rest.get(i % rest.len().max(1)).copied().unwrap_or(0);
    (0..n)
        .map(|i| {
            i32::from_le_bytes([b(4 * i), b(4 * i + 1), b(4 * i + 2), b(4 * i + 3)]) >> (32 - bits)
        })
        .collect()
}
