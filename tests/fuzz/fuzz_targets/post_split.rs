#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = [p0][gap][off][bits][values...]: the stage 1 post filter on the runs at p0 % 40 + 16 and
// p1 = p0 + 256 + k, p0 + 240 + k, p0 + 16 or p0 - 16 (gap % 4 picks, k = gap / 4 % 4; the
// decoder's geometries: the stage 2 callers, the chroma calls (p0 - 48, p1 - 64, 32) and
// (p0 - 16, p1 - 32, 32), str_post4x4_stage1_alternate, and p1 below p0), with the offset 0 or
// 32, of about bits % 31 + 2 bits. The SIMD tier equals the scalar code bit for bit.
fuzz_target!(|data: &[u8]| {
    check_tier();
    let Some((&[p0, gap, off, bits], rest)) = data.split_first_chunk::<4>() else {
        return;
    };
    let p0 = isize::from(p0) % 40 + 16; // so p0 - 16 is not negative
    let k = isize::from(gap / 4) % 4;
    let p1 = match gap % 4 {
        0 => p0 + 256 + k,
        1 => p0 + 240 + k,
        2 => p0 + 16,
        _ => p0 - 16,
    };
    let off = if off & 1 == 0 { 0 } else { 32 };
    let bits = u32::from(bits) % 31 + 2;
    let a = values(rest, p0.max(p1) as usize + 80, bits);
    let (mut x, mut y) = (a.clone(), a);
    let l = limits();
    jpegxr::kernels::post_split(false, &mut x, p0, p1, off, &l).expect("scalar");
    jpegxr::kernels::post_split(true, &mut y, p0, p1, off, &l).expect("simd");
    assert!(x == y, "p0={p0} p1={p1} off={off} bits={bits}");
});
