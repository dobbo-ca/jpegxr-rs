#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = [n][s][d][shift][flags][bits][bias: i32 LE][values...]: the BGR8 output loop for n % 4
// groups of 16 pixels from offset s % 70 of y, u, v, into a buffer from offset d % 5, with a
// shift of 0 to 47 (31 and above take the scalar path in the SIMD tier too), R and B swapped by
// flags bit 0. The SIMD tier equals the scalar code bit for bit, and writes the same bytes.
fuzz_target!(|data: &[u8]| {
    check_tier();
    let Some((&[n, s, d, shift, flags, bits, b0, b1, b2, b3], rest)) =
        data.split_first_chunk::<10>()
    else {
        return;
    };
    let (n, s, d) = (usize::from(n) % 4, usize::from(s) % 70, usize::from(d) % 5);
    let (shift, bits) = (u32::from(shift) % 48, u32::from(bits) % 31 + 2);
    let ir_ib = if flags & 1 == 0 { (0, 2) } else { (2, 0) };
    let bias = i32::from_le_bytes([b0, b1, b2, b3]) >> (32 - bits);
    let len = s + 256 * n + 8;
    let (y, u, v) = (
        values(rest, len, bits),
        values(&rest[rest.len() / 3..], len, bits),
        values(&rest[rest.len() / 2..], len, bits),
    );
    let o = vec![0xa5u8; d + 48 * n + 3];
    let (mut x, mut z) = (o.clone(), o);
    let l = limits();
    jpegxr::kernels::out_bgr8(false, &mut x, d, (&y, &u, &v), s, n, bias, shift, ir_ib, &l)
        .expect("scalar");
    jpegxr::kernels::out_bgr8(true, &mut z, d, (&y, &u, &v), s, n, bias, shift, ir_ib, &l)
        .expect("simd");
    assert!(x == z, "n={n} s={s} d={d} shift={shift} bits={bits}");
});
