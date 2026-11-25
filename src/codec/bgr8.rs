//! The AVX2 tier of the BGR8 output loop, written with intrinsics as Go's `outBGR8Asm` is: four
//! groups of four pixels per macroblock, ICC, saturating narrow to 16 bytes a plane, then three
//! PSHUFB interleaves into 48 bytes. The kernel is a safe `#[target_feature]` fn with no `unsafe`:
//! the three load/store helpers are in `x86.rs` and take fixed-size array references.

use super::x86::{ld2, ld16, st16};
use std::arch::x86_64::*;

/// Source elements a macroblock's 16 pixels read: the last is element 197 (`GRP_OFF[15] = 196`, loaded with its neighbour), so 198.
const SRC: usize = 198;

/// Shuffle masks: `MASK[q][p]` picks plane `p`'s bytes for output bytes `16q..16q+16`, where byte
/// `3c + p` is plane `p` of pixel `c`. 0x80 zeroes the byte.
const MASK: [[[u8; 16]; 3]; 3] = {
    let mut m = [[[0x80u8; 16]; 3]; 3];
    let mut b = 0;
    while b < 48 {
        m[b / 16][b % 3][b % 16] = (b / 3) as u8;
        b += 1;
    }
    m
};

/// The first `N` elements of `s` as an array; panics, as indexing does, if `s` is shorter.
#[inline(always)]
fn arr<T, const N: usize>(s: &[T]) -> &[T; N] {
    match s[..N].first_chunk() {
        Some(a) => a,
        None => unreachable!(),
    }
}

#[inline(always)]
fn arr_mut<T, const N: usize>(s: &mut [T]) -> &mut [T; N] {
    match s[..N].first_chunk_mut() {
        Some(a) => a,
        None => unreachable!(),
    }
}

/// Pixels 4g..4g+3 of an MB row, at offsets `o`, `o+1`, `o+5`, `o+4`.
#[inline]
#[target_feature(enable = "avx2")]
fn grp(a: &[i32; SRC], o: usize) -> __m128i {
    _mm_shuffle_epi32::<0xb4>(_mm_unpacklo_epi64(ld2(arr(&a[o..])), ld2(arr(&a[o + 4..]))))
}

#[inline]
#[target_feature(enable = "avx2")]
fn load_mb(a: &[i32; SRC]) -> [__m128i; 4] {
    [grp(a, 0), grp(a, 64), grp(a, 128), grp(a, 192)]
}

#[inline]
#[target_feature(enable = "avx2")]
fn narrow(v: [__m128i; 4]) -> __m128i {
    _mm_packus_epi16(_mm_packs_epi32(v[0], v[1]), _mm_packs_epi32(v[2], v[3]))
}

/// Bit-exact with `out_bgr8_scalar` for `shift < 32`.
#[target_feature(enable = "avx2")]
pub(super) fn out_bgr8(
    out: &mut [u8],
    d: usize,
    (y, u, v): (&[i32], &[i32], &[i32]),
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    (i_r, _): (usize, usize),
) {
    let (bias, one, sh) = (
        _mm_set1_epi32(bias),
        _mm_set1_epi32(1),
        _mm_cvtsi32_si128(shift as i32),
    );
    let mask = MASK.map(|q| q.map(|m| ld16(&m)));
    for m in 0..n {
        let b = s + 256 * m;
        let (y, u, v) = (
            load_mb(arr(&y[b..])),
            load_mb(arr(&u[b..])),
            load_mb(arr(&v[b..])),
        );
        let (mut r, mut g, mut bl) = (
            [_mm_setzero_si128(); 4],
            [_mm_setzero_si128(); 4],
            [_mm_setzero_si128(); 4],
        );
        for i in 0..4 {
            // _ICC
            let gi = _mm_add_epi32(y[i], bias);
            let ri = _mm_sub_epi32(_mm_setzero_si128(), u[i]);
            let gi = _mm_sub_epi32(gi, _mm_srai_epi32::<1>(ri));
            let t = _mm_sub_epi32(_mm_srai_epi32::<1>(_mm_add_epi32(v[i], one)), gi);
            let ri = _mm_sub_epi32(ri, t);
            let bi = _mm_add_epi32(v[i], ri);
            (r[i], g[i], bl[i]) = (
                _mm_sra_epi32(ri, sh),
                _mm_sra_epi32(gi, sh),
                _mm_sra_epi32(bi, sh),
            );
        }
        let (r, g, bl) = (narrow(r), narrow(g), narrow(bl));
        let pl = if i_r == 0 { [r, g, bl] } else { [bl, g, r] };
        let dst: &mut [u8; 48] = arr_mut(&mut out[d + 48 * m..]);
        for (q, chunk) in dst.as_chunks_mut::<16>().0.iter_mut().enumerate() {
            let x = _mm_or_si128(
                _mm_shuffle_epi8(pl[0], mask[q][0]),
                _mm_or_si128(
                    _mm_shuffle_epi8(pl[1], mask[q][1]),
                    _mm_shuffle_epi8(pl[2], mask[q][2]),
                ),
            );
            st16(chunk, x);
        }
    }
}
