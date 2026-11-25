//! Buffer reuse (`Scratch`): a decode on a kept `Scratch` gives what a fresh `Decoder::new` gives
//! (the result and every byte of `dst`), whatever the scratch held, and keeps the limit outcomes
//! of a fresh decode. The allocation counts are in `tests/reuse_alloc.rs`.

mod common;

use common::*;
use jpegxr::{Decoder, Error, Format, Limits, Scratch};
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn stride(w: usize, f: Format) -> usize {
    match f {
        Format::Gray8 => w,
        Format::Gray16 => 2 * w,
        Format::Bgr24 => 3 * w,
        Format::Bgr48 => 6 * w,
    }
}

/// The result of a decode and the whole of its `dst`; an error from the header is the outer one.
type Out = Result<(Result<(), Error>, Vec<u8>), Error>;

/// One decode on `d`; `uncapped` runs the pipeline whatever the size.
fn run(d: &Decoder<'_>, cancel: &AtomicBool, uncapped: bool) -> (Result<(), Error>, Vec<u8>) {
    let (w, h, f) = d.config();
    let s = stride(w, f);
    let mut dst = vec![0u8; s * h];
    let r = d.decode_into_probe(&mut dst, s, cancel, uncapped).0;
    (r, dst)
}

fn fresh(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Out {
    let d = Decoder::new(data, limits)?;
    Ok(run(&d, cancel, limits.max_threads > 1))
}

fn kept(data: &[u8], limits: &Limits, cancel: &AtomicBool, scratch: &mut Scratch) -> Out {
    let d = Decoder::reusing(data, limits, None, scratch)?;
    Ok(run(&d, cancel, limits.max_threads > 1))
}

fn read_all(dir: &Path) -> Vec<(String, Vec<u8>)> {
    jxr_files(dir)
        .into_iter()
        .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The synthetic and alpha streams, mutants of them (cut short, one byte changed: these fail in
/// the header or in a row), the fuzz corpus, and a few conformance streams when the corpus is
/// there.
fn streams() -> Vec<(String, Vec<u8>)> {
    let mut all = read_all(&testdata("synth"));
    all.extend(read_all(&testdata("alpha")));
    let mut mutants = Vec::new();
    for (name, data) in &all {
        mutants.push((format!("{name}~cut"), data[..data.len() * 2 / 3].to_vec()));
        let mut flipped = data.clone();
        flipped[data.len() * 3 / 4] ^= 0x55;
        mutants.push((format!("{name}~flip"), flipped));
    }
    all.extend(mutants);
    if let Ok(rd) = std::fs::read_dir(testdata("fuzz")) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for p in files.into_iter().filter(|p| p.is_file()) {
            all.push((p.display().to_string(), std::fs::read(&p).unwrap()));
        }
    }
    let dir = conformance_dir();
    if dir.exists() {
        for f in golden_lines(&testdata("conformance/golden.txt"))
            .iter()
            .step_by(7)
        {
            let data = std::fs::read(dir.join(&f[0])).unwrap();
            if gate_a_format(&data) && data.len() < 300_000 {
                all.push((f[0].clone(), data));
            }
        }
    }
    all
}

#[test]
fn a_kept_scratch_gives_what_a_fresh_decoder_gives() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<Scratch>();
    assert_sync::<Decoder<'static>>();

    let streams = streams();
    assert!(streams.len() >= 60, "{} streams", streams.len());
    let go = AtomicBool::new(false);
    let stop = AtomicBool::new(true);
    for max_threads in [1, 2] {
        let limits = Limits {
            max_threads,
            ..Limits::default()
        };
        for reverse in [false, true] {
            let mut order: Vec<_> = streams.iter().collect();
            if reverse {
                order.reverse();
            }
            // One scratch for the whole run: every stream sees what the ones before it left.
            let mut scratch = Scratch::default();
            let mut errors = 0;
            for (name, data) in order {
                // A cancelled decode in between leaves a half-used scratch.
                assert_eq!(
                    kept(data, &limits, &stop, &mut scratch),
                    fresh(data, &limits, &stop),
                    "{name}: cancelled, threads {max_threads}, reverse {reverse}"
                );
                let want = fresh(data, &limits, &go);
                errors += usize::from(!matches!(want, Ok((Ok(()), _))));
                assert_eq!(
                    kept(data, &limits, &go, &mut scratch),
                    want,
                    "{name}: threads {max_threads}, reverse {reverse}"
                );
            }
            assert!(errors > 0, "no stream failed to decode");
        }
    }
}

#[test]
fn a_kept_scratch_keeps_the_limit_outcomes_of_a_fresh_decode() {
    let one = Limits {
        max_threads: 1,
        ..Limits::default()
    };
    let go = AtomicBool::new(false);
    let streams: Vec<_> = read_all(&testdata("synth"));
    // The stream with the largest estimate warms the scratch.
    let big = streams
        .iter()
        .max_by_key(|(_, d)| Decoder::new(d, &one).unwrap().estimate())
        .unwrap();
    let mut checked = 0;
    for (name, data) in &streams {
        let d = Decoder::new(data, &one).unwrap();
        let (w, h, f) = d.config();
        let s = stride(w, f);
        let mut dst = vec![0u8; s * h];
        let (r, probe) = d.decode_into_probe(&mut dst, s, &go, false);
        if r.is_err() {
            continue;
        }
        // What a fresh decode needs: the estimate is checked first, then the charges.
        let fits = probe.charged_peak.max(d.estimate());
        for max_alloc in [fits, fits - 1, probe.charged_peak, probe.charged_peak - 1] {
            let l = Limits { max_alloc, ..one };
            let mut scratch = Scratch::default();
            let warm = Decoder::reusing(&big.1, &one, None, &mut scratch).unwrap();
            assert_eq!(run(&warm, &go, false).0, Ok(()), "{}", big.0);
            drop(warm);

            let d = Decoder::new(data, &l).unwrap();
            let mut want = vec![0u8; s * h];
            let (want_r, _) = d.decode_into_probe(&mut want, s, &go, false);
            let d = Decoder::reusing(data, &l, None, &mut scratch).unwrap();
            let mut got = vec![0u8; s * h];
            let (got_r, got_probe) = d.decode_into_probe(&mut got, s, &go, false);
            assert_eq!(got_r, want_r, "{name}: max_alloc {max_alloc}");
            assert_eq!(got, want, "{name}: max_alloc {max_alloc}");
            if got_r.is_ok() {
                assert!(
                    got_probe.charged_peak <= max_alloc,
                    "{name}: charged {} over {max_alloc}",
                    got_probe.charged_peak
                );
            }
            // The estimate is checked first, so `fits` is the estimate and the limits that meet
            // the ledger's charges are tested in `codec/tests.rs`.
            if max_alloc == fits {
                assert_eq!(got_r, Ok(()), "{name}: a fresh decode's budget");
            }
        }
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} streams decoded");
}
