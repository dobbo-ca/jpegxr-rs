#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = [n][s][d][shift][n_len][bits][bias: i32 LE][values...]: the Gray16 output loop for n % 4
// groups of 16 pixels from offset s % 70 of y, into a buffer from offset d % 5, with a shift of
// 0 to 47 and a left shift n_len of 0 to 39 (31 and above take the scalar path in the SIMD tier
// too). The SIMD tier equals the scalar code bit for bit, and writes the same bytes.
fuzz_target!(|data: &[u8]| {
    check_tier();
    let Some((&[n, s, d, shift, n_len, bits, b0, b1, b2, b3], rest)) =
        data.split_first_chunk::<10>()
    else {
        return;
    };
    let (n, s, d) = (usize::from(n) % 4, usize::from(s) % 70, usize::from(d) % 5);
    let (shift, n_len, bits) = (
        u32::from(shift) % 48,
        u32::from(n_len) % 40,
        u32::from(bits) % 31 + 2,
    );
    let bias = i32::from_le_bytes([b0, b1, b2, b3]) >> (32 - bits);
    let y = values(rest, s + 256 * n + 8, bits);
    let o = vec![0xa5u8; d + 32 * n + 3];
    let (mut x, mut z) = (o.clone(), o);
    let l = limits();
    jpegxr::kernels::out_gray16(false, &mut x, d, &y, s, n, bias, shift, n_len, &l)
        .expect("scalar");
    jpegxr::kernels::out_gray16(true, &mut z, d, &y, s, n, bias, shift, n_len, &l).expect("simd");
    assert!(
        x == z,
        "n={n} s={s} d={d} shift={shift} n_len={n_len} bits={bits}"
    );
});
