//go:build !purego

#include "textflag.h"

// Lane k of each register holds block k (low half) or k+4 (high half).
// Constants: Y12 = 1, Y13 = 4, Y14 = 3, Y15 = 0. Temps: Y8-Y11.

// Transpose four rows in Y0-Y3, per 128-bit half.
#define TRANSPOSE \
	VPUNPCKLDQ  Y1, Y0, Y4 \
	VPUNPCKHDQ  Y1, Y0, Y5 \
	VPUNPCKLDQ  Y3, Y2, Y6 \
	VPUNPCKHDQ  Y3, Y2, Y7 \
	VPUNPCKLQDQ Y6, Y4, Y0 \
	VPUNPCKHQDQ Y6, Y4, Y1 \
	VPUNPCKLQDQ Y7, Y5, Y2 \
	VPUNPCKHQDQ Y7, Y5, Y3

// t = (x*3 + k) >> s
#define MUL3ADDSHR(x, k, s, t) \
	VPSLLD $1, x, t \
	VPADDD x, t, t \
	VPADDD k, t, t \
	VPSRAD $s, t, t

// strDCT2x2up with rnd Y12, strDCT2x2dn with zero Y15.
#define DCT2X2(a, b, C, d, rnd) \
	VPADDD d, a, a \
	VPSUBD C, b, b \
	VPSUBD b, a, Y8 \
	VPADDD rnd, Y8, Y8 \
	VPSRAD $1, Y8, Y8 \
	VPSUBD C, Y8, Y9 \
	VPSUBD d, Y8, C \
	VMOVDQA Y9, d \
	VPSUBD d, a, a \
	VPADDD C, b, b

#define INVODD(a, b, c, d) \
	VPADDD d, b, b \
	VPSUBD c, a, a \
	VPSRAD $1, b, Y8 \
	VPSUBD Y8, d, d \
	VPADDD Y12, a, Y8 \
	VPSRAD $1, Y8, Y8 \
	VPADDD Y8, c, c \
	MUL3ADDSHR(b, Y13, 3, Y8) \
	VPSUBD Y8, a, a \
	MUL3ADDSHR(a, Y13, 3, Y8) \
	VPADDD Y8, b, b \
	MUL3ADDSHR(d, Y13, 3, Y8) \
	VPSUBD Y8, c, c \
	MUL3ADDSHR(c, Y13, 3, Y8) \
	VPADDD Y8, d, d \
	VPADDD Y12, b, Y8 \
	VPSRAD $1, Y8, Y8 \
	VPSUBD Y8, c, c \
	VPADDD Y12, a, Y8 \
	VPSRAD $1, Y8, Y8 \
	VPSUBD d, Y8, d \
	VPADDD c, b, b \
	VPSUBD d, a, a

#define INVODDODD(a, b, c, d) \
	VPADDD a, d, d \
	VPSUBD b, c, c \
	VPSRAD $1, d, Y9 \
	VPSUBD Y9, a, a \
	VPSRAD $1, c, Y10 \
	VPADDD Y10, b, b \
	MUL3ADDSHR(b, Y14, 3, Y8) \
	VPSUBD Y8, a, a \
	MUL3ADDSHR(a, Y14, 2, Y8) \
	VPADDD Y8, b, b \
	MUL3ADDSHR(b, Y13, 3, Y8) \
	VPSUBD Y8, a, a \
	VPSUBD Y10, b, b \
	VPADDD Y9, a, a \
	VPADDD b, c, c \
	VPSUBD a, d, d \
	VPSUBD b, Y15, b \
	VPSUBD c, Y15, c

// Blocks 0-3 sit at bytes 0, 64, 256, 320; blocks 4-7 at +512.
#define LOADROW(r) \
	VMOVDQU     (r)(DI), X0 \
	VINSERTI128 $1, (512+r)(DI), Y0, Y0 \
	VMOVDQU     (64+r)(DI), X1 \
	VINSERTI128 $1, (576+r)(DI), Y1, Y1 \
	VMOVDQU     (256+r)(DI), X2 \
	VINSERTI128 $1, (768+r)(DI), Y2, Y2 \
	VMOVDQU     (320+r)(DI), X3 \
	VINSERTI128 $1, (832+r)(DI), Y3, Y3

#define STOREROW(r) \
	VMOVDQU      X0, (r)(DI) \
	VEXTRACTI128 $1, Y0, (512+r)(DI) \
	VMOVDQU      X1, (64+r)(DI) \
	VEXTRACTI128 $1, Y1, (576+r)(DI) \
	VMOVDQU      X2, (256+r)(DI) \
	VEXTRACTI128 $1, Y2, (768+r)(DI) \
	VMOVDQU      X3, (320+r)(DI) \
	VEXTRACTI128 $1, Y3, (832+r)(DI)

// Scratch slot for element e is e*32(SP).
#define SPILL(e0, e1, e2, e3) \
	VMOVDQU Y0, (e0*32)(SP) \
	VMOVDQU Y1, (e1*32)(SP) \
	VMOVDQU Y2, (e2*32)(SP) \
	VMOVDQU Y3, (e3*32)(SP)

#define FILL(e0, e1, e2, e3) \
	VMOVDQU (e0*32)(SP), Y0 \
	VMOVDQU (e1*32)(SP), Y1 \
	VMOVDQU (e2*32)(SP), Y2 \
	VMOVDQU (e3*32)(SP), Y3

// func idct4x4Stage1PairsAsm(p *int32, n int)
TEXT ·idct4x4Stage1PairsAsm(SB), NOSPLIT, $512-16
	MOVQ p+0(FP), DI
	MOVQ n+8(FP), CX
	// VEX MOVD: legacy SSE stalls Intel.
	MOVL $1, AX
	VMOVD AX, X12
	VPBROADCASTD X12, Y12
	MOVL $4, AX
	VMOVD AX, X13
	VPBROADCASTD X13, Y13
	MOVL $3, AX
	VMOVD AX, X14
	VPBROADCASTD X14, Y14
	VPXOR Y15, Y15, Y15

loop:
	// Row r: transpose, then its 1D pass.
	LOADROW(0)
	TRANSPOSE
	DCT2X2(Y0, Y1, Y2, Y3, Y12)
	SPILL(0, 1, 2, 3)
	LOADROW(16)
	TRANSPOSE
	INVODD(Y1, Y0, Y3, Y2)
	SPILL(4, 5, 6, 7)
	LOADROW(32)
	TRANSPOSE
	INVODD(Y2, Y0, Y3, Y1)
	SPILL(8, 9, 10, 11)
	LOADROW(48)
	TRANSPOSE
	INVODDODD(Y3, Y2, Y1, Y0)
	SPILL(12, 13, 14, 15)

	// fourButterflyHardcoded1, column by column.
	FILL(0, 4, 8, 12)
	DCT2X2(Y0, Y1, Y2, Y3, Y15)
	SPILL(0, 4, 8, 12)
	FILL(1, 5, 9, 13)
	DCT2X2(Y0, Y1, Y2, Y3, Y15)
	SPILL(1, 5, 9, 13)
	FILL(2, 6, 10, 14)
	DCT2X2(Y0, Y1, Y2, Y3, Y15)
	SPILL(2, 6, 10, 14)
	FILL(3, 7, 11, 15)
	DCT2X2(Y0, Y1, Y2, Y3, Y15)
	SPILL(3, 7, 11, 15)

	FILL(0, 1, 2, 3)
	TRANSPOSE
	STOREROW(0)
	FILL(4, 5, 6, 7)
	TRANSPOSE
	STOREROW(16)
	FILL(8, 9, 10, 11)
	TRANSPOSE
	STOREROW(32)
	FILL(12, 13, 14, 15)
	TRANSPOSE
	STOREROW(48)

	ADDQ $1024, DI
	SUBQ $4, CX
	JNZ  loop
	VZEROUPPER
	RET

// func post4x4Stage1SplitAsm(a, b, c, d *int32)
TEXT ·post4x4Stage1SplitAsm(SB), NOSPLIT, $0-32
	MOVQ a+0(FP), DI
	MOVQ b+8(FP), SI
	MOVQ c+16(FP), DX
	MOVQ d+24(FP), CX
	MOVL $1, AX
	MOVD AX, X12
	VPBROADCASTD X12, X12
	MOVL $4, AX
	MOVD AX, X13
	VPBROADCASTD X13, X13
	VMOVDQU (DI), X0
	VMOVDQU (SI), X1
	VMOVDQU (DX), X2
	VMOVDQU (CX), X3

	// strDCT2x2dn(a, b, c, d); c lands in X2.
	VPADDD X3, X0, X0
	VPSUBD X2, X1, X1
	VPSUBD X1, X0, X8
	VPSRAD $1, X8, X8
	VPSUBD X2, X8, X9
	VPSUBD X3, X8, X2
	VMOVDQA X9, X3
	VPSUBD X3, X0, X0
	VPADDD X2, X1, X1

	// invOddOddPost on the lanes of d.
	VMOVD   X3, AX
	VPEXTRD $1, X3, BX
	VPEXTRD $2, X3, R8
	VPEXTRD $3, X3, R9
	ADDL AX, R9
	SUBL BX, R8
	MOVL R9, R10
	SARL $1, R10
	SUBL R10, AX
	MOVL R8, R11
	SARL $1, R11
	ADDL R11, BX
	LEAL 6(BX)(BX*2), R12
	SARL $3, R12
	SUBL R12, AX
	LEAL 2(AX)(AX*2), R12
	SARL $2, R12
	ADDL R12, BX
	LEAL 4(BX)(BX*2), R12
	SARL $3, R12
	SUBL R12, AX
	SUBL R11, BX
	ADDL R10, AX
	ADDL BX, R8
	SUBL AX, R9
	VMOVD   AX, X3
	VPINSRD $1, BX, X3, X3
	VPINSRD $2, R8, X3, X3
	VPINSRD $3, R9, X3, X3

	// irotate1 on c(2,3), c(0,1), b(1,3), b(0,2):
	// A = {c0, c2, b0, b1}, B = {c1, c3, b2, b3}.
	VPSHUFD     $0xd8, X2, X6
	VPUNPCKLQDQ X1, X6, X7
	VPUNPCKHQDQ X1, X6, X8
	VPADDD      X12, X8, X9
	VPSRAD      $1, X9, X9
	VPSUBD      X9, X7, X7
	VPADDD      X12, X7, X9
	VPSRAD      $1, X9, X9
	VPADDD      X9, X8, X8
	VPUNPCKLDQ  X8, X7, X2
	VPUNPCKHQDQ X8, X7, X1

	// strHSTdec1Alternate(a, d)
	VPADDD X3, X0, X0
	VPSRAD $1, X0, X8
	VPSUBD X3, X8, X3
	VPSLLD $1, X3, X8
	VPADDD X3, X8, X8
	VPSRAD $3, X8, X8
	VPADDD X8, X0, X0
	VPSLLD $1, X0, X8
	VPADDD X0, X8, X8
	VPSRAD $4, X8, X8
	VPADDD X8, X3, X3
	VPSRAD $7, X0, X8
	VPADDD X8, X3, X3
	VPSRAD $10, X0, X8
	VPSUBD X8, X3, X3

	// strHSTdec(a, b, c, d)
	VPSUBD X2, X1, X1
	VPSLLD $1, X3, X8
	VPADDD X3, X8, X8
	VPADDD X13, X8, X8
	VPSRAD $3, X8, X8
	VPADDD X8, X0, X0
	VPSRAD $1, X1, X8
	VPSUBD X8, X3, X3
	VPSUBD X1, X0, X8
	VPSRAD $1, X8, X8
	VPSUBD X2, X8, X2
	VPSUBD X2, X0, X0
	VPADDD X3, X1, X1

	VMOVDQU X0, (DI)
	VMOVDQU X1, (SI)
	VMOVDQU X3, (DX)
	VMOVDQU X2, (CX)
	RET

// Pixels 4g..4g+3 of an MB row are elements +0, +1, +5, +4.
#define LOADGRP(R, off, X) \
	VMOVQ       off(R), X \
	VMOVQ       (off+16)(R), X12 \
	VPUNPCKLQDQ X12, X, X \
	VPSHUFD     $0xb4, X, X

#define LOADMB(R, X0, X1, X2, X3) \
	LOADGRP(R, 0, X0) \
	LOADGRP(R, 256, X1) \
	LOADGRP(R, 512, X2) \
	LOADGRP(R, 768, X3)

// icc on (y, u, v) in place: g in y, r in u, b in v.
#define ICC(y, u, v) \
	VPADDD X15, y, y \
	VPXOR  X12, X12, X12 \
	VPSUBD u, X12, u \
	VPSRAD $1, u, X12 \
	VPSUBD X12, y, y \
	VPADDD X13, v, X12 \
	VPSRAD $1, X12, X12 \
	VPSUBD y, X12, X12 \
	VPSUBD X12, u, u \
	VPADDD u, v, v \
	VPSRAD X14, y, y \
	VPSRAD X14, u, u \
	VPSRAD X14, v, v

// Saturate a..e to 16 bytes in a.
#define NARROW8(a, b, c, e) \
	VPACKSSDW b, a, a \
	VPACKSSDW e, c, c \
	VPACKUSWB c, a, a

// Byte 16k+i of three planes interleaved.
#define INTERLEAVE(m0, m1, m2, off) \
	VPSHUFB m0(SB), X4, X1 \
	VPSHUFB m1(SB), X0, X2 \
	VPOR    X2, X1, X1 \
	VPSHUFB m2(SB), X8, X2 \
	VPOR    X2, X1, X1 \
	VMOVDQU X1, off(DI)

// func outBGR8Asm(dst *byte, y, u, v *int32, n int, bias int32, shift uint32, rgb int)
TEXT ·outBGR8Asm(SB), NOSPLIT, $0-56
	MOVQ dst+0(FP), DI
	MOVQ y+8(FP), SI
	MOVQ u+16(FP), DX
	MOVQ v+24(FP), CX
	MOVQ n+32(FP), BX
	MOVQ rgb+48(FP), R8
	MOVL bias+40(FP), AX
	MOVD AX, X15
	VPBROADCASTD X15, X15
	MOVL shift+44(FP), AX
	MOVD AX, X14
	MOVL $1, AX
	MOVD AX, X13
	VPBROADCASTD X13, X13

bgrloop:
	LOADMB(SI, X0, X1, X2, X3)
	LOADMB(DX, X4, X5, X6, X7)
	LOADMB(CX, X8, X9, X10, X11)
	ADDQ $1024, SI
	ADDQ $1024, DX
	ADDQ $1024, CX
	ICC(X0, X4, X8)
	ICC(X1, X5, X9)
	ICC(X2, X6, X10)
	ICC(X3, X7, X11)
	NARROW8(X0, X1, X2, X3)
	NARROW8(X4, X5, X6, X7)
	NARROW8(X8, X9, X10, X11)
	TESTQ R8, R8
	JNZ   rgbfirst
	VMOVDQA X4, X12
	VMOVDQA X8, X4
	VMOVDQA X12, X8
rgbfirst:
	INTERLEAVE(bgrmask00<>, bgrmask01<>, bgrmask02<>, 0)
	INTERLEAVE(bgrmask10<>, bgrmask11<>, bgrmask12<>, 16)
	INTERLEAVE(bgrmask20<>, bgrmask21<>, bgrmask22<>, 32)
	ADDQ $48, DI
	DECQ BX
	JNZ  bgrloop
	RET

// func outGray16Asm(dst *byte, y *int32, n int, bias int32, shift, nLen uint32)
TEXT ·outGray16Asm(SB), NOSPLIT, $0-36
	MOVQ dst+0(FP), DI
	MOVQ y+8(FP), SI
	MOVQ n+16(FP), BX
	MOVL bias+24(FP), AX
	MOVD AX, X15
	VPBROADCASTD X15, X15
	MOVL shift+28(FP), AX
	MOVD AX, X14
	MOVL nLen+32(FP), AX
	MOVD AX, X13

g16loop:
	LOADMB(SI, X0, X1, X2, X3)
	ADDQ $1024, SI
	VPADDD X15, X0, X0
	VPADDD X15, X1, X1
	VPADDD X15, X2, X2
	VPADDD X15, X3, X3
	VPSRAD X14, X0, X0
	VPSRAD X14, X1, X1
	VPSRAD X14, X2, X2
	VPSRAD X14, X3, X3
	VPSLLD X13, X0, X0
	VPSLLD X13, X1, X1
	VPSLLD X13, X2, X2
	VPSLLD X13, X3, X3
	VPACKUSDW X1, X0, X0
	VPACKUSDW X3, X2, X2
	VMOVDQU X0, (DI)
	VMOVDQU X2, 16(DI)
	ADDQ $32, DI
	DECQ BX
	JNZ  g16loop
	RET

// pshufb masks: pixel (16k+i)/3 when channel (16k+i)%3 is c.
DATA bgrmask00<>+0(SB)/8, $0x8002808001808000
DATA bgrmask00<>+8(SB)/8, $0x0580800480800380
GLOBL bgrmask00<>(SB), RODATA|NOPTR, $16

DATA bgrmask01<>+0(SB)/8, $0x0280800180800080
DATA bgrmask01<>+8(SB)/8, $0x8080048080038080
GLOBL bgrmask01<>(SB), RODATA|NOPTR, $16

DATA bgrmask02<>+0(SB)/8, $0x8080018080008080
DATA bgrmask02<>+8(SB)/8, $0x8004808003808002
GLOBL bgrmask02<>(SB), RODATA|NOPTR, $16

DATA bgrmask10<>+0(SB)/8, $0x8080078080068080
DATA bgrmask10<>+8(SB)/8, $0x800a808009808008
GLOBL bgrmask10<>(SB), RODATA|NOPTR, $16

DATA bgrmask11<>+0(SB)/8, $0x8007808006808005
DATA bgrmask11<>+8(SB)/8, $0x0a80800980800880
GLOBL bgrmask11<>(SB), RODATA|NOPTR, $16

DATA bgrmask12<>+0(SB)/8, $0x0780800680800580
DATA bgrmask12<>+8(SB)/8, $0x8080098080088080
GLOBL bgrmask12<>(SB), RODATA|NOPTR, $16

DATA bgrmask20<>+0(SB)/8, $0x0d80800c80800b80
DATA bgrmask20<>+8(SB)/8, $0x80800f80800e8080
GLOBL bgrmask20<>(SB), RODATA|NOPTR, $16

DATA bgrmask21<>+0(SB)/8, $0x80800c80800b8080
DATA bgrmask21<>+8(SB)/8, $0x800f80800e80800d
GLOBL bgrmask21<>(SB), RODATA|NOPTR, $16

DATA bgrmask22<>+0(SB)/8, $0x800c80800b80800a
DATA bgrmask22<>+8(SB)/8, $0x0f80800e80800d80
GLOBL bgrmask22<>(SB), RODATA|NOPTR, $16

// func cpuid(eaxArg, ecxArg uint32) (eax, ebx, ecx, edx uint32)
TEXT ·cpuid(SB), NOSPLIT, $0-24
	MOVL eaxArg+0(FP), AX
	MOVL ecxArg+4(FP), CX
	CPUID
	MOVL AX, eax+8(FP)
	MOVL BX, ebx+12(FP)
	MOVL CX, ecx+16(FP)
	MOVL DX, edx+20(FP)
	RET

// func xgetbv() (eax uint32)
TEXT ·xgetbv(SB), NOSPLIT, $0-4
	MOVL $0, CX
	XGETBV
	MOVL AX, eax+0(FP)
	RET

// Flexbit loop of decodeBlockAdaptive; the caller guarantees 48 bytes at p.
// AX acc, BX nb, R11 f; p lives in its argument slot.
// acc above thr has its top f bits nonzero.
// func flexbitsAsm(coef *int32, order *[16]int, p *byte, acc uint64, nb, f uint32, q int32) (accOut uint64, nbOut uint32, adv uintptr)
TEXT ·flexbitsAsm(SB), NOSPLIT, $8-72
	MOVQ coef+0(FP), DI
	MOVQ order+8(FP), SI
	MOVQ p+16(FP), R12
	MOVQ R12, adv+64(FP)
	MOVQ acc+24(FP), AX
	MOVL nb+32(FP), BX
	MOVL f+36(FP), R11
	MOVL $64, CX
	SUBL R11, CX
	MOVQ $1, R12
	SHLQ CX, R12
	DECQ R12
	MOVQ R12, thr-8(SP)
	LEAQ 128(SI), R8
	ADDQ $8, SI

flexloop:
	MOVQ (SI), R9
	MOVL (DI)(R9*4), R10
	CMPL BX, R11
	JHI  flexnofill
	MOVQ p+16(FP), R12
	MOVQ (R12), R13
	BSWAPQ R13
	MOVL BX, CX
	SHRQ CX, R13
	ORQ  R13, AX
	MOVL $63, CX
	SUBL BX, CX
	SHRL $3, CX
	ADDQ CX, R12
	MOVQ R12, p+16(FP)
	ORL  $56, BX

flexnofill:
	MOVL $63, CX
	SUBL R11, CX
	MOVQ AX, R12
	SHRQ CX, R12
	MOVL R12, R13
	SHRL $1, R13
	ANDL $1, R12
	NEGL R12
	MOVL R10, CX
	SARL $31, CX
	MOVQ $-1, DX
	TESTL R10, R10
	CMOVLNE CX, R12
	CMOVQEQ thr-8(SP), DX
	XORL R12, R13
	SUBL R12, R13
	IMULL q+40(FP), R13
	ADDL R10, R13
	MOVL R13, (DI)(R9*4)
	CMPQ DX, AX
	MOVL R11, DX
	ADCL $0, DX
	MOVL DX, CX
	SHLQ CX, AX
	SUBL DX, BX
	ADDQ $8, SI
	CMPQ SI, R8
	JNE  flexloop

	MOVQ p+16(FP), R12
	SUBQ adv+64(FP), R12
	MOVQ R12, adv+64(FP)
	MOVQ AX, accOut+48(FP)
	MOVL BX, nbOut+56(FP)
	RET
