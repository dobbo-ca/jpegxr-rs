// Ported from jxrlib strPredQuant.c, strPredQuantDec.c (BSD-2, see NOTICE).

use super::*;

const ORIENT_WEIGHT: i32 = 4;

const QP_RECIP_TABLE: [[i64; 2]; 32] = [
    [0x0, 0],
    [0x0, 0],
    [0x0, 1],
    [0xaaaaaaab, 1],
    [0x0, 2],
    [0xcccccccd, 2],
    [0xaaaaaaab, 2],
    [0x92492493, 2],
    [0x0, 3],
    [0xe38e38e4, 3],
    [0xcccccccd, 3],
    [0xba2e8ba3, 3],
    [0xaaaaaaab, 3],
    [0x9d89d89e, 3],
    [0x92492493, 3],
    [0x88888889, 3],
    [0x0, 4],
    [0xf0f0f0f1, 4],
    [0xe38e38e4, 4],
    [0xd79435e6, 4],
    [0xcccccccd, 4],
    [0xc30c30c4, 4],
    [0xba2e8ba3, 4],
    [0xb21642c9, 4],
    [0xaaaaaaab, 4],
    [0xa3d70a3e, 4],
    [0x9d89d89e, 4],
    [0x97b425ee, 4],
    [0x92492493, 4],
    [0x8d3dcb09, 4],
    [0x88888889, 4],
    [0x84210843, 4],
];

pub(super) fn remap_qp(q: &mut Quantizer, i_shift: i32, b_scaled_arith: bool) {
    let idx = i32::from(q.i_index);
    if idx == 0 {
        q.i_qp = 1;
        q.i_man = 0;
        q.i_exp = 0;
        q.i_offset = 0;
        return;
    }
    let (man, exp): (i32, i32);
    if !b_scaled_arith {
        const CI_SHIFT: i32 = -QP_FRAC_BITS;
        if idx < 32 {
            man = (idx + 3) >> 2;
            exp = CI_SHIFT + 2;
        } else if idx < 48 {
            man = (16 + (idx & 0xf) + 1) >> 1;
            exp = ((idx >> 4) - 1) + 1 + CI_SHIFT;
        } else {
            man = 16 + (idx & 0xf);
            exp = ((idx >> 4) - 1) + CI_SHIFT;
        }
    } else if idx < 16 {
        man = idx;
        exp = i_shift;
    } else {
        man = 16 + (idx & 0xf);
        exp = ((idx >> 4) - 1) + i_shift;
    }
    q.i_qp = man << (exp as u32 & 31);
    q.i_man = QP_RECIP_TABLE[man as usize][0] as i32;
    q.i_exp = QP_RECIP_TABLE[man as usize][1] as i32 + exp;
    q.i_offset = q.i_qp.wrapping_mul(3).wrapping_add(1) >> 3;
}

fn abs32(x: i32) -> i32 {
    if x < 0 { x.wrapping_neg() } else { x }
}

fn get_ac_pred_mode(mb: &MbInfo, cf: usize) -> i32 {
    let c = &mb.i_block_dc[0];
    let mut str_h = abs32(c[1])
        .wrapping_add(abs32(c[2]))
        .wrapping_add(abs32(c[3]));
    let mut str_v = abs32(c[4])
        .wrapping_add(abs32(c[8]))
        .wrapping_add(abs32(c[12]));
    if cf != Y_ONLY && cf != N_COMPONENT {
        let u = &mb.i_block_dc[1];
        let v = &mb.i_block_dc[2];
        str_h = str_h.wrapping_add(abs32(u[1])).wrapping_add(abs32(v[1]));
        match cf {
            YUV420 => str_v = str_v.wrapping_add(abs32(u[2])).wrapping_add(abs32(v[2])),
            YUV422 => {
                str_v = str_v
                    .wrapping_add(abs32(u[2]))
                    .wrapping_add(abs32(v[2]))
                    .wrapping_add(abs32(u[6]))
                    .wrapping_add(abs32(v[6]));
                str_h = str_h.wrapping_add(abs32(u[5])).wrapping_add(abs32(v[5]));
            }
            _ => str_v = str_v.wrapping_add(abs32(u[4])).wrapping_add(abs32(v[4])),
        }
    }
    if str_h.wrapping_mul(ORIENT_WEIGHT) < str_v {
        return 1;
    }
    if str_v.wrapping_mul(ORIENT_WEIGHT) < str_h {
        return 0;
    }
    2
}

fn num_ones(mut i: i32) -> i32 {
    const G: [i32; 16] = [0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4];
    let mut r = 0;
    i &= 0xffff;
    while i != 0 {
        r += G[(i & 0xf) as usize];
        i >>= 4;
    }
    r
}

fn saturate32(x: &mut i32) {
    if (x.wrapping_add(16)) as u32 >= 32 {
        if *x < 0 {
            *x = -16;
        } else {
            *x = 15;
        }
    }
}

fn update_cbp_state(m: &mut CbpModel, c1: usize, i_n_orig: i32) {
    const I_N_DIFF: i32 = 3;
    m.count0[c1] += i_n_orig - I_N_DIFF;
    saturate32(&mut m.count0[c1]);
    m.count1[c1] += 16 - i_n_orig - I_N_DIFF;
    saturate32(&mut m.count1[c1]);
    if m.count0[c1] < 0 {
        if m.count0[c1] < m.count1[c1] {
            m.state[c1] = 1;
        } else {
            m.state[c1] = 2;
        }
    } else if m.count1[c1] < 0 {
        m.state[c1] = 2;
    } else {
        m.state[c1] = 0;
    }
}

const PRED_TOP_BLK_IDX: [usize; 12] = [1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15];

impl StrCodec {
    pub(super) fn allocate_pred_info(&mut self, led: &Ledger) -> Result<(), Error> {
        let nch = self.param.c_num_channels;
        for i in 0..nch {
            led.reuse(&mut self.pred_info[i], self.cmb_width, PredInfo::default())?;
            led.reuse(
                &mut self.pred_info_prev[i],
                self.cmb_width,
                PredInfo::default(),
            )?;
        }
        // Channels this stream lacks are empty, as in a fresh codec.
        for i in nch..MAX_CHANNELS {
            self.pred_info[i].clear();
            self.pred_info_prev[i].clear();
        }
        Ok(())
    }

    fn get_dc_ac_pred_mode(&self, mb_x: usize) -> usize {
        let i_dc_mode: usize = if self.ctx_left && self.ctx_top {
            3
        } else if self.ctx_left {
            1
        } else if self.ctx_top {
            0
        } else {
            let cf = self.param.cf_color_format;
            let i_l = self.pred_info[0][mb_x - 1].i_dc;
            let i_t = self.pred_info_prev[0][mb_x].i_dc;
            let i_tl = self.pred_info_prev[0][mb_x - 1].i_dc;
            let (str_h, str_v) = if cf == Y_ONLY || cf == N_COMPONENT {
                (abs32(i_tl.wrapping_sub(i_l)), abs32(i_tl.wrapping_sub(i_t)))
            } else {
                let (tu, lu, tlu) = (
                    &self.pred_info_prev[1][mb_x],
                    &self.pred_info[1][mb_x - 1],
                    &self.pred_info_prev[1][mb_x - 1],
                );
                let (tv, lv, tlv) = (
                    &self.pred_info_prev[2][mb_x],
                    &self.pred_info[2][mb_x - 1],
                    &self.pred_info_prev[2][mb_x - 1],
                );
                let scale = match cf {
                    YUV420 => 8,
                    YUV422 => 4,
                    _ => 2,
                };
                (
                    abs32(i_tl.wrapping_sub(i_l))
                        .wrapping_mul(scale)
                        .wrapping_add(abs32(tlu.i_dc.wrapping_sub(lu.i_dc)))
                        .wrapping_add(abs32(tlv.i_dc.wrapping_sub(lv.i_dc))),
                    abs32(i_tl.wrapping_sub(i_t))
                        .wrapping_mul(scale)
                        .wrapping_add(abs32(tlu.i_dc.wrapping_sub(tu.i_dc)))
                        .wrapping_add(abs32(tlv.i_dc.wrapping_sub(tv.i_dc))),
                )
            };
            if str_h.wrapping_mul(ORIENT_WEIGHT) < str_v {
                1
            } else if str_v.wrapping_mul(ORIENT_WEIGHT) < str_h {
                0
            } else {
                2
            }
        };
        let mut i_ad_mode: usize = 2;
        if i_dc_mode == 1
            && i32::from(self.mb.i_q_index_lp) == self.pred_info_prev[0][mb_x].i_qp_index
        {
            i_ad_mode = 1;
        }
        if i_dc_mode == 0
            && i32::from(self.mb.i_q_index_lp) == self.pred_info[0][mb_x - 1].i_qp_index
        {
            i_ad_mode = 0;
        }
        i_dc_mode + (i_ad_mode << 2)
    }

    pub(super) fn update_pred_info(&mut self, mb_x: usize, cf: usize) {
        let mut i_channels = self.param.c_num_channels;
        if cf == YUV420 || cf == YUV422 {
            i_channels = 1;
        }
        let q_lp = i32::from(self.mb.i_q_index_lp);
        for i in 0..i_channels {
            let pi = &mut self.pred_info[i][mb_x];
            let p = &self.mb.i_block_dc[i];
            pi.i_dc = p[0];
            pi.i_qp_index = q_lp;
            pi.i_ad[0] = p[1];
            pi.i_ad[1] = p[2];
            pi.i_ad[2] = p[3];
            pi.i_ad[3] = p[4];
            pi.i_ad[4] = p[8];
            pi.i_ad[5] = p[12];
        }
        match cf {
            YUV420 => {
                for i in 1..3 {
                    let pi = &mut self.pred_info[i][mb_x];
                    let p = &self.mb.i_block_dc[i];
                    pi.i_dc = p[0];
                    pi.i_qp_index = q_lp;
                    pi.i_ad[0] = p[1];
                    pi.i_ad[1] = p[2];
                }
            }
            YUV422 => {
                for i in 1..3 {
                    let pi = &mut self.pred_info[i][mb_x];
                    pi.i_qp_index = q_lp;
                    let p = &self.mb.i_block_dc[i];
                    pi.i_dc = p[0];
                    pi.i_ad[0] = p[1];
                    pi.i_ad[1] = p[2];
                    pi.i_ad[2] = p[5];
                    pi.i_ad[3] = p[6];
                    pi.i_ad[4] = p[4];
                }
            }
            _ => {}
        }
    }

    pub(super) fn dequantize_macroblock(&mut self) {
        let cf = self.param.cf_color_format;
        let tc = self.c_tile_column as usize;
        for i in 0..self.param.c_num_channels {
            let b = self.p1(i);
            let org = self.mb.i_block_dc[i];
            let t = &self.tiles[tc];
            let p = &mut self.mb_buf[b..];
            p[0] = org[0].wrapping_mul(t.q_dc[i][0].i_qp);
            if self.wmiscp.sb_subband == SB_DC_ONLY {
                continue;
            }
            let qp = t.q_lp[i][self.mb.i_q_index_lp as usize].i_qp;
            if i == 0 || (cf != YUV422 && cf != YUV420) {
                for k in 1..16 {
                    p[DCT_INDEX[2][k]] = org[k].wrapping_mul(qp);
                }
            } else if cf == YUV422 {
                p[64] = org[1].wrapping_mul(qp);
                p[16] = org[2].wrapping_mul(qp);
                p[80] = org[3].wrapping_mul(qp);
                p[32] = org[4].wrapping_mul(qp);
                p[96] = org[5].wrapping_mul(qp);
                p[48] = org[6].wrapping_mul(qp);
                p[112] = org[7].wrapping_mul(qp);
            } else {
                p[32] = org[1].wrapping_mul(qp);
                p[16] = org[2].wrapping_mul(qp);
                p[48] = org[3].wrapping_mul(qp);
            }
        }
    }

    pub(super) fn pred_dcac_dec(&mut self) {
        let cf = self.param.cf_color_format;
        let mut i_channels = self.param.c_num_channels;
        if cf == YUV420 || cf == YUV422 {
            i_channels = 1;
        }
        let mb_x = self.c_column;
        let mode = self.get_dc_ac_pred_mode(mb_x);
        let dc_mode = mode & 3;
        let ad_mode = mode & 0xc;

        for ii in 0..i_channels {
            let org = &mut self.mb.i_block_dc[ii];
            match dc_mode {
                1 => org[0] = org[0].wrapping_add(self.pred_info_prev[ii][mb_x].i_dc),
                0 => org[0] = org[0].wrapping_add(self.pred_info[ii][mb_x - 1].i_dc),
                2 => {
                    org[0] = org[0].wrapping_add(
                        self.pred_info[ii][mb_x - 1]
                            .i_dc
                            .wrapping_add(self.pred_info_prev[ii][mb_x].i_dc)
                            >> 1,
                    )
                }
                _ => {}
            }
            match ad_mode {
                4 => {
                    let r = &self.pred_info_prev[ii][mb_x].i_ad;
                    org[4] = org[4].wrapping_add(r[3]);
                    org[8] = org[8].wrapping_add(r[4]);
                    org[12] = org[12].wrapping_add(r[5]);
                }
                0 => {
                    let r = &self.pred_info[ii][mb_x - 1].i_ad;
                    org[1] = org[1].wrapping_add(r[0]);
                    org[2] = org[2].wrapping_add(r[1]);
                    org[3] = org[3].wrapping_add(r[2]);
                }
                _ => {}
            }
        }

        match cf {
            YUV420 => {
                for ii in 1..3 {
                    let org = &mut self.mb.i_block_dc[ii];
                    match dc_mode {
                        1 => org[0] = org[0].wrapping_add(self.pred_info_prev[ii][mb_x].i_dc),
                        0 => org[0] = org[0].wrapping_add(self.pred_info[ii][mb_x - 1].i_dc),
                        2 => {
                            org[0] = org[0].wrapping_add(
                                self.pred_info[ii][mb_x - 1]
                                    .i_dc
                                    .wrapping_add(self.pred_info_prev[ii][mb_x].i_dc)
                                    .wrapping_add(1)
                                    >> 1,
                            )
                        }
                        _ => {}
                    }
                    match ad_mode {
                        4 => org[2] = org[2].wrapping_add(self.pred_info_prev[ii][mb_x].i_ad[1]),
                        0 => org[1] = org[1].wrapping_add(self.pred_info[ii][mb_x - 1].i_ad[0]),
                        _ => {}
                    }
                }
            }
            YUV422 => {
                for ii in 1..3 {
                    let org = &mut self.mb.i_block_dc[ii];
                    match dc_mode {
                        1 => org[0] = org[0].wrapping_add(self.pred_info_prev[ii][mb_x].i_dc),
                        0 => org[0] = org[0].wrapping_add(self.pred_info[ii][mb_x - 1].i_dc),
                        2 => {
                            org[0] = org[0].wrapping_add(
                                self.pred_info[ii][mb_x - 1]
                                    .i_dc
                                    .wrapping_add(self.pred_info_prev[ii][mb_x].i_dc)
                                    .wrapping_add(1)
                                    >> 1,
                            )
                        }
                        _ => {}
                    }
                    if ad_mode == 4 {
                        org[4] = org[4].wrapping_add(self.pred_info_prev[ii][mb_x].i_ad[4]);
                        org[2] = org[2].wrapping_add(self.pred_info_prev[ii][mb_x].i_ad[3]);
                        org[6] = org[6].wrapping_add(org[2]);
                    } else if ad_mode == 0 {
                        org[4] = org[4].wrapping_add(self.pred_info[ii][mb_x - 1].i_ad[4]);
                        org[1] = org[1].wrapping_add(self.pred_info[ii][mb_x - 1].i_ad[0]);
                        org[5] = org[5].wrapping_add(self.pred_info[ii][mb_x - 1].i_ad[2]);
                    } else if dc_mode == 1 {
                        org[6] = org[6].wrapping_add(org[2]);
                    }
                }
            }
            _ => {}
        }

        self.mb.i_orientation = 2 - get_ac_pred_mode(&self.mb, cf);
    }

    pub(super) fn pred_ac_dec(&mut self) {
        let cf = self.param.cf_color_format;
        let mut i_channels = self.param.c_num_channels;
        if cf == YUV420 || cf == YUV422 {
            i_channels = 1;
        }
        let mode = 2 - self.mb.i_orientation;

        for i in 0..i_channels {
            let b = self.p1(i);
            let src = &mut self.mb_buf[b..];
            match mode {
                1 => {
                    for &blk in &PRED_TOP_BLK_IDX {
                        let o = 16 * blk;
                        let r = o - 16;
                        src[o + 2] = src[o + 2].wrapping_add(src[r + 2]);
                        src[o + 10] = src[o + 10].wrapping_add(src[r + 10]);
                        src[o + 9] = src[o + 9].wrapping_add(src[r + 9]);
                    }
                }
                0 => {
                    let mut o = 64;
                    while o < 256 {
                        let r = o - 64;
                        src[o + 1] = src[o + 1].wrapping_add(src[r + 1]);
                        src[o + 5] = src[o + 5].wrapping_add(src[r + 5]);
                        src[o + 6] = src[o + 6].wrapping_add(src[r + 6]);
                        o += 16;
                    }
                }
                _ => {}
            }
        }

        match cf {
            YUV420 => {
                for i in 1..3 {
                    let b = self.p1(i);
                    let src = &mut self.mb_buf[b..];
                    match mode {
                        1 => {
                            for j in [1usize, 3] {
                                let o = 16 * j;
                                let r = o - 16;
                                src[o + 2] = src[o + 2].wrapping_add(src[r + 2]);
                                src[o + 10] = src[o + 10].wrapping_add(src[r + 10]);
                                src[o + 9] = src[o + 9].wrapping_add(src[r + 9]);
                            }
                        }
                        0 => {
                            for j in 2usize..=3 {
                                let o = 16 * j;
                                let r = o - 32;
                                src[o + 1] = src[o + 1].wrapping_add(src[r + 1]);
                                src[o + 5] = src[o + 5].wrapping_add(src[r + 5]);
                                src[o + 6] = src[o + 6].wrapping_add(src[r + 6]);
                            }
                        }
                        _ => {}
                    }
                }
            }
            YUV422 => {
                for i in 1..3 {
                    let b = self.p1(i);
                    let src = &mut self.mb_buf[b..];
                    match mode {
                        1 => {
                            for j in 2..8 {
                                let o = BLK_OFFSET_UV422[j];
                                let r = o - 16;
                                src[o + 10] = src[o + 10].wrapping_add(src[r + 10]);
                                src[o + 2] = src[o + 2].wrapping_add(src[r + 2]);
                                src[o + 9] = src[o + 9].wrapping_add(src[r + 9]);
                            }
                        }
                        0 => {
                            for j in (1..8).step_by(2) {
                                let o = BLK_OFFSET_UV422[j];
                                let r = o - 64;
                                src[o + 1] = src[o + 1].wrapping_add(src[r + 1]);
                                src[o + 5] = src[o + 5].wrapping_add(src[r + 5]);
                                src[o + 6] = src[o + 6].wrapping_add(src[r + 6]);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn pred_cbpc_dec(&self, mut i_cbp: i32, mb_x: usize, c: usize, m: &mut CbpModel) -> i32 {
        let c1 = usize::from(c != 0);
        match m.state[c1] {
            0 => {
                if self.ctx_left {
                    if self.ctx_top {
                        i_cbp ^= 1;
                    } else {
                        i_cbp ^= (self.pred_info_prev[c][mb_x].i_cbp >> 10) & 1;
                    }
                } else {
                    i_cbp ^= (self.pred_info[c][mb_x - 1].i_cbp >> 5) & 1;
                }
                i_cbp ^= 0x02 & (i_cbp << 1);
                i_cbp ^= 0x10 & (i_cbp << 3);
                i_cbp ^= 0x20 & (i_cbp << 1);
                i_cbp ^= (i_cbp & 0x33) << 2;
                i_cbp ^= (i_cbp & 0xcc) << 6;
                i_cbp ^= (i_cbp & 0x3300) << 2;
            }
            2 => i_cbp ^= 0xffff,
            _ => {}
        }
        update_cbp_state(m, c1, num_ones(i_cbp));
        i_cbp
    }

    fn pred_cbpc420_dec(&self, mut i_cbp: i32, mb_x: usize, c: usize, m: &mut CbpModel) -> i32 {
        match m.state[1] {
            0 => {
                if self.ctx_left {
                    if self.ctx_top {
                        i_cbp ^= 1;
                    } else {
                        i_cbp ^= (self.pred_info_prev[c][mb_x].i_cbp >> 2) & 1;
                    }
                } else {
                    i_cbp ^= (self.pred_info[c][mb_x - 1].i_cbp >> 1) & 1;
                }
                i_cbp ^= 0x02 & (i_cbp << 1);
                i_cbp ^= (i_cbp & 0x3) << 2;
            }
            2 => i_cbp ^= 0xf,
            _ => {}
        }
        update_cbp_state(m, 1, num_ones(i_cbp) * 4);
        i_cbp
    }

    fn pred_cbpc422_dec(&self, mut i_cbp: i32, mb_x: usize, c: usize, m: &mut CbpModel) -> i32 {
        match m.state[1] {
            0 => {
                if self.ctx_left {
                    if self.ctx_top {
                        i_cbp ^= 1;
                    } else {
                        i_cbp ^= (self.pred_info_prev[c][mb_x].i_cbp >> 6) & 1;
                    }
                } else {
                    i_cbp ^= (self.pred_info[c][mb_x - 1].i_cbp >> 1) & 1;
                }
                i_cbp ^= (i_cbp & 0x1) << 1;
                i_cbp ^= (i_cbp & 0x3) << 2;
                i_cbp ^= (i_cbp & 0xc) << 2;
                i_cbp ^= (i_cbp & 0x30) << 2;
            }
            2 => i_cbp ^= 0xff,
            _ => {}
        }
        update_cbp_state(m, 1, num_ones(i_cbp) * 2);
        i_cbp
    }

    pub(super) fn pred_cbp_dec(&mut self, m: &mut CbpModel) {
        let cf = self.param.cf_color_format;
        let mut i_channels = self.param.c_num_channels;
        if cf == YUV420 || cf == YUV422 {
            i_channels = 1;
        }
        let mb_x = self.c_column;
        for i in 0..i_channels {
            self.mb.i_cbp[i] = self.pred_cbpc_dec(self.mb.i_diff_cbp[i], mb_x, i, m);
            self.pred_info[i][mb_x].i_cbp = self.mb.i_cbp[i];
        }
        match cf {
            YUV422 => {
                for c in 1..3 {
                    self.mb.i_cbp[c] = self.pred_cbpc422_dec(self.mb.i_diff_cbp[c], mb_x, c, m);
                    self.pred_info[c][mb_x].i_cbp = self.mb.i_cbp[c];
                }
            }
            YUV420 => {
                for c in 1..3 {
                    self.mb.i_cbp[c] = self.pred_cbpc420_dec(self.mb.i_diff_cbp[c], mb_x, c, m);
                    self.pred_info[c][mb_x].i_cbp = self.mb.i_cbp[c];
                }
            }
            _ => {}
        }
    }
}
