// Ported from jxrlib strInvTransform.c, strTransform.c (BSD-2, see NOTICE).

use super::simd;
use super::wrap::W;
use super::*;
use std::ops::{Index, IndexMut};

/// A macroblock buffer indexed by signed offsets, so the kernels read like the C they port.
pub(super) struct P<'a>(pub(super) &'a mut [i32]);

impl P<'_> {
    #[inline(always)]
    fn g(&self, i: isize) -> W {
        W(self.0[i as usize])
    }

    #[inline(always)]
    fn add(&mut self, i: isize, v: i32) {
        let x = &mut self.0[i as usize];
        *x = x.wrapping_add(v);
    }

    #[inline(always)]
    fn sub(&mut self, i: isize, v: i32) {
        let x = &mut self.0[i as usize];
        *x = x.wrapping_sub(v);
    }
}

impl Index<isize> for P<'_> {
    type Output = i32;
    #[inline(always)]
    fn index(&self, i: isize) -> &i32 {
        &self.0[i as usize]
    }
}

impl IndexMut<isize> for P<'_> {
    #[inline(always)]
    fn index_mut(&mut self, i: isize) -> &mut i32 {
        &mut self.0[i as usize]
    }
}

// Every kernel takes one channel's two-row buffer and indices into it.

fn str_dct2x2dn(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, cc, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    a += d;
    b -= cc;
    let t = (a - b) >> 1;
    let c = t - d;
    d = t - cc;
    a -= d;
    b += c;
    p[ia] = a.0;
    p[ib] = b.0;
    p[ic] = c.0;
    p[id] = d.0;
}

fn str_dct2x2up(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, cc, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    a += d;
    b -= cc;
    let t = (a - b + 1) >> 1;
    let c = t - d;
    d = t - cc;
    a -= d;
    b += c;
    p[ia] = a.0;
    p[ib] = b.0;
    p[ic] = c.0;
    p[id] = d.0;
}

fn str_dct2x2dn_dec(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, cc, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    a += d;
    b -= cc;
    let t = (a - b) >> 1;
    let c = t - d;
    d = t - cc;
    a -= d;
    b += c;
    p[ia] = (a * 2).0;
    p[ib] = (b * 2).0;
    p[ic] = (c * 2).0;
    p[id] = (d * 2).0;
}

fn four_butterfly_hardcoded1(p: &mut P, o: isize) {
    str_dct2x2dn(p, o, o + 4, o + 8, o + 12);
    str_dct2x2dn(p, o + 1, o + 5, o + 9, o + 13);
    str_dct2x2dn(p, o + 2, o + 6, o + 10, o + 14);
    str_dct2x2dn(p, o + 3, o + 7, o + 11, o + 15);
}

fn irotate1(p: &mut P, ia: isize, ib: isize) {
    let (a, b) = (p.g(ia), p.g(ib));
    let a = a - ((b + 1) >> 1);
    let b = b + ((a + 1) >> 1);
    p[ia] = a.0;
    p[ib] = b.0;
}

fn inv_odd(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, mut c, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    b += d;
    a -= c;
    d -= b >> 1;
    c += (a + 1) >> 1;
    a -= (b * 3 + 4) >> 3;
    b += (a * 3 + 4) >> 3;
    c -= (d * 3 + 4) >> 3;
    d += (c * 3 + 4) >> 3;
    c -= (b + 1) >> 1;
    d = ((a + 1) >> 1) - d;
    b += c;
    a -= d;
    p[ia] = a.0;
    p[ib] = b.0;
    p[ic] = c.0;
    p[id] = d.0;
}

fn inv_odd_odd(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, mut c, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a -= (b * 3 + 3) >> 3;
    b += (a * 3 + 3) >> 2;
    a -= (b * 3 + 4) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    p[ia] = a.0;
    p[ib] = (-b).0;
    p[ic] = (-c).0;
    p[id] = d.0;
}

fn inv_odd_odd_post(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, mut c, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a -= (b * 3 + 6) >> 3;
    b += (a * 3 + 2) >> 2;
    a -= (b * 3 + 4) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    p[ia] = a.0;
    p[ib] = b.0;
    p[ic] = c.0;
    p[id] = d.0;
}

fn str_hst_dec1_alternate(p: &mut P, ia: isize, id: isize) {
    let (mut a, mut d) = (p.g(ia), p.g(id));
    a += d;
    d = (a >> 1) - d;
    a += (d * 3) >> 3;
    d += (a * 3) >> 4;
    d += a >> 7;
    d -= a >> 10;
    p[ia] = a.0;
    p[id] = d.0;
}

fn str_hst_dec1_edge(mut a: W, mut d: W) -> (W, W) {
    a += d;
    d = (a >> 1) - d;
    a += (d * 3) >> 3;
    d += (a * 3) >> 4;
    d += a >> 7;
    d -= a >> 10;
    a += (d * 3 + 4) >> 3;
    d -= a >> 1;
    a += d;
    (a, -d)
}

fn str_hst_dec(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, mut c, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    b -= c;
    a += (d * 3 + 4) >> 3;
    d -= b >> 1;
    c = ((a - b) >> 1) - c;
    p[ic] = d.0;
    p[id] = c.0;
    p[ia] = (a - c).0;
    p[ib] = (b + d).0;
}

fn str_post2_alternate(p: &mut P, ia: isize, ib: isize) {
    let (mut a, mut b) = (p.g(ia), p.g(ib));
    b += (a + 2) >> 2;
    a += (b + 1) >> 1;
    a += b >> 5;
    a += b >> 9;
    a += b >> 13;
    b += (a + 2) >> 2;
    p[ia] = a.0;
    p[ib] = b.0;
}

fn str_post2x2_alternate(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, mut c, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    b += (a + 2) >> 2;
    a += (b + 1) >> 1;
    a += b >> 5;
    a += b >> 9;
    a += b >> 13;
    b += (a + 2) >> 2;
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    p[ia] = a.0;
    p[ib] = b.0;
    p[ic] = c.0;
    p[id] = d.0;
}

fn str_post4_alternate(p: &mut P, ia: isize, ib: isize, ic: isize, id: isize) {
    let (mut a, mut b, mut c, mut d) = (p.g(ia), p.g(ib), p.g(ic), p.g(id));
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    (a, d) = str_hst_dec1_edge(a, d);
    (b, c) = str_hst_dec1_edge(b, c);
    c -= (d + 1) >> 1;
    d += (c + 1) >> 1;
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    p[ia] = a.0;
    p[ib] = b.0;
    p[ic] = c.0;
    p[id] = d.0;
}

/// Inverts one 4x4 block at `o`.
fn str_idct4x4_stage1(p: &mut P, o: isize) {
    str_dct2x2up(p, o, o + 1, o + 2, o + 3);
    inv_odd(p, o + 5, o + 4, o + 7, o + 6);
    inv_odd(p, o + 10, o + 8, o + 11, o + 9);
    inv_odd_odd(p, o + 15, o + 14, o + 13, o + 12);
    four_butterfly_hardcoded1(p, o);
}

/// Pair k is blocks `o+64k` and `o+64k+16`.
fn str_idct4x4_stage1_pairs(p: &mut P, o: isize, n: isize) {
    simd::idct_pairs(simd::tier(), p.0, o, n);
}

/// The scalar reference of `str_idct4x4_stage1_pairs`.
pub(super) fn str_idct4x4_stage1_pairs_scalar(p: &mut P, mut o: isize, mut n: isize) {
    while n > 0 {
        str_idct4x4_stage1(p, o);
        str_idct4x4_stage1(p, o + 16);
        o += 64;
        n -= 1;
    }
}

fn str_idct4x4_stage2(p: &mut P, o: isize) {
    inv_odd(p, o + 32, o + 48, o + 96, o + 112);
    inv_odd(p, o + 128, o + 192, o + 144, o + 208);
    inv_odd_odd(p, o + 160, o + 224, o + 176, o + 240);
    str_dct2x2up(p, o, o + 64, o + 16, o + 80);
    str_dct2x2dn(p, o, o + 192, o + 48, o + 240);
    str_dct2x2dn(p, o + 64, o + 128, o + 112, o + 176);
    str_dct2x2dn(p, o + 16, o + 208, o + 32, o + 224);
    str_dct2x2dn(p, o + 80, o + 144, o + 96, o + 160);
}

fn str_normalize_dec(p: &mut P, o: isize, chroma: bool) {
    if chroma {
        let mut i = 0;
        while i < 256 {
            p.add(o + i, p[o + i]);
            i += 16;
        }
    }
}

fn str_post4x4_stage1_split_alternate(p: &mut P, p0: isize, p1: isize, i_offset: isize) {
    simd::post_split(simd::tier(), p.0, p0, p1, i_offset);
}

/// The scalar reference of `str_post4x4_stage1_split_alternate`.
pub(super) fn str_post4x4_stage1_split_alternate_scalar(
    p: &mut P,
    p0: isize,
    p1: isize,
    i_offset: isize,
) {
    let p2 = p0 + 72 - i_offset;
    let p3 = p1 + 64 - i_offset;
    let p0 = p0 + 12;
    let p1 = p1 + 4;

    for k in 0..4 {
        str_dct2x2dn(p, p0 + k, p2 + k, p1 + k, p3 + k);
    }
    inv_odd_odd_post(p, p3, p3 + 1, p3 + 2, p3 + 3);
    irotate1(p, p1 + 2, p1 + 3);
    irotate1(p, p1, p1 + 1);
    irotate1(p, p2 + 1, p2 + 3);
    irotate1(p, p2, p2 + 2);
    for k in 0..4 {
        str_hst_dec1_alternate(p, p0 + k, p3 + k);
    }
    for k in 0..4 {
        str_hst_dec(p, p0 + k, p2 + k, p1 + k, p3 + k);
    }
}

fn str_post4x4_stage1_alternate(p: &mut P, o: isize, i_offset: isize) {
    str_post4x4_stage1_split_alternate(p, o, o + 16, i_offset);
}

fn str_post4x4_stage2_split_alternate(p: &mut P, p0: isize, p1: isize) {
    str_dct2x2dn(p, p0 - 96, p0 + 96, p1 - 112, p1 + 80);
    str_dct2x2dn(p, p0 - 32, p0 + 32, p1 - 48, p1 + 16);
    str_dct2x2dn(p, p0 - 80, p0 + 112, p1 - 128, p1 + 64);
    str_dct2x2dn(p, p0 - 16, p0 + 48, p1 - 64, p1);

    inv_odd_odd_post(p, p1, p1 + 64, p1 + 16, p1 + 80);

    irotate1(p, p0 + 48, p0 + 32);
    irotate1(p, p0 + 112, p0 + 96);
    irotate1(p, p1 - 64, p1 - 128);
    irotate1(p, p1 - 48, p1 - 112);

    str_hst_dec1_alternate(p, p0 - 96, p1 + 80);
    str_hst_dec1_alternate(p, p0 - 32, p1 + 16);
    str_hst_dec1_alternate(p, p0 - 80, p1 + 64);
    str_hst_dec1_alternate(p, p0 - 16, p1);

    str_hst_dec(p, p0 - 96, p1 - 112, p0 + 96, p1 + 80);
    str_hst_dec(p, p0 - 32, p1 - 48, p0 + 32, p1 + 16);
    str_hst_dec(p, p0 - 80, p1 - 128, p0 + 112, p1 + 64);
    str_hst_dec(p, p0 - 16, p1 - 64, p0 + 48, p1);
}

/// The 4x4 post filter at the four edges of a row of macroblocks, shared by the 444 and 422
/// chroma branches below.
fn edge444(
    p: &mut P,
    (p0, p1): (isize, isize),
    j: isize,
    (top, bottom, top_or_bottom, htb): (bool, bool, bool, bool),
) {
    if !top {
        let q = p0 + 16 + j;
        str_post4_alternate(p, q, q - 2, q + 6, q + 8);
        str_post4_alternate(p, q + 1, q - 1, q + 7, q + 9);
        str_post4_alternate(p, q + 16, q + 14, q + 22, q + 24);
        str_post4_alternate(p, q + 17, q + 15, q + 23, q + 25);
    }
    if !bottom {
        let q = p1 + j;
        str_post4_alternate(p, q, q - 2, q + 6, q + 8);
        str_post4_alternate(p, q + 1, q - 1, q + 7, q + 9);
    }
    if !top_or_bottom && !htb {
        str_post4_alternate(p, p0 + 48 + j, p0 + 48 + j - 2, p1 - 10 + j, p1 - 8 + j);
        str_post4_alternate(p, p0 + 48 + j + 1, p0 + 48 + j - 1, p1 - 9 + j, p1 - 7 + j);
    }
}

impl StrCodec {
    /// `invTransformMacroblock_alteredOperators_hard`, used for subversions 1 and 9.
    pub(super) fn inv_transform_macroblock_altered_operators_hard(&mut self) {
        let ol = self.wmiscp.ol_overlap;
        let cf = self.param.cf_color_format;
        let left = self.c_column == 0;
        let right = self.c_column == self.cmb_width;
        let top = self.c_row == 0;
        let bottom = self.c_row == self.cmb_height;
        let top_or_bottom = top || bottom;
        let left_or_right = left || right;
        let bottom_or_right = bottom || right;
        let left_adjacent_column = self.c_column == 1;
        let right_adjacent_column = self.c_column == self.cmb_width.wrapping_sub(1);
        let mut i_channels = self.param.c_num_channels;
        if cf == YUV420 || cf == YUV422 {
            i_channels = 1;
        }

        if self.wmiscp.b_use_hard_tile_boundaries {
            let w = &self.wmiscp;
            if self.c_column == 0 {
                self.vert_tile_boundary = false;
                self.tile_y = 0;
            }
            self.one_mb_left_vert_tb = false;
            self.one_mb_right_vert_tb = false;
            // jxrlib compares columns with uiTileY; kept for bit-exactness.
            if self.tile_y > 0
                && self.tile_y <= w.c_num_of_slice_minus1_h
                && (self.c_column.wrapping_sub(1)) as u32 == w.ui_tile_y[self.tile_y as usize]
            {
                self.one_mb_right_vert_tb = true;
            }
            if self.tile_y < w.c_num_of_slice_minus1_h
                && self.c_column as u32 == w.ui_tile_y[self.tile_y as usize + 1]
            {
                self.vert_tile_boundary = true;
                self.tile_y += 1;
            } else {
                self.vert_tile_boundary = false;
            }
            if self.tile_y < w.c_num_of_slice_minus1_h
                && (self.c_column + 1) as u32 == w.ui_tile_y[self.tile_y as usize + 1]
            {
                self.one_mb_left_vert_tb = true;
            }
            if self.c_row == 0 {
                self.hori_tile_boundary = false;
                self.tile_x = 0;
            } else if self.mb_y != self.c_row
                && self.tile_x < w.c_num_of_slice_minus1_v
                && self.c_row as u32 == w.ui_tile_x[self.tile_x as usize + 1]
            {
                self.hori_tile_boundary = true;
                self.tile_x += 1;
            } else if self.mb_y != self.c_row {
                self.hori_tile_boundary = false;
            }
        } else {
            self.vert_tile_boundary = false;
            self.hori_tile_boundary = false;
            self.one_mb_left_vert_tb = false;
            self.one_mb_right_vert_tb = false;
        }
        self.mb_x = self.c_column;
        self.mb_y = self.c_row;
        let vtb = self.vert_tile_boundary;
        let htb = self.hori_tile_boundary;
        let one_l = self.one_mb_left_vert_tb;
        let one_r = self.one_mb_right_vert_tb;
        let scaled_arith = self.param.scaled_arith;

        let offs: [(isize, isize); MAX_CHANNELS] = std::array::from_fn(|i| {
            if i < self.param.c_num_channels {
                (self.p0(i) as isize, self.p1(i) as isize)
            } else {
                (0, 0)
            }
        });
        let mut p = P(&mut self.mb_buf);

        //================================================================
        // 400_Y, 444_YUV
        for i in 0..i_channels {
            let (p0, p1) = offs[i];

            if !bottom_or_right {
                str_idct4x4_stage2(&mut p, p1);
                if scaled_arith {
                    str_normalize_dec(&mut p, p1, i != 0);
                }
            }

            if ol == OL_TWO {
                if (top || htb) && (left || vtb) {
                    str_post4_alternate(&mut p, p1, p1 + 64, p1 + 16, p1 + 64 + 16);
                }
                if (top || htb) && (right || vtb) {
                    str_post4_alternate(&mut p, p1 - 128, p1 - 64, p1 - 128 + 16, p1 - 64 + 16);
                }
                if (bottom || htb) && (left || vtb) {
                    str_post4_alternate(&mut p, p0 + 32, p0 + 96, p0 + 32 + 16, p0 + 96 + 16);
                }
                if (bottom || htb) && (right || vtb) {
                    str_post4_alternate(&mut p, p0 - 96, p0 - 32, p0 - 96 + 16, p0 - 32 + 16);
                }
                if (left_or_right || vtb) && (!top_or_bottom && !htb) {
                    if left || vtb {
                        let j = 0;
                        str_post4_alternate(&mut p, p0 + j + 32, p0 + j + 48, p1 + j, p1 + j + 16);
                        str_post4_alternate(
                            &mut p,
                            p0 + j + 96,
                            p0 + j + 112,
                            p1 + j + 64,
                            p1 + j + 80,
                        );
                    }
                    if right || vtb {
                        let j = -128;
                        str_post4_alternate(&mut p, p0 + j + 32, p0 + j + 48, p1 + j, p1 + j + 16);
                        str_post4_alternate(
                            &mut p,
                            p0 + j + 96,
                            p0 + j + 112,
                            p1 + j + 64,
                            p1 + j + 80,
                        );
                    }
                }
                if !left_or_right {
                    if (top_or_bottom || htb) && !vtb {
                        if top || htb {
                            let q = p1;
                            str_post4_alternate(&mut p, q - 128, q - 64, q, q + 64);
                            str_post4_alternate(&mut p, q - 112, q - 48, q + 16, q + 80);
                        }
                        if bottom || htb {
                            let q = p0 + 32;
                            str_post4_alternate(&mut p, q - 128, q - 64, q, q + 64);
                            str_post4_alternate(&mut p, q - 112, q - 48, q + 16, q + 80);
                        }
                    }
                    if !top_or_bottom && !htb && !vtb {
                        str_post4x4_stage2_split_alternate(&mut p, p0, p1);
                    }
                }
            }

            if !top {
                let j_end: isize = if right { 32 } else { 160 };
                let j: isize = if left { 32 } else { -96 };
                str_idct4x4_stage1_pairs(&mut p, p0 + j, (j_end - j) / 64);
            }
            if !bottom {
                let j_end: isize = if right { 0 } else { 128 };
                let j: isize = if left { 0 } else { -128 };
                str_idct4x4_stage1_pairs(&mut p, p1 + j, (j_end - j) / 64);
            }

            if ol != OL_NONE {
                if left_or_right || vtb {
                    if (top || htb) && (left || vtb) {
                        str_post4_alternate(&mut p, p1, p1 + 1, p1 + 2, p1 + 3);
                    }
                    if (top || htb) && (right || vtb) {
                        str_post4_alternate(&mut p, p1 - 59, p1 - 60, p1 - 57, p1 - 58);
                    }
                    if (bottom || htb) && (left || vtb) {
                        str_post4_alternate(
                            &mut p,
                            p0 + 48 + 10,
                            p0 + 48 + 11,
                            p0 + 48 + 8,
                            p0 + 48 + 9,
                        );
                    }
                    if (bottom || htb) && (right || vtb) {
                        str_post4_alternate(&mut p, p0 - 1, p0 - 2, p0 - 3, p0 - 4);
                    }
                    let flags = (top, bottom, top_or_bottom, htb);
                    if left || vtb {
                        edge444(&mut p, (p0, p1), 10, flags);
                    }
                    if right || vtb {
                        edge444(&mut p, (p0, p1), -64 + 14, flags);
                    }
                }

                let j_start: isize = if left { 0 } else { -192 };
                let j_end: isize = if right { -64 } else { 64 };
                if top || htb {
                    for j in (j_start..j_end).step_by(64) {
                        if !vtb || j != -64 {
                            let q = p1 + j;
                            str_post4_alternate(&mut p, q + 5, q + 4, q + 64, q + 65);
                            str_post4_alternate(&mut p, q + 7, q + 6, q + 66, q + 67);
                            str_post4x4_stage1_alternate(&mut p, p1 + j, 0);
                        }
                    }
                }
                if bottom || htb {
                    for j in (j_start..j_end).step_by(64) {
                        if !vtb || j != -64 {
                            str_post4x4_stage1_alternate(&mut p, p0 + 16 + j, 0);
                            str_post4x4_stage1_alternate(&mut p, p0 + 32 + j, 0);
                            let q = p0 + 48 + j;
                            str_post4_alternate(&mut p, q + 15, q + 14, q + 74, q + 75);
                            str_post4_alternate(&mut p, q + 13, q + 12, q + 72, q + 73);
                        }
                    }
                }
                if !top && !bottom && !htb {
                    for j in (j_start..j_end).step_by(64) {
                        if !vtb || j != -64 {
                            str_post4x4_stage1_alternate(&mut p, p0 + 16 + j, 0);
                            str_post4x4_stage1_alternate(&mut p, p0 + 32 + j, 0);
                            str_post4x4_stage1_split_alternate(&mut p, p0 + 48 + j, p1 + j, 0);
                            str_post4x4_stage1_alternate(&mut p, p1 + j, 0);
                        }
                    }
                }
            }
        }

        //================================================================
        // 420_UV
        if cf == YUV420 {
            for i in 0..2 {
                let (p0, p1) = offs[1 + i];

                if !bottom_or_right {
                    if !scaled_arith {
                        str_dct2x2dn(&mut p, p1, p1 + 32, p1 + 16, p1 + 48);
                    } else {
                        str_dct2x2dn_dec(&mut p, p1, p1 + 32, p1 + 16, p1 + 48);
                    }
                }

                if ol == OL_TWO {
                    if (left_adjacent_column || one_r) && (top || htb) {
                        p.sub(p1 - 64, p[p1 - 64 + 32]);
                    }
                    if (right_adjacent_column || one_l) && (top || htb) {
                        self.i_pred_before[i][0] = p[p1];
                    }
                    if (right || vtb) && (top || htb) {
                        p.sub(p1 - 64 + 32, self.i_pred_before[i][0]);
                    }
                    if (left_adjacent_column || one_r) && (bottom || htb) {
                        p.sub(p0 - 64 + 16, p[p0 - 64 + 48]);
                    }
                    if (right_adjacent_column || one_l) && (bottom || htb) {
                        self.i_pred_before[i][1] = p[p0 + 16];
                    }
                    if (right || vtb) && (bottom || htb) {
                        p.sub(p0 - 64 + 48, self.i_pred_before[i][1]);
                    }

                    if (left_or_right || vtb) && !top_or_bottom && !htb {
                        if left || vtb {
                            str_post2_alternate(&mut p, p0 + 16, p1);
                        }
                        if right || vtb {
                            str_post2_alternate(&mut p, p0 - 32 + 16, p1 - 32);
                        }
                    }
                    if !left_or_right {
                        if (top_or_bottom || htb) && !vtb {
                            if top || htb {
                                str_post2_alternate(&mut p, p1 - 32, p1);
                            }
                            if bottom || htb {
                                str_post2_alternate(&mut p, p0 + 16 - 32, p0 + 16);
                            }
                        } else if !top_or_bottom && !htb && !vtb {
                            str_post2x2_alternate(&mut p, p0 - 16, p0 + 16, p1 - 32, p1);
                        }
                    }

                    if (left_adjacent_column || one_r) && (top || htb) {
                        p.add(p1 - 64, p[p1 - 64 + 32]);
                    }
                    if (right_adjacent_column || one_l) && (top || htb) {
                        self.i_pred_after[i][0] = p[p1];
                    }
                    if (right || vtb) && (top || htb) {
                        p.add(p1 - 64 + 32, self.i_pred_after[i][0]);
                    }
                    if (left_adjacent_column || one_r) && (bottom || htb) {
                        p.add(p0 - 64 + 16, p[p0 - 64 + 48]);
                    }
                    if (right_adjacent_column || one_l) && (bottom || htb) {
                        self.i_pred_after[i][1] = p[p0 + 16];
                    }
                    if (right || vtb) && (bottom || htb) {
                        p.add(p0 - 64 + 48, self.i_pred_after[i][1]);
                    }
                }

                if !top {
                    let mut j: isize = -16;
                    if left {
                        j = 48;
                    } else if left_adjacent_column || one_r {
                        j = -48;
                    }
                    let j_end: isize = if right || vtb { 16 } else { 48 };
                    while j < j_end {
                        str_idct4x4_stage1(&mut p, p0 + j);
                        j += 32;
                    }
                }
                if !bottom {
                    let mut j: isize = -32;
                    if left {
                        j = 32;
                    } else if left_adjacent_column || one_r {
                        j = -64;
                    }
                    let j_end: isize = if right || vtb { 0 } else { 32 };
                    while j < j_end {
                        str_idct4x4_stage1(&mut p, p1 + j);
                        j += 32;
                    }
                }

                if ol != OL_NONE {
                    if (top || htb) && (left_adjacent_column || one_r) {
                        str_post4_alternate(&mut p, p1 - 64, p1 - 64 + 1, p1 - 64 + 2, p1 - 64 + 3);
                    }
                    if (top || htb) && (right || vtb) {
                        str_post4_alternate(&mut p, p1 - 27, p1 - 28, p1 - 25, p1 - 26);
                    }
                    if (bottom || htb) && (left_adjacent_column || one_r) {
                        str_post4_alternate(
                            &mut p,
                            p0 - 64 + 16 + 10,
                            p0 - 64 + 16 + 11,
                            p0 - 64 + 16 + 8,
                            p0 - 64 + 16 + 9,
                        );
                    }
                    if (bottom || htb) && (right || vtb) {
                        str_post4_alternate(&mut p, p0 - 1, p0 - 2, p0 - 3, p0 - 4);
                    }
                    if !left && !top {
                        if left_adjacent_column || one_r {
                            if !bottom && !htb {
                                str_post4_alternate(
                                    &mut p,
                                    p0 - 64 + 26,
                                    p0 - 64 + 24,
                                    p1 - 64,
                                    p1 - 64 + 2,
                                );
                                str_post4_alternate(
                                    &mut p,
                                    p0 - 64 + 27,
                                    p0 - 64 + 25,
                                    p1 - 64 + 1,
                                    p1 - 64 + 3,
                                );
                            }
                            str_post4_alternate(
                                &mut p,
                                p0 - 64 + 10,
                                p0 - 64 + 8,
                                p0 - 64 + 16,
                                p0 - 64 + 18,
                            );
                            str_post4_alternate(
                                &mut p,
                                p0 - 64 + 11,
                                p0 - 64 + 9,
                                p0 - 64 + 17,
                                p0 - 64 + 19,
                            );
                        }
                        if bottom || htb {
                            let mut q = p0 - 48;
                            str_post4_alternate(&mut p, q + 15, q + 14, q + 42, q + 43);
                            str_post4_alternate(&mut p, q + 13, q + 12, q + 40, q + 41);
                            if !right && !vtb {
                                q = p0 - 16;
                                str_post4_alternate(&mut p, q + 15, q + 14, q + 42, q + 43);
                                str_post4_alternate(&mut p, q + 13, q + 12, q + 40, q + 41);
                            }
                        } else {
                            str_post4x4_stage1_split_alternate(&mut p, p0 - 48, p1 - 16 - 48, 32);
                            if !right && !vtb {
                                str_post4x4_stage1_split_alternate(
                                    &mut p,
                                    p0 - 16,
                                    p1 - 16 - 16,
                                    32,
                                );
                            }
                        }
                        if right || vtb {
                            if !bottom && !htb {
                                str_post4_alternate(&mut p, p0 - 2, p0 - 4, p1 - 28, p1 - 26);
                                str_post4_alternate(&mut p, p0 - 1, p0 - 3, p1 - 27, p1 - 25);
                            }
                            str_post4_alternate(&mut p, p0 - 18, p0 - 20, p0 - 12, p0 - 10);
                            str_post4_alternate(&mut p, p0 - 17, p0 - 19, p0 - 11, p0 - 9);
                        } else {
                            str_post4x4_stage1_alternate(&mut p, p0 - 32, 32);
                        }
                        str_post4x4_stage1_alternate(&mut p, p0 - 64, 32);
                    }
                    if top || htb {
                        if !left {
                            let q = p1 - 64 + 4;
                            str_post4_alternate(&mut p, q + 1, q, q + 28, q + 29);
                            str_post4_alternate(&mut p, q + 3, q + 2, q + 30, q + 31);
                        }
                        if !left && !right && !vtb {
                            let q = p1 - 32 + 4;
                            str_post4_alternate(&mut p, q + 1, q, q + 28, q + 29);
                            str_post4_alternate(&mut p, q + 3, q + 2, q + 30, q + 31);
                        }
                    }
                }
            }
        }

        //================================================================
        // 422_UV
        if cf == YUV422 {
            for i in 0..2 {
                let (p0, p1) = offs[1 + i];

                if !bottom_or_right {
                    p.sub(p1, p[p1 + 32].wrapping_add(1) >> 1);
                    p.add(p1 + 32, p[p1]);
                    if !scaled_arith {
                        str_dct2x2dn(&mut p, p1, p1 + 64, p1 + 16, p1 + 80);
                        str_dct2x2dn(&mut p, p1 + 32, p1 + 96, p1 + 48, p1 + 112);
                    } else {
                        str_dct2x2dn_dec(&mut p, p1, p1 + 64, p1 + 16, p1 + 80);
                        str_dct2x2dn_dec(&mut p, p1 + 32, p1 + 96, p1 + 48, p1 + 112);
                    }
                }

                if ol == OL_TWO {
                    if (left_adjacent_column || one_r) && (top || htb) {
                        p.sub(p1 - 128, p[p1 - 128 + 64]);
                    }
                    if (right_adjacent_column || one_l) && (top || htb) {
                        self.i_pred_before[i][0] = p[p1];
                    }
                    if (right || vtb) && (top || htb) {
                        p.sub(p1 - 128 + 64, self.i_pred_before[i][0]);
                    }
                    if (left_adjacent_column || one_r) && (bottom || htb) {
                        p.sub(p0 - 128 + 48, p[p0 - 128 + 112]);
                    }
                    if (right_adjacent_column || one_l) && (bottom || htb) {
                        self.i_pred_before[i][1] = p[p0 + 48];
                    }
                    if (right || vtb) && (bottom || htb) {
                        p.sub(p0 - 128 + 112, self.i_pred_before[i][1]);
                    }

                    if !bottom {
                        if left_or_right || vtb {
                            if !top && !htb {
                                if left || vtb {
                                    str_post2_alternate(&mut p, p0 + 48, p1);
                                }
                                if right || vtb {
                                    str_post2_alternate(&mut p, p0 + 48 - 64, p1 - 64);
                                }
                            }
                            if left || vtb {
                                str_post2_alternate(&mut p, p1 + 16, p1 + 16 + 16);
                            }
                            if right || vtb {
                                str_post2_alternate(&mut p, p1 - 48, p1 - 48 + 16);
                            }
                        }
                        if !left_or_right && !vtb {
                            if top || htb {
                                str_post2_alternate(&mut p, p1 - 64, p1);
                            } else {
                                str_post2x2_alternate(&mut p, p0 - 16, p0 + 48, p1 - 64, p1);
                            }
                            str_post2x2_alternate(&mut p, p1 - 48, p1 + 16, p1 - 32, p1 + 32);
                        }
                    }
                    if (bottom || htb) && (!left_or_right && !vtb) {
                        str_post2_alternate(&mut p, p0 - 16, p0 + 48);
                    }

                    if (left_adjacent_column || one_r) && (top || htb) {
                        p.add(p1 - 128, p[p1 - 128 + 64]);
                    }
                    if (right_adjacent_column || one_l) && (top || htb) {
                        self.i_pred_after[i][0] = p[p1];
                    }
                    if (right || vtb) && (top || htb) {
                        p.add(p1 - 128 + 64, self.i_pred_after[i][0]);
                    }
                    if (left_adjacent_column || one_r) && (bottom || htb) {
                        p.add(p0 - 128 + 48, p[p0 - 128 + 112]);
                    }
                    if (right_adjacent_column || one_l) && (bottom || htb) {
                        self.i_pred_after[i][1] = p[p0 + 48];
                    }
                    if (right || vtb) && (bottom || htb) {
                        p.add(p0 - 128 + 112, self.i_pred_after[i][1]);
                    }
                }

                if !top {
                    let mut j: isize = -16;
                    if left {
                        j = 112;
                    } else if left_adjacent_column || one_r {
                        j = -80;
                    }
                    let j_end: isize = if right || vtb { 48 } else { 112 };
                    while j < j_end {
                        str_idct4x4_stage1(&mut p, p0 + j);
                        j += 64;
                    }
                }
                if !bottom {
                    let mut j: isize = -64;
                    if left {
                        j = 64;
                    } else if left_adjacent_column || one_r {
                        j = -128;
                    }
                    let j_end: isize = if right || vtb { 0 } else { 64 };
                    while j < j_end {
                        str_idct4x4_stage1(&mut p, p1 + j);
                        str_idct4x4_stage1(&mut p, p1 + j + 16);
                        str_idct4x4_stage1(&mut p, p1 + j + 32);
                        j += 64;
                    }
                }

                if ol != OL_NONE {
                    if (top || htb) && (left_adjacent_column || one_r) {
                        str_post4_alternate(
                            &mut p,
                            p1 - 128,
                            p1 - 128 + 1,
                            p1 - 128 + 2,
                            p1 - 128 + 3,
                        );
                    }
                    if (top || htb) && (right || vtb) {
                        str_post4_alternate(&mut p, p1 - 59, p1 - 60, p1 - 57, p1 - 58);
                    }
                    if (bottom || htb) && (left_adjacent_column || one_r) {
                        str_post4_alternate(
                            &mut p,
                            p0 - 128 + 48 + 10,
                            p0 - 128 + 48 + 11,
                            p0 - 128 + 48 + 8,
                            p0 - 128 + 48 + 9,
                        );
                    }
                    if (bottom || htb) && (right || vtb) {
                        str_post4_alternate(&mut p, p0 - 1, p0 - 2, p0 - 3, p0 - 4);
                    }
                    let j_start: isize = if left { 0 } else { -128 };
                    let j_end: isize = if right || vtb { -64 } else { 0 };
                    if !top {
                        if left_adjacent_column || one_r {
                            let q = p0 + 32 + 10 - 128;
                            str_post4_alternate(&mut p, q, q - 2, q + 6, q + 8);
                            str_post4_alternate(&mut p, q + 1, q - 1, q + 7, q + 9);
                        }
                        if right || vtb {
                            let q = p0 - 32 + 14;
                            str_post4_alternate(&mut p, q, q - 2, q + 6, q + 8);
                            str_post4_alternate(&mut p, q + 1, q - 1, q + 7, q + 9);
                        }
                        for j in (j_start..j_end).step_by(64) {
                            str_post4x4_stage1_alternate(&mut p, p0 + j + 32, 0);
                        }
                    }
                    if !bottom {
                        if left_adjacent_column || one_r {
                            let mut q = p1 + 10 - 128;
                            str_post4_alternate(&mut p, q, q - 2, q + 6, q + 8);
                            str_post4_alternate(&mut p, q + 1, q - 1, q + 7, q + 9);
                            q += 16;
                            str_post4_alternate(&mut p, q, q - 2, q + 6, q + 8);
                            str_post4_alternate(&mut p, q + 1, q - 1, q + 7, q + 9);
                        }
                        if right || vtb {
                            let mut q = p1 - 64 + 14;
                            str_post4_alternate(&mut p, q, q - 2, q + 6, q + 8);
                            str_post4_alternate(&mut p, q + 1, q - 1, q + 7, q + 9);
                            q += 16;
                            str_post4_alternate(&mut p, q, q - 2, q + 6, q + 8);
                            str_post4_alternate(&mut p, q + 1, q - 1, q + 7, q + 9);
                        }
                        for j in (j_start..j_end).step_by(64) {
                            str_post4x4_stage1_alternate(&mut p, p1 + j, 0);
                            str_post4x4_stage1_alternate(&mut p, p1 + j + 16, 0);
                        }
                    }
                    if top_or_bottom || htb {
                        if top || htb {
                            let q = p1 + 5;
                            for j in (j_start..j_end).step_by(64) {
                                str_post4_alternate(
                                    &mut p,
                                    q + j,
                                    q + j - 1,
                                    q + j + 59,
                                    q + j + 60,
                                );
                                str_post4_alternate(
                                    &mut p,
                                    q + j + 2,
                                    q + j + 1,
                                    q + j + 61,
                                    q + j + 62,
                                );
                            }
                        }
                        if bottom || htb {
                            let q = p0 + 48 + 13;
                            for j in (j_start..j_end).step_by(64) {
                                str_post4_alternate(
                                    &mut p,
                                    q + j,
                                    q + j - 1,
                                    q + j + 59,
                                    q + j + 60,
                                );
                                str_post4_alternate(
                                    &mut p,
                                    q + j + 2,
                                    q + j + 1,
                                    q + j + 61,
                                    q + j + 62,
                                );
                            }
                        }
                    } else {
                        if left_adjacent_column || one_r {
                            let j = -128;
                            str_post4_alternate(
                                &mut p,
                                p0 + j + 48 + 10,
                                p0 + j + 48 + 10 - 2,
                                p1 + j,
                                p1 + j + 2,
                            );
                            str_post4_alternate(
                                &mut p,
                                p0 + j + 48 + 10 + 1,
                                p0 + j + 48 + 10 - 1,
                                p1 + j + 1,
                                p1 + j + 3,
                            );
                        }
                        if right || vtb {
                            let j = -64 + 4;
                            str_post4_alternate(
                                &mut p,
                                p0 + j + 48 + 10,
                                p0 + j + 48 + 10 - 2,
                                p1 + j,
                                p1 + j + 2,
                            );
                            str_post4_alternate(
                                &mut p,
                                p0 + j + 48 + 10 + 1,
                                p0 + j + 48 + 10 - 1,
                                p1 + j + 1,
                                p1 + j + 3,
                            );
                        }
                        for j in (j_start..j_end).step_by(64) {
                            str_post4x4_stage1_split_alternate(&mut p, p0 + j + 48, p1 + j, 0);
                        }
                    }
                }
            }
        }
    }
}
