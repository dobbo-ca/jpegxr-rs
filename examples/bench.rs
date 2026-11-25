//! `bench <streams> <n>`: prints name, output bytes and ns per decode for each `.jxr` that
//! decodes, as `oracle -bench` does for Go. See `tests/parity/bench.sh`. `BENCH_THREADS` sets
//! `Limits::max_threads` (default: the machine's).

use jpegxr::{Decoder, Format, Limits, Scratch};
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use std::{env, fs, io};

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let [_, dir, n] = args.as_slice() else {
        eprintln!("usage: bench <streams> <n>");
        std::process::exit(2);
    };
    let n: u32 = n.parse().unwrap_or(1).max(1);
    let mut limits = Limits::default();
    if let Some(t) = env::var("BENCH_THREADS").ok().and_then(|v| v.parse().ok()) {
        limits.max_threads = t;
    }
    let cancel = AtomicBool::new(false);
    let mut names: Vec<String> = fs::read_dir(dir)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".jxr"))
        .collect();
    names.sort();
    // One dst and one scratch for all streams, as the Go side does.
    let mut dst = Vec::new();
    let mut scratch = Scratch::default();
    for name in &names {
        let data = fs::read(format!("{dir}/{name}"))?;
        let mut decode = || -> Result<usize, jpegxr::Error> {
            let d = Decoder::reusing(&data, &limits, None, &mut scratch)?;
            let (w, h, f) = d.config();
            let stride = match f {
                Format::Gray8 => w,
                Format::Gray16 => 2 * w,
                Format::Bgr24 => 3 * w,
                Format::Bgr48 => 6 * w,
            };
            if dst.len() < stride * h {
                dst.resize(stride * h, 0);
            }
            d.decode_into(&mut dst[..stride * h], stride, &cancel)?;
            Ok(stride * h)
        };
        let Ok(len) = decode() else { continue };
        let t = Instant::now();
        for _ in 0..n {
            // A stream that decoded once must keep decoding, as in Go's bench.
            if let Err(e) = decode() {
                eprintln!("bench: {name}: {e}");
                std::process::exit(1);
            }
        }
        println!("{name}\t{len}\t{}", t.elapsed().as_nanos() / u128::from(n));
    }
    Ok(())
}
