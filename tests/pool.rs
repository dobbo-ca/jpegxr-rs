//! Which threads a call runs on (conventions, Threading): `max_threads` 1 never starts the crate's
//! shared pool, a caller's pool is used instead of it, and a call that is worth threading starts
//! it. The shared pool is process state, so this binary holds one test and runs the steps in order.

mod common;

use common::*;
use jpegxr::{Decoder, Limits, ThreadPool, decode_probe, shared_pool_started};
use std::sync::atomic::AtomicBool;

fn limits(max_threads: usize) -> Limits {
    Limits {
        max_threads,
        ..Limits::default()
    }
}

/// The most workers of a decode in up to 100 tries (a pool task that wakes after the caller took
/// every row is not a worker the call used).
fn peak(data: &[u8], t: usize, pool: Option<&ThreadPool>) -> usize {
    let cancel = AtomicBool::new(false);
    let mut best = 1;
    for _ in 0..100 {
        best = best.max(match pool {
            None => {
                decode_probe(data, None, &limits(t), &cancel, true)
                    .1
                    .threads_peak
            }
            Some(p) => {
                let d = Decoder::with_pool(data, &limits(t), p).unwrap();
                let (w, h, f) = d.config();
                let stride = match f {
                    jpegxr::Format::Gray8 => w,
                    jpegxr::Format::Gray16 => 2 * w,
                    jpegxr::Format::Bgr24 => 3 * w,
                    jpegxr::Format::Bgr48 => 6 * w,
                };
                let mut dst = vec![0; stride * h];
                d.decode_into_probe(&mut dst, stride, &cancel, true)
                    .1
                    .threads_peak
            }
        });
        if best > 1 {
            break;
        }
    }
    best
}

#[test]
fn pools() {
    let data = std::fs::read(testdata("synth/Bgr24_lossless_untiled_ov1.jxr")).unwrap();
    let cancel = AtomicBool::new(false);
    let want = jpegxr::decode(&data, &limits(1), &cancel).unwrap();

    // max_threads 1 never starts the shared pool, whatever the input.
    assert!(!shared_pool_started());
    assert_eq!(peak(&data, 1, None), 1);
    assert!(!shared_pool_started());

    // A caller's pool is used instead of the shared one.
    let pool = ThreadPool::new(3).unwrap();
    assert_eq!(pool.threads(), 3);
    assert!(peak(&data, 4, Some(&pool)) > 1);
    assert!(!shared_pool_started(), "a caller's pool must win");

    // A pool of 1 thread gives a call two workers at most (itself and that one).
    let one = ThreadPool::new(1).unwrap();
    assert!(peak(&data, 8, Some(&one)) <= 2);
    assert!(!shared_pool_started());

    // No pool of the caller's and max_threads above 1: the shared pool starts, once, and the image
    // is the same.
    assert!(peak(&data, 4, None) > 1);
    assert!(shared_pool_started());
    assert_eq!(
        decode_probe(&data, None, &limits(4), &cancel, true)
            .0
            .unwrap(),
        want
    );
}
