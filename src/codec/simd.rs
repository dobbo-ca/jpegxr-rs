//! SIMD tiers of the four data-parallel kernels the Go library wrote in assembly: the 4x4 IDCT
//! stage 1, the stage 1 post filter, and the BGR8 and Gray16 output loops. Each kernel is written
//! once on fixed-width lanes (`V`) and compiled twice, under `#[target_feature(enable = "avx2")]`
//! on x86_64 and `neon` on aarch64, so LLVM emits that tier's vector instructions. The scalar
//! code in `transform.rs` and `output.rs` is the reference; every lane op wraps like it does, so
//! the tiers are bit-exact. The two entropy kernels (`flexbits`, `highpass`) are bit-serial and
//! stay scalar: see the README.
//!
//! `JPEGXR_SIMD=scalar` forces the scalar tier.

use super::output::{GRP_OFF, out_bgr8_scalar, out_gray16_scalar};
use super::segdec::{decode_block_adaptive, decode_block_highpass};
use super::transform::{
    P, str_idct4x4_stage1_pairs_scalar, str_post4x4_stage1_split_alternate_scalar,
};
use super::{AdaptiveHuffman, AdaptiveScan, BitIO, DCT_INDEX, IoSet, NUM_VLC_TABLES};
use std::ops::{Add, Neg, Sub};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tier {
    Scalar,
    Avx2,
    Neon,
}

/// The best tier this CPU has, ignoring `JPEGXR_SIMD`.
pub(crate) fn detect() -> Tier {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        return Tier::Avx2;
    }
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("neon") {
        return Tier::Neon;
    }
    Tier::Scalar
}

/// The tier the decoder runs, resolved once.
pub(super) fn tier() -> Tier {
    static T: OnceLock<Tier> = OnceLock::new();
    *T.get_or_init(|| {
        if std::env::var_os("JPEGXR_SIMD").is_some_and(|v| v == "scalar") {
            Tier::Scalar
        } else {
            detect()
        }
    })
}

/// `L` independent i32 lanes. Every op wraps, as the scalar code does with overflow checks off.
#[derive(Clone, Copy)]
struct V<const L: usize>([i32; L]);

impl<const L: usize> V<L> {
    #[inline(always)]
    fn splat(x: i32) -> Self {
        V([x; L])
    }
    #[inline(always)]
    fn sar(self, n: u32) -> Self {
        V(self.0.map(|x| x >> n))
    }
    #[inline(always)]
    fn shl(self, n: u32) -> Self {
        V(self.0.map(|x| x.wrapping_shl(n)))
    }
    #[inline(always)]
    fn mul(self, k: i32) -> Self {
        V(self.0.map(|x| x.wrapping_mul(k)))
    }
    #[inline(always)]
    fn add_k(self, k: i32) -> Self {
        V(self.0.map(|x| x.wrapping_add(k)))
    }
    #[inline(always)]
    fn clamp(self, lo: i32, hi: i32) -> Self {
        V(self.0.map(|x| x.clamp(lo, hi)))
    }
    /// `(3x + k) >> s`.
    #[inline(always)]
    fn q3(self, k: i32, s: u32) -> Self {
        self.mul(3).add_k(k).sar(s)
    }
}

impl<const L: usize> Add for V<L> {
    type Output = Self;
    #[inline(always)]
    fn add(self, o: Self) -> Self {
        V(std::array::from_fn(|i| self.0[i].wrapping_add(o.0[i])))
    }
}

impl<const L: usize> Sub for V<L> {
    type Output = Self;
    #[inline(always)]
    fn sub(self, o: Self) -> Self {
        V(std::array::from_fn(|i| self.0[i].wrapping_sub(o.0[i])))
    }
}

impl<const L: usize> Neg for V<L> {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        V(self.0.map(|x| x.wrapping_neg()))
    }
}

type Q<const L: usize> = (V<L>, V<L>, V<L>, V<L>);

/// `strDCT2x2up` (`up`) or `strDCT2x2dn`.
#[inline(always)]
fn dct2x2<const L: usize>(a: V<L>, b: V<L>, c: V<L>, d: V<L>, up: bool) -> Q<L> {
    let (a, b) = (a + d, b - c);
    let t = if up {
        (a - b).add_k(1).sar(1)
    } else {
        (a - b).sar(1)
    };
    let (c2, d2) = (t - d, t - c);
    (a - d2, b + c2, c2, d2)
}

#[inline(always)]
fn inv_odd<const L: usize>(a: V<L>, b: V<L>, c: V<L>, d: V<L>) -> Q<L> {
    let (b, a) = (b + d, a - c);
    let d = d - b.sar(1);
    let c = c + a.add_k(1).sar(1);
    let a = a - b.q3(4, 3);
    let b = b + a.q3(4, 3);
    let c = c - d.q3(4, 3);
    let d = d + c.q3(4, 3);
    let c = c - b.add_k(1).sar(1);
    let d = a.add_k(1).sar(1) - d;
    let b = b + c;
    (a - d, b, c, d)
}

/// `invOddOdd` with rounding constants `(k1, k2)`: `(3, 3)` for the IDCT, `(6, 2)` for the post
/// filter. The IDCT also negates b and c on the way out.
#[inline(always)]
fn inv_odd_odd<const L: usize>(a: V<L>, b: V<L>, c: V<L>, d: V<L>, k: (i32, i32)) -> Q<L> {
    let (d, c) = (d + a, c - b);
    let t1 = d.sar(1);
    let a = a - t1;
    let t2 = c.sar(1);
    let b = b + t2;
    let a = a - b.q3(k.0, 3);
    let b = b + a.q3(k.1, 2);
    let a = a - b.q3(4, 3);
    let b = b - t2;
    (a + t1, b, c + b, d - (a + t1))
}

/// Stage 1 of the 4x4 IDCT on 16 coefficients per lane (`x[e]` is coefficient `e`).
#[inline(always)]
fn idct4x4_stage1<const L: usize>(x: &mut [V<L>; 16]) {
    (x[0], x[1], x[2], x[3]) = dct2x2(x[0], x[1], x[2], x[3], true);
    (x[5], x[4], x[7], x[6]) = inv_odd(x[5], x[4], x[7], x[6]);
    (x[10], x[8], x[11], x[9]) = inv_odd(x[10], x[8], x[11], x[9]);
    (x[15], x[14], x[13], x[12]) = inv_odd_odd(x[15], x[14], x[13], x[12], (3, 3));
    x[14] = -x[14];
    x[13] = -x[13];
    for k in 0..4 {
        (x[k], x[4 + k], x[8 + k], x[12 + k]) = dct2x2(x[k], x[4 + k], x[8 + k], x[12 + k], false);
    }
}

/// Eight blocks (four pairs) per pass, as the Go AVX2 kernel.
const PAIRS: isize = 4;

/// Lane `i` is block `64*(i/2) + 16*(i%2)` of the run at `o`; `pairs` is a multiple of 4.
#[inline(always)]
fn idct_lanes(p: &mut [i32], o: usize, pairs: usize) {
    for g in (0..pairs).step_by(PAIRS as usize) {
        let s = &mut p[o + 64 * g..o + 64 * g + 64 * 3 + 32];
        let mut x = [V::<8>::splat(0); 16];
        for i in 0..8 {
            let b = 64 * (i / 2) + 16 * (i % 2);
            for e in 0..16 {
                x[e].0[i] = s[b + e];
            }
        }
        idct4x4_stage1(&mut x);
        for i in 0..8 {
            let b = 64 * (i / 2) + 16 * (i % 2);
            for e in 0..16 {
                s[b + e] = x[e].0[i];
            }
        }
    }
}

#[inline(always)]
fn load4(p: &[i32], i: usize) -> V<4> {
    let s = &p[i..i + 4];
    V([s[0], s[1], s[2], s[3]])
}

fn store4(p: &mut [i32], i: usize, v: V<4>) {
    p[i..i + 4].copy_from_slice(&v.0);
}

/// `strPost4x4Stage1SplitAlternate` on the four runs of four at `a`, `b`, `c`, `d` (which never
/// overlap): lane k is `A[k], B[k], C[k], D[k]`.
#[inline(always)]
fn post_lanes(p: &mut [i32], (ia, ib, ic, id): (usize, usize, usize, usize)) {
    let (a, b, c, d) = (load4(p, ia), load4(p, ib), load4(p, ic), load4(p, id));
    let (a, mut b, mut c, d) = dct2x2(a, b, c, d, false);
    // invOddOddPost on D's four coefficients.
    let q = inv_odd_odd(V([d.0[0]]), V([d.0[1]]), V([d.0[2]]), V([d.0[3]]), (6, 2));
    let d = V([q.0.0[0], q.1.0[0], q.2.0[0], q.3.0[0]]);
    // irotate1 on (C2,C3), (C0,C1), (B1,B3), (B0,B2).
    let mut x = V([c.0[2], c.0[0], b.0[1], b.0[0]]);
    let mut y = V([c.0[3], c.0[1], b.0[3], b.0[2]]);
    x = x - y.add_k(1).sar(1);
    y = y + x.add_k(1).sar(1);
    (c.0[2], c.0[0], b.0[1], b.0[0]) = (x.0[0], x.0[1], x.0[2], x.0[3]);
    (c.0[3], c.0[1], b.0[3], b.0[2]) = (y.0[0], y.0[1], y.0[2], y.0[3]);
    // strHSTdec1Alternate on (A, D).
    let a = a + d;
    let d = a.sar(1) - d;
    let a = a + d.mul(3).sar(3);
    let d = d + a.mul(3).sar(4);
    let d = d + a.sar(7);
    let d = d - a.sar(10);
    // strHSTdec.
    b = b - c;
    let a = a + d.q3(4, 3);
    let d = d - b.sar(1);
    c = (a - b).sar(1) - c;
    store4(p, ia, a - c);
    store4(p, ib, b + d);
    store4(p, ic, d);
    store4(p, id, c);
}

/// The macroblock row's Y values of 16 pixels, in pixel order.
#[inline(always)]
fn gather(y: &[i32], base: usize) -> V<16> {
    V(std::array::from_fn(|c| y[base + GRP_OFF[c]]))
}

#[inline(always)]
fn out_bgr8_lanes(
    out: &mut [u8],
    d: usize,
    (y, u, v): (&[i32], &[i32], &[i32]),
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    (i_r, i_b): (usize, usize),
) {
    for m in 0..n {
        let base = s + 256 * m;
        let g = gather(y, base).add_k(bias);
        let (u, v) = (gather(u, base), gather(v, base));
        // _ICC
        let r = -u;
        let g = g - r.sar(1);
        let r = r - (v.add_k(1).sar(1) - g);
        let b = v + r;
        let (r, g, b) = (
            r.sar(shift).clamp(0, 255),
            g.sar(shift).clamp(0, 255),
            b.sar(shift).clamp(0, 255),
        );
        let dst = &mut out[d + 48 * m..d + 48 * m + 48];
        for c in 0..16 {
            dst[3 * c + i_r] = r.0[c] as u8;
            dst[3 * c + 1] = g.0[c] as u8;
            dst[3 * c + i_b] = b.0[c] as u8;
        }
    }
}

#[inline(always)]
fn out_gray16_lanes(
    out: &mut [u8],
    d: usize,
    y: &[i32],
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    n_len: u32,
) {
    for m in 0..n {
        let p = gather(y, s + 256 * m)
            .add_k(bias)
            .sar(shift)
            .shl(n_len)
            .clamp(0, 65535);
        let dst = &mut out[d + 32 * m..d + 32 * m + 32];
        for c in 0..16 {
            dst[2 * c..2 * c + 2].copy_from_slice(&(p.0[c] as u16).to_le_bytes());
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod avx2 {
    // Safe `#[target_feature]` fns (Rust 1.87): the bodies are safe code, and calling one needs
    // `unsafe` only because the CPU must have the feature; the per-tier dispatch below does that,
    // after `detect`. Only `bgr8` uses pointer intrinsics, through the array-typed helpers of
    // `x86.rs`; the other kernels are `[i32; L]` lane arrays that LLVM vectorizes.
    #[target_feature(enable = "avx2")]
    pub(super) fn idct(p: &mut [i32], o: usize, pairs: usize) {
        super::idct_lanes(p, o, pairs)
    }
    #[target_feature(enable = "avx2")]
    pub(super) fn post(p: &mut [i32], i: (usize, usize, usize, usize)) {
        super::post_lanes(p, i)
    }
    #[target_feature(enable = "avx2")]
    pub(super) fn bgr8(
        out: &mut [u8],
        d: usize,
        yuv: (&[i32], &[i32], &[i32]),
        s: usize,
        n: usize,
        bias: i32,
        shift: u32,
        ir_ib: (usize, usize),
    ) {
        super::super::bgr8::out_bgr8(out, d, yuv, s, n, bias, shift, ir_ib)
    }
    #[target_feature(enable = "avx2")]
    pub(super) fn gray16(
        out: &mut [u8],
        d: usize,
        y: &[i32],
        s: usize,
        n: usize,
        bias: i32,
        shift: u32,
        n_len: u32,
    ) {
        super::out_gray16_lanes(out, d, y, s, n, bias, shift, n_len)
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    // Safe `#[target_feature]` fns (Rust 1.87): the bodies are safe code, and calling one needs
    // `unsafe` only because the CPU must have the feature; the per-tier dispatch below does that,
    // after `detect`. No kernel uses a pointer intrinsic (the lanes are `[i32; L]` arrays that
    // LLVM vectorizes), so there are no load/store helpers to bound-check.
    #[target_feature(enable = "neon")]
    pub(super) fn idct(p: &mut [i32], o: usize, pairs: usize) {
        super::idct_lanes(p, o, pairs)
    }
    #[target_feature(enable = "neon")]
    pub(super) fn post(p: &mut [i32], i: (usize, usize, usize, usize)) {
        super::post_lanes(p, i)
    }
    #[target_feature(enable = "neon")]
    pub(super) fn bgr8(
        out: &mut [u8],
        d: usize,
        yuv: (&[i32], &[i32], &[i32]),
        s: usize,
        n: usize,
        bias: i32,
        shift: u32,
        ir_ib: (usize, usize),
    ) {
        super::out_bgr8_lanes(out, d, yuv, s, n, bias, shift, ir_ib)
    }
    #[target_feature(enable = "neon")]
    pub(super) fn gray16(
        out: &mut [u8],
        d: usize,
        y: &[i32],
        s: usize,
        n: usize,
        bias: i32,
        shift: u32,
        n_len: u32,
    ) {
        super::out_gray16_lanes(out, d, y, s, n, bias, shift, n_len)
    }
}

/// Pair k is blocks `o+64k` and `o+64k+16`; the SIMD tiers take whole groups of four pairs and
/// the scalar code the rest.
pub(crate) fn idct_pairs(t: Tier, p: &mut [i32], mut o: isize, mut n: isize) {
    if t != Tier::Scalar && n >= PAIRS {
        let m = n & !(PAIRS - 1);
        match t {
            #[cfg(target_arch = "x86_64")]
            Tier::Avx2 => {
                // SAFETY: `t` is `Tier::Avx2`, which only `detect` returns, after
                // `is_x86_feature_detected!("avx2")`: the target feature is present.
                unsafe { avx2::idct(p, o as usize, m as usize) }
            }
            #[cfg(target_arch = "aarch64")]
            Tier::Neon => {
                // SAFETY: `t` is `Tier::Neon`, which only `detect` returns, after
                // `is_aarch64_feature_detected!("neon")`: the target feature is present.
                unsafe { neon::idct(p, o as usize, m as usize) }
            }
            _ => return str_idct4x4_stage1_pairs_scalar(&mut P(p), o, n),
        }
        o += 64 * m;
        n -= m;
    }
    str_idct4x4_stage1_pairs_scalar(&mut P(p), o, n);
}

pub(crate) fn post_split(t: Tier, p: &mut [i32], p0: isize, p1: isize, i_offset: isize) {
    let i = (
        (p0 + 12) as usize,
        (p0 + 72 - i_offset) as usize,
        (p1 + 4) as usize,
        (p1 + 64 - i_offset) as usize,
    );
    match t {
        #[cfg(target_arch = "x86_64")]
        Tier::Avx2 => {
            // SAFETY: `t` is `Tier::Avx2`, which only `detect` returns, after
            // `is_x86_feature_detected!("avx2")`: the target feature is present.
            unsafe { avx2::post(p, i) }
        }
        #[cfg(target_arch = "aarch64")]
        Tier::Neon => {
            // SAFETY: `t` is `Tier::Neon`, which only `detect` returns, after
            // `is_aarch64_feature_detected!("neon")`: the target feature is present.
            unsafe { neon::post(p, i) }
        }
        _ => str_post4x4_stage1_split_alternate_scalar(&mut P(p), p0, p1, i_offset),
    }
}

/// Shift counts above 31 are Go's "shift everything out"; the scalar code has that.
pub(crate) fn out_bgr8(
    t: Tier,
    out: &mut [u8],
    d: usize,
    yuv: (&[i32], &[i32], &[i32]),
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    ir_ib: (usize, usize),
) {
    match t {
        #[cfg(target_arch = "x86_64")]
        Tier::Avx2 if shift < 32 => {
            // SAFETY: `t` is `Tier::Avx2`, which only `detect` returns, after
            // `is_x86_feature_detected!("avx2")`: the target feature is present.
            unsafe { avx2::bgr8(out, d, yuv, s, n, bias, shift, ir_ib) }
        }
        #[cfg(target_arch = "aarch64")]
        Tier::Neon if shift < 32 => {
            // SAFETY: `t` is `Tier::Neon`, which only `detect` returns, after
            // `is_aarch64_feature_detected!("neon")`: the target feature is present.
            unsafe { neon::bgr8(out, d, yuv, s, n, bias, shift, ir_ib) }
        }
        _ => out_bgr8_scalar(out, d, yuv, s, n, bias, shift, ir_ib),
    }
}

pub(crate) fn out_gray16(
    t: Tier,
    out: &mut [u8],
    d: usize,
    y: &[i32],
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    n_len: u32,
) {
    match t {
        #[cfg(target_arch = "x86_64")]
        Tier::Avx2 if shift < 32 && n_len < 32 => {
            // SAFETY: `t` is `Tier::Avx2`, which only `detect` returns, after
            // `is_x86_feature_detected!("avx2")`: the target feature is present.
            unsafe { avx2::gray16(out, d, y, s, n, bias, shift, n_len) }
        }
        #[cfg(target_arch = "aarch64")]
        Tier::Neon if shift < 32 && n_len < 32 => {
            // SAFETY: `t` is `Tier::Neon`, which only `detect` returns, after
            // `is_aarch64_feature_detected!("neon")`: the target feature is present.
            unsafe { neon::gray16(out, d, y, s, n, bias, shift, n_len) }
        }
        _ => out_gray16_scalar(out, d, y, s, n, bias, shift, n_len),
    }
}

/// Little-endian bytes of the parity generators' `canon` (tests/parity/go/upstream/internal/codec/
/// kernel_export.go).
fn canon_io(c: &mut Vec<u8>, io: &BitIO) {
    c.extend_from_slice(&io.acc.to_le_bytes());
    c.extend_from_slice(&u64::from(io.nb).to_le_bytes());
    c.extend_from_slice(&io.off.to_le_bytes());
}

fn canon_i32s(c: &mut Vec<u8>, s: &[i32]) {
    for x in s {
        c.extend_from_slice(&x.to_le_bytes());
    }
}

/// The scalar flexbits path, as `KernelFlexbits` runs the generic Go one: three blocks through
/// `decode_block_adaptive` on one reader (`buf`, `skip` bits read first). Returns `coef` and the
/// reader's state. This is the entry the doc-hidden `kernels::flexbits` calls (`flexbits` itself
/// is private to `segdec`). `coef.len()` must be at least 16.
pub(crate) fn flexbits_case(
    coef: &mut [i32],
    buf: &[u8],
    skip: i32,
    model_trim_qp: (i32, i32, i32),
) -> Vec<u8> {
    let mut io = BitIO::new();
    io.attach(buf, 0);
    io.get_bit16((skip % 16) as u32);
    io.get_bit32((skip / 16 % 32) as u32);
    let mut ios = IoSet {
        v: vec![io],
        stream: buf,
    };
    let mut ah = [AdaptiveHuffman::default(); NUM_VLC_TABLES];
    let mut scan = [AdaptiveScan::default(); 16];
    let Some(blk) = coef.first_chunk_mut::<16>() else {
        return Vec::new();
    };
    for _ in 0..3 {
        decode_block_adaptive(
            false,
            false,
            &mut ah,
            &mut ios,
            (0, 0),
            blk,
            &mut scan,
            model_trim_qp,
            &DCT_INDEX[0],
            false,
        );
    }
    let mut c = Vec::new();
    canon_i32s(&mut c, coef);
    canon_io(&mut c, &ios.v[0]);
    c
}

/// One table of [`highpass_case`]: `(n_symbols, table_index, disc, disc1)`.
pub(crate) type HpTable = (usize, i32, i32, i32);

/// `decode_block_highpass` on tables built as `KernelHighpass` builds them. Returns the count,
/// `coef`, every table, the scan and the reader's state. `coef.len()` must be at least 16, every
/// `n_symbols` at most 12 and every `table_index` below the table count of its symbol count
/// (the callers check).
pub(crate) fn highpass_case(
    chroma: bool,
    skip: u32,
    qp: i32,
    tables: &[HpTable; NUM_VLC_TABLES],
    scans: &[(u32, u32); 16],
    buf: &[u8],
    coef: &mut [i32],
) -> Vec<u8> {
    let mut ah = [AdaptiveHuffman::default(); NUM_VLC_TABLES];
    for (a, &(n_symbols, table_index, disc, disc1)) in ah.iter_mut().zip(tables) {
        a.n_symbols = n_symbols;
        a.initialize = true;
        a.table_index = table_index;
        a.lower_bound = i32::MIN;
        a.upper_bound = 1 << 30;
        a.adapt_discriminant();
        a.disc = disc;
        a.disc1 = disc1;
    }
    let mut scan = [AdaptiveScan::default(); 16];
    for (s, &(u_total, u_scan)) in scan.iter_mut().zip(scans) {
        *s = AdaptiveScan { u_total, u_scan };
    }
    let mut io = BitIO::new();
    io.attach(buf, 0);
    io.get_bit16(skip % 16);
    io.get_bit32(skip / 16);
    let Some(blk) = coef.first_chunk_mut::<16>() else {
        return Vec::new();
    };
    let n = decode_block_highpass(chroma, &mut ah, &mut io, qp, blk, &mut scan);
    let mut c = Vec::new();
    c.extend_from_slice(&(n as u64).to_le_bytes());
    canon_i32s(&mut c, coef);
    for a in &ah {
        for x in a.huf_dec_table {
            c.extend_from_slice(&x.to_le_bytes());
        }
        canon_i32s(&mut c, &a.delta);
        canon_i32s(&mut c, &a.delta1);
        canon_i32s(&mut c, &[a.disc, a.disc1]);
        c.extend_from_slice(&(a.n_symbols as u64).to_le_bytes());
        c.extend_from_slice(&i64::from(a.table_index).to_le_bytes());
        c.push(u8::from(a.initialize));
        canon_i32s(&mut c, &[a.upper_bound, a.lower_bound]);
    }
    for s in &scan {
        c.extend_from_slice(&s.u_total.to_le_bytes());
        c.extend_from_slice(&s.u_scan.to_le_bytes());
    }
    canon_io(&mut c, &io);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// A value of about `bits` bits (2 to 32), either sign.
        fn val(&mut self, bits: u32) -> i32 {
            (self.next() as i32) >> (32 - bits)
        }
        fn fill(&mut self, n: usize, bits: u32) -> Vec<i32> {
            (0..n).map(|_| self.val(bits)).collect()
        }
        /// Go's `fills` edge buffers (kernel_test.go): `k % 9` picks one of the seven edge values
        /// repeated, the MAX/MIN alternation, or a random pick among the edges per element.
        fn edge_fill(&mut self, n: usize, k: usize) -> Vec<i32> {
            const EDGES: [i32; 7] = [0, -1, 1, i32::MAX, i32::MIN, i32::MAX - 3, i32::MIN + 3];
            match k % 9 {
                j @ 0..=6 => vec![EDGES[j]; n],
                7 => (0..n).map(|i| EDGES[3 + i % 2]).collect(),
                _ => (0..n).map(|_| EDGES[(self.next() % 7) as usize]).collect(),
            }
        }
        /// The fill kinds of the tests: 8, 16, 31 and 32 bit random values, then the edge buffers.
        fn kind_fill(&mut self, n: usize, kind: usize) -> Vec<i32> {
            match kind {
                0 => self.fill(n, 8),
                1 => self.fill(n, 16),
                2 => self.fill(n, 31),
                3 => self.fill(n, 32),
                k => self.edge_fill(n, k - 4),
            }
        }
    }

    /// 4 random kinds, then the 9 edge kinds.
    const KINDS: usize = 13;

    /// The tier this CPU has beyond scalar; the CI runners have AVX2 (x86_64) and NEON (aarch64).
    fn simd_tier() -> Tier {
        let t = detect();
        #[cfg(target_arch = "x86_64")]
        assert_eq!(t, Tier::Avx2, "AVX2 not detected");
        #[cfg(target_arch = "aarch64")]
        assert_eq!(t, Tier::Neon, "NEON not detected");
        t
    }

    #[test]
    fn tier_is_detected() {
        let t = simd_tier();
        assert_ne!(t, Tier::Scalar);
        // `JPEGXR_SIMD=scalar` changes what the decoder runs, not what the CPU has.
        let want = if std::env::var_os("JPEGXR_SIMD").is_some_and(|v| v == "scalar") {
            Tier::Scalar
        } else {
            t
        };
        assert_eq!(tier(), want);
    }

    #[test]
    fn kernels_refuse_bad_ranges() {
        use crate::{Error, Limits, kernels};
        let l = Limits::default();
        let bad = |r: Result<(), Error>| matches!(r, Err(Error::LimitExceeded(_)));
        let mut p = vec![0i32; 100];
        // Pair 3 would end at 64 * 2 + 32 + o.
        assert!(bad(kernels::idct_pairs(false, &mut p, 0, 3, &l)));
        assert!(bad(kernels::idct_pairs(false, &mut p, -1, 1, &l)));
        assert!(bad(kernels::idct_pairs(true, &mut p, 70, 1, &l)));
        assert!(kernels::idct_pairs(true, &mut p, 4, 1, &l).is_ok());
        // Runs at 12, 72, 20 and 80 fit; p0 = 0, p1 = 8 puts two runs on 12; p1 = 100 runs off.
        assert!(kernels::post_split(true, &mut p, 0, 16, 0, &l).is_ok());
        assert!(bad(kernels::post_split(true, &mut p, 0, 8, 0, &l)));
        assert!(bad(kernels::post_split(true, &mut p, 0, 100, 0, &l)));
        assert!(bad(kernels::post_split(true, &mut p, isize::MAX, 0, 0, &l)));
        let (y, mut out) = (vec![0i32; 256 + 198], vec![0u8; 100]);
        // 256 + 198 values fit two groups from 0, not from 1.
        assert!(bad(kernels::out_gray16(
            true, &mut out, 0, &y, 1, 2, 0, 0, 0, &l
        )));
        assert!(bad(kernels::out_gray16(
            true, &mut out, 40, &y, 0, 2, 0, 0, 0, &l
        )));
        assert!(kernels::out_gray16(true, &mut out, 36, &y, 0, 2, 0, 0, 0, &l).is_ok());
        assert!(bad(kernels::out_bgr8(
            true,
            &mut out,
            0,
            (&y, &y, &y),
            0,
            2,
            0,
            0,
            (0, 3),
            &l
        )));
        assert!(bad(kernels::out_bgr8(
            true,
            &mut out,
            0,
            (&y, &y, &y),
            0,
            usize::MAX,
            0,
            0,
            (0, 2),
            &l
        )));
    }

    #[test]
    fn idct_pairs_match_scalar() {
        let t = simd_tier();
        let mut rng = Rng(0x9e3779b97f4a7c15);
        for n in [0isize, 1, 3, 4, 5, 7, 8, 9, 13, 17] {
            for o in [0isize, 1, 3, 8, 16] {
                for kind in 0..KINDS {
                    let len = o as usize + 64 * n as usize + 40;
                    let a = rng.kind_fill(len, kind);
                    let (mut x, mut y) = (a.clone(), a);
                    idct_pairs(Tier::Scalar, &mut x, o, n);
                    idct_pairs(t, &mut y, o, n);
                    assert_eq!(x, y, "n={n} o={o} kind={kind}");
                }
            }
        }
    }

    #[test]
    fn post_split_matches_scalar() {
        let t = simd_tier();
        let mut rng = Rng(0x2545f4914f6cdd1d);
        for it in 0..3000 {
            // p1 - p0, as the decoder calls it: 256+ (stage 2 callers), 240+ (the chroma calls
            // `(p0 - 48, p1 - 64, 32)` and `(p0 - 16, p1 - 32, 32)` of a 256+ pair), 16
            // (`str_post4x4_stage1_alternate`, p1 = p0 + 16) and -16 (p1 below p0).
            let p0 = 64 + (rng.next() % 40) as isize;
            let k = (rng.next() % 4) as isize;
            let p1 = p0 + [256 + k, 240 + k, 16, -16][(rng.next() % 4) as usize];
            let off = [0, 32][(rng.next() & 1) as usize];
            let kind = it % KINDS;
            let a = rng.kind_fill(p0.max(p1) as usize + 400, kind);
            let (mut x, mut y) = (a.clone(), a);
            post_split(Tier::Scalar, &mut x, p0, p1, off);
            post_split(t, &mut y, p0, p1, off);
            assert_eq!(x, y, "p0={p0} p1={p1} off={off} kind={kind}");
        }
    }

    #[test]
    fn out_bgr8_matches_scalar() {
        let t = simd_tier();
        let mut rng = Rng(0x1234567887654321);
        for n in [0usize, 1, 2, 3] {
            for s in [0usize, 1, 5, 64] {
                for shift in [0u32, 1, 3, 8, 16, 31, 32, 40] {
                    for ir_ib in [(0, 2), (2, 0)] {
                        for kind in 0..KINDS {
                            let len = s + 256 * n + 8;
                            let (y, u, v) = (
                                rng.kind_fill(len, kind),
                                rng.kind_fill(len, kind + 1),
                                rng.kind_fill(len, kind + 5),
                            );
                            let d = (rng.next() % 5) as usize;
                            let bias = rng.val(if kind < 3 { 8 + 8 * kind as u32 } else { 32 });
                            let o = vec![0xa5u8; d + 48 * n + 3];
                            let (mut x, mut z) = (o.clone(), o);
                            out_bgr8(
                                Tier::Scalar,
                                &mut x,
                                d,
                                (&y, &u, &v),
                                s,
                                n,
                                bias,
                                shift,
                                ir_ib,
                            );
                            out_bgr8(t, &mut z, d, (&y, &u, &v), s, n, bias, shift, ir_ib);
                            assert_eq!(x, z, "n={n} s={s} shift={shift} kind={kind}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn out_gray16_matches_scalar() {
        let t = simd_tier();
        let mut rng = Rng(0x0fedcba987654321);
        for n in [0usize, 1, 2, 3] {
            for s in [0usize, 1, 5, 64] {
                for (shift, n_len) in [
                    (0u32, 0u32),
                    (1, 0),
                    (3, 2),
                    (8, 4),
                    (16, 0),
                    (31, 31),
                    (32, 0),
                    (0, 32),
                    (3, 200),
                    (40, 4),
                    (0, 31),
                ] {
                    for kind in 0..KINDS {
                        let y = rng.kind_fill(s + 256 * n + 8, kind);
                        let d = (rng.next() % 5) as usize;
                        let bias = rng.val(if kind < 3 { 8 + 8 * kind as u32 } else { 32 });
                        let o = vec![0xa5u8; d + 32 * n + 3];
                        let (mut x, mut z) = (o.clone(), o);
                        out_gray16(Tier::Scalar, &mut x, d, &y, s, n, bias, shift, n_len);
                        out_gray16(t, &mut z, d, &y, s, n, bias, shift, n_len);
                        assert_eq!(x, z, "n={n} s={s} shift={shift} n_len={n_len} kind={kind}");
                    }
                }
            }
        }
    }
}
