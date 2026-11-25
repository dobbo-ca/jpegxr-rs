//! `ppm [--threads N] <inputs> <outputs>`: decodes every `.jxr` of `<inputs>` (sorted) with
//! `Decoder::decode_into` and writes `<stem>.ppm` (Bgr24, Bgr48) or `<stem>.pgm` (Gray8, Gray16),
//! as `oracle -ppm` does for Go. A stream that does not decode writes nothing. 16-bit samples are
//! written as decoded, two bytes each, so a Gray16 file is a P5 of 2*w pixels and a Bgr48 file a P6
//! of 2*w pixels. `--threads N` sets `Limits::max_threads`.
use jpegxr::{Decoder, Format, Limits};
use std::sync::atomic::AtomicBool;
use std::{env, fs, io};

fn main() -> io::Result<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut limits = Limits::default();
    if args.first().is_some_and(|a| a == "--threads") && args.len() > 1 {
        limits.max_threads = args[1].parse().unwrap_or(1);
        args.drain(..2);
    }
    let [dir, out] = args.as_slice() else {
        eprintln!("usage: ppm [--threads N] <inputs> <outputs>");
        std::process::exit(2);
    };
    let cancel = AtomicBool::new(false);
    let mut names: Vec<String> = fs::read_dir(dir)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".jxr"))
        .collect();
    names.sort();
    fs::create_dir_all(out)?;
    for name in &names {
        let data = fs::read(format!("{dir}/{name}"))?;
        let Ok(d) = Decoder::new(&data, &limits) else {
            continue;
        };
        let (w, h, f) = d.config();
        let (magic, ext, bytes_per_px, pw) = match f {
            Format::Gray8 => ("P5", "pgm", 1, w),
            Format::Gray16 => ("P5", "pgm", 2, 2 * w),
            Format::Bgr24 => ("P6", "ppm", 3, w),
            Format::Bgr48 => ("P6", "ppm", 6, 2 * w),
        };
        let stride = bytes_per_px * w;
        let mut dst = Vec::new();
        if dst.try_reserve_exact(stride * h).is_err() {
            continue;
        }
        dst.resize(stride * h, 0);
        if d.decode_into(&mut dst, stride, &cancel).is_err() {
            continue;
        }
        if f == Format::Bgr24 {
            dst.as_chunks_mut::<3>()
                .0
                .iter_mut()
                .for_each(|p| p.swap(0, 2));
        }
        let mut b = format!("{magic}\n{pw} {h}\n255\n").into_bytes();
        b.extend_from_slice(&dst);
        let stem = name.strip_suffix(".jxr").unwrap_or(name);
        fs::write(format!("{out}/{stem}.{ext}"), b)?;
    }
    Ok(())
}
