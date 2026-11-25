//! The x86_64 tier's memory helpers: the only `unsafe` the BGR8 kernel needs, one pointer
//! intrinsic each. They take array references, so the array type is the bounds proof and nothing
//! is checked at run time; the kernel gets the arrays in safe code (`first_chunk`,
//! `as_chunks_mut`).

use std::arch::x86_64::*;

/// Two i32 in the low half.
#[inline(always)]
pub(super) fn ld2(a: &[i32; 2]) -> __m128i {
    // SAFETY: `a` is 8 readable bytes and `_mm_loadl_epi64` reads 8, unaligned.
    unsafe { _mm_loadl_epi64(a.as_ptr().cast()) }
}

/// 16 bytes.
#[inline(always)]
pub(super) fn ld16(m: &[u8; 16]) -> __m128i {
    // SAFETY: `m` is 16 readable bytes and `_mm_loadu_si128` reads 16, unaligned.
    unsafe { _mm_loadu_si128(m.as_ptr().cast()) }
}

/// Stores 16 bytes.
#[inline(always)]
pub(super) fn st16(dst: &mut [u8; 16], v: __m128i) {
    // SAFETY: `dst` is 16 writable bytes and `_mm_storeu_si128` writes 16, unaligned.
    unsafe { _mm_storeu_si128(dst.as_mut_ptr().cast(), v) }
}
