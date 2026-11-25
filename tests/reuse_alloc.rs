//! Buffer reuse (`Scratch`): the second decode of a stream on one `Scratch` makes no call to the
//! allocator with one thread, and a small bounded number with two, whatever the image size. This
//! binary counts every allocator call, so it holds exactly one test: another test running beside
//! it would be counted too.

mod common;

use common::*;
use jpegxr::{Decoder, Format, Limits, Scratch, ThreadPool};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

struct Counting;

/// Calls of `alloc`, `alloc_zeroed` and `realloc` since the last reset.
static CALLS: AtomicU64 = AtomicU64::new(0);

// SAFETY: every method forwards to `System` with the caller's arguments, so `System`'s contract
// carries over; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Relaxed);
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, which `System` shares.
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Relaxed);
        // SAFETY: as in `alloc`.
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` and `l` come from a matching `alloc` call, so they are valid for `System`.
        unsafe { System.dealloc(p, l) };
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CALLS.fetch_add(1, Relaxed);
        // SAFETY: as in `dealloc`, with the caller's new size.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The allocator calls a pipelined decode may still make on a warm `Scratch`, none of which grows
/// with the image. In `par::pipe`: the list of finished rows (`Ring::done`, one allocation for up
/// to three rows). In rayon: the box of the job that `in_place_scope` spawns for the worker, and
/// the injector's block, which it adds once in 63 jobs (at most one here). That is 3; 2 more are
/// margin for a rayon version that adds one.
const THREADED_MAX: u64 = 5;

fn stride(w: usize, f: Format) -> usize {
    match f {
        Format::Gray8 => w,
        Format::Gray16 => 2 * w,
        Format::Bgr24 => 3 * w,
        Format::Bgr48 => 6 * w,
    }
}

#[test]
fn a_warm_scratch_decodes_without_allocating() {
    let cancel = AtomicBool::new(false);
    let one = Limits {
        max_threads: 1,
        ..Limits::default()
    };
    let mut files = jxr_files(&testdata("synth"));
    files.extend(jxr_files(&testdata("alpha")));
    let mut scratch = Scratch::default();
    let mut checked = 0;
    // The largest stream that decodes, for the threaded case.
    let mut largest: Option<(usize, std::path::PathBuf)> = None;
    for path in &files {
        let data = std::fs::read(path).unwrap();
        // The warm-up decode, which also sizes `dst`.
        let Ok(d) = Decoder::reusing(&data, &one, None, &mut scratch) else {
            continue;
        };
        let (w, h, f) = d.config();
        let s = stride(w, f);
        let mut dst = vec![0u8; s * h];
        if d.decode_into(&mut dst, s, &cancel).is_err() {
            continue;
        }
        drop(d);
        let first = dst.clone();
        dst.fill(0xa5);

        CALLS.store(0, Relaxed);
        let d = Decoder::reusing(&data, &one, None, &mut scratch).unwrap();
        d.decode_into(&mut dst, s, &cancel).unwrap();
        drop(d);
        let calls = CALLS.load(Relaxed);

        assert_eq!(
            calls,
            0,
            "{}: allocator calls in a warm decode",
            path.display()
        );
        assert!(dst == first, "{}: the warm decode differs", path.display());
        checked += 1;
        if largest.as_ref().is_none_or(|(n, _)| data.len() > *n) {
            largest = Some((data.len(), path.clone()));
        }
    }
    assert!(checked >= 20, "only {checked} streams decoded");

    // Two threads, on a pool of our own whose thread is started before anything is counted.
    // `uncapped` runs even a 64 x 64 stream through the pipeline.
    let (_, path) = largest.unwrap();
    let data = std::fs::read(&path).unwrap();
    let two = Limits {
        max_threads: 2,
        ..Limits::default()
    };
    let pool = ThreadPool::new(1).unwrap();
    let mut scratch = Scratch::default();
    let (w, h, f) = Decoder::new(&data, &two).unwrap().config();
    let s = stride(w, f);
    let mut serial = vec![0u8; s * h];
    Decoder::new(&data, &one)
        .unwrap()
        .decode_into(&mut serial, s, &cancel)
        .unwrap();
    let mut dst = vec![0u8; s * h];
    let mut calls = 0;
    // The first rounds size the buffers and start whatever rayon starts lazily; the last counts.
    for round in 0..3 {
        dst.fill(0xa5);
        CALLS.store(0, Relaxed);
        let d = Decoder::reusing(&data, &two, Some(&pool), &mut scratch).unwrap();
        let (r, probe) = d.decode_into_probe(&mut dst, s, &cancel, true);
        drop(d);
        calls = CALLS.load(Relaxed);
        r.unwrap();
        assert!(dst == serial, "{}: round {round} differs", path.display());
        eprintln!(
            "{}: round {round}: {calls} allocator calls, {} threads",
            path.display(),
            probe.threads_peak
        );
    }
    assert!(
        calls <= THREADED_MAX,
        "{}: {calls} allocator calls in a warm pipelined decode, over {THREADED_MAX}",
        path.display()
    );
}
