//! The Rust side of the parity check: `oracle <inputs> <outputs>` decodes every `.jxr` input like
//! `tests/parity/go/oracle` and writes `<name>.<op>.out` (the output), `<name>.<op>.err` (the error
//! class) and `<name>.<op>.partial.out` (what the buffer holds after a failed `into`). The class
//! table is in `docs/parity.md`.
//!
//! `oracle -kernels <dir> <out>` is the kernel check instead: it reads the `.in` files that
//! `tests/parity/go/upstream/cmd/kernelgen` wrote, runs the scalar kernels and the SIMD tier on them
//! and writes `<out>/scalar/<kernel>.out` and `<out>/simd/<kernel>.out`, the digests that
//! kernelgen wrote for the generic Go kernels.

use jpegxr::{ColorModel, Decoder, Error, Format, Limits, Scratch};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::{env, fs, io};

/// The Go oracle's caps: `memLimit`, and `maxPixels`, over which an input is not decoded (the
/// higher one for the mutated inputs, which are written as digests).
const MEM_LIMIT: u64 = 512 << 20;
const MAX_PIXELS: u64 = 1 << 20;
const MAX_PIXELS_DIGEST: u64 = 1 << 24;

fn class(e: &Error) -> String {
    match e {
        Error::Internal(_) => "internal".into(),
        Error::PixelFormat => "pixfmt".into(),
        Error::TooLarge => "toolarge".into(),
        Error::Container => "container".into(),
        Error::Codestream => "codestream".into(),
        Error::Unsupported => "unsupported".into(),
        Error::DstTooSmall => "dst".into(),
        Error::OddStride => "oddstride".into(),
        Error::AllocLimit { .. } => "alloc".into(),
        Error::Cancelled => "cancelled".into(),
        e => format!("other:{e}"),
    }
}

/// FNV-1a 64 and the length of `b`, as a line: `digest` of the Go oracle.
fn digest(b: &[u8]) -> Vec<u8> {
    format!("{:016x} {}\n", digest_u64(b), b.len()).into_bytes()
}

fn digest_u64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &c| {
        (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01b3)
    })
}

struct Out<'a> {
    dir: &'a Path,
    name: &'a str,
    /// A mutated input: write digests of the bytes, not the bytes.
    digests: bool,
}

impl Out<'_> {
    fn write(&self, op: &str, ext: &str, b: &[u8]) {
        let p = self.dir.join(format!("{}.{op}.{ext}", self.name));
        if let Err(e) = fs::write(&p, b) {
            eprintln!("{}: {e}", p.display());
            std::process::exit(1);
        }
    }
    fn bytes(&self, op: &str, ext: &str, b: &[u8]) {
        if self.digests {
            self.write(op, ext, &digest(b));
        } else {
            self.write(op, ext, b);
        }
    }
    fn fail(&self, op: &str, e: &Error) {
        self.write(op, "err", format!("{}\n", class(e)).as_bytes());
    }
    fn fail_class(&self, op: &str, c: &str) {
        self.write(op, "err", format!("{c}\n").as_bytes());
    }
    /// The partial output of a failed operation, if any byte of `dst` is non-zero.
    fn partial(&self, op: &str, dst: &[u8]) {
        if dst.iter().any(|&c| c != 0) {
            self.bytes(op, "partial.out", dst);
        }
    }
}

fn model_name(m: ColorModel) -> &'static str {
    match m {
        ColorModel::Gray => "Gray",
        ColorModel::Gray16 => "Gray16",
        ColorModel::Rgba => "RGBA",
        ColorModel::Rgba64 => "RGBA64",
    }
}

fn format_name(f: Format) -> &'static str {
    match f {
        Format::Bgr24 => "Bgr24",
        Format::Bgr48 => "Bgr48",
        Format::Gray8 => "Gray8",
        Format::Gray16 => "Gray16",
    }
}

fn row_bytes(w: usize, f: Format) -> usize {
    match f {
        Format::Gray8 => w,
        Format::Gray16 => 2 * w,
        Format::Bgr24 => 3 * w,
        Format::Bgr48 => 6 * w,
    }
}

fn pix(img: &jpegxr::Image) -> Vec<u8> {
    let mut b = format!(
        "{} {} {}\n",
        img.width,
        img.height,
        model_name(img.color_model)
    )
    .into_bytes();
    b.extend_from_slice(&img.pix);
    b
}

/// What `reuse` runs every input through: one `Scratch` (the Go oracle's shared `Decoder`) and
/// the input before.
#[derive(Default)]
struct Shared {
    scratch: Scratch,
    prev: Option<Vec<u8>>,
}

/// `Decoder::new` (or `Decoder::reusing` on `scratch`) and `decode_into` into a dst whose rows are
/// padded by `pad` bytes (0xa5 where the decoder writes nothing).
fn into(
    o: &Out,
    op: &str,
    data: &[u8],
    limits: &Limits,
    cancel: &AtomicBool,
    pad: usize,
    scratch: Option<&mut Scratch>,
) {
    let d = match scratch {
        Some(s) => Decoder::reusing(data, limits, None, s),
        None => Decoder::new(data, limits),
    };
    let d = match d {
        Ok(d) => d,
        Err(e) => return o.fail(op, &e),
    };
    let (w, h, f) = d.config();
    let stride = row_bytes(w, f) + pad;
    let mut dst = vec![if pad > 0 { 0xa5 } else { 0 }; stride * h];
    match d.decode_into(&mut dst, stride, cancel) {
        Err(e) => {
            o.fail(op, &e);
            // The partial dst of a reused Go Decoder depends on its history (docs/parity.md).
            if op != "reuse" {
                o.partial(op, &dst);
            }
        }
        Ok(()) => {
            let mut b = format!("{w} {h} {}\n", format_name(f)).into_bytes();
            b.extend_from_slice(&dst);
            o.bytes(op, "out", &b);
        }
    }
}

/// `decode_into` with a dst one byte short, and a stride one byte short.
fn short(o: &Out, data: &[u8], limits: &Limits, cancel: &AtomicBool) {
    let d = match Decoder::new(data, limits) {
        Ok(d) => d,
        Err(e) => return o.fail("short", &e),
    };
    let (w, h, f) = d.config();
    let s = row_bytes(w, f);
    let mut dst = vec![0u8; s * h];
    let r = |e: Result<(), Error>| e.err().map_or("ok".to_string(), |e| class(&e));
    let mut e1 = Ok(());
    let mut e2 = Ok(());
    if !dst.is_empty() {
        let n = dst.len() - 1;
        e1 = d.decode_into(&mut dst[..n], s, cancel);
    }
    if s > 0 {
        e2 = d.decode_into(&mut dst, s - 1, cancel);
    }
    o.write(
        "short",
        "out",
        format!("dst {}\nstride {}\n", r(e1), r(e2)).as_bytes(),
    );
}

fn process(o: &Out, data: &[u8], shared: &mut Shared) {
    let limits = Limits {
        max_alloc: std::env::var("ORACLE_MEM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MEM_LIMIT),
        max_pixels: u64::MAX,
        max_width: u64::MAX,
        max_height: u64::MAX,
        // `ORACLE_THREADS` (default: the machine's): the output must not depend on it.
        max_threads: std::env::var("ORACLE_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Limits::default().max_threads),
    };
    let cancel = AtomicBool::new(false);
    let ops = [
        "dec", "c0", "c1", "c2", "c3", "into", "ints", "intodd", "short", "reuse",
    ];
    let max_pixels = if o.digests {
        MAX_PIXELS_DIGEST
    } else {
        MAX_PIXELS
    };
    match jpegxr::decode_config(data, &limits) {
        Ok(c) => {
            o.write(
                "cfg",
                "out",
                format!("{} {} {}\n", c.width, c.height, model_name(c.color_model)).as_bytes(),
            );
            if (c.width as u64).wrapping_mul(c.height as u64) > max_pixels {
                for op in ops {
                    o.fail_class(op, "oversize");
                }
                return;
            }
        }
        Err(Error::TooLarge) => {
            // Go's DecodeConfig has no cap and Reset has; decode_config applies it too.
            o.fail_class("cfg", "toolarge");
            for op in ops {
                o.fail_class(op, "oversize");
            }
            return;
        }
        Err(e) => o.fail("cfg", &e),
    }
    match jpegxr::decode(data, &limits, &cancel) {
        Ok(img) => o.bytes("dec", "out", &pix(&img)),
        Err(e) => o.fail("dec", &e),
    }
    for k in 0..4 {
        let op = format!("c{k}");
        match jpegxr::decode_component(data, k, &limits, &cancel) {
            Ok(img) => o.bytes(&op, "out", &pix(&img)),
            Err(e) => o.fail(&op, &e),
        }
    }
    // A 2^32 x 2^32 header: the three ops below would allocate a wrapped stride * h (32 GiB).
    if let Ok(c) = jpegxr::decode_config(data, &limits)
        && (c.width as u64).checked_mul(c.height as u64).is_none()
    {
        for op in ["into", "ints", "intodd", "short", "reuse"] {
            o.fail_class(op, "skipped");
        }
        return;
    }
    into(o, "into", data, &limits, &cancel, 0, None);
    into(o, "ints", data, &limits, &cancel, 8, None);
    into(o, "intodd", data, &limits, &cancel, 3, None);
    short(o, data, &limits, &cancel);
    // As the Go oracle's `reuse`: the decoder that has seen every earlier input is asked for the
    // input before (its result ignored: a failed or dropped decoder leaves its buffers in
    // whatever state), then decodes this one.
    if let Some(prev) = &shared.prev {
        let _ = Decoder::reusing(prev, &limits, None, &mut shared.scratch);
    }
    shared.prev = Some(data.to_vec());
    into(
        o,
        "reuse",
        data,
        &limits,
        &cancel,
        0,
        Some(&mut shared.scratch),
    );
}

/// A reader over a kernelgen `.in` file.
struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        a
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().unwrap())
    }
    fn int(&mut self) -> i64 {
        self.u64() as i64
    }
    fn i32s(&mut self) -> Vec<i32> {
        let n = self.u64() as usize;
        let (words, _) = self.take(4 * n).as_chunks::<4>();
        words.iter().map(|w| i32::from_le_bytes(*w)).collect()
    }
    fn bytes(&mut self) -> &'a [u8] {
        let n = self.u64() as usize;
        self.take(n)
    }
}

fn i32le(p: &[i32]) -> Vec<u8> {
    p.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The canonical bytes of a kernel's result: what kernelgen hashes. An error is a marker that
/// matches nothing.
fn canon(r: Result<Vec<u8>, Error>) -> Vec<u8> {
    r.unwrap_or_else(|e| format!("kernel error: {e}").into_bytes())
}

type Kernel = fn(&mut Rd, bool, &Limits) -> Vec<u8>;

fn k_idct(rd: &mut Rd, simd: bool, l: &Limits) -> Vec<u8> {
    let (o, n) = (rd.int() as isize, rd.int() as isize);
    let mut p = rd.i32s();
    canon(jpegxr::kernels::idct_pairs(simd, &mut p, o, n, l).map(|()| i32le(&p)))
}

fn k_post(rd: &mut Rd, simd: bool, l: &Limits) -> Vec<u8> {
    let (p0, p1, off) = (rd.int() as isize, rd.int() as isize, rd.int() as isize);
    let mut p = rd.i32s();
    canon(jpegxr::kernels::post_split(simd, &mut p, p0, p1, off, l).map(|()| i32le(&p)))
}

fn k_bgr8(rd: &mut Rd, simd: bool, l: &Limits) -> Vec<u8> {
    let (d, s, n) = (rd.int() as usize, rd.int() as usize, rd.int() as usize);
    let (bias, shift) = (rd.int() as i32, rd.int() as u32);
    let ir_ib = (rd.int() as usize, rd.int() as usize);
    let mut out = vec![0u8; rd.int() as usize];
    let (y, u, v) = (rd.i32s(), rd.i32s(), rd.i32s());
    let r = jpegxr::kernels::out_bgr8(simd, &mut out, d, (&y, &u, &v), s, n, bias, shift, ir_ib, l);
    canon(r.map(|()| out))
}

fn k_gray16(rd: &mut Rd, simd: bool, l: &Limits) -> Vec<u8> {
    let (d, s, n) = (rd.int() as usize, rd.int() as usize, rd.int() as usize);
    let (bias, shift, n_len) = (rd.int() as i32, rd.int() as u32, rd.int() as u32);
    let mut out = vec![0u8; rd.int() as usize];
    let y = rd.i32s();
    let r = jpegxr::kernels::out_gray16(simd, &mut out, d, &y, s, n, bias, shift, n_len, l);
    canon(r.map(|()| out))
}

/// The scalar flexbits path has no SIMD tier: `simd` is not read.
fn k_flexbits(rd: &mut Rd, _simd: bool, l: &Limits) -> Vec<u8> {
    let (skip, model, trim, qp) = (
        rd.int() as i32,
        rd.int() as i32,
        rd.int() as i32,
        rd.int() as i32,
    );
    let mut coef = rd.i32s();
    let buf = rd.bytes();
    canon(jpegxr::kernels::flexbits(
        &mut coef,
        buf,
        skip,
        (model, trim, qp),
        l,
    ))
}

/// The scalar highpass has no SIMD tier: `simd` is not read.
fn k_highpass(rd: &mut Rd, _simd: bool, l: &Limits) -> Vec<u8> {
    let (chroma, skip, qp) = (rd.int() != 0, rd.int() as u32, rd.int() as i32);
    let mut tables = [(0usize, 0i32, 0i32, 0i32); 21];
    for t in &mut tables {
        *t = (
            rd.int() as usize,
            rd.int() as i32,
            rd.int() as i32,
            rd.int() as i32,
        );
    }
    let mut scans = [(0u32, 0u32); 16];
    for s in &mut scans {
        *s = (rd.u64() as u32, rd.u64() as u32);
    }
    let mut coef = rd.i32s();
    let buf = rd.bytes();
    canon(jpegxr::kernels::highpass(
        chroma, skip, qp, &tables, &scans, buf, &mut coef, l,
    ))
}

/// `-kernels`: every case of every `.in` file under both tiers.
fn kernels(input: &Path, output: &Path) -> io::Result<()> {
    let limits = Limits {
        max_alloc: MEM_LIMIT,
        max_pixels: u64::MAX,
        max_width: u64::MAX,
        max_height: u64::MAX,
        max_threads: 1,
    };
    let tier = jpegxr::kernels::tier_name(&limits);
    if tier == "scalar" {
        eprintln!("no SIMD tier on this CPU: the kernel check would compare scalar with scalar");
        std::process::exit(1);
    }
    let all: [(&str, Kernel); 6] = [
        ("idct_pairs", k_idct),
        ("post_split", k_post),
        ("out_bgr8", k_bgr8),
        ("out_gray16", k_gray16),
        ("flexbits", k_flexbits),
        ("highpass", k_highpass),
    ];
    for (name, kernel) in all {
        let data = fs::read(input.join(format!("{name}.in")))?;
        let cases = Rd(&data).u64();
        for (dir, simd) in [("scalar", false), ("simd", true)] {
            let mut rd = Rd(&data);
            rd.u64();
            let mut digests = Vec::new();
            for _ in 0..cases {
                let h = digest_u64(&kernel(&mut rd, simd, &limits));
                digests.extend_from_slice(&h.to_le_bytes());
            }
            fs::create_dir_all(output.join(dir))?;
            fs::write(output.join(dir).join(format!("{name}.out")), digests)?;
        }
        println!("{name}: {cases} cases, tiers scalar and {tier}");
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if let [_, flag, input, output] = args.as_slice()
        && flag == "-kernels"
    {
        return kernels(Path::new(input), Path::new(output));
    }
    let [_, input, output] = args.as_slice() else {
        eprintln!("usage: oracle <inputs> <outputs> | oracle -kernels <dir> <outputs>");
        std::process::exit(2);
    };
    // A panic in the decoder is caught and returned as Error::Internal; keep stderr quiet.
    std::panic::set_hook(Box::new(|_| {}));
    fs::create_dir_all(output)?;
    let mut names: Vec<String> = fs::read_dir(input)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".jxr"))
        .collect();
    names.sort();
    let mut shared = Shared::default();
    for name in &names {
        let data = fs::read(Path::new(input).join(name))?;
        process(
            &Out {
                dir: Path::new(output),
                name,
                digests: name.contains('~'),
            },
            &data,
            &mut shared,
        );
    }
    Ok(())
}
