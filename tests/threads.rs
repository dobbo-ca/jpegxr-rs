//! Threads (conventions, Threading): the output and the error are the same for every
//! `max_threads`, partial output included, and the threads of a call stay under `max_threads`.
//! `uncapped` sends every image, however small, through the pipeline.

mod common;

use common::*;
use jpegxr::{Decoder, Error, Format, Image, Limits, decode_config, decode_probe};
use std::sync::atomic::{AtomicBool, Ordering};

const THREADS: [usize; 4] = [2, 3, 4, 8];

fn limits(max_threads: usize) -> Limits {
    Limits {
        max_threads,
        ..Limits::default()
    }
}

/// The synthetic streams, the fuzz seeds, and some conformance streams when the corpus is there.
fn streams() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for dir in ["tests/data/synth", "tests/data/fuzz", "tests/fuzz/seeds"] {
        let mut names: Vec<_> = std::fs::read_dir(root().join(dir))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_none_or(|x| x != "txt"))
            .collect();
        names.sort();
        for p in names {
            out.push((p.display().to_string(), std::fs::read(&p).unwrap()));
        }
    }
    // Every fourth conformance stream under 400 KB, at most 40.
    let conf: Vec<_> = jxr_files(&conformance_dir())
        .into_iter()
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() < 400_000))
        .step_by(4)
        .take(40)
        .collect();
    for p in conf {
        out.push((p.display().to_string(), std::fs::read(&p).unwrap()));
    }
    out
}

/// Streams that decode_config accepts and that are small enough to decode in a test.
fn decodable(data: &[u8]) -> bool {
    decode_config(data, &Limits::default()).is_ok_and(|c| (c.width * c.height) as u64 <= 1 << 22)
}

fn row_bytes(w: usize, f: Format) -> usize {
    match f {
        Format::Gray8 => w,
        Format::Gray16 => 2 * w,
        Format::Bgr24 => 3 * w,
        Format::Bgr48 => 6 * w,
    }
}

/// `decode` (k none) or `decode_component`, with the threads the call used.
fn image(data: &[u8], k: Option<usize>, t: usize) -> (Result<Image, Error>, usize) {
    let cancel = AtomicBool::new(false);
    let (r, probe) = decode_probe(data, k, &limits(t), &cancel, true);
    (r, probe.threads_peak)
}

/// `decode_into` into a buffer of 0x5a with rows padded by `pad` bytes: the result, and every
/// byte of the buffer, partial output after an error included.
fn into(data: &[u8], pad: usize, t: usize) -> Option<(Result<(), Error>, Vec<u8>, usize)> {
    let cancel = AtomicBool::new(false);
    let d = Decoder::new(data, &limits(t)).ok()?;
    let (w, h, f) = d.config();
    let row = row_bytes(w, f);
    let stride = row + pad;
    let mut dst = vec![0x5a; if h == 0 { 0 } else { (h - 1) * stride + row }];
    let (r, probe) = d.decode_into_probe(&mut dst, stride, &cancel, true);
    Some((r, dst, probe.threads_peak))
}

#[test]
fn every_thread_count_gives_the_same_output() {
    let all = streams();
    assert!(all.len() > 30);
    let mut ran = 0;
    for (name, data) in &all {
        if !decodable(data) {
            continue;
        }
        ran += 1;
        for k in [None, Some(0), Some(1), Some(2), Some(3)] {
            let (want, peak1) = image(data, k, 1);
            assert_eq!(peak1, 1, "{name} {k:?}: one thread");
            for t in THREADS {
                let (got, peak) = image(data, k, t);
                assert_eq!(got, want, "{name} {k:?}: {t} threads");
                assert!(peak <= t, "{name} {k:?}: {t} threads, peak {peak}");
            }
        }
        // Tight and padded strides (even: 16-bit samples take stride / 2).
        for pad in [0, 8] {
            let Some((want, bytes, peak1)) = into(data, pad, 1) else {
                continue;
            };
            assert_eq!(peak1, 1, "{name}: one thread");
            for t in THREADS {
                let (got, b, peak) = into(data, pad, t).unwrap();
                assert_eq!(got, want, "{name} pad {pad}: {t} threads");
                assert!(b == bytes, "{name} pad {pad}: {t} threads, other bytes");
                assert!(peak <= t, "{name} pad {pad}: {t} threads, peak {peak}");
            }
        }
    }
    assert!(ran > 20, "only {ran} streams ran");
}

/// A stream cut at every sixteenth gives the same error and the same partial output on every
/// thread count.
#[test]
fn truncated_streams_agree() {
    for (name, data) in streams().into_iter().filter(|(n, _)| n.contains("synth")) {
        for i in 1..16 {
            let cut = &data[..data.len() * i / 16];
            for k in [None, Some(0)] {
                let want = image(cut, k, 1).0;
                for t in THREADS {
                    assert_eq!(
                        image(cut, k, t).0,
                        want,
                        "{name} cut {i}/16 {k:?}: {t} threads"
                    );
                }
            }
            if let Some((want, bytes, _)) = into(cut, 0, 1) {
                for t in THREADS {
                    let (got, b, _) = into(cut, 0, t).unwrap();
                    assert_eq!(got, want, "{name} cut {i}/16: {t} threads");
                    assert!(b == bytes, "{name} cut {i}/16: {t} threads, other bytes");
                }
            }
        }
    }
}

/// A stream with several macroblock rows uses a second thread, in a few tries (a pool thread that
/// wakes after the caller did everything is not a worker the call used). Under 4 CPUs (a 2-vCPU
/// hosted runner) the other tests of this binary keep the CPUs busy, so the pool thread may never
/// get one in time: the check runs only with 4 CPUs or more.
#[test]
fn two_threads_are_used() {
    if std::thread::available_parallelism().map_or(1, |n| n.get()) < 4 {
        return;
    }
    let data = std::fs::read(testdata("synth/Bgr24_lossless_untiled_ov1.jxr")).unwrap();
    let best = (0..100)
        .map(|_| image(&data, None, 2).1)
        .find(|&p| p > 1)
        .unwrap_or(1);
    assert_eq!(best, 2);
}

/// A cancel flag set by another thread stops a call that runs the pipeline.
#[test]
fn cancel_from_another_thread() {
    let data = std::fs::read(testdata("synth/Bgr24_lossless_untiled_ov1.jxr")).unwrap();
    let cancel = AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            cancel.store(true, Ordering::Relaxed);
        });
        // Decodes until the flag shows: each is Ok or Cancelled.
        while !cancel.load(Ordering::Relaxed) {
            let r = decode_probe(&data, None, &limits(2), &cancel, true).0;
            assert!(r.is_ok() || r == Err(Error::Cancelled));
        }
    });
    for t in [2, 3] {
        assert_eq!(
            decode_probe(&data, None, &limits(t), &cancel, true).0,
            Err(Error::Cancelled)
        );
    }
}

/// Fuzz crashes: row 0 fails on the calling thread before the tile quantizers exist, and the
/// worker must not run row 1 on that state (it panicked in dequantize_macroblock).
#[test]
fn no_row_runs_after_a_failed_one() {
    for n in ["pipeline-crash-1", "pipeline-crash-2"] {
        let data = std::fs::read(root().join("tests/fuzz/seeds").join(n)).unwrap();
        let stream = &data[..data.len() - 1]; // the fuzz target's pad byte
        for pad in [0, 2, 4] {
            let (r1, b1, _) = into(stream, pad, 1).unwrap();
            let (r2, b2, _) = into(stream, pad, 2).unwrap();
            assert_eq!(r1, Err(Error::Codestream), "{n}");
            assert_eq!(r1, r2, "{n}");
            assert!(b1 == b2, "{n}: bytes differ");
        }
    }
}
