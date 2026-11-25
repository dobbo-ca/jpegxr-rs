//! Ports of jpegxr-go's public-API tests: `decode_test.go`, `pin_test.go`, `component_test.go`,
//! `decoder_test.go` (short destination), `conformance_test.go` (gate A) and `example_test.go`.

mod common;

use common::*;
use jpegxr::{
    ColorModel, Decoder, Error, Format, Image, Limits, decode, decode_component, decode_config,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

fn limits() -> Limits {
    Limits::default()
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn dec(data: &[u8]) -> Result<Image, Error> {
    decode(data, &limits(), &no_cancel())
}

/// `TestSynth`: decodes `tests/data/synth` and compares with the output of libCZI's JPEG XR decoder
/// (`tests/data/synth/golden.txt`). libCZI has no 48bpp3Channels, so the N3x16 lines hash
/// JxrDecApp's TIFF strip.
#[test]
fn synth() {
    for f in golden_lines(&testdata("synth/golden.txt")) {
        let data = read(&testdata("synth").join(&f[0]));
        let img = dec(&data).unwrap_or_else(|e| panic!("{}: {e}", f[0]));
        let (raw, label) = raw_pixels(&img, true);
        let raw = if label == "Bgr48" {
            raw_pixels(&img, false).0
        } else {
            raw
        };
        let got = format!(
            "{}\t{}\t{}\t{}",
            img.width,
            img.height,
            label,
            sha256::hex(&raw)
        );
        assert_eq!(got, f[1..].join("\t"), "{}", f[0]);
    }
}

/// `TestDecodeConfig`: header sizes against every golden stream.
#[test]
fn config_matches_golden() {
    let check = |path: &Path, w: &str, h: &str, conformance: bool| {
        let data = read(path);
        match decode_config(&data, &limits()) {
            Ok(c) => assert_eq!(
                format!("{}\t{}", c.width, c.height),
                format!("{w}\t{h}"),
                "{}",
                path.display()
            ),
            Err(e) => {
                if gate_a_format(&data) || !conformance {
                    panic!("{}: {e}", path.display());
                }
            }
        }
    };
    for f in golden_lines(&testdata("synth/golden.txt")) {
        check(&testdata("synth").join(&f[0]), &f[1], &f[2], false);
    }
    let dir = conformance_dir();
    if dir.exists() {
        for f in golden_lines(&testdata("conformance/golden.txt")) {
            check(&dir.join(&f[0]), &f[1], &f[2], true);
        }
    }
}

#[test]
fn config_color_models() {
    let c = decode_config(
        &read(&testdata("synth/Gray8_lossy_untiled_ov1.jxr")),
        &limits(),
    )
    .unwrap();
    assert_eq!(
        (c.width, c.height, c.color_model),
        (64, 64, ColorModel::Gray)
    );
    let c = decode_config(
        &read(&testdata("synth/Gray16_lossless_untiled_ov1.jxr")),
        &limits(),
    )
    .unwrap();
    assert_eq!(c.color_model, ColorModel::Gray16);
    let c = decode_config(
        &read(&testdata("synth/Bgr24_lossy_untiled_ov1.jxr")),
        &limits(),
    )
    .unwrap();
    assert_eq!(c.color_model, ColorModel::Rgba);
    let c = decode_config(
        &read(&testdata("synth/Bgr48_lossless_untiled_ov1.jxr")),
        &limits(),
    )
    .unwrap();
    assert_eq!(c.color_model, ColorModel::Rgba64);
}

/// `FuzzDecode`'s seeds and its tests/data corpus run as tests: no input may end in an internal
/// error.
#[test]
fn fuzz_corpus_has_no_internal_error() {
    let mut inputs = Vec::new();
    for dir in ["tests/data/synth", "tests/data/fuzz"] {
        for e in std::fs::read_dir(root().join(dir)).unwrap().flatten() {
            if e.path().extension().is_none_or(|x| x != "txt") {
                inputs.push(read(&e.path()));
            }
        }
    }
    assert!(inputs.len() > 20);
    for b in inputs {
        let Ok(c) = decode_config(&b, &limits()) else {
            continue;
        };
        if (c.width * c.height) as u64 > 1 << 22 {
            continue;
        }
        if let Err(Error::Internal(m)) = dec(&b) {
            panic!("internal error: {m}");
        }
    }
}

/// The pinned decode of one stream: size, pixel type and sample hash, or the error.
fn pin(data: &[u8]) -> String {
    match dec(data) {
        Err(e) => format!("error\t{e}"),
        Ok(img) => describe(&img, false),
    }
}

/// `TestPinned`: the public decode of every tests/data and conformance stream against
/// `tests/data/pinned.txt`, so a change to one format cannot move another. `JXR_PIN_WRITE=1`
/// rewrites the file.
#[test]
fn pinned() {
    let mut got = std::collections::BTreeMap::new();
    for p in jxr_files(&root().join("tests/data")) {
        let key = p
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        got.insert(key, pin(&read(&p)));
    }
    let corpus = root().join("corpus/conformance");
    for p in jxr_files(&corpus) {
        let key = format!(
            "../../corpus/conformance/{}",
            p.strip_prefix(&corpus).unwrap().to_string_lossy()
        );
        got.insert(key, pin(&read(&p)));
    }
    let file = testdata("pinned.txt");
    if std::env::var("JXR_PIN_WRITE").as_deref() == Ok("1") {
        let lines: String = got.iter().map(|(p, v)| format!("{p}\t{v}\n")).collect();
        std::fs::write(&file, lines).unwrap();
        return;
    }
    let mut n = 0;
    for f in golden_lines(&file) {
        let on_disk = match f[0].strip_prefix("../../corpus/") {
            Some(rest) => root().join("corpus").join(rest).exists(),
            None => true,
        };
        if !on_disk {
            continue; // corpus not fetched
        }
        n += 1;
        let want = f[1..].join("\t");
        assert_eq!(got.get(&f[0]), Some(&want), "{}", f[0]);
        got.remove(&f[0]);
    }
    assert!(
        got.is_empty(),
        "not in pinned.txt: {:?}",
        got.keys().collect::<Vec<_>>()
    );
    assert!(n >= 23);
}

/// Channel `k` of `img` as big-endian Gray16 bytes.
fn channel(img: &Image, k: usize) -> Result<Vec<u8>, Error> {
    let n = img.width * img.height;
    match img.color_model {
        ColorModel::Gray16 if k == 0 => Ok(img.pix.clone()),
        ColorModel::Rgba64 if k <= 2 => {
            let mut out = Vec::with_capacity(2 * n);
            for i in 0..n {
                out.extend_from_slice(&img.pix[8 * i + 2 * k..8 * i + 2 * k + 2]);
            }
            Ok(out)
        }
        _ => Err(Error::PixelFormat),
    }
}

/// Compares `decode_component` with `decode`'s channel for `k` 0 to 3 and reports how many
/// decodes compared pixels.
fn check_components(name: &str, data: &[u8]) -> usize {
    let mut n = 0;
    let img = dec(data);
    for k in 0..4 {
        let want = match &img {
            Ok(i) => channel(i, k),
            Err(e) => Err(e.clone()),
        };
        let got = decode_component(data, k, &limits(), &no_cancel());
        match (want, got) {
            (Ok(w), Ok(g)) => {
                assert_eq!(
                    g.pix, w,
                    "{name} k {k}: decode_component differs from decode's channel"
                );
                n += 1;
            }
            (Err(_), Err(_)) => {}
            (w, g) => panic!(
                "{name} k {k}: decode_component {:?}, decode {:?}",
                g.err(),
                w.err()
            ),
        }
    }
    n
}

/// `TestDecodeComponent`: `decode_component` against `decode`'s channels on every tests/data and
/// conformance stream.
#[test]
fn decode_component_matches_channels() {
    let mut n = 0;
    for root_dir in [root().join("tests/data"), root().join("corpus")] {
        for p in jxr_files(&root_dir) {
            n += check_components(&p.display().to_string(), &read(&p));
        }
    }
    assert!(n > 0, "no stream compared");
}

#[test]
fn decode_component_rejects_8bit() {
    let data = read(&testdata("synth/Gray8_lossy_untiled_ov1.jxr"));
    assert_eq!(
        decode_component(&data, 0, &limits(), &no_cancel()).unwrap_err(),
        Error::PixelFormat
    );
    let data = read(&testdata("synth/Gray16_lossless_untiled_ov1.jxr"));
    assert_eq!(
        decode_component(&data, 1, &limits(), &no_cancel()).unwrap_err(),
        Error::PixelFormat
    );
}

/// `TestDecodeIntoShortDst`: a short dst or stride is an error, not a panic or a partial image.
#[test]
fn decode_into_short_dst() {
    for name in [
        "Gray8_lossy_untiled_ov1.jxr",
        "Bgr24_lossless_tiled_ov0.jxr",
    ] {
        let data = read(&testdata("synth").join(name));
        let d = Decoder::new(&data, &limits()).unwrap();
        let (w, h, f) = d.config();
        let s = match f {
            Format::Gray8 => w,
            Format::Gray16 => 2 * w,
            Format::Bgr24 => 3 * w,
            Format::Bgr48 => 6 * w,
        };
        let mut dst = vec![0u8; s * h];
        let n = dst.len();
        assert_eq!(
            d.decode_into(&mut dst[..n - 1], s, &no_cancel()),
            Err(Error::DstTooSmall),
            "{name} short dst"
        );
        assert_eq!(
            d.decode_into(&mut dst, s - 1, &no_cancel()),
            Err(Error::DstTooSmall),
            "{name} short stride"
        );
        assert!(dst.iter().all(|&b| b == 0), "{name}: wrote on error");
    }
}

/// `ExampleDecode`.
#[test]
fn example_decode() {
    let data = read(&testdata("synth/Gray8_lossy_untiled_ov1.jxr"));
    let img = dec(&data).unwrap();
    assert_eq!(
        (img.width, img.height, img.color_model),
        (64, 64, ColorModel::Gray)
    );
}

/// `ExampleDecoder`: the same stream decoded twice into one buffer.
#[test]
fn example_decoder() {
    let data = read(&testdata("synth/Gray8_lossy_untiled_ov1.jxr"));
    let mut dst: Vec<u8> = Vec::new();
    for _ in 0..2 {
        let d = Decoder::new(&data, &limits()).unwrap();
        let (w, h, f) = d.config();
        assert_eq!(f, Format::Gray8);
        if dst.is_empty() {
            dst = vec![0; w * h]; // Gray8 stride is w
        }
        d.decode_into(&mut dst, w, &no_cancel()).unwrap();
        assert_eq!((w, h, dst.len()), (64, 64, 4096));
    }
}

#[test]
fn decoder_matches_decode() {
    let data = read(&testdata("synth/Bgr24_lossy_untiled_ov1.jxr"));
    let d = Decoder::new(&data, &limits()).unwrap();
    let (w, h, f) = d.config();
    assert_eq!(f, Format::Bgr24);
    let mut raw = vec![0u8; 3 * w * h];
    d.decode_into(&mut raw, 3 * w, &no_cancel()).unwrap();
    let img = dec(&data).unwrap();
    for i in 0..w * h {
        assert_eq!(
            [
                img.pix[4 * i],
                img.pix[4 * i + 1],
                img.pix[4 * i + 2],
                img.pix[4 * i + 3]
            ],
            [raw[3 * i + 2], raw[3 * i + 1], raw[3 * i], 255]
        );
    }
}

#[test]
fn cancel_flag_stops_decode() {
    let data = read(&testdata("synth/Bgr24_lossy_untiled_ov1.jxr"));
    let cancel = AtomicBool::new(true);
    assert_eq!(
        decode(&data, &limits(), &cancel).unwrap_err(),
        Error::Cancelled
    );
    assert_eq!(
        decode_component(
            &read(&testdata("synth/Gray16_lossless_untiled_ov1.jxr")),
            0,
            &limits(),
            &cancel
        )
        .unwrap_err(),
        Error::Cancelled
    );
    cancel.store(false, Ordering::Relaxed);
    assert!(decode(&data, &limits(), &cancel).is_ok());
}

/// Go's budget tests (`reserve` fits, too large) become `Limits::max_alloc` checks: an estimate
/// over the cap is `AllocLimit`, distinct from the other limit errors.
#[test]
fn max_alloc_is_charged_before_decoding() {
    let data = read(&testdata("synth/Bgr24_lossy_untiled_ov1.jxr"));
    let d = Decoder::new(&data, &limits()).unwrap();
    let est = d.estimate();
    let (w, h, _) = d.config();
    let l = Limits {
        max_alloc: est - 1,
        ..limits()
    };
    let mut dst = vec![0u8; 3 * w * h];
    let d = Decoder::new(&data, &l).unwrap();
    assert_eq!(
        d.decode_into(&mut dst, 3 * w, &no_cancel()),
        Err(Error::AllocLimit {
            need: est,
            max: est - 1
        })
    );
    assert!(dst.iter().all(|&b| b == 0));
    let l = Limits {
        max_alloc: est,
        ..limits()
    };
    assert!(
        Decoder::new(&data, &l)
            .unwrap()
            .decode_into(&mut dst, 3 * w, &no_cancel())
            .is_ok()
    );
    // decode charges the output image as well
    let l = Limits {
        max_alloc: est,
        ..limits()
    };
    assert!(matches!(
        decode(&data, &l, &no_cancel()),
        Err(Error::AllocLimit { .. })
    ));
    let l = Limits {
        max_alloc: 0,
        ..limits()
    };
    assert!(matches!(
        decode_component(
            &read(&testdata("synth/Gray16_lossless_untiled_ov1.jxr")),
            0,
            &l,
            &no_cancel()
        ),
        Err(Error::AllocLimit { .. })
    ));
}

#[test]
fn image_size_limits() {
    let data = read(&testdata("synth/Bgr24_lossy_untiled_ov1.jxr")); // 64 x 64
    let l = Limits {
        max_width: 63,
        ..limits()
    };
    assert_eq!(decode_config(&data, &l), Err(Error::LimitExceeded("width")));
    assert!(matches!(
        Decoder::new(&data, &l),
        Err(Error::LimitExceeded("width"))
    ));
    let l = Limits {
        max_height: 63,
        ..limits()
    };
    assert_eq!(
        decode_config(&data, &l),
        Err(Error::LimitExceeded("height"))
    );
    let l = Limits {
        max_pixels: 64 * 64 - 1,
        ..limits()
    };
    assert_eq!(
        decode(&data, &l, &no_cancel()).unwrap_err(),
        Error::LimitExceeded("pixels")
    );
    let l = Limits {
        max_pixels: 64 * 64,
        max_width: 64,
        max_height: 64,
        ..limits()
    };
    assert!(decode(&data, &l, &no_cancel()).is_ok());
}

#[test]
fn truncated_and_garbage_input_is_an_error() {
    let data = read(&testdata("synth/N3x16_lossy_tiled_ov1.jxr"));
    for n in [0, 1, 7, 8, 16, 40, data.len() / 2, data.len() - 1] {
        let r = dec(&data[..n]);
        assert!(!matches!(r, Err(Error::Internal(_))), "len {n}: {r:?}");
    }
    assert_eq!(dec(&[]).unwrap_err(), Error::Container);
    assert_eq!(
        dec(b"not a jpeg xr file at all").unwrap_err(),
        Error::Container
    );
}

/// `TestConformanceGateA`: decodes of the CZI-format T.834 streams against jxrlib's TIFF strip
/// bytes. Skipped without the corpus (`corpus/conformance`, never in git).
#[test]
fn conformance_gate_a() {
    let dir = conformance_dir();
    if !dir.exists() {
        eprintln!(
            "no conformance corpus; run jpegxr-go's internal/jpegxr/testdata/conformance/fetch.sh"
        );
        return;
    }
    let (mut pass, mut total) = (0, 0);
    for f in golden_lines(&testdata("conformance/golden.txt")) {
        assert_eq!(f.len(), 5);
        let data = read(&dir.join(&f[0]));
        if !gate_a_format(&data) {
            continue;
        }
        total += 1;
        let img = dec(&data).unwrap_or_else(|e| panic!("{}: {e}", f[0]));
        let (raw, _) = raw_pixels(&img, false);
        let got = format!("{}\t{}\t{}", img.width, img.height, sha256::hex(&raw));
        assert_eq!(got, format!("{}\t{}\t{}", f[1], f[2], f[4]), "{}", f[0]);
        pass += 1;
    }
    eprintln!("gateA pass {pass}/{total}");
    assert_eq!(total, 282);
}

/// Bits, most significant first, padded with zeros to whole bytes.
fn bits(fields: &[(u32, u32)]) -> Vec<u8> {
    let (mut out, mut acc, mut n) = (Vec::new(), 0u64, 0u32);
    for &(v, w) in fields {
        acc = (acc << w) | u64::from(v);
        n += w;
        while n >= 8 {
            out.push((acc >> (n - 8)) as u8);
            n -= 8;
        }
    }
    if n > 0 {
        out.push((acc << (8 - n)) as u8);
    }
    out
}

/// A container with a real IFD (copied from a synthetic Gray8 stream) around `codestream`.
fn with_codestream(codestream: &[u8]) -> Vec<u8> {
    let base = read(&testdata("synth/Gray8_lossless_untiled_ov1.jxr"));
    assert_eq!(&base[0x86..0x8d], b"WMPHOTO");
    let mut v = base[..0x86].to_vec();
    v.extend_from_slice(codestream);
    v
}

/// The header of a short, tiled, soft-tile Gray8 image of `nv` + 1 by `nh` + 1 tiles, with
/// `tail` bytes after the fields.
fn tiled_header(nv: u32, nh: u32, stretch: bool, tail: usize) -> Vec<u8> {
    let mut cs = b"WMPHOTO\0".to_vec();
    cs.extend(bits(&[
        (1, 4),
        (1, 4), // version, subversion (soft tiles)
        (1, 1), // tiling
        (0, 1), // spatial
        (0, 3), // orientation
        (1, 1), // index table
        (1, 2), // overlap
        (1, 1), // short header: 16-bit sizes, 8-bit tile fields
        (0, 1),
        (0, 1),
        (0, 1),
        (u32::from(stretch), 1),
        (0, 1),
        (0, 1),
        (0, 1), // alpha
        (0, 4),
        (1, 4), // Y_ONLY, 8 bits
        (63, 16),
        (63, 16),
        (nv, 12),
        (nh, 12),
    ]));
    cs.extend(std::iter::repeat_n(0, tail));
    with_codestream(&cs)
}

/// A header whose tile counts the stream cannot hold is a bad codestream (as Go's bit reader finds
/// it), with no allocation and no scan of the 16 million tile_stretch bytes.
#[test]
fn tile_fields_past_the_input_are_a_codestream_error() {
    for stretch in [false, true] {
        let b = tiled_header(4095, 4095, stretch, 20);
        assert_eq!(
            decode_config(&b, &limits()),
            Err(Error::Codestream),
            "stretch {stretch}"
        );
        assert!(matches!(
            Decoder::new(&b, &limits()),
            Err(Error::Codestream)
        ));
    }
    // Fields that just fit are read (and the rest of the header then fails, or not, as a stream).
    let b = tiled_header(3, 3, true, 6 + 16 + 40);
    assert!(!matches!(
        decode_config(&b, &limits()),
        Err(Error::AllocLimit { .. })
    ));
}

/// The header's tile tables are charged against `max_alloc` (a deviation from Go, which had no
/// cap; its largest table is 32 KiB, under the parity floor of 64 KiB).
#[test]
fn tile_tables_are_charged() {
    let b = tiled_header(100, 100, false, 220);
    let need = 4 * (100 + 100 + 2);
    let l = Limits {
        max_alloc: need - 1,
        ..limits()
    };
    assert_eq!(
        decode_config(&b, &l),
        Err(Error::AllocLimit {
            need,
            max: need - 1
        })
    );
    assert!(matches!(
        Decoder::new(&b, &l),
        Err(Error::AllocLimit { .. })
    ));
    let l = Limits {
        max_alloc: need,
        ..limits()
    };
    assert!(!matches!(
        decode_config(&b, &l),
        Err(Error::AllocLimit { .. })
    ));
}

/// 16-bit samples with an odd stride are refused (jxrlib and Go take `stride / 2` samples a row),
/// after the size check: one byte short is still `DstTooSmall`, and nothing is written.
#[test]
fn odd_stride_is_refused_for_16_bit_formats() {
    for name in [
        "synth/Gray16_lossless_untiled_ov1.jxr",
        "synth/Bgr48_lossless_untiled_ov1.jxr",
    ] {
        let data = read(&testdata(name));
        let d = Decoder::new(&data, &limits()).unwrap();
        let (w, h, f) = d.config();
        let row = match f {
            Format::Gray16 => 2 * w,
            _ => 6 * w,
        };
        let stride = row + 1;
        let need = (h - 1) * stride + row;
        let mut dst = vec![0x5a; need];
        assert_eq!(
            d.decode_into(&mut dst, stride, &no_cancel()),
            Err(Error::OddStride),
            "{name}"
        );
        assert!(dst.iter().all(|&b| b == 0x5a), "{name}: wrote");
        assert_eq!(
            d.decode_into(&mut dst[..need - 1], stride, &no_cancel()),
            Err(Error::DstTooSmall),
            "{name}"
        );
        // One byte under a row is odd too, and short: DstTooSmall wins.
        assert_eq!(
            d.decode_into(&mut dst, row - 1, &no_cancel()),
            Err(Error::DstTooSmall),
            "{name}"
        );
        // An even stride decodes.
        let mut ok = vec![0; (h - 1) * (row + 2) + row];
        assert_eq!(d.decode_into(&mut ok, row + 2, &no_cancel()), Ok(()));
    }
    // 8-bit formats take any stride.
    let data = read(&testdata("synth/Gray8_lossless_untiled_ov1.jxr"));
    let d = Decoder::new(&data, &limits()).unwrap();
    let (w, h, _) = d.config();
    let mut dst = vec![0; (h - 1) * (w + 1) + w];
    assert_eq!(d.decode_into(&mut dst, w + 1, &no_cancel()), Ok(()));
}

/// A 1-channel N_COMPONENT codestream in a 48bpp3Channels container (fuzz crash, seed
/// `tests/fuzz/seeds/n-component-short-codestream`): the codestream has no bytes for 4 of the 6 bytes
/// of each pixel. They are written as zero, so the output does not depend on the old `dst`.
#[test]
fn short_n_component_writes_every_byte() {
    let data = read(&root().join("tests/fuzz/seeds/n-component-short-codestream"));
    let (stream, pad) = (
        &data[..data.len() - 1],
        usize::from(data[data.len() - 1] % 5),
    );
    let d = Decoder::new(stream, &limits()).unwrap();
    let (w, h, f) = d.config();
    assert_eq!(f, Format::Bgr48);
    let stride = 6 * w + pad;
    assert_eq!(stride % 2, 0);
    let mut zeroed = vec![0u8; (h - 1) * stride + 6 * w];
    let mut stale = vec![0xa5u8; zeroed.len()];
    assert_eq!(d.decode_into(&mut zeroed, stride, &no_cancel()), Ok(()));
    assert_eq!(d.decode_into(&mut stale, stride, &no_cancel()), Ok(()));
    for y in 0..h {
        let row = y * stride..y * stride + 6 * w;
        assert!(
            zeroed[row.clone()] == stale[row],
            "row {y} depends on the old dst"
        );
    }
    // The padding is never written.
    assert!(stale[6 * w..stride].iter().all(|&b| b == 0xa5));
}

/// `kernels::out_bgr8` implements only the pairs the decoder uses, (0, 2) and (2, 0).
#[test]
fn out_bgr8_rejects_other_channel_pairs() {
    let plane = vec![0i32; 198];
    for ir_ib in [(0, 0), (1, 1), (2, 2), (1, 0), (0, 1), (3, 0)] {
        let mut out = vec![0xa5u8; 48];
        let r = jpegxr::kernels::out_bgr8(
            true,
            &mut out,
            0,
            (&plane, &plane, &plane),
            0,
            1,
            0,
            0,
            ir_ib,
            &limits(),
        );
        assert_eq!(
            r,
            Err(Error::LimitExceeded("kernel argument out of range")),
            "{ir_ib:?}"
        );
    }
    for ir_ib in [(0, 2), (2, 0)] {
        let mut out = vec![0xa5u8; 48];
        let r = jpegxr::kernels::out_bgr8(
            true,
            &mut out,
            0,
            (&plane, &plane, &plane),
            0,
            1,
            0,
            0,
            ir_ib,
            &limits(),
        );
        assert_eq!(r, Ok(()), "{ir_ib:?}");
    }
}
