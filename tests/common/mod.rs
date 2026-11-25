//! Shared helpers for the integration tests: golden files, the SHA-256, and Go's `rawPixels`.
#![allow(dead_code)]

pub mod sha256;

use jpegxr::{ColorModel, Image};
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A path under `tests/data/`.
pub fn testdata(rel: &str) -> PathBuf {
    root().join("tests/data").join(rel)
}

/// The T.834 conformance corpus, fetched by Go's `fetch.sh`; never in git.
pub fn conformance_dir() -> PathBuf {
    root().join("corpus/conformance/software/JXR_ConformanceSuite")
}

/// Reads a tab-separated golden file.
pub fn golden_lines(path: &Path) -> Vec<Vec<String>> {
    let b = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    b.trim()
        .split('\n')
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

/// Every `.jxr` under `dir`, sorted.
pub fn jxr_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "jxr") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Lays out `img` as interleaved samples, 16-bit little endian, in R,G,B order, or B,G,R when
/// `bgr` is set. The label names the libCZI pixel type.
pub fn raw_pixels(img: &Image, bgr: bool) -> (Vec<u8>, &'static str) {
    let n = img.width * img.height;
    let mut raw = Vec::new();
    match img.color_model {
        ColorModel::Gray => {
            raw.extend_from_slice(&img.pix);
            (raw, "Gray8")
        }
        ColorModel::Gray16 => {
            for i in 0..n {
                raw.push(img.pix[2 * i + 1]);
                raw.push(img.pix[2 * i]);
            }
            (raw, "Gray16")
        }
        ColorModel::Rgba => {
            for i in 0..n {
                let (mut r, g, mut b) = (img.pix[4 * i], img.pix[4 * i + 1], img.pix[4 * i + 2]);
                if bgr {
                    std::mem::swap(&mut r, &mut b);
                }
                raw.extend_from_slice(&[r, g, b]);
            }
            (raw, "Bgr24")
        }
        ColorModel::Rgba64 => {
            for i in 0..n {
                let p = &img.pix[8 * i..8 * i + 6];
                let (mut r, g, mut b) = ([p[0], p[1]], [p[2], p[3]], [p[4], p[5]]);
                if bgr {
                    std::mem::swap(&mut r, &mut b);
                }
                for c in [r, g, b] {
                    raw.push(c[1]);
                    raw.push(c[0]);
                }
            }
            (raw, "Bgr48")
        }
    }
}

/// `"w\th\tlabel\tsha256"` of `img`'s raw pixels.
pub fn describe(img: &Image, bgr: bool) -> String {
    let (raw, label) = raw_pixels(img, bgr);
    format!(
        "{}\t{}\t{}\t{}",
        img.width,
        img.height,
        label,
        sha256::hex(&raw)
    )
}

/// Whether the container's pixel format is 24bppBGR, 48bppRGB, 8bppGray, 16bppGray or
/// 48bpp3Channels.
pub fn gate_a_format(data: &[u8]) -> bool {
    // GUID 24C3DD6F-034E-FE4B-B185-3D77768DC9xx as stored (JXRGlueJxr.c:1553).
    const PREFIX: [u8; 15] = [
        0x24, 0xc3, 0xdd, 0x6f, 0x03, 0x4e, 0xfe, 0x4b, 0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9,
    ];
    if data.len() < 8 {
        return false;
    }
    let u32_at = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
    let ifd = u32_at(4) as usize;
    if ifd + 2 > data.len() {
        return false;
    }
    let n = usize::from(u16_at(ifd));
    for i in 0..n {
        let e = ifd + 2 + 12 * i;
        if e + 12 > data.len() {
            return false;
        }
        if u16_at(e) != 0xbc01 {
            continue;
        }
        let off = u32_at(e + 8) as usize;
        if off + 16 > data.len() || data[off..off + 15] != PREFIX {
            return false;
        }
        return matches!(data[off + 15], 0x0c | 0x15 | 0x08 | 0x0b | 0x26);
    }
    false
}
