#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = [n][o][bits][values...]: the 4x4 IDCT stage 1 on n % 24 pairs of blocks from offset
// o % 20, as the decoder calls it (a buffer of o + 64n + 40 values), of about bits % 31 + 2
// bits. The SIMD tier equals the scalar code bit for bit, from any start and any count (so any
// tail after the groups of four pairs).
fuzz_target!(|data: &[u8]| {
    check_tier();
    let Some((&[n, o, bits], rest)) = data.split_first_chunk::<3>() else {
        return;
    };
    let (n, o, bits) = (
        usize::from(n) % 24,
        usize::from(o) % 20,
        u32::from(bits) % 31 + 2,
    );
    let a = values(rest, o + 64 * n + 40, bits);
    let (mut x, mut y) = (a.clone(), a);
    let l = limits();
    jpegxr::kernels::idct_pairs(false, &mut x, o as isize, n as isize, &l).expect("scalar");
    jpegxr::kernels::idct_pairs(true, &mut y, o as isize, n as isize, &l).expect("simd");
    assert!(x == y, "n={n} o={o} bits={bits}");
});
