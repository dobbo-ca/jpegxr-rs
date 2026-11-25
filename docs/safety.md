# Safety and SIMD

Four kernels have an AVX2 (x86_64) and a NEON (aarch64) tier beside the scalar reference: the 4x4
IDCT stage 1 (eight blocks a pass, as Go's AVX2 kernel), the stage 1 post filter (four lanes), and
the BGR8 and Gray16 output loops (sixteen pixels a macroblock). Each is written once on fixed
width i32 lanes (`src/codec/simd.rs`) and compiled under `#[target_feature(enable = "avx2")]` or
`neon`, so the vector instructions come from LLVM rather than from intrinsics or assembly; every
lane operation wraps as the scalar code does, so the tiers are bit-exact. The tier is detected
once at first use (`is_x86_feature_detected!`, `is_aarch64_feature_detected!`);
`JPEGXR_SIMD=scalar` forces the scalar one. The x86_64 BGR8 loop is the exception: it uses
intrinsics (`src/codec/bgr8.rs`, as Go's `outBGR8Asm`). `unsafe` is confined to the calls of the
target-feature functions in `simd.rs` and to three one-line load/store helpers in
`src/codec/x86.rs` that take array references; each has a `// SAFETY:` comment; the kernel bodies
are safe code. As in Go, shift counts above 31 take the scalar path.

Go's other two asm kernels, `flexbits` (refinement bits) and `highpass` (the Huffman/run-length
block decode), are bit-serial: there is no data parallelism for SIMD, and Go wrote them in asm to
escape its compiler's code. They stay scalar Rust. The benchmark job will show whether that holds.

Tests (`src/codec/simd.rs`) compare each kernel's tier with scalar on xorshift data across
sizes, offsets, shifts and value ranges (including 31-bit values that wrap), and assert that the
tier was detected (AVX2 on x86_64, NEON on aarch64).
The one other `unsafe` is the counting `GlobalAlloc` in `tests/alloc.rs`, a test crate, not the library
(it also counts the live heap and its peak, so the limits tests need no second allocator).
