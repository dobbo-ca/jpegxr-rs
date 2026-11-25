// Ported from jxrlib strdec.c outputMBRow and helpers (BSD-2, see NOTICE).

use super::simd;
use super::*;

fn clip8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn clip_u16(v: i32) -> u16 {
    v.clamp(0, 65535) as u16
}

/// `_CLIP16`: clamp to a signed 16-bit range.
fn clip16(v: i32) -> i16 {
    v.clamp(-32768, 32767) as i16
}

/// `_CLIP2`: clamp `v` to `[lo, hi]`.
fn clip2(lo: i32, v: i32, hi: i32) -> i32 {
    v.clamp(lo, hi)
}

// Go's shifts: a count of the type's width or more shifts everything out.
fn shl32(x: i32, n: u32) -> i32 {
    x.checked_shl(n).unwrap_or(0)
}

fn shr32(x: i32, n: u32) -> i32 {
    x.checked_shr(n).unwrap_or(x >> 31)
}

fn shr_u32(x: u32, n: u32) -> u32 {
    x.checked_shr(n).unwrap_or(0)
}

fn put16(out: &mut [u8], d: usize, v: u16) {
    out[d..d + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut [u8], d: usize, v: u32) {
    out[d..d + 4].copy_from_slice(&v.to_le_bytes());
}

/// `_ICC`: inverse colour conversion to r, g, b.
fn icc(y: i32, u: i32, v: i32) -> (i32, i32, i32) {
    let mut g = y;
    let mut r = u.wrapping_neg();
    let mut b = v;
    g = g.wrapping_sub(r >> 1);
    r = r.wrapping_sub((b.wrapping_add(1) >> 1).wrapping_sub(g));
    b = b.wrapping_add(r);
    (r, g, b)
}

/// `_ICC_CMYK`: inverse colour conversion, strdec.c:405.
fn icc_cmyk(mut c: i32, mut m: i32, mut y: i32, mut k: i32) -> (i32, i32, i32, i32) {
    k = k.wrapping_sub(m.wrapping_add(1) >> 1);
    m = m.wrapping_sub((c >> 1).wrapping_sub(k));
    c = c.wrapping_sub((y.wrapping_add(1) >> 1).wrapping_sub(m));
    y = y.wrapping_add(c);
    (c, m, y, k)
}

/// Converts an internal sample to Half-16, strdec.c:504-510.
fn backward_half(h: i32) -> u16 {
    let s = h >> 31;
    (((h & 0x7fff) ^ s) - s) as u16
}

/// `pixel2float`, strdec.c:464-501.
fn pixel2float(h: i32, exp_bias: i8, lm: u8) -> f32 {
    let lm = u32::from(lm);
    let lmshift = shl32(1, lm);
    let s = h >> 31;
    let h = (h ^ s).wrapping_sub(s); // abs
    let mut e = shr_u32(h as u32, lm) as i32;
    let mut m = (h & (lmshift - 1)) | lmshift;
    if e == 0 {
        m ^= lmshift;
        e = 1;
    }
    e = e.wrapping_add(127 - i32::from(exp_bias));
    while m < lmshift && e > 1 && m > 0 {
        e -= 1;
        m <<= 1;
    }
    if m < lmshift {
        e = 0;
    } else {
        m ^= lmshift;
    }
    // Go panics on a negative count here; that needs a mantissa length over 23, which only a
    // 32-bit float image can carry, and the public API decodes none.
    m = shl32(m, (23 - lm as i32) as u32);
    let bits = ((s as u32) & 0x8000_0000) | ((e as u32) << 23) | (m as u32);
    f32::from_bits(bits)
}

/// `inverseConvert`, strdec.c:415-430.
fn inverse_convert(f: i32) -> (u8, u8) {
    if f <= 0 {
        (0, 0)
    } else if (f >> 7) > 1 {
        (((f & 0x7f) as u8) | 0x80, (f >> 7) as u8)
    } else {
        (f as u8, 1)
    }
}

/// `inverseConvertRGBE`, strdec.c:436-460.
fn inverse_convert_rgbe(fr: i32, fg: i32, fb: i32) -> (u8, u8, u8, u8) {
    let (mut r, re) = inverse_convert(fr);
    let (mut g, ge) = inverse_convert(fg);
    let (mut b, be) = inverse_convert(fb);
    let mut e = re;
    if ge > e {
        e = ge;
    }
    if be > e {
        e = be;
    }
    if e > re {
        r = shr32(i32::from(r) * 2 + 1, u32::from(e - re) + 1) as u8;
    }
    if e > ge {
        g = shr32(i32::from(g) * 2 + 1, u32::from(e - ge) + 1) as u8;
    }
    if e > be {
        b = shr32(i32::from(b) * 2 + 1, u32::from(e - be) + 1) as u8;
    }
    (r, g, b, e)
}

/// Pixel c of an MB row sits at `IDX_CC[row][0] + GRP_OFF[c]`.
pub(super) const GRP_OFF: [usize; 16] = [
    0, 1, 5, 4, 64, 65, 69, 68, 128, 129, 133, 132, 192, 193, 197, 196,
];

/// Splits columns `[c, e)` at whole macroblocks `[c0, c0+16n)`.
fn full_mbs(c: usize, e: usize) -> (usize, usize) {
    let c0 = (c + 15) & !15;
    if c0 >= e {
        return (e, 0);
    }
    (c0, (e - c0) >> 4)
}

/// The offset in a macroblock row of pixel `col` of row `row`.
fn px(row: usize, col: usize) -> usize {
    ((col >> 4) << 8) + usize::from(IDX_CC[row][col & 15])
}

/// Writes `n` MBs of one row from `s`, 3 bytes a pixel at `d`.
fn out_bgr8(
    out: &mut [u8],
    d: usize,
    yuv: (&[i32], &[i32], &[i32]),
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    i_rb: (usize, usize),
) {
    simd::out_bgr8(simd::tier(), out, d, yuv, s, n, bias, shift, i_rb);
}

/// The scalar reference of `out_bgr8`.
pub(super) fn out_bgr8_scalar(
    out: &mut [u8],
    d: usize,
    (y, u, v): (&[i32], &[i32], &[i32]),
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    (i_r, i_b): (usize, usize),
) {
    for m in 0..n {
        for c in 0..16 {
            let idx = s + 256 * m + GRP_OFF[c];
            let (r, g, b) = icc(y[idx].wrapping_add(bias), u[idx], v[idx]);
            let o = d + 48 * m + 3 * c;
            out[o + i_r] = clip8(shr32(r, shift));
            out[o + 1] = clip8(shr32(g, shift));
            out[o + i_b] = clip8(shr32(b, shift));
        }
    }
}

/// Writes `n` MBs of one row from `s`, 2 bytes a pixel at `d`.
fn out_gray16(
    out: &mut [u8],
    d: usize,
    y: &[i32],
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    n_len: u32,
) {
    simd::out_gray16(simd::tier(), out, d, y, s, n, bias, shift, n_len);
}

/// The scalar reference of `out_gray16`.
pub(super) fn out_gray16_scalar(
    out: &mut [u8],
    d: usize,
    y: &[i32],
    s: usize,
    n: usize,
    bias: i32,
    shift: u32,
    n_len: u32,
) {
    for m in 0..n {
        for c in 0..16 {
            let p = shl32(
                shr32(y[s + 256 * m + GRP_OFF[c]].wrapping_add(bias), shift),
                n_len,
            );
            put16(out, d + 32 * m + 2 * c, clip_u16(p));
        }
    }
}

/// Writes `n` MBs of one row from `s`, 1 byte a pixel at `d`.
fn out_gray8(out: &mut [u8], d: usize, y: &[i32], s: usize, n: usize, bias: i32, shift: u32) {
    for m in 0..n {
        for c in 0..16 {
            out[d + 16 * m + c] =
                shr32(y[s + 256 * m + GRP_OFF[c]].wrapping_add(bias), shift).clamp(0, 255) as u8;
        }
    }
}

impl StrCodec {
    fn interpolate_uv(&mut self) {
        let cf_ext = self.wmii.cf_color_format;
        let c_width = self.cmb_width * 16;
        let src_u = &self.mb_buf[self.a0_off[1]..];
        let src_v = &self.mb_buf[self.a0_off[2]..];
        let dst_u = &mut self.p_res_u;
        let dst_v = &mut self.p_res_v;

        if self.param.cf_color_format == YUV422 {
            for row in 0..16 {
                let mut idx_d = 0usize;
                let mut col = 0usize;
                while col < c_width {
                    let idx_s = ((col >> 4) << 7) + usize::from(IDX_CC[row][(col >> 1) & 7]);
                    idx_d = ((col >> 4) << 8) + usize::from(IDX_CC[row][col & 15]);
                    dst_u[idx_d] = src_u[idx_s];
                    dst_v[idx_d] = src_v[idx_s];
                    if col > 0 {
                        let l = col - 2;
                        let idx_l = ((l >> 4) << 8) + usize::from(IDX_CC[row][l & 15]);
                        let c = col - 1;
                        let idx_c = ((c >> 4) << 8) + usize::from(IDX_CC[row][c & 15]);
                        dst_u[idx_c] = dst_u[idx_l].wrapping_add(dst_u[idx_d]).wrapping_add(1) >> 1;
                        dst_v[idx_c] = dst_v[idx_l].wrapping_add(dst_v[idx_d]).wrapping_add(1) >> 1;
                    }
                    col += 2;
                }
                let idx_s = (((col - 1) >> 4) << 8) + usize::from(IDX_CC[row][(col - 1) & 15]);
                dst_u[idx_s] = dst_u[idx_d];
                dst_v[idx_s] = dst_v[idx_d];
            }
            return;
        }

        let c_shift: usize = if cf_ext == YUV422 { 3 } else { 4 };
        let next_u = &self.mb_buf[self.a1_off[1]..];
        let next_v = &self.mb_buf[self.a1_off[2]..];
        let mut col = 0usize;
        while col < c_width {
            let c_mb = (col >> 4) << (4 + c_shift);
            let c_pix = (col >> (4 - c_shift)) & ((1 << c_shift) - 1);
            let mut idx_d = 0usize;
            let mut row = 0usize;
            while row < 16 {
                let idx_s = ((col >> 4) << 6) + usize::from(IDX_CC420[row >> 1][(col >> 1) & 7]);
                idx_d = c_mb + usize::from(IDX_CC[row][c_pix]);
                dst_u[idx_d] = src_u[idx_s];
                dst_v[idx_d] = src_v[idx_s];
                if row > 0 {
                    let idx_t = c_mb + usize::from(IDX_CC[row - 2][c_pix]);
                    let idx_c = c_mb + usize::from(IDX_CC[row - 1][c_pix]);
                    dst_u[idx_c] = dst_u[idx_t].wrapping_add(dst_u[idx_d]).wrapping_add(1) >> 1;
                    dst_v[idx_c] = dst_v[idx_t].wrapping_add(dst_v[idx_d]).wrapping_add(1) >> 1;
                }
                row += 2;
            }
            let idx_s = c_mb + usize::from(IDX_CC[15][c_pix]);
            if self.c_row == self.cmb_height {
                dst_u[idx_s] = dst_u[idx_d];
                dst_v[idx_s] = dst_v[idx_d];
            } else {
                let idx_b = ((col >> 4) << 6) + usize::from(IDX_CC420[0][(col >> 1) & 7]);
                dst_u[idx_s] = next_u[idx_b].wrapping_add(dst_u[idx_d]).wrapping_add(1) >> 1;
                dst_v[idx_s] = next_v[idx_b].wrapping_add(dst_v[idx_d]).wrapping_add(1) >> 1;
            }
            col += 2;
        }

        if cf_ext != YUV422 {
            for row in 0..16 {
                let mut idx_s = 0usize;
                let mut col = 1usize;
                while col + 2 < c_width {
                    let idx_l = (((col - 1) >> 4) << 8) + usize::from(IDX_CC[row][(col - 1) & 15]);
                    let idx_d = ((col >> 4) << 8) + usize::from(IDX_CC[row][col & 15]);
                    idx_s = (((col + 1) >> 4) << 8) + usize::from(IDX_CC[row][(col + 1) & 15]);
                    dst_u[idx_d] = dst_u[idx_s].wrapping_add(dst_u[idx_l]).wrapping_add(1) >> 1;
                    dst_v[idx_d] = dst_v[idx_s].wrapping_add(dst_v[idx_l]).wrapping_add(1) >> 1;
                    col += 2;
                }
                let idx_d =
                    (((c_width - 1) >> 4) << 8) + usize::from(IDX_CC[row][(c_width - 1) & 15]);
                dst_u[idx_d] = dst_u[idx_s];
                dst_v[idx_d] = dst_v[idx_s];
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn output_n_channel(
        &self,
        out: &mut [u8],
        (i_first_row, i_first_column, c_width, c_height): (usize, usize, usize, usize),
        i_shift: u32,
        i_bias: i32,
    ) {
        let mut c_channel = self.wmiscp.c_channel;
        if self.wmii.cf_color_format == Y_ONLY {
            c_channel = 1;
        }
        let n_len = u32::from(self.wmiscp.n_len_mantissa_or_shift);
        let n_exp_bias = self.wmiscp.n_exp_bias;
        // On the stack: a row is no reason to call the allocator.
        let mut ch: [&[i32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
        for i in 0..c_channel {
            ch[i] = &self.mb_buf[self.a0_off[i]..];
        }
        if self.uv_resolution_change {
            ch[1] = &self.p_res_u;
            ch[2] = &self.p_res_v;
        }
        let off_x = &self.dp.p_offset_x;
        let off_y = &self.dp.p_offset_y[(self.c_row - 1) * 16..];
        // A codestream with fewer channels than the pixel format has leaves the rest of each
        // pixel unwritten in jxrlib and Go; zero it so the output never depends on `out`'s old
        // bytes (the same as Go whenever `out` starts zeroed).
        let cb = match self.wmii.bd_bit_depth {
            BD8 => 1,
            BD16 | BD16S | BD16F => 2,
            _ => 4,
        };
        let written = cb * c_channel;
        let pixel = self.wmii.c_bits_per_unit >> 3;
        // Only an N_COMPONENT output: a Y_ONLY decode of a planar alpha stream writes into the
        // middle of pixels that hold the primary's samples, which must stay. Not for one plane
        // either (`out` has one sample a pixel).
        if written < pixel && self.wmii.cf_color_format == N_COMPONENT && self.plane == 0 {
            for row in i_first_row..c_height {
                for col in i_first_column..c_width {
                    let d = cb * (off_y[row] + off_x[col]);
                    out[d + written..d + pixel].fill(0);
                }
            }
        }

        match self.wmii.bd_bit_depth {
            BD8 => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    let (mut c0, mut n) = (c_width, 0);
                    if c_channel == 1 && self.wmii.c_bits_per_unit == 8 {
                        (c0, n) = full_mbs(i_first_column, c_width);
                    }
                    if n > 0 {
                        out_gray8(
                            out,
                            i_y + off_x[c0],
                            ch[0],
                            ((c0 >> 4) << 8) + usize::from(IDX_CC[row][0]),
                            n,
                            i_bias,
                            i_shift,
                        );
                    }
                    for (c_start, c_end) in [(i_first_column, c0), (c0 + 16 * n, c_width)] {
                        for col in c_start..c_end {
                            let d = i_y + off_x[col];
                            let idx = px(row, col);
                            for c in 0..c_channel {
                                out[d + c] = clip8(shr32(ch[c][idx].wrapping_add(i_bias), i_shift));
                            }
                        }
                    }
                }
            }
            BD16 => {
                let chs: &[&[i32]] = if self.plane > 0 {
                    &ch[self.plane - 1..self.plane]
                } else {
                    &ch[..c_channel]
                };
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    let (mut c0, mut n) = (c_width, 0);
                    if chs.len() == 1 && (self.plane > 0 || self.wmii.c_bits_per_unit == 16) {
                        (c0, n) = full_mbs(i_first_column, c_width);
                    }
                    if n > 0 {
                        out_gray16(
                            out,
                            2 * (i_y + off_x[c0]),
                            chs[0],
                            ((c0 >> 4) << 8) + usize::from(IDX_CC[row][0]),
                            n,
                            i_bias,
                            i_shift,
                            n_len,
                        );
                    }
                    for (c_start, c_end) in [(i_first_column, c0), (c0 + 16 * n, c_width)] {
                        for col in c_start..c_end {
                            let d = 2 * (i_y + off_x[col]);
                            let idx = px(row, col);
                            for (c, v) in chs.iter().enumerate() {
                                let p = shl32(shr32(v[idx].wrapping_add(i_bias), i_shift), n_len);
                                put16(out, d + 2 * c, clip_u16(p));
                            }
                        }
                    }
                }
            }
            BD16S => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let d = 2 * (i_y + off_x[col]);
                        let idx = px(row, col);
                        for c in 0..c_channel {
                            let p = shl32(shr32(ch[c][idx].wrapping_add(i_bias), i_shift), n_len);
                            put16(out, d + 2 * c, clip16(p) as u16);
                        }
                    }
                }
            }
            BD16F => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let d = 2 * (i_y + off_x[col]);
                        let idx = px(row, col);
                        for c in 0..c_channel {
                            let p = shr32(ch[c][idx].wrapping_add(i_bias), i_shift);
                            put16(out, d + 2 * c, backward_half(p));
                        }
                    }
                }
            }
            BD32S => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let d = 4 * (i_y + off_x[col]);
                        let idx = px(row, col);
                        for c in 0..c_channel {
                            let p = shl32(shr32(ch[c][idx].wrapping_add(i_bias), i_shift), n_len);
                            put32(out, d + 4 * c, p as u32);
                        }
                    }
                }
            }
            BD32F => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let d = 4 * (i_y + off_x[col]);
                        let idx = px(row, col);
                        for c in 0..c_channel {
                            let p = shr32(ch[c][idx].wrapping_add(i_bias), i_shift);
                            put32(
                                out,
                                d + 4 * c,
                                pixel2float(p, n_exp_bias, n_len as u8).to_bits(),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn output_mb_row(&mut self, out: &mut [u8]) -> Result<(), Error> {
        let mut cf_ext = self.wmii.cf_color_format;
        if self.param.cf_color_format == Y_ONLY {
            cf_ext = Y_ONLY;
        }
        let bd = self.wmii.bd_bit_depth;
        let mut i_shift = 0u32;
        if self.param.scaled_arith {
            i_shift = (SHIFT_ZERO + QP_FRAC_BITS) as u32;
        }
        let row0 = ((self.c_row - 1) * 16) as isize;
        let mut c_height = self.dp.c_roi_bottom_y + 1 - row0;
        if c_height > 16 {
            c_height = 16;
        }
        let c_height = c_height.max(0) as usize;
        let c_width = (self.dp.c_roi_right_x + 1) as usize;
        let mut i_first_row = 0usize;
        if row0 <= self.dp.c_roi_top_y {
            i_first_row = (self.dp.c_roi_top_y & 0xf) as usize;
        }
        let i_first_column = self.dp.c_roi_left_x as usize;
        let n_len = u32::from(self.wmiscp.n_len_mantissa_or_shift);
        let n_exp_bias = self.wmiscp.n_exp_bias;

        if self.uv_resolution_change {
            self.interpolate_uv();
        }
        let p_y = &self.mb_buf[self.a0_off[0]..];
        let mut p_u: &[i32] = &[];
        let mut p_v: &[i32] = &[];
        if self.param.c_num_channels >= 3 {
            p_u = &self.mb_buf[self.a0_off[1]..];
            p_v = &self.mb_buf[self.a0_off[2]..];
        }
        if self.uv_resolution_change {
            p_u = &self.p_res_u;
            p_v = &self.p_res_v;
        }
        let (i_b, i_r) = if self.wmii.b_rgb {
            (2usize, 0usize)
        } else {
            (0, 2)
        };
        let off_x = &self.dp.p_offset_x;
        let off_y = &self.dp.p_offset_y[(self.c_row - 1) * 16..];
        let dims = (i_first_row, i_first_column, c_width, c_height);
        let scaled = self.param.scaled_arith;
        let bias_scaled: i32 = (1 << (SHIFT_ZERO + QP_FRAC_BITS - 1)) - 1;

        match bd {
            BD8 => {
                let i_bias1 = 128i32 << i_shift;
                let i_bias2 = if scaled { bias_scaled } else { 0 };
                let i_bias = i_bias1.wrapping_add(i_bias2);
                match cf_ext {
                    CF_RGB => match self.alpha.as_ref() {
                        None => {
                            for row in i_first_row..c_height {
                                let i_y = off_y[row];
                                let (mut c0, mut n) = (c_width, 0);
                                if self.wmii.c_bits_per_unit == 24 {
                                    (c0, n) = full_mbs(i_first_column, c_width);
                                }
                                if n > 0 {
                                    out_bgr8(
                                        out,
                                        off_x[c0] + i_y,
                                        (p_y, p_u, p_v),
                                        ((c0 >> 4) << 8) + usize::from(IDX_CC[row][0]),
                                        n,
                                        i_bias,
                                        i_shift,
                                        (i_r, i_b),
                                    );
                                }
                                for (c_start, c_end) in
                                    [(i_first_column, c0), (c0 + 16 * n, c_width)]
                                {
                                    for col in c_start..c_end {
                                        let idx = px(row, col);
                                        let (r, g, b) =
                                            icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                        let (r, g, b) = (
                                            shr32(r, i_shift),
                                            shr32(g, i_shift),
                                            shr32(b, i_shift),
                                        );
                                        let d = off_x[col] + i_y;
                                        out[d + i_r] = clip8(r);
                                        out[d + 1] = clip8(g);
                                        out[d + i_b] = clip8(b);
                                    }
                                }
                            }
                        }
                        Some(a) => {
                            // BD8 RGB inline alpha (strdec.c:993-1036): the alpha channel
                            // writes out[d+3], sharing the RGB bias/shift.
                            let p_a = &a.sc.mb_buf[a.sc.a0_off[0]..];
                            for row in i_first_row..c_height {
                                let i_y = off_y[row];
                                for col in i_first_column..c_width {
                                    let idx = px(row, col);
                                    let (r, g, b) =
                                        icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                    let a = p_a[idx].wrapping_add(i_bias);
                                    let (r, g, b, a) = (
                                        shr32(r, i_shift),
                                        shr32(g, i_shift),
                                        shr32(b, i_shift),
                                        shr32(a, i_shift),
                                    );
                                    let d = off_x[col] + i_y;
                                    out[d + i_r] = clip8(r);
                                    out[d + 1] = clip8(g);
                                    out[d + i_b] = clip8(b);
                                    out[d + 3] = clip8(a);
                                }
                            }
                        }
                    },
                    CF_RGBE => {
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (r, g, b) =
                                    icc(p_y[idx].wrapping_add(i_bias2), p_u[idx], p_v[idx]);
                                let d = off_x[col] + i_y;
                                let (r, g, b, e) = inverse_convert_rgbe(
                                    shr32(r, i_shift),
                                    shr32(g, i_shift),
                                    shr32(b, i_shift),
                                );
                                out[d] = r;
                                out[d + 1] = g;
                                out[d + 2] = b;
                                out[d + 3] = e;
                            }
                        }
                    }
                    CMYK => {
                        let p_k = &self.mb_buf[self.a0_off[3]..];
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (m, c, y, k) = (
                                    p_y[idx].wrapping_neg().wrapping_add(i_bias1),
                                    p_u[idx],
                                    p_v[idx].wrapping_neg(),
                                    p_k[idx].wrapping_add(i_bias2),
                                );
                                let (c, m, y, k) = icc_cmyk(c, m, y, k);
                                let d = off_x[col] + i_y;
                                out[d] = clip8(shr32(c, i_shift));
                                out[d + 1] = clip8(shr32(m, i_shift));
                                out[d + 2] = clip8(shr32(y, i_shift));
                                out[d + 3] = clip8(shr32(k, i_shift));
                            }
                        }
                    }
                    Y_ONLY | YUV444 | N_COMPONENT => {
                        self.output_n_channel(out, dims, i_shift, i_bias)
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
            BD16 => {
                let mut i_bias = shl32(shr32(1 << 15, n_len), i_shift);
                if i_shift != 0 {
                    i_bias = i_bias.wrapping_add(1 << (i_shift - 1));
                }
                match cf_ext {
                    CF_RGB => {
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (r, g, b) =
                                    icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                let (r, g, b) = (
                                    shl32(shr32(r, i_shift), n_len),
                                    shl32(shr32(g, i_shift), n_len),
                                    shl32(shr32(b, i_shift), n_len),
                                );
                                let d = 2 * (off_x[col] + i_y);
                                put16(out, d, clip_u16(r));
                                put16(out, d + 2, clip_u16(g));
                                put16(out, d + 4, clip_u16(b));
                            }
                        }
                    }
                    CMYK => {
                        let i_bias1 = shl32(shr32(32768, n_len), i_shift);
                        let i_bias2 = i_bias.wrapping_sub(i_bias1);
                        let p_k = &self.mb_buf[self.a0_off[3]..];
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (m, c, y, k) = (
                                    p_y[idx].wrapping_neg().wrapping_add(i_bias1),
                                    p_u[idx],
                                    p_v[idx].wrapping_neg(),
                                    p_k[idx].wrapping_add(i_bias2),
                                );
                                let (c, m, y, k) = icc_cmyk(c, m, y, k);
                                let f = |v: i32| shl32(shr32(v, i_shift), n_len);
                                let d = 2 * (off_x[col] + i_y);
                                put16(out, d, clip_u16(f(c)));
                                put16(out, d + 2, clip_u16(f(m)));
                                put16(out, d + 4, clip_u16(f(y)));
                                put16(out, d + 6, clip_u16(f(k)));
                            }
                        }
                    }
                    Y_ONLY | YUV444 | N_COMPONENT => {
                        self.output_n_channel(out, dims, i_shift, i_bias)
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
            BD16S => {
                let i_bias = if scaled { bias_scaled } else { 0 };
                match cf_ext {
                    CF_RGB => {
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (r, g, b) =
                                    icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                let f = |v: i32| shl32(shr32(v, i_shift), n_len);
                                let d = 2 * (off_x[col] + i_y);
                                put16(out, d, clip16(f(r)) as u16);
                                put16(out, d + 2, clip16(f(g)) as u16);
                                put16(out, d + 4, clip16(f(b)) as u16);
                            }
                        }
                    }
                    CMYK => {
                        let p_k = &self.mb_buf[self.a0_off[3]..];
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (m, c, y, k) = (
                                    p_y[idx].wrapping_neg(),
                                    p_u[idx],
                                    p_v[idx].wrapping_neg(),
                                    p_k[idx].wrapping_add(i_bias),
                                );
                                let (c, m, y, k) = icc_cmyk(c, m, y, k);
                                let f = |v: i32| shl32(shr32(v, i_shift), n_len);
                                let d = 2 * (off_x[col] + i_y);
                                put16(out, d, f(c) as i16 as u16);
                                put16(out, d + 2, f(m) as i16 as u16);
                                put16(out, d + 4, f(y) as i16 as u16);
                                put16(out, d + 6, f(k) as i16 as u16);
                            }
                        }
                    }
                    Y_ONLY | YUV444 | N_COMPONENT => {
                        self.output_n_channel(out, dims, i_shift, i_bias)
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
            BD16F => {
                let i_bias = if scaled { bias_scaled } else { 0 };
                match cf_ext {
                    CF_RGB => {
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (r, g, b) =
                                    icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                let d = 2 * (off_x[col] + i_y);
                                put16(out, d, backward_half(shr32(r, i_shift)));
                                put16(out, d + 2, backward_half(shr32(g, i_shift)));
                                put16(out, d + 4, backward_half(shr32(b, i_shift)));
                            }
                        }
                    }
                    Y_ONLY | YUV444 | N_COMPONENT => {
                        self.output_n_channel(out, dims, i_shift, i_bias)
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
            BD32S => {
                let i_bias = if scaled { bias_scaled } else { 0 };
                match cf_ext {
                    CF_RGB => {
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (r, g, b) =
                                    icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                let d = 4 * (off_x[col] + i_y);
                                put32(out, d, shl32(shr32(r, i_shift), n_len) as u32);
                                put32(out, d + 4, shl32(shr32(g, i_shift), n_len) as u32);
                                put32(out, d + 8, shl32(shr32(b, i_shift), n_len) as u32);
                            }
                        }
                    }
                    Y_ONLY | YUV444 | N_COMPONENT => {
                        self.output_n_channel(out, dims, i_shift, i_bias)
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
            BD32F => {
                let i_bias = if scaled { bias_scaled } else { 0 };
                match cf_ext {
                    CF_RGB => {
                        for row in i_first_row..c_height {
                            let i_y = off_y[row];
                            for col in i_first_column..c_width {
                                let idx = px(row, col);
                                let (r, g, b) =
                                    icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                                let d = 4 * (off_x[col] + i_y);
                                let f = |v: i32| {
                                    pixel2float(shr32(v, i_shift), n_exp_bias, n_len as u8)
                                        .to_bits()
                                };
                                put32(out, d, f(r));
                                put32(out, d + 4, f(g));
                                put32(out, d + 8, f(b));
                            }
                        }
                    }
                    Y_ONLY | YUV444 | N_COMPONENT => {
                        self.output_n_channel(out, dims, i_shift, i_bias)
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
            BD5 => {
                let mut i_bias = 16i32 << i_shift;
                if scaled {
                    i_bias = i_bias.wrapping_add(bias_scaled);
                }
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let idx = px(row, col);
                        let (r, g, b) = icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                        let (r, g, b) = (shr32(r, i_shift), shr32(g, i_shift), shr32(b, i_shift));
                        let d = 2 * (off_x[col] + i_y);
                        let c = |v: i32| clip2(0, v, 31) as u16;
                        let v = if self.param.rb_swapped {
                            c(b) | (c(g) << 5) | (c(r) << 10)
                        } else {
                            c(r) | (c(g) << 5) | (c(b) << 10)
                        };
                        put16(out, d, v);
                    }
                }
            }
            BD565 => {
                let mut i_bias = 32i32 << i_shift;
                if scaled {
                    i_bias = i_bias.wrapping_add(bias_scaled);
                }
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let idx = px(row, col);
                        let (r, g, b) = icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                        let (g, b, r) = (
                            shr32(g, i_shift),
                            shr32(b, i_shift + 1),
                            shr32(r, i_shift + 1),
                        );
                        let d = 2 * (off_x[col] + i_y);
                        let c = |v: i32, hi: i32| clip2(0, v, hi) as u16;
                        let v = if self.param.rb_swapped {
                            c(b, 31) | (c(g, 63) << 5) | (c(r, 31) << 11)
                        } else {
                            c(r, 31) | (c(g, 63) << 5) | (c(b, 31) << 11)
                        };
                        put16(out, d, v);
                    }
                }
            }
            BD10 => {
                let mut i_bias = 512i32 << i_shift;
                if scaled {
                    i_bias = i_bias.wrapping_add(bias_scaled);
                }
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let idx = px(row, col);
                        let (r, g, b) = icc(p_y[idx].wrapping_add(i_bias), p_u[idx], p_v[idx]);
                        let (r, g, b) = (shr32(r, i_shift), shr32(g, i_shift), shr32(b, i_shift));
                        let d = 4 * (off_x[col] + i_y);
                        let c = |v: i32| clip2(0, v, 1023) as u32;
                        let v = if self.param.rb_swapped {
                            c(b) | (c(g) << 10) | (c(r) << 20)
                        } else {
                            c(r) | (c(g) << 10) | (c(b) << 20)
                        };
                        put32(out, d, v);
                    }
                }
            }
            BD1 => {
                let mut i_th = 1i32;
                if i_shift > 0 {
                    i_th = 1 << (i_shift - 1);
                }
                let bw = i32::from(self.wmiscp.b_black_white);
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let idx = px(row, col);
                        let byte_idx = (off_x[col] >> 3) + i_y;
                        let c_byte = out[byte_idx];
                        let shift = (7 - (off_x[col] & 7)) as u32;
                        let bit = i32::from(p_y[idx] >= i_th);
                        out[byte_idx] ^=
                            (((bw + bit + i32::from(c_byte >> shift)) & 1) as u8) << shift;
                    }
                }
            }
            _ => return Err(Error::Unsupported),
        }

        // REENTRANT_MODE fixup: grey to RGB.
        if self.wmii.cf_color_format == CF_RGB && self.param.cf_color_format == Y_ONLY {
            for row in i_first_row..c_height {
                let i_y = off_y[row];
                for col in i_first_column..c_width {
                    if bd == BD8 {
                        let d = i_y + off_x[col];
                        out[d + 1] = out[d];
                        out[d + 2] = out[d];
                    } else {
                        let d = 2 * (i_y + off_x[col]);
                        out.copy_within(d..d + 2, d + 2);
                        out.copy_within(d..d + 2, d + 4);
                    }
                }
            }
        }
        if self.alpha.is_some() {
            self.output_alpha(out, dims, i_shift)?;
        }
        Ok(())
    }

    /// `outputMBRowAlpha`, strdec.c:791-871: composite an interleaved alpha plane into the
    /// trailing channel of each output pixel.
    fn output_alpha(
        &self,
        out: &mut [u8],
        (i_first_row, i_first_column, c_width, c_height): (usize, usize, usize, usize),
        i_shift: u32,
    ) -> Result<(), Error> {
        let bd = self.wmii.bd_bit_depth;
        // jxrlib tests internal format: never true.
        if bd == BD8 && self.wmiscp.cf_color_format == CF_RGB {
            return Ok(());
        }
        let i_alpha_pos: usize = if self.wmii.cf_color_format == CMYK {
            4
        } else if self.wmii.cf_color_format != CF_RGB {
            return Err(Error::Unsupported);
        } else {
            3
        };
        let n_len = u32::from(self.wmiscp.n_len_mantissa_or_shift);
        let n_exp_bias = self.wmiscp.n_exp_bias;
        let Some(alpha) = self.alpha.as_ref() else {
            return Ok(());
        };
        let p_a = &alpha.sc.mb_buf[alpha.sc.a0_off[0]..];
        let off_x = &self.dp.p_offset_x;
        let off_y = &self.dp.p_offset_y[(self.c_row - 1) * 16..];
        let half = if i_shift != 0 {
            1i32 << (i_shift - 1)
        } else {
            0
        };

        match bd {
            BD8 => {
                let i_bias = (1i32 << (i_shift + 7)) + half;
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let a = shr32(p_a[px(row, col)].wrapping_add(i_bias), i_shift);
                        out[off_x[col] + i_y + i_alpha_pos] = clip8(a);
                    }
                }
            }
            BD16 => {
                let i_bias = (1i32 << (i_shift + 15)) + half;
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let a = shl32(
                            shr32(p_a[px(row, col)].wrapping_add(i_bias), i_shift),
                            n_len,
                        );
                        let d = 2 * (off_x[col] + i_y) + 2 * i_alpha_pos;
                        put16(out, d, clip_u16(a));
                    }
                }
            }
            BD16S => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let a = shl32(shr32(p_a[px(row, col)].wrapping_add(half), i_shift), n_len);
                        let d = 2 * (off_x[col] + i_y) + 2 * i_alpha_pos;
                        put16(out, d, clip16(a) as u16);
                    }
                }
            }
            BD16F => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let a = shr32(p_a[px(row, col)].wrapping_add(half), i_shift);
                        let d = 2 * (off_x[col] + i_y) + 2 * i_alpha_pos;
                        put16(out, d, backward_half(a));
                    }
                }
            }
            BD32S => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let a = shl32(shr32(p_a[px(row, col)].wrapping_add(half), i_shift), n_len);
                        let d = 4 * (off_x[col] + i_y) + 4 * i_alpha_pos;
                        put32(out, d, a as u32);
                    }
                }
            }
            BD32F => {
                for row in i_first_row..c_height {
                    let i_y = off_y[row];
                    for col in i_first_column..c_width {
                        let a = shr32(p_a[px(row, col)].wrapping_add(half), i_shift);
                        let d = 4 * (off_x[col] + i_y) + 4 * i_alpha_pos;
                        put32(out, d, pixel2float(a, n_exp_bias, n_len as u8).to_bits());
                    }
                }
            }
            _ => return Err(Error::Unsupported),
        }
        Ok(())
    }
}
