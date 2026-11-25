# Fuzzing

`tests/fuzz/` is a cargo-fuzz crate: `decode`, `decode_config`, `decode_component` and `decoder`
(`Decoder` and `decode_into` into a reused buffer, with row padding, short dst and cancel) fuzz
the public entry points; `idct_pairs`, `post_split`, `out_bgr8` and `out_gray16` compare each SIMD
kernel with its scalar code (through the `#[doc(hidden)] jpegxr::kernels` module). Seeds are the
jpegxr-go `testdata/fuzz` entries (`tests/fuzz/seeds`) and the synth and alpha streams. `fuzz.yml` runs them
weekly and on dispatch (`fuzz-arm64` runs the kernel targets on a native arm64 runner).
