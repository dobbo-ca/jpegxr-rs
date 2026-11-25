//go:build !purego

#include "textflag.h"

// Lane k of each register holds block k.
// Constants: V28 = 1, V29 = 4, V30 = 3, V31 = 0. Temps: V16-V19.

// Transpose row r of four blocks, in place.
#define TRANSPOSE(r0, r1, r2, r3) \
	VTRN1 r1.S4, r0.S4, V16.S4 \
	VTRN2 r1.S4, r0.S4, V17.S4 \
	VTRN1 r3.S4, r2.S4, V18.S4 \
	VTRN2 r3.S4, r2.S4, V19.S4 \
	VZIP1 V18.D2, V16.D2, r0.D2 \
	VZIP2 V18.D2, V16.D2, r2.D2 \
	VZIP1 V19.D2, V17.D2, r1.D2 \
	VZIP2 V19.D2, V17.D2, r3.D2

// t = (x*3 + k) >> s
#define MUL3ADDSHR(x, k, s, t) \
	VSHL $1, x.S4, t.S4 \
	VADD x.S4, t.S4, t.S4 \
	VADD k.S4, t.S4, t.S4 \
	VSSHR $s, t.S4, t.S4

// strDCT2x2up with rnd V28, strDCT2x2dn with zero V31.
#define DCT2X2(a, b, C, d, rnd) \
	VADD d.S4, a.S4, a.S4 \
	VSUB C.S4, b.S4, b.S4 \
	VSUB b.S4, a.S4, V16.S4 \
	VADD rnd.S4, V16.S4, V16.S4 \
	VSSHR $1, V16.S4, V16.S4 \
	VSUB C.S4, V16.S4, V17.S4 \
	VSUB d.S4, V16.S4, C.S4 \
	VORR V17.B16, V17.B16, d.B16 \
	VSUB d.S4, a.S4, a.S4 \
	VADD C.S4, b.S4, b.S4

#define INVODD(a, b, c, d) \
	VADD d.S4, b.S4, b.S4 \
	VSUB c.S4, a.S4, a.S4 \
	VSSHR $1, b.S4, V16.S4 \
	VSUB V16.S4, d.S4, d.S4 \
	VADD V28.S4, a.S4, V16.S4 \
	VSSHR $1, V16.S4, V16.S4 \
	VADD V16.S4, c.S4, c.S4 \
	MUL3ADDSHR(b, V29, 3, V16) \
	VSUB V16.S4, a.S4, a.S4 \
	MUL3ADDSHR(a, V29, 3, V16) \
	VADD V16.S4, b.S4, b.S4 \
	MUL3ADDSHR(d, V29, 3, V16) \
	VSUB V16.S4, c.S4, c.S4 \
	MUL3ADDSHR(c, V29, 3, V16) \
	VADD V16.S4, d.S4, d.S4 \
	VADD V28.S4, b.S4, V16.S4 \
	VSSHR $1, V16.S4, V16.S4 \
	VSUB V16.S4, c.S4, c.S4 \
	VADD V28.S4, a.S4, V16.S4 \
	VSSHR $1, V16.S4, V16.S4 \
	VSUB d.S4, V16.S4, d.S4 \
	VADD c.S4, b.S4, b.S4 \
	VSUB d.S4, a.S4, a.S4

#define INVODDODD(a, b, c, d) \
	VADD a.S4, d.S4, d.S4 \
	VSUB b.S4, c.S4, c.S4 \
	VSSHR $1, d.S4, V17.S4 \
	VSUB V17.S4, a.S4, a.S4 \
	VSSHR $1, c.S4, V18.S4 \
	VADD V18.S4, b.S4, b.S4 \
	MUL3ADDSHR(b, V30, 3, V16) \
	VSUB V16.S4, a.S4, a.S4 \
	MUL3ADDSHR(a, V30, 2, V16) \
	VADD V16.S4, b.S4, b.S4 \
	MUL3ADDSHR(b, V29, 3, V16) \
	VSUB V16.S4, a.S4, a.S4 \
	VSUB V18.S4, b.S4, b.S4 \
	VADD V17.S4, a.S4, a.S4 \
	VADD b.S4, c.S4, c.S4 \
	VSUB a.S4, d.S4, d.S4 \
	VNEG b.S4, b.S4 \
	VNEG c.S4, c.S4

// func idct4x4Stage1PairsAsm(p *int32, n int)
TEXT ·idct4x4Stage1PairsAsm(SB), NOSPLIT, $0-16
	MOVD p+0(FP), R0
	MOVD n+8(FP), R1
	MOVW $1, R2
	VDUP R2, V28.S4
	MOVW $4, R2
	VDUP R2, V29.S4
	MOVW $3, R2
	VDUP R2, V30.S4
	VEOR V31.B16, V31.B16, V31.B16

loop:
	ADD  $64, R0, R2
	ADD  $256, R0, R3
	ADD  $320, R0, R4
	VLD1 (R0), [V0.S4, V1.S4, V2.S4, V3.S4]
	VLD1 (R2), [V4.S4, V5.S4, V6.S4, V7.S4]
	VLD1 (R3), [V8.S4, V9.S4, V10.S4, V11.S4]
	VLD1 (R4), [V12.S4, V13.S4, V14.S4, V15.S4]

	// Element 4r+i now sits in V(4i+r).
	TRANSPOSE(V0, V4, V8, V12)
	TRANSPOSE(V1, V5, V9, V13)
	TRANSPOSE(V2, V6, V10, V14)
	TRANSPOSE(V3, V7, V11, V15)

	DCT2X2(V0, V4, V8, V12, V28)
	INVODD(V5, V1, V13, V9)
	INVODD(V10, V2, V14, V6)
	INVODDODD(V15, V11, V7, V3)

	DCT2X2(V0, V1, V2, V3, V31)
	DCT2X2(V4, V5, V6, V7, V31)
	DCT2X2(V8, V9, V10, V11, V31)
	DCT2X2(V12, V13, V14, V15, V31)

	TRANSPOSE(V0, V4, V8, V12)
	TRANSPOSE(V1, V5, V9, V13)
	TRANSPOSE(V2, V6, V10, V14)
	TRANSPOSE(V3, V7, V11, V15)

	VST1 [V0.S4, V1.S4, V2.S4, V3.S4], (R0)
	VST1 [V4.S4, V5.S4, V6.S4, V7.S4], (R2)
	VST1 [V8.S4, V9.S4, V10.S4, V11.S4], (R3)
	VST1 [V12.S4, V13.S4, V14.S4, V15.S4], (R4)

	ADD  $512, R0, R0
	SUBS $2, R1, R1
	BNE  loop
	RET

// func post4x4Stage1SplitAsm(a, b, c, d *int32)
TEXT ·post4x4Stage1SplitAsm(SB), NOSPLIT, $0-32
	MOVD a+0(FP), R0
	MOVD b+8(FP), R1
	MOVD c+16(FP), R2
	MOVD d+24(FP), R3
	MOVW $1, R9
	VDUP R9, V28.S4
	MOVW $4, R9
	VDUP R9, V29.S4
	VEOR V31.B16, V31.B16, V31.B16
	VLD1 (R0), [V0.S4]
	VLD1 (R1), [V1.S4]
	VLD1 (R2), [V2.S4]
	VLD1 (R3), [V3.S4]

	// strDCT2x2dn(a, b, c, d); c lands in V2.
	DCT2X2(V0, V1, V2, V3, V31)

	// invOddOddPost on the lanes of d.
	VMOV V3.S[0], R4
	VMOV V3.S[1], R5
	VMOV V3.S[2], R6
	VMOV V3.S[3], R7
	ADDW R4, R7, R7
	SUBW R5, R6, R6
	ASRW $1, R7, R8
	SUBW R8, R4, R4
	ASRW $1, R6, R9
	ADDW R9, R5, R5
	ADDW R5<<1, R5, R10
	ADDW $6, R10, R10
	ASRW $3, R10, R10
	SUBW R10, R4, R4
	ADDW R4<<1, R4, R10
	ADDW $2, R10, R10
	ASRW $2, R10, R10
	ADDW R10, R5, R5
	ADDW R5<<1, R5, R10
	ADDW $4, R10, R10
	ASRW $3, R10, R10
	SUBW R10, R4, R4
	SUBW R9, R5, R5
	ADDW R8, R4, R4
	ADDW R5, R6, R6
	SUBW R4, R7, R7
	VMOV R4, V3.S[0]
	VMOV R5, V3.S[1]
	VMOV R6, V3.S[2]
	VMOV R7, V3.S[3]

	// irotate1 on c(2,3), c(0,1), b(1,3), b(0,2):
	// A = {c0, c2, b0, b1}, B = {c1, c3, b2, b3}.
	VUZP1 V2.S4, V2.S4, V16.S4
	VUZP2 V2.S4, V2.S4, V17.S4
	VZIP1 V1.D2, V16.D2, V16.D2
	VZIP2 V1.D2, V17.D2, V17.D2
	VADD  V28.S4, V17.S4, V18.S4
	VSSHR $1, V18.S4, V18.S4
	VSUB  V18.S4, V16.S4, V16.S4
	VADD  V28.S4, V16.S4, V18.S4
	VSSHR $1, V18.S4, V18.S4
	VADD  V18.S4, V17.S4, V17.S4
	VZIP1 V17.S4, V16.S4, V2.S4
	VZIP2 V17.D2, V16.D2, V1.D2

	// strHSTdec1Alternate(a, d)
	VADD  V3.S4, V0.S4, V0.S4
	VSSHR $1, V0.S4, V16.S4
	VSUB  V3.S4, V16.S4, V3.S4
	VSHL  $1, V3.S4, V16.S4
	VADD  V3.S4, V16.S4, V16.S4
	VSSHR $3, V16.S4, V16.S4
	VADD  V16.S4, V0.S4, V0.S4
	VSHL  $1, V0.S4, V16.S4
	VADD  V0.S4, V16.S4, V16.S4
	VSSHR $4, V16.S4, V16.S4
	VADD  V16.S4, V3.S4, V3.S4
	VSSHR $7, V0.S4, V16.S4
	VADD  V16.S4, V3.S4, V3.S4
	VSSHR $10, V0.S4, V16.S4
	VSUB  V16.S4, V3.S4, V3.S4

	// strHSTdec(a, b, c, d)
	VSUB  V2.S4, V1.S4, V1.S4
	MUL3ADDSHR(V3, V29, 3, V16)
	VADD  V16.S4, V0.S4, V0.S4
	VSSHR $1, V1.S4, V16.S4
	VSUB  V16.S4, V3.S4, V3.S4
	VSUB  V1.S4, V0.S4, V16.S4
	VSSHR $1, V16.S4, V16.S4
	VSUB  V2.S4, V16.S4, V2.S4
	VSUB  V2.S4, V0.S4, V0.S4
	VADD  V3.S4, V1.S4, V1.S4

	VST1 [V0.S4], (R0)
	VST1 [V1.S4], (R1)
	VST1 [V3.S4], (R2)
	VST1 [V2.S4], (R3)
	RET

// Pixels 4g..4g+3 of an MB row are elements +0, +1, +5, +4.
#define LOADGRP(R, off, FV, V) \
	FMOVD off(R), FV \
	FMOVD (off+16)(R), F16 \
	VREV64 V16.S4, V16.S4 \
	VZIP1 V16.D2, V.D2, V.D2

#define LOADMB(R, F0, V0, F1, V1, F2, V2, F3, V3) \
	LOADGRP(R, 0, F0, V0) \
	LOADGRP(R, 256, F1, V1) \
	LOADGRP(R, 512, F2, V2) \
	LOADGRP(R, 768, F3, V3)

// Saturate four S4 registers to 16 bytes in d.
#define NARROW8(a, b, c, e, d) \
	VSQXTUN a.S4, V16.H4 \
	VSQXTUN2 b.S4, V16.H8 \
	VSQXTUN c.S4, V17.H4 \
	VSQXTUN2 e.S4, V17.H8 \
	VUQXTN V16.H8, d.B8 \
	VUQXTN2 V17.H8, d.B16

// icc on (y, u, v) in place: g in y, r in u, b in v.
#define ICC(y, u, v) \
	VADD V20.S4, y.S4, y.S4 \
	VNEG u.S4, u.S4 \
	VSSHR $1, u.S4, V16.S4 \
	VSUB V16.S4, y.S4, y.S4 \
	VADD V28.S4, v.S4, V16.S4 \
	VSSHR $1, V16.S4, V16.S4 \
	VSUB y.S4, V16.S4, V16.S4 \
	VSUB V16.S4, u.S4, u.S4 \
	VADD u.S4, v.S4, v.S4 \
	VSSHL V21.S4, y.S4, y.S4 \
	VSSHL V21.S4, u.S4, u.S4 \
	VSSHL V21.S4, v.S4, v.S4

// func outBGR8Asm(dst *byte, y, u, v *int32, n int, bias int32, shift uint32, rgb int)
TEXT ·outBGR8Asm(SB), NOSPLIT, $0-56
	MOVD  dst+0(FP), R0
	MOVD  y+8(FP), R1
	MOVD  u+16(FP), R2
	MOVD  v+24(FP), R3
	MOVD  n+32(FP), R4
	MOVW  bias+40(FP), R5
	MOVWU shift+44(FP), R6
	MOVD  rgb+48(FP), R7
	VDUP  R5, V20.S4
	NEG   R6, R6
	VDUP  R6, V21.S4
	MOVW  $1, R5
	VDUP  R5, V28.S4

bgrloop:
	LOADMB(R1, F0, V0, F1, V1, F2, V2, F3, V3)
	LOADMB(R2, F4, V4, F5, V5, F6, V6, F7, V7)
	LOADMB(R3, F8, V8, F9, V9, F10, V10, F11, V11)
	ADD $1024, R1, R1
	ADD $1024, R2, R2
	ADD $1024, R3, R3
	ICC(V0, V4, V8)
	ICC(V1, V5, V9)
	ICC(V2, V6, V10)
	ICC(V3, V7, V11)
	NARROW8(V0, V1, V2, V3, V25)
	CBZ R7, bgr
	NARROW8(V4, V5, V6, V7, V24)
	NARROW8(V8, V9, V10, V11, V26)
	B   store
bgr:
	NARROW8(V8, V9, V10, V11, V24)
	NARROW8(V4, V5, V6, V7, V26)
store:
	VST3.P [V24.B16, V25.B16, V26.B16], 48(R0)
	SUBS $1, R4, R4
	BNE  bgrloop
	RET

// func outGray16Asm(dst *byte, y *int32, n int, bias int32, shift, nLen uint32)
TEXT ·outGray16Asm(SB), NOSPLIT, $0-36
	MOVD  dst+0(FP), R0
	MOVD  y+8(FP), R1
	MOVD  n+16(FP), R4
	MOVW  bias+24(FP), R5
	MOVWU shift+28(FP), R6
	MOVWU nLen+32(FP), R7
	VDUP  R5, V20.S4
	NEG   R6, R6
	VDUP  R6, V21.S4
	VDUP  R7, V22.S4

g16loop:
	LOADMB(R1, F0, V0, F1, V1, F2, V2, F3, V3)
	ADD $1024, R1, R1
	VADD  V20.S4, V0.S4, V0.S4
	VADD  V20.S4, V1.S4, V1.S4
	VADD  V20.S4, V2.S4, V2.S4
	VADD  V20.S4, V3.S4, V3.S4
	VSSHL V21.S4, V0.S4, V0.S4
	VSSHL V21.S4, V1.S4, V1.S4
	VSSHL V21.S4, V2.S4, V2.S4
	VSSHL V21.S4, V3.S4, V3.S4
	VSSHL V22.S4, V0.S4, V0.S4
	VSSHL V22.S4, V1.S4, V1.S4
	VSSHL V22.S4, V2.S4, V2.S4
	VSSHL V22.S4, V3.S4, V3.S4
	VSQXTUN  V0.S4, V24.H4
	VSQXTUN2 V1.S4, V24.H8
	VSQXTUN  V2.S4, V25.H4
	VSQXTUN2 V3.S4, V25.H8
	VST1.P [V24.H8, V25.H8], 32(R0)
	SUBS $1, R4, R4
	BNE  g16loop
	RET

// Flexbit loop of decodeBlockAdaptive; the caller guarantees 48 bytes at p.
// R3 acc, R4 nb, R5 f, R21 63-f, R25 2**(64-f)-1: acc above it has v != 0.
TEXT ·flexbitsAsm(SB), NOSPLIT, $0-72
	MOVD  coef+0(FP), R0
	MOVD  order+8(FP), R1
	MOVD  p+16(FP), R2
	MOVD  acc+24(FP), R3
	MOVWU nb+32(FP), R4
	MOVWU f+36(FP), R5
	MOVW  q+40(FP), R6
	MOVD  R2, R24
	MOVD  $63, R21
	SUB   R5, R21, R21
	ADD   $1, R5, R11
	MOVD  $1, R25
	LSL   R21, R25, R25
	LSL   $1, R25, R25
	SUB   $1, R25, R25
	MOVD  $-1, R26
	MOVD  $1, R7

flexloop:
	MOVD (R1)(R7<<3), R9
	MOVW (R0)(R9<<2), R10
	CMP  R11, R4
	BHS  flexnofill
	MOVD (R2), R12
	REV  R12, R12
	LSR  R4, R12, R12
	ORR  R12, R3, R3
	MOVD $63, R13
	SUB  R4, R13, R13
	LSR  $3, R13, R13
	ADD  R13, R2, R2
	ORR  $56, R4, R4

flexnofill:
	LSR   R21, R3, R14
	LSR   $1, R14, R15
	AND   $1, R14, R19
	NEG   R19, R19
	ASRW  $31, R10, R20
	CMPW  $0, R10
	CSEL  EQ, R19, R20, R20
	CSEL  EQ, R25, R26, R22
	CMP   R22, R3
	CINC  HI, R5, R22
	EORW  R20, R15, R23
	SUBW  R20, R23, R23
	MULW  R6, R23, R23
	ADDW  R10, R23, R23
	MOVW  R23, (R0)(R9<<2)
	LSL   R22, R3, R3
	SUB   R22, R4, R4
	ADD   $1, R7, R7
	CMP   $16, R7
	BNE   flexloop

	SUB   R24, R2, R2
	MOVD  R3, accOut+48(FP)
	MOVW  R4, nbOut+56(FP)
	MOVD  R2, adv+64(FP)
	RET

// decodeBlockHighpassGo, line for line, with the same refills. The
// caller guarantees 160 bytes at p, so loads are unchecked.
// R0 ah (also the run table), R1 io, R2 iQP, R3 coef, R4 scan, R5 ah1,
// R6 p, R7 acc, R8 nb, R9 iLoc, R10 iNumNonzero, R11 iSRn, R12 iCont,
// R13 level, R20 runTab[t], R21 runTab[t][14], R22/R23 level tables,
// R24/R25 index tables, R26 slow-path return site. Temps R14-R17, R19.

#include "go_asm.h"

// Same as bitIO.refill with 8 bytes left.
#define HPREFILL \
	MOVD (R6), R16 \
	REV  R16, R16 \
	LSR  R8, R16, R16 \
	ORR  R16, R7, R7 \
	MOVD $63, R17 \
	SUB  R8, R17, R17 \
	ADD  R17>>3, R6, R6 \
	ORR  $56, R8, R8

// Consume the low 3 bits of sym as a code length.
#define HPFLUSH(sym) \
	AND $7, sym, R16 \
	LSL R16, R7, R7 \
	SUB R16, R8, R8

// a.disc += a.delta[i]; a.disc1 += a.delta1[i] with i scaled by 4.
// Uses R16 and R17 only.
#define HPDISC2(a, i) \
	ADD   $adaptiveHuffman_delta, a, R16 \
	MOVW  (R16)(i), R16 \
	MOVW  adaptiveHuffman_disc(a), R17 \
	ADDW  R16, R17, R17 \
	MOVW  R17, adaptiveHuffman_disc(a) \
	ADD   $adaptiveHuffman_delta1, a, R16 \
	MOVW  (R16)(i), R16 \
	MOVW  adaptiveHuffman_disc1(a), R17 \
	ADDW  R16, R17, R17 \
	MOVW  R17, adaptiveHuffman_disc1(a)

// R17 = run from runTab row base; consumes code and fixed bits.
#define HPRUN(base) \
	LSR   $59, R7, R16 \
	MOVWU (base)(R16<<2), R15 \
	UBFX  $8, R15, $8, R16 \
	LSL   R16, R7, R19 \
	LSR   $1, R19, R19 \
	LSR   $24, R15, R16 \
	LSR   R16, R19, R19 \
	UBFX  $16, R15, $8, R17 \
	ADD   R19, R17, R17 \
	AND   $0xff, R15, R16 \
	LSL   R16, R7, R7 \
	SUB   R16, R8, R8

// coef[scan[iLoc].uScan] = level, the scan update, then iLoc++.
#define HPSTORE(lbl) \
	ADD   R9<<3, R4, R14 \
	MOVWU adaptiveScan_uScan(R14), R15 \
	MOVW  R13, (R3)(R15<<2) \
	MOVWU adaptiveScan_uTotal(R14), R15 \
	ADDW  $1, R15, R15 \
	MOVW  R15, adaptiveScan_uTotal(R14) \
	CBZ   R9, lbl \
	MOVWU (adaptiveScan_uTotal-adaptiveScan__size)(R14), R16 \
	CMPW  R16, R15 \
	BLS   lbl \
	MOVD  (R14), R16 \
	MOVD  -adaptiveScan__size(R14), R17 \
	MOVD  R17, (R14) \
	MOVD  R16, -adaptiveScan__size(R14) \
lbl: \
	ADD   $1, R9, R9 \
	AND   $15, R9, R9

// func highpassAsm(ah *[numVLCTables]adaptiveHuffman, ah1 *adaptiveHuffman, io *bitIO, coef *int32, scan *[16]adaptiveScan, iQP int32) int
TEXT ·highpassAsm(SB), NOSPLIT, $0-56
	MOVD  ah+0(FP), R0
	MOVD  ah1+8(FP), R5
	MOVD  io+16(FP), R1
	MOVD  coef+24(FP), R3
	MOVD  scan+32(FP), R4
	MOVW  iQP+40(FP), R2
	MOVD  bitIO_acc(R1), R7
	MOVWU bitIO_nb(R1), R8
	MOVD  bitIO_buf(R1), R6
	MOVD  bitIO_off(R1), R14
	ADD   R14, R6, R6
	// runTab[ah[0].tableIndex]; adaptDiscriminant keeps it 0 or 1.
	MOVD  adaptiveHuffman_tableIndex(R0), R14
	AND   $1, R14, R14
	MOVD  $·runTab(SB), R20
	MOVD  $(15*128), R15
	MADD  R14, R20, R15, R20
	ADD   $(14*128), R20, R21
	MOVD  $(19*adaptiveHuffman__size), R14
	ADD   R14, R0, R22
	ADD   $adaptiveHuffman__size, R22, R23
	ADD   $adaptiveHuffman__size, R5, R24
	ADD   $adaptiveHuffman__size, R24, R25
	MOVD  $1, R9
	MOVD  $1, R10

	CMP   $24, R8
	BHS   hfirst
	HPREFILL

hfirst:
	// decodeFirstIndex, short code
	LSR   $59, R7, R14
	MOVH  (R5)(R14<<1), R15
	TBNZ  $63, R15, hfirstslow
	HPFLUSH(R15)
	ASR   $3, R15, R15
	AND   $15, R15, R14
	LSL   $2, R14, R14
	HPDISC2(R5, R14)

hfirstdone:
	// R15 idx
	AND   $1, R15, R14   // iSR
	ASRW  $2, R15, R11   // iSRn
	AND   R11, R14, R12  // iCont
	ASR   $63, R7, R16   // sign
	LSL   $1, R7, R7
	SUB   $1, R8, R8
	EORW  R16, R2, R13
	SUBW  R16, R13, R13  // level
	TBZ   $1, R15, hrun
	CMP   $0, R12
	CSEL  EQ, R22, R23, R19
	LSR   $59, R7, R16
	MOVH  (R19)(R16<<1), R17
	CMPW  $16, R17
	BHS   hlevelslow
	HPFLUSH(R17)
	ASR   $3, R17, R17
	AND   $15, R17, R16
	ADD   $adaptiveHuffman_delta, R19, R15
	MOVW  (R15)(R16<<2), R15
	MOVW  adaptiveHuffman_disc(R19), R16
	ADDW  R15, R16, R16
	MOVW  R16, adaptiveHuffman_disc(R19)
	ADDW  $2, R17, R17
	MULW  R17, R13, R13

hrun:
	TBNZ  $0, R14, hstore

hrunbody:
	// decodeSignificantRun(14)
	HPRUN(R21)
	ADD   R17, R9, R9

hstore:
	AND   $15, R9, R9
	HPSTORE(hstored)

loop:
	CBZW  R11, done
	CMP   $16, R10
	BGT   done
	CMP   $24, R8
	BHS   lrun
	HPREFILL

lrun:
	TBNZ  $0, R11, lindex
	MOVD  $14, R14
	SUB   R9, R14, R14   // m = iMaxRun - 1
	CMP   $4, R14
	BLT   lrunshort
	CMP   $14, R14
	BGE   ret16
	ADD   $1, R14, R14
	ADD   R14<<7, R20, R14
	HPRUN(R14)
	B     lrunadd

lrunshort:
	// m < 0 reads like m = 3
	MOVD  $3, R15
	CMP   $0, R14
	CSEL  LT, R15, R14, R14
	CLZ   R7, R15
	CMP   R14, R15
	CSEL  LT, R15, R14, R15  // k = min(clz, m)
	CINC  LT, R15, R16       // n = k + (k < m)
	LSL   R16, R7, R7
	SUB   R16, R8, R8
	ADD   $1, R15, R17

lrunadd:
	ADDW  R17, R9, R9
	CMPW  $16, R9
	BHS   ret16

lindex:
	CMP   $14, R9
	BHS   lindex14
	CMP   $0, R12
	CSEL  EQ, R24, R25, R19
	LSR   $59, R7, R16
	MOVH  (R19)(R16<<1), R15
	HPFLUSH(R15)
	ASR   $3, R15, R15
	AND   $15, R15, R14
	LSL   $2, R14, R14
	HPDISC2(R19, R14)
	B     lsign

lindex14:
	BHI   lindex15
	TBNZ  $63, R7, lindex14b
	MOVD  $0, R15
	LSL   $1, R7, R7
	SUB   $1, R8, R8
	B     lsign

lindex14b:
	TBNZ  $62, R7, lindex14c
	MOVD  $2, R15
	LSL   $2, R7, R7
	SUB   $2, R8, R8
	B     lsign

lindex14c:
	UBFX  $61, R7, $1, R15
	LSL   $1, R15, R15
	ADD   $1, R15, R15
	LSL   $3, R7, R7
	SUB   $3, R8, R8
	B     lsign

lindex15:
	LSR   $63, R7, R15
	LSL   $1, R7, R7
	SUB   $1, R8, R8

lsign:
	// R15 idx
	ASRW  $1, R15, R11
	AND   R11, R12, R12
	ASR   $63, R7, R16
	LSL   $1, R7, R7
	SUB   $1, R8, R8
	EORW  R16, R2, R13
	SUBW  R16, R13, R13
	TBZ   $0, R15, lstore
	CMP   $0, R12
	CSEL  EQ, R22, R23, R19
	LSR   $59, R7, R16
	MOVH  (R19)(R16<<1), R17
	CMPW  $16, R17
	BHS   llevelslow
	HPFLUSH(R17)
	ASR   $3, R17, R17
	AND   $15, R17, R16
	ADD   $adaptiveHuffman_delta, R19, R15
	MOVW  (R15)(R16<<2), R15
	MOVW  adaptiveHuffman_disc(R19), R16
	ADDW  R15, R16, R16
	MOVW  R16, adaptiveHuffman_disc(R19)
	ADDW  $2, R17, R17
	MULW  R17, R13, R13

lstore:
	HPSTORE(lstored)
	ADD   $1, R10, R10
	B     loop

ret16:
	MOVD  $16, R10

done:
	MOVD  R7, bitIO_acc(R1)
	MOVW  R8, bitIO_nb(R1)
	MOVD  bitIO_buf(R1), R14
	SUB   R14, R6, R6
	MOVD  R6, bitIO_off(R1)
	MOVD  R10, ret+48(FP)
	RET

	// Slow paths. R26 picks the return site: 0 first index, 2 loop
	// level, 1 or 3 head level with iSR 0 or 1.
hfirstslow:
	MOVD  R5, R14
	MOVD  $0, R26
	B     gethuff

hlevelslow:
	LSL   $1, R14, R26
	ADD   $1, R26, R26
	MOVD  R19, R14
	B     abslevel

llevelslow:
	MOVD  R19, R14
	MOVD  $2, R26

abslevel:
	// decodeSignificantAbsLevel(a = R14); level *= result
	B     gethuff

gethuffdone:
	// R15 = getHuff(R14), bitIO state current
	CBZ   R26, firstindexdone
	CMPW  $6, R15
	BLS   abs0
	MOVD  $0, R15
	B     abslevelmul

abs0:
	ADD   $adaptiveHuffman_delta, R14, R16
	MOVW  (R16)(R15<<2), R17
	MOVW  adaptiveHuffman_disc(R14), R19
	ADDW  R17, R19, R19
	MOVW  R19, adaptiveHuffman_disc(R14)
	CMP   $2, R15
	BHS   abs2
	ADD   $2, R15, R15
	B     abslevelmul

abs2:
	CMP   $6, R15
	BHS   abs6
	MOVD  $·absLevelRemap(SB), R16
	MOVW  (R16)(R15<<2), R19
	MOVD  $·absLevelFixedLength(SB), R16
	MOVWU (R16)(R15<<2), R15
	// R19 += getBit16(R15)
	CBZ   R15, abs2done
	CMP   R15, R8
	BHS   abs2nofill
	HPREFILL

abs2nofill:
	MOVD  $64, R16
	SUB   R15, R16, R16
	LSR   R16, R7, R16
	ADDW  R16, R19, R19
	LSL   R15, R7, R7
	SUB   R15, R8, R8

abs2done:
	MOVD  R19, R15
	B     abslevelmul

abs6:
	// fixed = getBit16(4) + 4
	CMP   $4, R8
	BHS   abs6a
	HPREFILL

abs6a:
	LSR   $60, R7, R15
	LSL   $4, R7, R7
	SUB   $4, R8, R8
	ADD   $4, R15, R15
	CMP   $19, R15
	BNE   abs6fixed
	CMP   $2, R8
	BHS   abs6b
	HPREFILL

abs6b:
	LSR   $62, R7, R16
	LSL   $2, R7, R7
	SUB   $2, R8, R8
	ADD   R16, R15, R15
	CMP   $22, R15
	BNE   abs6fixed
	CMP   $3, R8
	BHS   abs6c
	HPREFILL

abs6c:
	LSR   $61, R7, R16
	LSL   $3, R7, R7
	SUB   $3, R8, R8
	ADD   R16, R15, R15

abs6fixed:
	// level = 2 + 1<<fixed + getBit32(fixed), fixed >= 4
	CMP   R15, R8
	BHS   abs6d
	HPREFILL

abs6d:
	MOVD  $1, R19
	LSL   R15, R19, R19
	ADD   $2, R19, R19
	MOVD  $64, R16
	SUB   R15, R16, R16
	LSR   R16, R7, R16
	ADDW  R16, R19, R19
	LSL   R15, R7, R7
	SUB   R15, R8, R8
	MOVD  R19, R15

abslevelmul:
	MULW  R15, R13, R13
	CMP   $2, R26
	BEQ   lstore
	CMP   $8, R8
	BHS   abslevelhead
	HPREFILL

abslevelhead:
	TBNZ  $1, R26, hstore
	B     hrunbody

firstindexdone:
	// Go bounds-checks idx; tables keep it below 12.
	AND   $15, R15, R19
	LSL   $2, R19, R19
	ADD   $adaptiveHuffman_delta, R5, R16
	MOVW  (R16)(R19), R17
	MOVW  adaptiveHuffman_disc(R5), R14
	ADDW  R17, R14, R14
	MOVW  R14, adaptiveHuffman_disc(R5)
	ADD   $adaptiveHuffman_delta1, R5, R16
	MOVW  (R16)(R19), R17
	MOVW  adaptiveHuffman_disc1(R5), R14
	ADDW  R17, R14, R14
	MOVW  R14, adaptiveHuffman_disc1(R5)
	CMP   $16, R8
	BHS   hfirstdone
	HPREFILL
	B     hfirstdone

gethuff:
	// R15 = getHuff(R14.hufDecTable)
	CMP   $5, R8
	BHS   gethuff0
	HPREFILL

gethuff0:
	LSR   $59, R7, R16
	MOVH  (R14)(R16<<1), R15
	TBNZ  $63, R15, gethufflong
	HPFLUSH(R15)
	ASR   $3, R15, R15
	B     gethuffdone

gethufflong:
	LSL   $5, R7, R7
	SUB   $5, R8, R8

gethuffwalk:
	CBNZ  R8, gethuffbit
	HPREFILL

gethuffbit:
	LSR   $63, R7, R16
	LSL   $1, R7, R7
	SUB   $1, R8, R8
	ADD   R16, R15, R15
	ADD   $32768, R15, R15
	CMP   $const_hufTableLen, R15
	BHS   gethuffbad
	MOVH  (R14)(R15<<1), R15
	TBNZ  $63, R15, gethuffwalk
	B     gethuffdone

gethuffbad:
	MOVD  $0, R15
	B     gethuffdone
