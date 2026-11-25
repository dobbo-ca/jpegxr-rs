//! Ports of jpegxr-go's `internal/codec/strdec_test.go` and the gate B half of
//! `internal/jpegxr/conformance_test.go`, which decode through the codec with full formats.

use super::*;
use crate::Limits;
use crate::container;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[allow(dead_code)]
#[path = "../../tests/common/sha256.rs"]
mod sha256;

fn testdata(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(rel)
}

/// The codestream of a synth file.
fn synth_stream(name: &str) -> Vec<u8> {
    let b = std::fs::read(testdata("synth").join(name)).unwrap();
    let info = container::read(&b).unwrap();
    b[info.image_offset as usize..].to_vec()
}

#[test]
fn read_header() {
    for (name, cf, bd, internal, ol, tiled, scaled) in [
        (
            "Bgr24_lossless_tiled_ov0.jxr",
            CF_RGB,
            BD8,
            YUV444,
            OL_NONE,
            true,
            false,
        ),
        (
            "Bgr24_lossy_untiled_ov1.jxr",
            CF_RGB,
            BD8,
            YUV444,
            OL_ONE,
            false,
            true,
        ),
        (
            "Gray16_lossless_untiled_ov1.jxr",
            Y_ONLY,
            BD16,
            Y_ONLY,
            OL_ONE,
            false,
            false,
        ),
    ] {
        let h = Header::parse(&synth_stream(name), u64::MAX, &mut TileTables::default()).unwrap();
        let is_tiled = h.scp.c_num_of_slice_minus1_v + h.scp.c_num_of_slice_minus1_h > 0;
        assert!(
            h.width == 64
                && h.height == 64
                && h.color_format == cf
                && h.bit_depth == bd
                && h.param.cf_color_format == internal
                && h.scp.ol_overlap == ol
                && is_tiled == tiled
                && h.param.scaled_arith == scaled,
            "{name}: got {}x{} cf {} bd {} internal {} ol {} tiled {is_tiled} scaled {}",
            h.width,
            h.height,
            h.color_format,
            h.bit_depth,
            h.param.cf_color_format,
            h.scp.ol_overlap,
            h.param.scaled_arith
        );
    }
}

/// `FuzzHeader`'s seeds, plus the mutation the Go seeds carry: parsing never panics.
#[test]
fn header_seeds_do_not_panic() {
    for n in [
        "Bgr24_lossy_untiled_ov1.jxr",
        "Gray16_lossless_untiled_ov1.jxr",
        "Bgr48_lossless_tiled_ov1.jxr",
    ] {
        let s = synth_stream(n);
        let _ = Header::parse(&s, u64::MAX, &mut TileTables::default());
        let mut m = s.clone();
        m[8] ^= 0x0f; // flip subversion bits
        let _ = Header::parse(&m, u64::MAX, &mut TileTables::default());
        for len in 0..s.len().min(80) {
            let _ = Header::parse(&s[..len], u64::MAX, &mut TileTables::default());
        }
    }
}

/// `FuzzIndexTable`: `read_index_table` through the same setup `decode` uses, on tiled streams
/// where an index table is present.
#[test]
fn index_table_seeds_do_not_panic() {
    for n in [
        "Bgr24_lossless_tiled_ov0.jxr",
        "Bgr48_lossless_tiled_ov1.jxr",
    ] {
        let s = synth_stream(n);
        let mut m = s.clone();
        m[16] ^= 0xff; // corrupt index table region
        for b in [s, m] {
            let Ok(h) = Header::parse(&b, u64::MAX, &mut TileTables::default()) else {
                continue;
            };
            if (h.width * h.height) as u64 > 1 << 22 {
                continue;
            }
            let mut sc = StrCodec {
                stream_len: b.len(),
                param: h.param,
                wmiscp: h.scp.clone(),
                ..StrCodec::default()
            };
            let mut ios = IoSet {
                v: vec![BitIO::new()],
                stream: &b,
            };
            if sc
                .allocate_bit_io_info(b.len(), &mut ios, &Ledger::new(&Limits::default()))
                .is_err()
            {
                continue;
            }
            ios.attach(HEADER_IO, h.size as u64);
            let _ = sc.read_index_table(&mut ios);
        }
    }
}

/// A container pixel format's decode shape and its TIFF strip layout (JXRGluePFC.c conversions,
/// docs/reference/conformance.md).
#[derive(Clone, Copy)]
struct GateBFormat {
    f: Format,
    /// TIFF bytes a pixel; 0 means same as decoded.
    keep: usize,
    /// Swap bytes 0 and 2 (BGR <-> RGB), JXRGluePFC.c `BGR*_RGB*`.
    swap: bool,
}

fn fmt(color_format: usize, bit_depth: usize, bits_per_unit: usize, rgb: bool) -> Format {
    Format {
        color_format,
        bit_depth,
        bits_per_unit,
        rgb,
        alpha: false,
        plane: 0,
    }
}

/// Maps the last GUID byte (JXRGlue.c pixelInfo) to its decode format and TIFF pack.
fn gate_b_formats() -> HashMap<u8, GateBFormat> {
    let g = |f, keep, swap| GateBFormat { f, keep, swap };
    let mut m = HashMap::from([
        (0x05, g(fmt(Y_ONLY, BD1, 1, false), 0, false)),
        (0x09, g(fmt(CF_RGB, BD5, 16, false), 0, false)),
        (0x0a, g(fmt(CF_RGB, BD565, 16, false), 0, false)),
        (0x0d, g(fmt(CF_RGB, BD8, 24, true), 0, false)),
        (0x0e, g(fmt(CF_RGB, BD8, 32, false), 3, true)),
        (0x0f, g(fmt(CF_RGB, BD8, 32, false), 0, true)),
        (0x10, g(fmt(CF_RGB, BD8, 32, false), 0, true)),
        (0x11, g(fmt(Y_ONLY, BD32F, 32, false), 0, false)),
        (0x12, g(fmt(CF_RGB, BD16S, 48, true), 0, false)),
        (0x13, g(fmt(Y_ONLY, BD16S, 16, false), 0, false)),
        (0x14, g(fmt(CF_RGB, BD10, 32, false), 0, false)),
        (0x16, g(fmt(CF_RGB, BD16, 64, true), 0, false)),
        (0x17, g(fmt(CF_RGB, BD16, 64, true), 0, false)),
        (0x18, g(fmt(CF_RGB, BD32S, 96, true), 0, false)),
        (0x19, g(fmt(CF_RGB, BD32F, 128, true), 0, false)),
        (0x1a, g(fmt(CF_RGB, BD32F, 128, true), 0, false)),
        (0x1b, g(fmt(CF_RGB, BD32F, 128, true), 12, false)),
        (0x1c, g(fmt(CMYK, BD8, 32, false), 0, false)),
        (0x1d, g(fmt(CF_RGB, BD16S, 64, true), 0, false)),
        (0x1e, g(fmt(CF_RGB, BD32S, 128, true), 0, false)),
        (0x1f, g(fmt(CMYK, BD16, 64, false), 0, false)),
        (0x2c, g(fmt(CMYK, BD8, 40, false), 0, false)),
        (0x2d, g(fmt(CMYK, BD16, 80, false), 0, false)),
        (0x3a, g(fmt(CF_RGB, BD16F, 64, true), 0, false)),
        (0x3b, g(fmt(CF_RGB, BD16F, 48, true), 0, false)),
        (0x3d, g(fmt(CF_RGBE, BD8, 32, false), 0, false)),
        (0x3e, g(fmt(Y_ONLY, BD16F, 16, false), 0, false)),
        (0x3f, g(fmt(Y_ONLY, BD32S, 32, false), 0, false)),
        (0x40, g(fmt(CF_RGB, BD16S, 64, true), 6, false)),
        (0x41, g(fmt(CF_RGB, BD32S, 128, true), 12, false)),
        (0x42, g(fmt(CF_RGB, BD16F, 64, true), 6, false)),
    ]);
    for n in 3..=8usize {
        m.insert(
            0x20 + n as u8 - 3,
            g(fmt(N_COMPONENT, BD8, n * 8, false), 0, false),
        );
        m.insert(
            0x26 + n as u8 - 3,
            g(fmt(N_COMPONENT, BD16, n * 16, false), 0, false),
        );
        m.insert(
            0x2e + n as u8 - 3,
            g(fmt(N_COMPONENT, BD8, n * 8 + 8, false), 0, false),
        );
        m.insert(
            0x34 + n as u8 - 3,
            g(fmt(N_COMPONENT, BD16, n * 16 + 16, false), 0, false),
        );
    }
    m
}

/// The byte width of one planar-alpha sample at `bd`, which shares the primary plane's bit depth
/// (JXRGlueJxr.c:2075-2097).
fn alpha_sample_bytes(bd: usize) -> usize {
    match bd {
        BD16 | BD16S | BD16F => 2,
        BD32S | BD32F => 4,
        _ => 1,
    }
}

/// Lays out one decoded row of `w` pixels, `px` bytes each, into `keep` bytes a pixel (dropping
/// trailing padding bytes), optionally swapping bytes 0 and 2 of each pixel (BGR <-> RGB).
fn pack(row: &[u8], w: usize, px: usize, keep: usize, swap: bool) -> Vec<u8> {
    let mut out = vec![0u8; keep * w];
    for x in 0..w {
        out[keep * x..keep * x + keep].copy_from_slice(&row[px * x..px * x + keep]);
        if swap {
            out.swap(keep * x, keep * x + 2);
        }
    }
    out
}

fn decode_stream(stream: &[u8], f: &Format, dst: &mut [u8], stride: usize) -> Result<(), Error> {
    let h = Header::parse(stream, u64::MAX, &mut TileTables::default())?;
    run_decode(stream, &h, f, dst, stride)
}

fn run_decode(
    stream: &[u8],
    h: &Header,
    f: &Format,
    dst: &mut [u8],
    stride: usize,
) -> Result<(), Error> {
    let cancel = AtomicBool::new(false);
    let one = Limits {
        max_threads: 1,
        ..Limits::default()
    };
    let cx = Ctx::new(&one, None, &cancel, false);
    let before = dst.to_vec();
    let r = decode(
        stream,
        h,
        f,
        dst,
        stride,
        &cx,
        &mut |_, _| Ok(()),
        &mut Scratch::default(),
    );
    // The pipeline on two threads gives the same result and the same bytes, whatever the size
    // (the formats here include the interleaved alpha and the hard-tile streams). It and the
    // serial decode after it work in one scratch kept from every decode this thread ran: a kept
    // scratch gives a fresh decode's result, whatever it held (formats, sizes, alpha, errors).
    let two = Limits {
        max_threads: 2,
        ..Limits::default()
    };
    thread_local! {
        static KEPT: std::cell::RefCell<Scratch> = std::cell::RefCell::default();
    }
    KEPT.with_borrow_mut(|kept| {
        let cx = Ctx::new(&two, None, &cancel, true);
        let mut par = before.clone();
        let r2 = decode(
            stream,
            h,
            f,
            &mut par,
            stride,
            &cx,
            &mut |_, _| Ok(()),
            kept,
        );
        assert_eq!(r, r2, "two threads give another result");
        assert!(*dst == *par, "two threads give other bytes");
        let cx = Ctx::new(&one, None, &cancel, false);
        let mut again = before;
        let r3 = decode(
            stream,
            h,
            f,
            &mut again,
            stride,
            &cx,
            &mut |_, _| Ok(()),
            kept,
        );
        assert_eq!(r, r3, "a kept scratch gives another result");
        assert!(*dst == *again, "a kept scratch gives other bytes");
    });
    r
}

/// One single-threaded decode of `stream` on `scratch` with `max_alloc`, calling `decode`
/// directly: unlike a `Decoder`, it makes no estimate check first, so the limit meets the
/// ledger's own charges. Returns the result, the bytes and the ledger's peak.
fn decode_at(
    stream: &[u8],
    f: &Format,
    max_alloc: u64,
    scratch: &mut Scratch,
) -> (Result<(), Error>, Vec<u8>, u64) {
    let h = Header::parse(stream, u64::MAX, &mut TileTables::default()).unwrap();
    let stride = f.bits_per_unit / 8 * h.width;
    let mut dst = vec![0u8; stride * h.height];
    let cancel = AtomicBool::new(false);
    let l = Limits {
        max_alloc,
        max_threads: 1,
        ..Limits::default()
    };
    let cx = Ctx::new(&l, None, &cancel, false);
    let r = decode(
        stream,
        &h,
        f,
        &mut dst,
        stride,
        &cx,
        &mut |_, _| Ok(()),
        scratch,
    );
    (r, dst, cx.probe().charged_peak)
}

/// A scratch kept from a larger, tiled stream gives a fresh decode's result and bytes at limits
/// on the ledger's charges, and its peak stays within the limit: where the spare capacity does
/// not fit, `settle` fails and the buffers are cut.
#[test]
fn a_kept_scratch_meets_the_ledger_limits_of_a_fresh_decode() {
    let big = synth_stream("N3x16_lossless_tiled_ov1.jxr");
    let big_f = fmt(N_COMPONENT, BD16, 48, true);
    let small = synth_stream("Gray8_lossless_untiled_ov1.jxr");
    let small_f = fmt(Y_ONLY, BD8, 8, false);
    let warm = || {
        let mut sc = Scratch::default();
        assert_eq!(decode_at(&big, &big_f, u64::MAX, &mut sc).0, Ok(()));
        sc
    };
    // A fresh decode's peak, and the kept one's, which also books the spare capacity.
    let (r, _, p) = decode_at(&small, &small_f, u64::MAX, &mut Scratch::default());
    assert_eq!(r, Ok(()));
    let (r, _, k) = decode_at(&small, &small_f, u64::MAX, &mut warm());
    assert_eq!(r, Ok(()));
    assert!(
        k > p,
        "the kept scratch holds no spare capacity: {k} <= {p}"
    );
    let mut cut = false;
    for max_alloc in [p, p - 1, (p + k) / 2, k - 1, k] {
        let (want_r, want, _) = decode_at(&small, &small_f, max_alloc, &mut Scratch::default());
        let (got_r, got, peak) = decode_at(&small, &small_f, max_alloc, &mut warm());
        assert_eq!(got_r, want_r, "max_alloc {max_alloc}");
        assert!(got == want, "max_alloc {max_alloc}: other bytes");
        if got_r.is_ok() {
            assert!(peak <= max_alloc, "peak {peak} over {max_alloc}");
            cut |= peak < k;
        }
    }
    assert!(cut, "no case went through a failed settle");
}

/// The per-tile quantizer vectors stay within the bound `str_dec_init` charges after a decode, on
/// a fresh scratch and on one kept from a larger stream.
#[test]
fn tile_quantizers_stay_within_charged_bound() {
    let f = fmt(N_COMPONENT, BD16, 48, true);
    let mut sc = Scratch::default();
    for name in [
        "N3x16_lossy_tiled_ov2_odd.jxr",
        "N3x16_lossy_tiled_ov1.jxr",
        "N3x16_lossless_tiled_ov1.jxr",
        "N3x16_lossy_untiled_ov1.jxr",
    ] {
        assert_eq!(
            decode_at(&synth_stream(name), &f, u64::MAX, &mut sc).0,
            Ok(())
        );
        assert!(sc.quantizers_are_exact(), "{name}");
    }
}

/// Decodes one golden line's stream the way `JxrDecApp` writes its TIFF strip and compares the
/// hash.
fn decode_gate_b(fld: &[String], data: &[u8]) -> Result<(), String> {
    let info = container::read(data).map_err(|e| e.to_string())?;
    let formats = gate_b_formats();
    let gf = formats.get(&info.pixel_format[15]).ok_or_else(|| {
        format!(
            "no gate B table entry for GUID byte 0x{:02x}",
            info.pixel_format[15]
        )
    })?;
    let stream = &data[info.image_offset as usize..];
    let h =
        Header::parse(stream, u64::MAX, &mut TileTables::default()).map_err(|e| e.to_string())?;
    let planar = info.alpha_offset != 0 && info.alpha_byte_count != 0;
    let mut f = gf.f;
    f.alpha = h.alpha && !planar;

    let (w, ht) = (h.width, h.height);
    let px = f.bits_per_unit / 8;
    let keep = if gf.keep == 0 { px } else { gf.keep };
    let (row_bytes, stride) = if f.bit_depth == BD1 {
        ((w + 7) >> 3, (w + 7) >> 3)
    } else {
        (keep * w, px.max(keep) * w)
    };
    let mut buf = vec![0u8; stride * ht];

    run_decode(stream, &h, &f, &mut buf, stride).map_err(|e| e.to_string())?;

    if planar {
        let sb = alpha_sample_bytes(f.bit_depth);
        // The alpha plane shares the primary's pixel stride (JXRGlueJxr.c:2069), so its samples
        // land at the leading-padding slot inside each interleaved pixel.
        let af = fmt(Y_ONLY, f.bit_depth, f.bits_per_unit, false);
        decode_stream(
            &data[info.alpha_offset as usize..],
            &af,
            &mut buf[px - sb..],
            stride,
        )
        .map_err(|e| format!("alpha: {e}"))?;
    }

    let mut hasher = sha256::Sha256::new();
    for y in 0..ht {
        let row = &buf[stride * y..];
        if f.bit_depth == BD1 {
            hasher.write(&row[..row_bytes]);
            continue;
        }
        hasher.write(&pack(row, w, px, keep, gf.swap));
    }
    let got = format!("{w}\t{ht}\t{}", hasher.hex());
    let want = format!("{}\t{}\t{}", fld[1], fld[2], fld[4]);
    if got != want {
        return Err(format!("got {got:?}, want {want:?}"));
    }
    Ok(())
}

fn golden(path: &std::path::Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .split('\n')
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

/// `TestLossyAlpha`: synthetic 32bppBGRA streams with lossy interleaved alpha (`JxrEncApp -c 22
/// -a 3`), which no T.834 stream covers, against `JxrDecApp`'s TIFF strip bytes.
#[test]
fn lossy_alpha() {
    let lines = golden(&testdata("alpha/golden.txt"));
    assert_eq!(lines.len(), 2);
    for fld in lines {
        let data = std::fs::read(testdata("alpha").join(&fld[0])).unwrap();
        if let Err(e) = decode_gate_b(&fld, &data) {
            panic!("{}: {e}", fld[0]);
        }
    }
}

/// `TestConformanceGateB`: the non-CZI-format T.834 streams against `JxrDecApp`'s TIFF strip
/// bytes. Skipped without the corpus (`corpus/conformance`, never in git).
#[test]
fn conformance_gate_b() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("corpus/conformance/software/JXR_ConformanceSuite");
    if !dir.exists() {
        eprintln!(
            "no conformance corpus; run jpegxr-go's internal/jpegxr/testdata/conformance/fetch.sh"
        );
        return;
    }
    let (mut pass, mut total) = (0, 0);
    for fld in golden(&testdata("conformance/golden.txt")) {
        let data = std::fs::read(dir.join(&fld[0])).unwrap();
        if crate::tests::gate_a_format(&data) {
            continue;
        }
        total += 1;
        match crate::run(|| decode_gate_b(&fld, &data).map_err(Error::Internal)) {
            Ok(()) => pass += 1,
            Err(e) => eprintln!("{}: {e}", fld[0]),
        }
    }
    eprintln!("gateB pass {pass}/{total}");
    assert_eq!((pass, total), (161, 161));
}

/// The estimate uses Go's sizes (see `Header::estimate`); the Rust structs must not be larger, or a
/// decode would allocate more than it charges.
#[test]
fn estimate_constants() {
    use std::mem::size_of;
    assert_eq!(size_of::<PredInfo>() as u64, strdec::PRED_INFO_BYTES);
    // Go's also holds three backing-array slices (72 bytes) that the Rust one does not.
    assert!(size_of::<WmiTile>() as u64 <= strdec::WMI_TILE_BYTES);
    assert!(size_of::<BitIO<'static>>() as u64 <= strdec::BIT_IO_BYTES);
    assert!(size_of::<CodingContext>() as u64 <= strdec::CTX_BYTES);
}
