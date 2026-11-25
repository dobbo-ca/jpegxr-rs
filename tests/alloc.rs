//! Resource controls (conventions, Limits): `TestEstimate` (the header estimate bounds what a
//! decode allocates), and the heap peak of a call stays under `max_alloc` and under what its
//! ledger charged, for several of each and for 1, 2 and 8 threads. This binary counts every
//! allocation, so it holds exactly one test: another test running beside it would be counted too.

mod common;

use common::*;
use jpegxr::{Decoder, Error, Format, Limits, decode_config, decode_probe};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

struct Counting;

/// Bytes ever allocated, for `TestEstimate`.
static ALLOCATED: AtomicU64 = AtomicU64::new(0);
/// Live heap bytes and their peak, for the limits.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    ALLOCATED.fetch_add(n as u64, Relaxed);
    let now = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: every method forwards to `System` with the caller's arguments, so `System`'s contract
// carries over; the counters are atomics.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, which `System` shares.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: as in `alloc`.
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` and `l` come from a matching `alloc` call, so they are valid for `System`.
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        // SAFETY: as in `dealloc`, with the caller's new size.
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            if n >= l.size() {
                grow(n - l.size());
            } else {
                LIVE.fetch_sub(l.size() - n, Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// What a call may hold beyond its ledger, now that the header's tile tables are charged: the
/// fixed buffers only. One bit reader (a few hundred bytes), a 16-entry slice list per row, the
/// per-row channel list, `Shared` and the `Arc`s of the pipeline, and rayon's scope and job boxes
/// (each well under 1 KiB). About 2 KiB in all; 8 KiB leaves room for allocator rounding of them.
const SLACK: usize = 8 << 10;

/// Runs `f` and returns its value and the heap peak above what was live before it.
fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let r = f();
    (r, PEAK.load(Relaxed).saturating_sub(base))
}

fn limits(max_threads: usize, max_alloc: u64) -> Limits {
    Limits {
        max_threads,
        max_alloc,
        ..Limits::default()
    }
}

fn stride(w: usize, f: Format) -> usize {
    match f {
        Format::Gray8 => w,
        Format::Gray16 => 2 * w,
        Format::Bgr24 => 3 * w,
        Format::Bgr48 => 6 * w,
    }
}

/// The bytes `decode` adds to the estimate: the raw decode and the converted image.
fn output_bytes(w: usize, h: usize, f: Format) -> u64 {
    let per_px = match f {
        Format::Gray8 => 1 + 1,
        Format::Gray16 => 2,
        Format::Bgr24 => 3 + 4,
        Format::Bgr48 => 6 + 8,
    };
    (per_px * w * h) as u64
}

/// The streams: the synthetic ones, and a few conformance ones when the corpus is there.
fn streams() -> Vec<(String, Vec<u8>)> {
    let mut files = jxr_files(&testdata("synth"));
    assert!(files.len() >= 21);
    files.extend(
        jxr_files(&conformance_dir())
            .into_iter()
            .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() < 200_000))
            .step_by(40)
            .take(6),
    );
    files
        .into_iter()
        .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

fn is_alloc_limit(r: &Result<jpegxr::Image, Error>) -> bool {
    matches!(r, Err(Error::AllocLimit { .. }))
}

/// The only test in this binary, so no other thread allocates while it measures.
#[test]
fn estimate_and_peaks() {
    let cancel = AtomicBool::new(false);
    let streams = streams();

    // Start the shared pool before anything is measured: its threads allocate once.
    for (_, data) in &streams {
        if decode_probe(data, None, &limits(2, 1 << 31), &cancel, true)
            .0
            .is_ok()
        {
            break;
        }
    }

    // TestEstimate: the header estimate bounds what a decode allocates.
    for (name, data) in streams.iter().filter(|(n, _)| n.contains("synth")) {
        let l = limits(1, 1 << 31);
        let d = Decoder::new(data, &l).unwrap();
        let (w, h, f) = d.config();
        let s = stride(w, f);
        let mut dst = vec![0u8; s * h];
        let before = ALLOCATED.load(Relaxed);
        d.decode_into(&mut dst, s, &cancel).unwrap();
        let got = ALLOCATED.load(Relaxed) - before;
        assert!(
            got <= d.estimate(),
            "{name}: allocated {got}, estimate {}",
            d.estimate()
        );
    }

    for (name, data) in &streams {
        let Ok(c) = decode_config(data, &Limits::default()) else {
            continue;
        };
        let d = Decoder::new(data, &Limits::default()).unwrap();
        let (w, h, f) = d.config();
        assert_eq!((c.width, c.height), (w, h));

        // The serial ledger peak stays under Go's estimate plus the output: the limit a stream
        // passes on Go's side it passes here.
        let (base, _) = decode_probe(data, None, &limits(1, 1 << 31), &cancel, false);
        let Ok(base_img) = base else { continue };
        let serial_peak = decode_probe(data, None, &limits(1, 1 << 31), &cancel, false)
            .1
            .charged_peak;
        assert!(
            serial_peak <= d.estimate() + output_bytes(w, h, f),
            "{name}: ledger peak {serial_peak} over the estimate {} plus the output",
            d.estimate()
        );
        // Go's bound is the cap: one byte less is AllocLimit, and exactly it is enough for the
        // ledger, whose serial peak is under it.
        let cap = d.estimate() + output_bytes(w, h, f);
        let (r, _) = decode_probe(data, None, &limits(1, cap - 1), &cancel, false);
        assert!(is_alloc_limit(&r), "{name}: {r:?}");
        let (r, _) = decode_probe(data, None, &limits(1, cap), &cancel, false);
        assert_eq!(r.as_ref(), Ok(&base_img), "{name}");

        for t in [1, 2, 8] {
            let (_, probe) = decode_probe(data, None, &limits(t, 1 << 31), &cancel, true);
            let charged = probe.charged_peak;
            let caps = [
                d.estimate() + output_bytes(w, h, f),
                charged,
                charged - 1,
                charged / 2,
                64 << 10,
            ];
            for max_alloc in caps {
                let ((r, probe), heap) =
                    measured(|| decode_probe(data, None, &limits(t, max_alloc), &cancel, true));
                match &r {
                    Ok(img) => {
                        assert_eq!(img, &base_img, "{name} t {t} max {max_alloc}");
                        assert!(
                            probe.charged_peak <= max_alloc,
                            "{name} t {t}: charged {} over {max_alloc}",
                            probe.charged_peak
                        );
                        assert!(
                            heap <= max_alloc as usize + SLACK,
                            "{name} t {t}: heap peak {heap} over {max_alloc}"
                        );
                        assert!(
                            heap <= probe.charged_peak as usize + SLACK,
                            "{name} t {t}: heap peak {heap} over the charge {}",
                            probe.charged_peak
                        );
                    }
                    Err(Error::AllocLimit { .. }) => {}
                    Err(e) => panic!("{name} t {t} max {max_alloc}: {e:?}"),
                }
                // The same outcome as one thread: a thread never causes an error.
                let (one, _) = decode_probe(data, None, &limits(1, max_alloc), &cancel, true);
                assert_eq!(r.is_ok(), one.is_ok(), "{name} t {t} max {max_alloc}");
                if let (Ok(a), Ok(b)) = (&r, &one) {
                    assert_eq!(a, b);
                }
            }
        }
    }
}
