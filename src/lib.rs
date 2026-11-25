//! A pure Rust JPEG XR (ITU-T T.832) decoder, built for the JPEG XR tiles inside Zeiss CZI
//! slides. Decode only; a port of dobbo-ca/jpegxr-go.
//!
//! # Resource limits
//!
//! Every entry point takes [`Limits`]. A zero field is a real cap, except `max_threads`, where 0
//! counts as 1. Go had no per-call caps; the defaults are this crate's own.
//!
//! | field | default | unit | over it |
//! |---|---|---|---|
//! | [`Limits::max_alloc`] | 2 GiB | bytes, the peak of the call, all threads together | [`Error::AllocLimit`] |
//! | [`Limits::max_pixels`] | 2^30 | pixels | [`Error::LimitExceeded`]`("pixels")` (and [`Error::TooLarge`] over 2^40, which no limit lifts) |
//! | [`Limits::max_width`] | 2^20 | pixels | [`Error::LimitExceeded`]`("width")` |
//! | [`Limits::max_height`] | 2^20 | pixels | [`Error::LimitExceeded`]`("height")` |
//! | [`Limits::max_threads`] | `available_parallelism` | threads, the calling thread included | none: the call uses fewer |
//!
//! - `max_alloc` is the enforced peak of one call, threads included. Go's estimate of the decode
//!   ([`Decoder::estimate`]) is checked first (plus the output for `decode` and
//!   `decode_component`), then every allocation that grows with the input is charged to a ledger
//!   before it is made, and all of a decode's charges are given back when it returns. The
//!   borrowed input is not charged. The header's tile tables are capped by it too.
//! - `max_threads` = 1 spawns nothing and never touches a pool. Above 1 the decoder is a
//!   two-stage pipeline (entropy decoding on a pool worker, transform and output on the calling
//!   thread), so it never uses more than 2. It runs on fewer when the image is small (under 16
//!   macroblock columns or 256 macroblocks) or the buffers for a second thread do not fit in
//!   `max_alloc`; the output and any error are the same for every value.
//! - Pools: a call without a caller's pool uses a lazy shared pool inside the crate, created on
//!   the first call that uses threads and never larger than `available_parallelism` threads.
//!   [`Decoder::with_pool`] runs on the caller's [`ThreadPool`] instead, and a caller's pool
//!   always wins (the shared pool is then never started). `max_threads` = 1 never touches either.
//!   A pool's threads are not charged to `max_alloc`. A busy pool never costs a call its result
//!   (the calling thread does the rows), but the call returns only after a pool thread has
//!   picked up its queued worker job, which then exits at once.
//! - A panic in the decoder is caught and returned as [`Error::Internal`].
//!
//! The cancel flag is checked once per macroblock row (and between 16 MiB chunks of the output
//! buffer's zero fill); a set flag returns [`Error::Cancelled`]. A pipelined call on a busy pool
//! returns it only after a pool thread has picked up its queued worker job (see above).
//!
//! ```
//! use jpegxr::Limits;
//! use std::sync::atomic::AtomicBool;
//!
//! let data = std::fs::read(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/tests/data/synth/Gray8_lossless_untiled_ov1.jxr"
//! ))?;
//! // A 2-CPU, 1 GiB pod: two threads, 768 MiB.
//! let limits = Limits { max_threads: 2, max_alloc: 768 << 20, ..Limits::default() };
//! let cancel = AtomicBool::new(false);
//! let image = jpegxr::decode(&data, &limits, &cancel)?;
//! assert_eq!((image.width, image.height), (64, 64));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![deny(
    unsafe_code,
    unsafe_op_in_unsafe_fn,
    clippy::undocumented_unsafe_blocks
)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

#[cfg(not(target_pointer_width = "64"))]
compile_error!("jpegxr supports 64-bit targets only: header sizes are held in usize");

mod codec;
mod container;
mod error;
mod limits;
mod par;

pub use error::Error;
pub use limits::Limits;
pub use par::{Probe, ThreadPool, shared_pool_started};

use codec::{BD8, BD16, CF_RGB, N_COMPONENT, Y_ONLY};
use par::{Ctx, OnRows};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
mod tests;

// The README's code blocks, compiled (not run) as doc-tests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The SIMD kernels beside their scalar references, for the cross-tier fuzz targets and the kernel
/// parity check. Each takes `simd`: `false` runs the scalar code, `true` the tier this CPU has
/// (ignoring `JPEGXR_SIMD`). Every index and length is checked against the slices before the
/// kernel runs, so a bad argument is `Error::LimitExceeded`, not a panic (a panic would still be
/// returned as `Error::Internal`, unless the caller builds with `panic = "abort"`). A slice over
/// `Limits::max_alloc` bytes is refused.
#[doc(hidden)]
pub mod kernels {
    use crate::codec::simd::{self as k, Tier};
    use crate::{Error, Limits, run};

    /// The tier `simd: true` runs: "avx2", "neon" or "scalar". Allocates nothing, so `limits` is
    /// not read; it is here because every public function takes one.
    pub fn tier_name(_limits: &Limits) -> &'static str {
        match k::detect() {
            Tier::Avx2 => "avx2",
            Tier::Neon => "neon",
            Tier::Scalar => "scalar",
        }
    }

    fn tier(simd: bool) -> Tier {
        if simd { k::detect() } else { Tier::Scalar }
    }

    fn charge(bytes: usize, limits: &Limits) -> Result<(), Error> {
        if bytes as u64 > limits.max_alloc {
            return Err(Error::LimitExceeded("kernel buffer"));
        }
        Ok(())
    }

    fn bad() -> Error {
        Error::LimitExceeded("kernel argument out of range")
    }

    /// `x` as an index: not negative.
    fn idx(x: isize) -> Result<usize, Error> {
        usize::try_from(x).map_err(|_| bad())
    }

    /// `start + n <= len`, without overflow.
    fn fits(len: usize, start: usize, n: usize) -> Result<(), Error> {
        match start.checked_add(n) {
            Some(end) if end <= len => Ok(()),
            _ => Err(bad()),
        }
    }

    /// `s + 256 * (n - 1) + 198 <= len`: the last of the 16 pixels of the last of `n` groups.
    fn fits_groups(len: usize, s: usize, n: usize) -> Result<(), Error> {
        match n.checked_sub(1) {
            None => Ok(()),
            Some(m) => fits(
                len,
                s,
                m.checked_mul(256).ok_or_else(bad)?.saturating_add(198),
            ),
        }
    }

    /// The 4x4 IDCT stage 1 on `n` pairs of blocks from `p[o..]`.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] when the slices pass `max_alloc` ("kernel buffer") or an index or
    /// length is out of range ("kernel argument out of range"); [`Error::Internal`] for a caught
    /// panic.
    pub fn idct_pairs(
        simd: bool,
        p: &mut [i32],
        o: isize,
        n: isize,
        limits: &Limits,
    ) -> Result<(), Error> {
        charge(4 * p.len(), limits)?;
        let (ou, nu) = (idx(o)?, idx(n)?);
        if let Some(m) = nu.checked_sub(1) {
            // The last pair's two blocks end 32 past its start.
            fits(
                p.len(),
                ou,
                m.checked_mul(64).ok_or_else(bad)?.saturating_add(32),
            )?;
        }
        run(|| {
            k::idct_pairs(tier(simd), p, o, n);
            Ok(())
        })
    }

    /// The stage 1 post filter on the runs at `p0` and `p1`. The four runs of four it touches
    /// (`p0 + 12`, `p0 + 72 - i_offset`, `p1 + 4`, `p1 + 64 - i_offset`) must lie inside `p` and
    /// not overlap, as in the decoder; the SIMD tier reads all four before it writes any.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] when the slices pass `max_alloc` ("kernel buffer") or an index or
    /// length is out of range ("kernel argument out of range"); [`Error::Internal`] for a caught
    /// panic.
    pub fn post_split(
        simd: bool,
        p: &mut [i32],
        p0: isize,
        p1: isize,
        i_offset: isize,
        limits: &Limits,
    ) -> Result<(), Error> {
        charge(4 * p.len(), limits)?;
        let runs = [
            p0.checked_add(12),
            p0.checked_add(72).and_then(|x| x.checked_sub(i_offset)),
            p1.checked_add(4),
            p1.checked_add(64).and_then(|x| x.checked_sub(i_offset)),
        ];
        let mut at = [0usize; 4];
        for (a, r) in at.iter_mut().zip(runs) {
            *a = idx(r.ok_or_else(bad)?)?;
            fits(p.len(), *a, 4)?;
        }
        if at
            .iter()
            .enumerate()
            .any(|(i, &a)| at[i + 1..].iter().any(|&b| a.abs_diff(b) < 4))
        {
            return Err(bad());
        }
        run(|| {
            k::post_split(tier(simd), p, p0, p1, i_offset);
            Ok(())
        })
    }

    /// The BGR8 output loop: `n` macroblock groups of 16 pixels. `ir_ib` is `(0, 2)` or `(2, 0)`,
    /// the only pairs the decoder uses and the SIMD tier implements.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] when the slices pass `max_alloc` ("kernel buffer") or an index or
    /// length is out of range ("kernel argument out of range"); [`Error::Internal`] for a caught
    /// panic.
    #[allow(clippy::too_many_arguments)]
    pub fn out_bgr8(
        simd: bool,
        out: &mut [u8],
        d: usize,
        yuv: (&[i32], &[i32], &[i32]),
        s: usize,
        n: usize,
        bias: i32,
        shift: u32,
        ir_ib: (usize, usize),
        limits: &Limits,
    ) -> Result<(), Error> {
        charge(
            out.len() + 4 * (yuv.0.len() + yuv.1.len() + yuv.2.len()),
            limits,
        )?;
        if !matches!(ir_ib, (0, 2) | (2, 0)) {
            return Err(bad());
        }
        fits(out.len(), d, n.checked_mul(48).ok_or_else(bad)?)?;
        for plane in [yuv.0, yuv.1, yuv.2] {
            fits_groups(plane.len(), s, n)?;
        }
        run(|| {
            k::out_bgr8(tier(simd), out, d, yuv, s, n, bias, shift, ir_ib);
            Ok(())
        })
    }

    /// The Gray16 output loop: `n` groups of 16 pixels.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] when the slices pass `max_alloc` ("kernel buffer") or an index or
    /// length is out of range ("kernel argument out of range"); [`Error::Internal`] for a caught
    /// panic.
    #[allow(clippy::too_many_arguments)]
    pub fn out_gray16(
        simd: bool,
        out: &mut [u8],
        d: usize,
        y: &[i32],
        s: usize,
        n: usize,
        bias: i32,
        shift: u32,
        n_len: u32,
        limits: &Limits,
    ) -> Result<(), Error> {
        charge(out.len() + 4 * y.len(), limits)?;
        fits(out.len(), d, n.checked_mul(32).ok_or_else(bad)?)?;
        fits_groups(y.len(), s, n)?;
        run(|| {
            k::out_gray16(tier(simd), out, d, y, s, n, bias, shift, n_len);
            Ok(())
        })
    }

    /// The scalar flexbits path through `decode_block_adaptive`: three blocks on one reader over
    /// `buf` that first reads `skip` bits. `model_trim_qp` is the block's model bits (0 to 15),
    /// trim (0 to the model bits) and quantizer; `coef` has at least 16 entries. Returns `coef`
    /// and the reader's `acc`, `nb` and `off` as little-endian bytes (the `canon` form of
    /// `tests/parity/go/upstream/cmd/kernelgen`). There is no SIMD tier.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] when the slices pass `max_alloc` ("kernel buffer") or an index or
    /// length is out of range ("kernel argument out of range"); [`Error::Internal`] for a caught
    /// panic.
    pub fn flexbits(
        coef: &mut [i32],
        buf: &[u8],
        skip: i32,
        model_trim_qp: (i32, i32, i32),
        limits: &Limits,
    ) -> Result<Vec<u8>, Error> {
        charge(4 * coef.len() + buf.len(), limits)?;
        let (model, trim, _) = model_trim_qp;
        if coef.len() < 16 || skip < 0 || !(0..=15).contains(&model) || !(0..=model).contains(&trim)
        {
            return Err(bad());
        }
        run(|| Ok(k::flexbits_case(coef, buf, skip, model_trim_qp)))
    }

    /// The scalar `decode_block_highpass` for one block, on the 21 tables
    /// `(n_symbols, table_index, disc, disc1)` and 16 scan entries `(u_total, u_scan)` (each
    /// `u_scan` below 16), after `skip` bits of `buf` (0 to 63). `coef` has at least 16 entries.
    /// Returns the count, `coef`, every table, the scan and the reader's state as little-endian
    /// bytes (the `canon` form of `tests/parity/go/upstream/cmd/kernelgen`). A table that names a
    /// table index its symbol count lacks panics in the decoder and comes back as
    /// `Error::Internal`. There is no SIMD tier.
    ///
    /// # Errors
    /// [`Error::LimitExceeded`] when the slices pass `max_alloc` ("kernel buffer") or an index or
    /// length is out of range ("kernel argument out of range"); [`Error::Internal`] for a caught
    /// panic.
    #[allow(clippy::too_many_arguments)]
    pub fn highpass(
        chroma: bool,
        skip: u32,
        qp: i32,
        tables: &[(usize, i32, i32, i32); 21],
        scans: &[(u32, u32); 16],
        buf: &[u8],
        coef: &mut [i32],
        limits: &Limits,
    ) -> Result<Vec<u8>, Error> {
        charge(4 * coef.len() + buf.len(), limits)?;
        if coef.len() < 16
            || skip > 63
            || tables
                .iter()
                .any(|&(n, t, _, _)| n > 12 || !(0..5).contains(&t))
            || scans.iter().any(|&(_, s)| s >= 16)
        {
            return Err(bad());
        }
        run(|| Ok(k::highpass_case(chroma, skip, qp, tables, scans, buf, coef)))
    }
}

/// libCZI's pixel type for a decoded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bgr24,
    Bgr48,
    Gray8,
    Gray16,
}

/// The byte layout of a decoded [`Image`], named for the Go `image` type it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorModel {
    /// `image.Gray`: one byte a pixel (8bppGray).
    Gray,
    /// `image.Gray16`: one big-endian 16-bit sample a pixel (16bppGray).
    Gray16,
    /// `image.RGBA`: R, G, B, 0xff (24bppBGR).
    Rgba,
    /// `image.RGBA64`: big-endian 16-bit R, G, B, 0xffff (48bppRGB and 48bpp3Channels, with
    /// channel k in R, G, B).
    Rgba64,
}

impl ColorModel {
    /// Bytes a pixel takes in [`Image::pix`].
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            ColorModel::Gray => 1,
            ColorModel::Gray16 => 2,
            ColorModel::Rgba => 4,
            ColorModel::Rgba64 => 8,
        }
    }
}

/// The size and colour model of a JPEG XR image (`image.Config`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub width: usize,
    pub height: usize,
    pub color_model: ColorModel,
}

/// A decoded image. Row `y` starts at `pix[y * width * color_model.bytes_per_pixel()]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub color_model: ColorModel,
    pub pix: Vec<u8>,
}

/// Maximum pixels: keeps the output buffer size within an int in Go.
const MAX_PIXELS: u64 = 1 << 40;

/// The stored GUID 24C3DD6F-034E-FE4B-B185-3D77768DC9xx.
const PIXFMT_PREFIX: [u8; 15] = [
    0x24, 0xc3, 0xdd, 0x6f, 0x03, 0x4e, 0xfe, 0x4b, 0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9,
];

/// Pixel formats by last GUID byte (JXRGlue.c).
fn pixel_format_of(last: u8) -> Option<codec::Format> {
    let f = |color_format, bit_depth, bits_per_unit, rgb| codec::Format {
        color_format,
        bit_depth,
        bits_per_unit,
        rgb,
        alpha: false,
        plane: 0,
    };
    Some(match last {
        0x0c => f(CF_RGB, BD8, 24, false),      // 24bppBGR
        0x15 => f(CF_RGB, BD16, 48, true),      // 48bppRGB
        0x08 => f(Y_ONLY, BD8, 8, false),       // 8bppGray
        0x0b => f(Y_ONLY, BD16, 16, false),     // 16bppGray
        0x26 => f(N_COMPONENT, BD16, 48, true), // 48bpp3Channels
        _ => return None,
    })
}

fn parse(data: &[u8]) -> Result<(codec::Format, &[u8]), Error> {
    let info = container::read(data)?;
    let f = pixel_format_of(info.pixel_format[15]);
    match f {
        Some(f) if info.pixel_format[..15] == PIXFMT_PREFIX => {
            Ok((f, &data[info.image_offset as usize..]))
        }
        _ => Err(Error::PixelFormat),
    }
}

fn format_of(f: &codec::Format) -> Format {
    if f.color_format == Y_ONLY && f.bit_depth == BD8 {
        Format::Gray8
    } else if f.color_format == Y_ONLY {
        Format::Gray16
    } else if f.bit_depth == BD8 {
        Format::Bgr24
    } else {
        Format::Bgr48
    }
}

fn color_model_of(f: &codec::Format) -> ColorModel {
    if f.color_format == Y_ONLY && f.bit_depth == BD8 {
        ColorModel::Gray
    } else if f.color_format == Y_ONLY {
        ColorModel::Gray16
    } else if f.bit_depth == BD8 {
        ColorModel::Rgba
    } else {
        ColorModel::Rgba64
    }
}

/// The row length in bytes libCZI expects for `format`.
fn stride_for(w: usize, format: Format) -> usize {
    match format {
        Format::Gray8 => w,
        Format::Gray16 => w * 2,
        Format::Bgr24 => w * 3,
        Format::Bgr48 => w * 6,
    }
}

/// `n * w * h` bytes, or [`Error::TooLarge`] when that overflows `usize`.
fn size(n: u64, w: usize, h: usize) -> Result<usize, Error> {
    (w as u64)
        .checked_mul(h as u64)
        .and_then(|px| px.checked_mul(n))
        .and_then(|b| usize::try_from(b).ok())
        .ok_or(Error::TooLarge)
}

fn check_dims(w: usize, h: usize, limits: &Limits) -> Result<(), Error> {
    let (w, h) = (w as u64, h as u64);
    // Go's product wraps (only at 2^32 x 2^32), so it skips this check and the limits below
    // catch it; with limits that allow it, `size` rejects it.
    if w.checked_mul(h).is_some_and(|px| px > MAX_PIXELS) {
        return Err(Error::TooLarge);
    }
    if w > limits.max_width {
        return Err(Error::LimitExceeded("width"));
    }
    if h > limits.max_height {
        return Err(Error::LimitExceeded("height"));
    }
    if w.saturating_mul(h) > limits.max_pixels {
        return Err(Error::LimitExceeded("pixels"));
    }
    Ok(())
}

/// Charges `est` bytes against `limits.max_alloc`: `Go`'s `errBudgetExceeded`.
fn check_alloc(est: u64, limits: &Limits) -> Result<(), Error> {
    if est > limits.max_alloc {
        return Err(Error::AllocLimit {
            need: est,
            max: limits.max_alloc,
        });
    }
    Ok(())
}

/// Returns the size and colour model of a JPEG XR image.
///
/// Enforces [`Limits::max_width`], [`Limits::max_height`] and [`Limits::max_pixels`], and caps the
/// header's tile tables by [`Limits::max_alloc`]. Allocates nothing else and uses no threads, so
/// `max_threads` has no effect. It takes no cancel flag: parsing the header is bounded by the
/// length of `data`.
///
/// # Errors
/// [`Error::Container`], [`Error::PixelFormat`] or [`Error::Codestream`] for a malformed or
/// unsupported stream; [`Error::TooLarge`] over 2^40 pixels; [`Error::LimitExceeded`] when the
/// width, height or pixel count passes its limit; [`Error::AllocLimit`] when the header's tile
/// tables alone pass `max_alloc`.
pub fn decode_config(data: &[u8], limits: &Limits) -> Result<Config, Error> {
    let (f, stream) = parse(data)?;
    let h = codec::Header::parse(stream, limits.max_alloc, &mut codec::TileTables::default())?;
    check_dims(h.width, h.height, limits)?;
    Ok(Config {
        width: h.width,
        height: h.height,
        color_model: color_model_of(&f),
    })
}

/// The working buffers of decodes, kept from one to the next: the header's tile tables and the
/// decoder's macroblock rows, prediction state, tile quantizers, coding contexts, bit readers,
/// index table, output offsets and the pipeline's ring (Go's `Decoder` kept them across
/// `Reset`). Make one per worker thread and pass it to [`Decoder::reusing`] for every tile: a
/// second decode of the same stream at [`Limits::max_threads`] 1 makes no allocator call
/// (`tests/reuse_alloc.rs`). A stream that is no larger in any buffer dimension (width, height,
/// colour format and channels, tiling, quantizers, index-table entries, alpha) than one decoded
/// before usually needs none; a larger one allocates what grows. With threads a few small
/// allocations remain, the same for any image size: the job that starts the pipeline's worker and
/// the pipeline's list of finished rows.
///
/// Every buffer is made as a fresh one at the start of a decode (cleared and refilled), so the
/// result of a decode does not depend on the decodes before it: not on their sizes or formats,
/// and not on whether they failed or were cancelled; it is the result of a decode on a fresh
/// [`Decoder::new`]. A buffer the call does not use stays as it is.
///
/// Limits: the charges a decode makes against [`Limits::max_alloc`] are those of a fresh decode,
/// so every limit gives the same outcome. What a kept buffer holds beyond what the call needs is
/// booked after those charges, once the call's own buffers are made; when it does not fit, the
/// buffer is cut to the size the call needs. Kept buffers the call does not use are not charged.
///
/// The memory stays with the `Scratch` until it drops: the buffers of the largest stream it has
/// seen. [`decode`] and [`decode_component`] do not use one and allocate their output, by design.
///
/// ```
/// use jpegxr::{Decoder, Format, Limits, Scratch};
/// use std::sync::atomic::AtomicBool;
///
/// // One worker: one `Scratch` and one `dst`, for every tile.
/// let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/synth/");
/// let limits = Limits { max_threads: 1, ..Limits::default() };
/// let cancel = AtomicBool::new(false);
/// let mut scratch = Scratch::default();
/// let mut dst = Vec::new();
/// for name in [
///     "Gray8_lossless_untiled_ov1.jxr",
///     "Bgr24_lossless_tiled_ov0.jxr",
///     "Gray8_lossless_untiled_ov1.jxr",
/// ] {
///     let data = std::fs::read(format!("{dir}{name}"))?;
///     let d = Decoder::reusing(&data, &limits, None, &mut scratch)?;
///     let (w, h, format) = d.config();
///     let stride = match format {
///         Format::Gray8 => w,
///         Format::Gray16 => 2 * w,
///         Format::Bgr24 => 3 * w,
///         Format::Bgr48 => 6 * w,
///     };
///     dst.resize(stride * h, 0);
///     d.decode_into(&mut dst, stride, &cancel)?;
///     assert_eq!((w, h), (64, 64));
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Default)]
pub struct Scratch {
    codec: codec::Scratch,
}

impl std::fmt::Debug for Scratch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scratch").finish_non_exhaustive()
    }
}

/// A JPEG XR stream whose container and image header are parsed, ready to decode into a caller's
/// buffer (Go's `Decoder` after `Reset`). Decoding several tiles of one slide is one `Decoder`
/// per tile; the decoder borrows `data`. To keep the working buffers from one tile to the next,
/// make the decoders with [`Decoder::reusing`].
pub struct Decoder<'a> {
    cf: codec::Format,
    format: Format,
    w: usize,
    h: usize,
    stream: &'a [u8],
    hdr: codec::Header,
    limits: Limits,
    pool: Option<&'a ThreadPool>,
    /// The caller's scratch; none for a decoder that makes its own buffers per call.
    scratch: Option<Mutex<&'a mut Scratch>>,
}

impl Drop for Decoder<'_> {
    /// Gives the header's tile tables back to the scratch.
    fn drop(&mut self) {
        if let Some(m) = self.scratch.as_mut() {
            let s = m.get_mut().unwrap_or_else(|e| e.into_inner());
            self.hdr.give_tiles(&mut s.codec.tiles);
        }
    }
}

impl<'a> Decoder<'a> {
    /// Parses `data`'s container and image header (Go's `Reset`). A decode on this decoder uses
    /// the crate's lazy shared pool when it uses threads at all.
    ///
    /// Enforces the same limits as [`decode_config`]. Allocates nothing else, uses no threads,
    /// and takes no cancel flag: parsing the header is bounded by the length of `data`.
    ///
    /// # Errors
    /// As [`decode_config`].
    pub fn new(data: &'a [u8], limits: &Limits) -> Result<Self, Error> {
        Self::open(data, limits, None, None)
    }

    /// As [`Decoder::new`], and its decodes run on `pool`'s threads instead of the shared pool,
    /// which this decoder then never starts. `Limits::max_threads` still caps the threads of a
    /// call, the calling thread included.
    ///
    /// # Errors
    /// As [`decode_config`].
    pub fn with_pool(data: &'a [u8], limits: &Limits, pool: &'a ThreadPool) -> Result<Self, Error> {
        Self::open(data, limits, Some(pool), None)
    }

    /// As [`Decoder::new`], parsing the header into `scratch`'s tile tables, and every decode on
    /// this decoder works in `scratch`'s buffers, which it keeps for the next one. `pool` is as
    /// in [`Decoder::with_pool`]: the caller's, or `None` for the shared pool. The decoder
    /// borrows `scratch` until it drops, which gives it the tile tables back; a worker makes one
    /// decoder per tile, one after the other, on one `Scratch`.
    ///
    /// With `max_threads` 1, the second decode of the same stream on `scratch`
    /// makes no allocator call (`tests/reuse_alloc.rs`). A stream that is no larger in any buffer
    /// dimension (width, height, colour format and channels, tiling, quantizers, index-table
    /// entries, alpha) than one decoded before usually needs none; a larger one allocates what
    /// grows. The result is the same as a fresh decoder's, whatever `scratch` held before (see
    /// [`Scratch`]). Concurrent calls of [`Decoder::decode_into`] on one decoder take turns.
    ///
    /// Enforces the same limits as [`decode_config`], and [`Decoder::decode_into`] enforces
    /// `max_alloc` as it does for a decoder from [`Decoder::new`].
    ///
    /// # Errors
    /// As [`decode_config`].
    pub fn reusing(
        data: &'a [u8],
        limits: &Limits,
        pool: Option<&'a ThreadPool>,
        scratch: &'a mut Scratch,
    ) -> Result<Self, Error> {
        Self::open(data, limits, pool, Some(scratch))
    }

    fn open(
        data: &'a [u8],
        limits: &Limits,
        pool: Option<&'a ThreadPool>,
        mut scratch: Option<&'a mut Scratch>,
    ) -> Result<Self, Error> {
        let (cf, stream) = parse(data)?;
        let mut own = codec::TileTables::default();
        let kept = match scratch.as_deref_mut() {
            Some(s) => &mut s.codec.tiles,
            None => &mut own,
        };
        let mut hdr = codec::Header::parse(stream, limits.max_alloc, kept)?;
        if let Err(e) = check_dims(hdr.width, hdr.height, limits) {
            hdr.give_tiles(kept);
            return Err(e);
        }
        Ok(Decoder {
            cf,
            format: format_of(&cf),
            w: hdr.width,
            h: hdr.height,
            stream,
            hdr,
            limits: *limits,
            pool,
            scratch: scratch.map(Mutex::new),
        })
    }

    /// Runs `f` on the caller's scratch (one call at a time), or on an empty one.
    fn with_scratch<R>(&self, f: impl FnOnce(&mut codec::Scratch) -> R) -> R {
        match &self.scratch {
            Some(m) => f(&mut m.lock().unwrap_or_else(|e| e.into_inner()).codec),
            None => f(&mut codec::Scratch::default()),
        }
    }

    /// The size and libCZI pixel type.
    pub fn config(&self) -> (usize, usize, Format) {
        (self.w, self.h, self.format)
    }

    /// The decoder's estimated peak heap for the stream, from its header, plus the input.
    pub fn estimate(&self) -> u64 {
        self.hdr.estimate(self.stream.len()) + self.stream.len() as u64
    }

    /// Decodes into `dst`, laid out as libCZI expects for [`Decoder::config`]'s format (B, G, R
    /// for `Bgr24`; 16-bit samples little endian, R, G, B for `Bgr48`).
    ///
    /// Enforces [`Limits::max_alloc`]: the estimate ([`Decoder::estimate`]) is checked first, and
    /// then the decoder's own allocations are charged as they are made, so the call's peak heap
    /// stays under it. The output is `dst`, which the caller owns and which is not charged. With
    /// [`Limits::max_threads`] above 1 and an image of enough macroblocks it runs as a two-stage
    /// pipeline on a pool thread and the calling thread, when the pipeline's buffers fit in
    /// `max_alloc`; `dst` and the result are the same for every thread count. `cancel` is
    /// checked once per macroblock row.
    ///
    /// # Errors
    /// [`Error::AllocLimit`] when the estimate or the charged allocations pass `max_alloc`.
    /// [`Error::DstTooSmall`], writing nothing, when `stride` is under one row or `dst` is under
    /// `stride * (h - 1)` plus one row. [`Error::OddStride`], writing nothing, when the format is
    /// 16-bit (`Gray16`, `Bgr48`) and `stride` is odd. [`Error::Cancelled`] when `cancel` is
    /// set, [`Error::Codestream`] or [`Error::Unsupported`] for a bad stream (`dst` then holds
    /// the rows decoded before the error), [`Error::OutOfMemory`] when the allocator refuses and
    /// [`Error::Internal`] for a decoder bug.
    pub fn decode_into(
        &self,
        dst: &mut [u8],
        stride: usize,
        cancel: &AtomicBool,
    ) -> Result<(), Error> {
        check_alloc(self.estimate(), &self.limits)?;
        let cx = Ctx::new(&self.limits, self.pool, cancel, false);
        self.with_scratch(|sc| self.decode(dst, stride, &cx, &mut |_, _| Ok(()), sc))
    }

    /// `decode_into` for the tests of `Limits`: also returns what the call charged and used.
    /// `uncapped` threads even a small image.
    ///
    /// # Errors
    /// As [`Decoder::decode_into`].
    #[doc(hidden)]
    pub fn decode_into_probe(
        &self,
        dst: &mut [u8],
        stride: usize,
        cancel: &AtomicBool,
        uncapped: bool,
    ) -> (Result<(), Error>, Probe) {
        if let Err(e) = check_alloc(self.estimate(), &self.limits) {
            return (Err(e), Probe::default());
        }
        let cx = Ctx::new(&self.limits, self.pool, cancel, uncapped);
        let r = self.with_scratch(|sc| self.decode(dst, stride, &cx, &mut |_, _| Ok(()), sc));
        (r, cx.probe())
    }

    /// `decode_into` without the memory charge.
    fn decode(
        &self,
        dst: &mut [u8],
        stride: usize,
        cx: &Ctx<'_>,
        on_rows: OnRows<'_>,
        sc: &mut codec::Scratch,
    ) -> Result<(), Error> {
        let row = stride_for(self.w, self.format);
        if stride < row || self.h > 0 && !fits(dst.len(), stride, self.h, row) {
            return Err(Error::DstTooSmall);
        }
        if matches!(self.format, Format::Gray16 | Format::Bgr48) && stride % 2 == 1 {
            return Err(Error::OddStride);
        }
        run(|| {
            codec::decode(
                self.stream,
                &self.hdr,
                &self.cf,
                dst,
                stride,
                cx,
                on_rows,
                sc,
            )
        })
    }

    /// Decodes channel `k` alone into `dst`, one 16-bit sample a pixel; the image must be
    /// `Gray16` (`k` 0) or 48bpp3Channels.
    fn decode_plane(
        &self,
        k: usize,
        dst: &mut [u8],
        stride: usize,
        cx: &Ctx<'_>,
        on_rows: OnRows<'_>,
        sc: &mut codec::Scratch,
    ) -> Result<(), Error> {
        if stride < 2 * self.w || self.h > 0 && !fits(dst.len(), stride, self.h, 2 * self.w) {
            return Err(Error::DstTooSmall);
        }
        if stride % 2 == 1 {
            return Err(Error::OddStride);
        }
        let mut cf = self.cf;
        if self.format != Format::Gray16 {
            cf.plane = k + 1;
        }
        run(|| codec::decode(self.stream, &self.hdr, &cf, dst, stride, cx, on_rows, sc))
    }

    /// `decode`: the image in a buffer the call allocates.
    fn image(&self, cx: &Ctx<'_>) -> Result<Image, Error> {
        let (w, ht, format) = self.config();
        let stride = stride_for(w, format);
        // raw plus the converted image; Gray16 decodes in place
        let per_px = match format {
            Format::Bgr24 => 4,
            Format::Bgr48 => 8,
            Format::Gray8 => 1,
            Format::Gray16 => 0,
        };
        let bpp = match format {
            Format::Gray8 => 1,
            Format::Gray16 => 2,
            Format::Bgr24 => 3,
            Format::Bgr48 => 6,
        };
        // Go's output size wraps at 2^32 x 2^32, leaving the estimate alone: charge that first so
        // the class is Go's `alloc`, not `toolarge`.
        check_alloc(self.estimate(), &self.limits)?;
        let mut sc = codec::Scratch::default();
        let raw_len = size(bpp, w, ht)?;
        let est = self
            .estimate()
            .saturating_add(raw_len as u64)
            .saturating_add(size(per_px, w, ht)? as u64);
        check_alloc(est, &self.limits)?;
        if format == Format::Gray16 {
            // decode in place, skip a copy
            let mut pix = zeroed(raw_len, cx)?;
            let mut done = 0;
            self.decode(
                &mut pix,
                stride,
                cx,
                &mut |p, y| {
                    swap16(&mut p[done * stride..y * stride]);
                    done = y;
                    cancelled(cx)
                },
                &mut sc,
            )?;
            return Ok(Image {
                width: w,
                height: ht,
                color_model: ColorModel::Gray16,
                pix,
            });
        }
        let mut raw = zeroed(raw_len, cx)?;
        let (color_model, out_bpp) = match format {
            Format::Gray8 => (ColorModel::Gray, 1),
            Format::Bgr24 => (ColorModel::Rgba, 4),
            _ => (ColorModel::Rgba64, 8),
        };
        if format == Format::Gray8 {
            self.decode(&mut raw, stride, cx, &mut |_, _| Ok(()), &mut sc)?;
            return Ok(Image {
                width: w,
                height: ht,
                color_model,
                pix: raw,
            });
        }
        let mut pix = zeroed(size(out_bpp, w, ht)?, cx)?;
        let mut done = 0;
        self.decode(
            &mut raw,
            stride,
            cx,
            &mut |r, y| {
                let (a, b) = (done * stride, y * stride);
                let (oa, ob) = (done * w * out_bpp as usize, y * w * out_bpp as usize);
                to_image(format, &r[a..b], &mut pix[oa..ob]);
                done = y;
                cancelled(cx)
            },
            &mut sc,
        )?;
        Ok(Image {
            width: w,
            height: ht,
            color_model,
            pix,
        })
    }

    /// `decode_component`.
    fn component(&self, k: usize, cx: &Ctx<'_>) -> Result<Image, Error> {
        let (w, ht, format) = self.config();
        let mut sc = codec::Scratch::default();
        if (format == Format::Gray16 && k == 0)
            || (format == Format::Bgr48 && self.cf.color_format == N_COMPONENT && k < 3)
        {
            check_alloc(self.estimate(), &self.limits)?; // as in `image`: Go's wrapped size leaves the estimate
            let len = size(2, w, ht)?;
            let est = self.estimate().saturating_add(len as u64);
            check_alloc(est, &self.limits)?;
            let mut pix = zeroed(len, cx)?;
            let mut done = 0;
            let r = self.decode_plane(
                k,
                &mut pix,
                2 * w,
                cx,
                &mut |p, y| {
                    swap16(&mut p[done * 2 * w..y * 2 * w]);
                    done = y;
                    cancelled(cx)
                },
                &mut sc,
            );
            match r {
                Ok(()) => {
                    return Ok(Image {
                        width: w,
                        height: ht,
                        color_model: ColorModel::Gray16,
                        pix,
                    });
                }
                Err(e) => {
                    if e != Error::Unsupported || format == Format::Gray16 {
                        return Err(e);
                    }
                    // e.g. fewer than 3 stored channels: take the full path, which gives the
                    // plane's budget back first.
                    drop(pix);
                    cx.ledger.release(len as u64);
                }
            }
        }
        if format != Format::Bgr48 || k > 2 {
            return Err(Error::PixelFormat);
        }
        check_alloc(self.estimate(), &self.limits)?;
        let est = self.estimate().saturating_add(size(8, w, ht)? as u64); // raw and the plane
        check_alloc(est, &self.limits)?;
        let mut raw = zeroed(size(6, w, ht)?, cx)?;
        let mut pix = zeroed(size(2, w, ht)?, cx)?;
        let mut done = 0;
        self.decode(
            &mut raw,
            6 * w,
            cx,
            &mut |r, y| {
                let n = y * w;
                let (outs, _) = pix.as_chunks_mut::<2>();
                for (i, o) in outs.iter_mut().enumerate().take(n).skip(done * w) {
                    o[0] = r[6 * i + 2 * k + 1];
                    o[1] = r[6 * i + 2 * k];
                }
                done = y;
                cancelled(cx)
            },
            &mut sc,
        )?;
        Ok(Image {
            width: w,
            height: ht,
            color_model: ColorModel::Gray16,
            pix,
        })
    }
}

/// Reports whether `len` holds `h - 1` strides and one `row`.
fn fits(len: usize, stride: usize, h: usize, row: usize) -> bool {
    stride
        .checked_mul(h - 1)
        .and_then(|n| n.checked_add(row))
        .is_some_and(|need| len >= need)
}

/// Runs a decode, turning a panic into [`Error::Internal`] (Go's `recover`).
fn run<T>(f: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = if let Some(s) = p.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = p.downcast_ref::<String>() {
                s.clone()
            } else {
                "panic".to_string()
            };
            Err(Error::Internal(msg))
        }
    }
}

fn cancelled(cx: &Ctx<'_>) -> Result<(), Error> {
    if cx.cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

/// `n` zero bytes, charged first, filled in chunks so a set cancel flag stops a large buffer.
fn zeroed(n: usize, cx: &Ctx<'_>) -> Result<Vec<u8>, Error> {
    // ponytail: 16 MiB is about 1-2 ms of memset between cancel polls.
    const CHUNK: usize = 16 << 20;
    cx.ledger.charge(n as u64)?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
    while v.len() < n {
        cancelled(cx)?;
        v.resize(v.len() + CHUNK.min(n - v.len()), 0);
    }
    Ok(v)
}

/// Turns little-endian samples big-endian in place.
fn swap16(p: &mut [u8]) {
    let (pairs, _) = p.as_chunks_mut::<2>();
    for c in pairs {
        c.swap(0, 1);
    }
}

/// Lays out `raw`, whole rows of jxrlib's output, in `pix`, the matching rows of an [`Image`]:
/// `Bgr24` as RGBA and `Bgr48` as big-endian RGBA64.
fn to_image(format: Format, raw: &[u8], pix: &mut [u8]) {
    match format {
        Format::Bgr24 => {
            let ((outs, _), (ins, _)) = (pix.as_chunks_mut::<4>(), raw.as_chunks::<3>());
            for (o, i) in outs.iter_mut().zip(ins) {
                o[0] = i[2];
                o[1] = i[1];
                o[2] = i[0];
                o[3] = 0xff;
            }
        }
        _ => {
            let ((outs, _), (ins, _)) = (pix.as_chunks_mut::<8>(), raw.as_chunks::<6>());
            for (o, i) in outs.iter_mut().zip(ins) {
                for c in 0..3 {
                    o[2 * c] = i[2 * c + 1];
                    o[2 * c + 1] = i[2 * c];
                }
                o[6] = 0xff;
                o[7] = 0xff;
            }
        }
    }
}

/// `decode` and `decode_component` on one call's pool, with a probe.
fn probed(
    data: &[u8],
    k: Option<usize>,
    limits: &Limits,
    cancel: &AtomicBool,
    pool: Option<&ThreadPool>,
    uncapped: bool,
) -> (Result<Image, Error>, Probe) {
    let d = match Decoder::open(data, limits, pool, None) {
        Ok(d) => d,
        Err(e) => return (Err(e), Probe::default()),
    };
    let cx = Ctx::new(limits, pool, cancel, uncapped);
    let r = match k {
        None => d.image(&cx),
        Some(k) => d.component(k, &cx),
    };
    (r, cx.probe())
}

/// Decodes a JPEG XR image in an HD Photo container. The image is [`ColorModel::Rgba`] for
/// 24bppBGR, `Rgba64` for 48bppRGB and 48bpp3Channels (channel k in R, G, B), `Gray` for 8bppGray
/// and `Gray16` for 16bppGray.
///
/// Enforces every [`Limits`] field. The estimate is checked against `max_alloc` first, with the
/// output image (and, for `Bgr24`, `Bgr48` and `Gray8`, the raw decode beside it) added; then the
/// decoder's allocations, the output buffers included, are charged as they are made. With
/// `max_threads` above 1 and an image of enough macroblocks it runs the two-stage pipeline on the
/// crate's lazy shared pool, when the pipeline's buffers fit in `max_alloc`; the image is the same
/// for every thread count. `cancel` is checked once per macroblock row, and between 16 MiB chunks
/// of the output buffers' zero fill.
///
/// # Errors
/// As [`decode_config`] for the header and limits; [`Error::TooLarge`] when the output size
/// overflows; [`Error::AllocLimit`] over `max_alloc`; [`Error::Cancelled`]; [`Error::Codestream`]
/// or [`Error::Unsupported`] for a bad stream; [`Error::OutOfMemory`] when the allocator
/// refuses; [`Error::Internal`] for a decoder bug.
pub fn decode(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<Image, Error> {
    probed(data, None, limits, cancel, None, false).0
}

/// Decodes channel `k` of a 16-bit JPEG XR image as a [`ColorModel::Gray16`] image equal to that
/// channel of [`decode`]'s image: `k` 0 of 16bppGray, or `k` 0 to 2 (R, G, B) of 48bppRGB and
/// 48bpp3Channels. 48bpp3Channels writes only channel `k`, skipping the other channels' output.
///
/// Enforces the same limits and uses threads as [`decode`] does.
///
/// # Errors
/// As [`decode`]; [`Error::PixelFormat`] when the image has no channel `k`.
pub fn decode_component(
    data: &[u8],
    k: usize,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Image, Error> {
    probed(data, Some(k), limits, cancel, None, false).0
}

/// `decode` (`k` none) or `decode_component` for the tests of `Limits`: also returns what the
/// call charged and used. `uncapped` threads even a small image.
///
/// # Errors
/// As [`decode`] (or [`decode_component`]).
#[doc(hidden)]
pub fn decode_probe(
    data: &[u8],
    k: Option<usize>,
    limits: &Limits,
    cancel: &AtomicBool,
    uncapped: bool,
) -> (Result<Image, Error>, Probe) {
    probed(data, k, limits, cancel, None, uncapped)
}
