// Ported from jxrlib segdec.c, decode.c, image.c (BSD-2, see NOTICE).

use super::*;

const HUFFMAN_DECODE_ROOT_BITS_LOG: u32 = 3;
const HUFFMAN_DECODE_ROOT_BITS: u32 = 5;
const MAX_TOTAL: u32 = 32767;
const MODEL_WEIGHT: i32 = 70;

const GRGI_ZIGZAG_INV4X4_LOWPASS: [u32; 16] =
    [0, 1, 4, 5, 2, 8, 6, 9, 3, 12, 10, 7, 13, 11, 14, 15];
const GRGI_ZIGZAG_INV4X4_H: [usize; 16] = [0, 1, 4, 5, 2, 8, 6, 9, 3, 12, 10, 7, 13, 11, 14, 15];
const GRGI_ZIGZAG_INV4X4_V: [usize; 16] = [0, 4, 8, 5, 1, 12, 9, 6, 2, 13, 3, 15, 7, 10, 14, 11];

pub(super) const G_SIGNIFICANT_RUN_BIN: [isize; 15] =
    [-1, -1, -1, -1, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0];
pub(super) const G_SIGNIFICANT_RUN_FIXED_LENGTH: [u32; 15] =
    [0, 0, 1, 1, 3, 0, 0, 1, 1, 2, 0, 0, 0, 0, 1];

//================================================================
// image.c
//================================================================

const A_WEIGHT0: [i32; 3] = [240, 12, 1];
const A_WEIGHT1: [[i32; MAX_CHANNELS]; 3] = [
    [
        0, 240, 120, 80, 60, 48, 40, 34, 30, 27, 24, 22, 20, 18, 17, 16,
    ],
    [0, 12, 6, 4, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1],
    [0, 16, 8, 5, 4, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1],
];
const A_WEIGHT2: [i32; 6] = [120, 37, 2, 120, 18, 1];

pub(super) fn update_model_mb(
    cf: usize,
    i_channels: usize,
    lm: &mut [i32; 2],
    m: &mut AdaptiveModel,
) {
    let band = (m.band - BAND_DC) as usize;
    lm[0] = lm[0].wrapping_mul(A_WEIGHT0[band]);
    match cf {
        YUV420 => lm[1] = lm[1].wrapping_mul(A_WEIGHT2[band]),
        YUV422 => lm[1] = lm[1].wrapping_mul(A_WEIGHT2[3 + band]),
        _ => {
            lm[1] = lm[1].wrapping_mul(A_WEIGHT1[band][i_channels - 1]);
            if m.band == BAND_AC {
                lm[1] >>= 4;
            }
        }
    }
    for j in 0..2 {
        let i_lm = lm[j];
        let mut i_ms = m.flc_state[j];
        let mut i_delta = i_lm.wrapping_sub(MODEL_WEIGHT) >> 2;
        if i_delta <= -8 {
            i_delta += 4;
            if i_delta < -16 {
                i_delta = -16;
            }
            i_ms += i_delta;
            if i_ms < -8 {
                if m.flc_bits[j] == 0 {
                    i_ms = -8;
                } else {
                    i_ms = 0;
                    m.flc_bits[j] -= 1;
                }
            }
        } else if i_delta >= 8 {
            i_delta -= 4;
            if i_delta > 15 {
                i_delta = 15;
            }
            i_ms += i_delta;
            if i_ms > 8 {
                if m.flc_bits[j] >= 15 {
                    m.flc_bits[j] = 15;
                    i_ms = 8;
                } else {
                    i_ms = 0;
                    m.flc_bits[j] += 1;
                }
            }
        }
        m.flc_state[j] = i_ms;
        if cf == Y_ONLY {
            break;
        }
    }
}

fn reset_coding_context(c: &mut CodingContext) {
    c.model_ac = AdaptiveModel {
        band: BAND_AC,
        ..AdaptiveModel::default()
    };
    c.model_lp = AdaptiveModel {
        band: BAND_LP,
        flc_bits: [4, 4],
        ..AdaptiveModel::default()
    };
    c.model_dc = AdaptiveModel {
        band: BAND_DC,
        flc_bits: [8, 8],
        ..AdaptiveModel::default()
    };
    c.cbp_count_max = 1;
    c.cbp_count_zero = 1;
    c.cbp_model = CbpModel {
        count0: [-4, -4],
        count1: [4, 4],
        state: [0, 0],
    };
}

fn init_zigzag_scan(c: &mut CodingContext) {
    for i in 0..16 {
        c.scan_lowpass[i].u_scan = GRGI_ZIGZAG_INV4X4_LOWPASS[i];
        c.scan_horiz[i].u_scan = DCT_INDEX[0][GRGI_ZIGZAG_INV4X4_H[i]] as u32;
        c.scan_vert[i].u_scan = DCT_INDEX[0][GRGI_ZIGZAG_INV4X4_V[i]] as u32;
    }
}

//================================================================
// decode.c
//================================================================

const A_ALPHABET: [usize; NUM_VLC_TABLES] = [
    5, 4, 8, 7, 7, 12, 6, 6, 12, 6, 6, 7, 7, 12, 6, 6, 12, 6, 6, 7, 7,
];

/// `AllocateCodingContextDec`: `n` fresh, reset contexts for a plane of colour format `cf`, in
/// `ctxs`.
pub(super) fn allocate_coding_context_dec(
    cf: usize,
    n: usize,
    led: &Ledger,
    ctxs: &mut Vec<CodingContext>,
) -> Result<(), Error> {
    if !(1..=MAX_TILES).contains(&n) {
        return Err(Error::Codestream);
    }
    let cbp_size = match cf {
        Y_ONLY | N_COMPONENT | CMYK => 5,
        _ => 9,
    };
    led.reuse(ctxs, n, CodingContext::default())?;
    for c in ctxs.iter_mut() {
        c.ah_cbpcy.n_symbols = cbp_size;
        c.ah_cbpcy1.n_symbols = 5;
        for k in 0..NUM_VLC_TABLES {
            c.ah_expt[k].n_symbols = A_ALPHABET[k];
        }
        reset_coding_context_dec(c);
    }
    Ok(())
}

pub(super) fn reset_coding_context_dec(c: &mut CodingContext) {
    c.ah_cbpcy.initialize = false;
    c.ah_cbpcy1.initialize = false;
    for k in 0..NUM_VLC_TABLES {
        c.ah_expt[k].initialize = false;
    }
    adapt_lowpass_dec(c);
    adapt_highpass_dec(c);
    init_zigzag_scan(c);
    reset_coding_context(c);
}

fn adapt_lowpass_dec(c: &mut CodingContext) {
    for k in 0..CONTEXT_X + CT_DC {
        c.ah_expt[k].adapt_discriminant();
    }
}

fn adapt_highpass_dec(c: &mut CodingContext) {
    c.ah_cbpcy.adapt_discriminant();
    c.ah_cbpcy1.adapt_discriminant();
    for k in 0..CONTEXT_X {
        c.ah_expt[k + CONTEXT_X + CT_DC].adapt_discriminant();
    }
}

//================================================================
// segdec.c
//================================================================

fn get_huff(t: &[i16], io: &mut BitIO) -> i32 {
    let sym = i32::from(t[io.peek_bit16(HUFFMAN_DECODE_ROOT_BITS) as usize]);
    if sym < 0 {
        io.flush_bit16(HUFFMAN_DECODE_ROOT_BITS);
    } else {
        io.flush_bit16((sym & ((1 << HUFFMAN_DECODE_ROOT_BITS_LOG) - 1)) as u32);
    }
    let mut huff = sym >> HUFFMAN_DECODE_ROOT_BITS_LOG;
    if huff < 0 {
        huff = sym;
        loop {
            let idx = huff as isize + 32768 + io.get_bit16(1) as isize;
            if idx < 0 || idx as usize >= t.len() {
                return 0;
            }
            huff = i32::from(t[idx as usize]);
            if huff >= 0 {
                break;
            }
        }
    }
    huff
}

fn get_huff_short(t: &[i16], io: &mut BitIO) -> i32 {
    let sym = i32::from(t[io.peek_bit16(HUFFMAN_DECODE_ROOT_BITS) as usize]);
    io.flush_bit16((sym & ((1 << HUFFMAN_DECODE_ROOT_BITS_LOG) - 1)) as u32);
    sym >> HUFFMAN_DECODE_ROOT_BITS_LOG
}

const CBP_TAB: [i32; 4] = [6, 9, 10, 12];
const G_FLC0: [u32; 6] = [0, 2, 1, 2, 2, 0];
const G_OFF0: [u32; 6] = [0, 4, 2, 8, 12, 1];
const G_OUT0: [i32; 16] = [0, 15, 3, 12, 1, 2, 4, 8, 5, 6, 9, 10, 7, 11, 13, 14];

pub(super) const RUN_REMAP: [i32; 15] = [1, 2, 3, 5, 7, 1, 2, 3, 5, 7, 1, 2, 3, 4, 5];

fn decode_significant_run(i_max_run: isize, ah: &AdaptiveHuffman, io: &mut BitIO) -> i32 {
    if i_max_run < 5 {
        if i_max_run == 1 || io.get_bool16() != 0 {
            return 1;
        } else if i_max_run == 2 || io.get_bool16() != 0 {
            return 2;
        } else if i_max_run == 3 || io.get_bool16() != 0 {
            return 3;
        }
        return 4;
    }
    if i_max_run as usize >= G_SIGNIFICANT_RUN_BIN.len() {
        return 16; // corrupt stream; C reads past the table
    }
    let bin = G_SIGNIFICANT_RUN_BIN[i_max_run as usize];
    let sym = i32::from(ah.huf_dec_table[io.peek_bit16(HUFFMAN_DECODE_ROOT_BITS) as usize]); // get_huff_short, inlined by hand
    io.flush_bit16((sym & ((1 << HUFFMAN_DECODE_ROOT_BITS_LOG) - 1)) as u32);
    let idx = ((sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) as isize + bin * 5) as usize;
    // Branchless: flc is often zero, unpredictably.
    let flc = G_SIGNIFICANT_RUN_FIXED_LENGTH[idx];
    if io.nb < flc {
        io.refill();
    }
    let run = RUN_REMAP[idx] + ((io.acc >> 1) >> (63u32.wrapping_sub(flc) & 63)) as i32;
    io.flush_bit16(flc);
    run
}

fn decode_first_index(ah: &mut AdaptiveHuffman, io: &mut BitIO) -> i32 {
    let idx = get_huff(&ah.huf_dec_table, io);
    ah.disc += ah.delta[idx as usize];
    ah.disc1 += ah.delta1[idx as usize];
    idx
}

fn decode_index(i_loc: isize, ah: &mut AdaptiveHuffman, io: &mut BitIO) -> i32 {
    if i_loc < 15 {
        let sym = i32::from(ah.huf_dec_table[io.peek_bit16(HUFFMAN_DECODE_ROOT_BITS) as usize]); // get_huff_short, inlined by hand
        io.flush_bit16((sym & ((1 << HUFFMAN_DECODE_ROOT_BITS_LOG) - 1)) as u32);
        let idx = sym >> HUFFMAN_DECODE_ROOT_BITS_LOG;
        ah.disc += ah.delta[idx as usize];
        ah.disc1 += ah.delta1[idx as usize];
        idx
    } else if i_loc == 15 {
        if io.get_bool16() == 0 {
            0
        } else if io.get_bool16() == 0 {
            2
        } else {
            1 + 2 * io.get_bool16() as i32
        }
    } else {
        io.get_bit16(1) as i32
    }
}

const ABS_LEVEL_REMAP: [i32; 6] = [2, 3, 4, 6, 10, 14];
const ABS_LEVEL_FIXED_LENGTH: [u32; 6] = [0, 0, 1, 2, 2, 2];

fn decode_significant_abs_level(ah: &mut AdaptiveHuffman, io: &mut BitIO) -> i32 {
    let idx = get_huff(&ah.huf_dec_table, io) as u32;
    if idx > 6 {
        return 0;
    }
    ah.disc += ah.delta[idx as usize];
    if idx < 2 {
        return idx as i32 + 2;
    } else if idx < 6 {
        return ABS_LEVEL_REMAP[idx as usize]
            + io.get_bit16(ABS_LEVEL_FIXED_LENGTH[idx as usize]) as i32;
    }
    let mut fixed = io.get_bit16(4) + 4;
    if fixed == 19 {
        fixed += io.get_bit16(2);
        if fixed == 22 {
            fixed += io.get_bit16(3);
        }
    }
    let mut level = (2u32.wrapping_add(1u32 << fixed)) as i32;
    level = level.wrapping_add(io.get_bit32(fixed) as i32);
    level
}

/// Returns the run/level pairs of one lowpass block.
fn decode_block(
    chroma: bool,
    coef: &mut [i32; 32],
    ah: &mut [AdaptiveHuffman; NUM_VLC_TABLES],
    ctx_off: usize,
    io: &mut BitIO,
    mut i_location: isize,
) -> usize {
    let b = if chroma { 3 } else { 0 };
    let mut i_num_nonzero = 1usize;

    let idx = decode_first_index(&mut ah[ctx_off + b], io);
    let mut i_sr = idx & 1;
    let mut i_sr_n = idx >> 2;
    let mut i_cont = i_sr & i_sr_n;
    let mut sign = io.get_sign();
    if idx & 2 != 0 {
        coef[1] = (decode_significant_abs_level(&mut ah[6 + ctx_off + i_cont as usize], io) ^ sign)
            - sign;
    } else {
        coef[1] = 1 | sign;
    }
    coef[0] = 0;
    if i_sr == 0 {
        coef[0] = decode_significant_run(15 - i_location, &ah[0], io);
    }
    i_location += coef[0] as isize + 1;

    while i_sr_n != 0 {
        if i_num_nonzero >= 16 {
            return 17;
        }
        i_sr = i_sr_n & 1;
        coef[i_num_nonzero * 2] = 0;
        if i_sr == 0 {
            coef[i_num_nonzero * 2] = decode_significant_run(15 - i_location, &ah[0], io);
        }
        i_location += coef[i_num_nonzero * 2] as isize + 1;
        let idx = decode_index(i_location, &mut ah[ctx_off + b + i_cont as usize + 1], io);
        i_sr_n = idx >> 1;
        i_cont &= i_sr_n;
        sign = io.get_sign();
        if idx & 1 != 0 {
            coef[i_num_nonzero * 2 + 1] =
                (decode_significant_abs_level(&mut ah[6 + ctx_off + i_cont as usize], io) ^ sign)
                    - sign;
        } else {
            coef[i_num_nonzero * 2 + 1] = 1 | sign;
        }
        i_num_nonzero += 1;
    }
    i_num_nonzero
}

/// `decodeBlockHighpassGo` keeps the bit accumulator in locals; `io` is current only across calls
/// and at return.
pub(super) fn decode_block_highpass(
    chroma: bool,
    ah: &mut [AdaptiveHuffman; NUM_VLC_TABLES],
    io: &mut BitIO,
    i_qp: i32,
    coef: &mut [i32; 16],
    scan: &mut [AdaptiveScan; 16],
) -> usize {
    const CTX_OFF: usize = CT_DC + CONTEXT_X;
    let b = if chroma { 3 } else { 0 };
    // `ah[0]` is only read (the run table) and every other table is written, so split it off;
    // `rest[k]` is `ah[k + 1]`.
    let (head, rest) = ah.split_at_mut(1);
    let t_run = &head[0].huf_dec_table;
    let a1 = CTX_OFF + b - 1; // ah1[0]
    let lv = 6 + CTX_OFF - 1; // level table 0
    let mut i_loc: u32 = 1;
    let mut i_num_nonzero = 1usize;

    let (mut acc, mut nb) = (io.acc, io.nb);
    if nb < 24 {
        // index, sign, level peek and run: 19 bits
        (acc, nb) = io.fill(acc, nb);
    }
    let idx: i32;
    {
        let a = &mut rest[a1];
        let sym = i32::from(a.huf_dec_table[(acc >> 59) as usize]);
        if sym >= 0 {
            // decode_first_index, short code
            acc <<= (sym as u32) & 7;
            nb = nb.wrapping_sub((sym as u32) & 7);
            idx = sym >> HUFFMAN_DECODE_ROOT_BITS_LOG;
            a.disc += a.delta[(idx & 15) as usize];
            a.disc1 += a.delta1[(idx & 15) as usize];
        } else {
            io.acc = acc;
            io.nb = nb;
            idx = decode_first_index(a, io);
            (acc, nb) = (io.acc, io.nb);
            if nb < 16 {
                (acc, nb) = io.fill(acc, nb);
            }
        }
    }
    let mut i_sr = idx & 1;
    let mut i_sr_n = idx >> 2;
    let mut i_cont = i_sr & i_sr_n;
    let mut sign = ((acc as i64) >> 63) as i32;
    acc <<= 1;
    nb = nb.wrapping_sub(1);
    let mut level = (i_qp ^ sign).wrapping_sub(sign);
    if idx & 2 != 0 {
        let a = &mut rest[lv + i_cont as usize];
        let sym = i32::from(a.huf_dec_table[(acc >> 59) as usize]);
        if (sym as u32) < (2 << HUFFMAN_DECODE_ROOT_BITS_LOG) {
            // level 2 or 3
            acc <<= (sym as u32) & 7;
            nb = nb.wrapping_sub((sym as u32) & 7);
            a.disc += a.delta[((sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) & 15) as usize];
            level = level.wrapping_mul((sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) + 2);
        } else {
            io.acc = acc;
            io.nb = nb;
            level = level.wrapping_mul(decode_significant_abs_level(a, io));
            (acc, nb) = (io.acc, io.nb);
            if nb < 8 {
                (acc, nb) = io.fill(acc, nb);
            }
        }
    }
    if i_sr == 0 {
        // decode_significant_run(14), bin 0
        let sym = i32::from(t_run[(acc >> 59) as usize]);
        acc <<= (sym as u32) & 7;
        nb = nb.wrapping_sub((sym as u32) & 7);
        let ri = (sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) as usize;
        let flc = G_SIGNIFICANT_RUN_FIXED_LENGTH[ri];
        i_loc = i_loc.wrapping_add(
            (RUN_REMAP[ri] + ((acc >> 1) >> (63u32.wrapping_sub(flc) & 63)) as i32) as u32,
        );
        acc <<= flc & 63;
        nb = nb.wrapping_sub(flc);
    }
    i_loc &= 0xf;
    coef[scan[i_loc as usize].u_scan as usize & 15] = level;
    scan[i_loc as usize].u_total += 1;
    if i_loc != 0 && scan[i_loc as usize].u_total > scan[i_loc as usize - 1].u_total {
        scan.swap(i_loc as usize, i_loc as usize - 1);
    }
    i_loc = (i_loc + 1) & 0xf;

    while i_sr_n != 0 {
        i_sr = i_sr_n & 1;
        if i_num_nonzero > 16 {
            io.acc = acc;
            io.nb = nb;
            return i_num_nonzero;
        }
        if nb < 24 {
            // run, index, sign and level peek: 19 bits
            (acc, nb) = io.fill(acc, nb);
        }
        if i_sr == 0 {
            // decode_significant_run, inlined by hand
            let mut m = 14 - i_loc as isize; // i_max_run - 1
            let run: u32;
            if m >= 4 {
                if m >= 14 {
                    io.acc = acc;
                    io.nb = nb;
                    return 16;
                }
                let sym = i32::from(t_run[(acc >> 59) as usize]);
                acc <<= (sym as u32) & 7;
                nb = nb.wrapping_sub((sym as u32) & 7);
                let ri = ((sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) as isize
                    + G_SIGNIFICANT_RUN_BIN[(m + 1) as usize] * 5)
                    as usize;
                let flc = G_SIGNIFICANT_RUN_FIXED_LENGTH[ri];
                run =
                    (RUN_REMAP[ri] + ((acc >> 1) >> (63u32.wrapping_sub(flc) & 63)) as i32) as u32;
                acc <<= flc & 63;
                nb = nb.wrapping_sub(flc);
            } else {
                if m < 0 {
                    m = 3; // i_max_run 0 reads like 4, then overruns
                }
                let k = (acc.leading_zeros() as isize).min(m);
                let mut n = k as u32;
                if k < m {
                    n += 1;
                }
                acc <<= n;
                nb = nb.wrapping_sub(n);
                run = k as u32 + 1;
            }
            i_loc = i_loc.wrapping_add(run);
            if i_loc >= 16 {
                io.acc = acc;
                io.nb = nb;
                return 16;
            }
        }
        // decode_index, inlined by hand
        let idx: i32;
        if i_loc < 14 {
            let a = &mut rest[a1 + i_cont as usize + 1];
            let sym = i32::from(a.huf_dec_table[(acc >> 59) as usize]);
            acc <<= (sym as u32) & 7;
            nb = nb.wrapping_sub((sym as u32) & 7);
            idx = sym >> HUFFMAN_DECODE_ROOT_BITS_LOG;
            a.disc += a.delta[(idx & 15) as usize];
            a.disc1 += a.delta1[(idx & 15) as usize];
        } else if i_loc == 14 {
            if (acc as i64) >= 0 {
                idx = 0;
                acc <<= 1;
                nb = nb.wrapping_sub(1);
            } else if ((acc << 1) as i64) >= 0 {
                idx = 2;
                acc <<= 2;
                nb = nb.wrapping_sub(2);
            } else {
                idx = 1 + 2 * (((acc >> 61) & 1) as i32);
                acc <<= 3;
                nb = nb.wrapping_sub(3);
            }
        } else {
            idx = (acc >> 63) as i32;
            acc <<= 1;
            nb = nb.wrapping_sub(1);
        }
        i_sr_n = idx >> 1;
        i_cont &= i_sr_n;
        sign = ((acc as i64) >> 63) as i32;
        acc <<= 1;
        nb = nb.wrapping_sub(1);
        level = (i_qp ^ sign).wrapping_sub(sign);
        if idx & 1 != 0 {
            let a = &mut rest[lv + i_cont as usize];
            let sym = i32::from(a.huf_dec_table[(acc >> 59) as usize]);
            if (sym as u32) < (2 << HUFFMAN_DECODE_ROOT_BITS_LOG) {
                // level 2 or 3
                acc <<= (sym as u32) & 7;
                nb = nb.wrapping_sub((sym as u32) & 7);
                a.disc += a.delta[((sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) & 15) as usize];
                level = level.wrapping_mul((sym >> HUFFMAN_DECODE_ROOT_BITS_LOG) + 2);
            } else {
                io.acc = acc;
                io.nb = nb;
                level = level.wrapping_mul(decode_significant_abs_level(a, io));
                (acc, nb) = (io.acc, io.nb);
            }
        }
        coef[scan[i_loc as usize].u_scan as usize & 15] = level;
        scan[i_loc as usize].u_total += 1;
        if i_loc != 0 && scan[i_loc as usize].u_total > scan[i_loc as usize - 1].u_total {
            scan.swap(i_loc as usize, i_loc as usize - 1);
        }
        i_loc = (i_loc + 1) & 0xf;
        i_num_nonzero += 1;
    }
    io.acc = acc;
    io.nb = nb;
    i_num_nonzero
}

/// Reads `f` flexbits per coefficient, 1 to 15.
fn flexbits(coef: &mut [i32; 16], order: &[usize; 16], io_fl: &mut BitIO, f: u32, q: i32) {
    // Branchless: flexbit signs are unpredictable.
    let (mut acc, mut nb) = (io_fl.acc, io_fl.nb);
    for k in 1..16 {
        let pk = order[k] & 15;
        let c = coef[pk];
        if nb < f + 1 {
            (acc, nb) = io_fl.fill(acc, nb);
        }
        let w = (acc >> (63u32.wrapping_sub(f) & 63)) as u32;
        let v = (w >> 1) as i32;
        let z = (((c as u32) | c.wrapping_neg() as u32) >> 31) as i32 - 1; // -1 iff c == 0
        let m = (c >> 31) | ((-((w & 1) as i32)) & z);
        coef[pk] = c.wrapping_add(q.wrapping_mul((v ^ m) - m));
        let n = f + (((v.wrapping_neg() as u32) >> 31) & (z as u32));
        acc <<= n & 63;
        nb = nb.wrapping_sub(n);
    }
    io_fl.acc = acc;
    io_fl.nb = nb;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_block_adaptive(
    no_skip: bool,
    chroma: bool,
    ah: &mut [AdaptiveHuffman; NUM_VLC_TABLES],
    ios: &mut IoSet<'_>,
    (io_ac, io_fl): (usize, usize),
    coef: &mut [i32; 16],
    scan: &mut [AdaptiveScan; 16],
    (i_model_bits, i_trim, i_qp): (i32, i32, i32),
    order: &[usize; 16],
    skip_flexbits: bool,
) -> usize {
    let mut i_num_nonzero = 0;
    let mut i_flex = i_model_bits - i_trim;
    if i_flex < 0 || skip_flexbits {
        i_flex = 0;
    }
    if no_skip {
        let i_qp1 = i_qp << (i_model_bits as u32 & 31);
        i_num_nonzero = decode_block_highpass(chroma, ah, &mut ios.v[io_ac], i_qp1, coef, scan);
    }
    if i_flex != 0 {
        let f = i_flex as u32;
        let mut q = i_qp << (i_trim as u32 & 31);
        if i_qp.wrapping_add(i_trim) == 1 {
            q = 1;
        }
        flexbits(coef, order, &mut ios.v[io_fl], f, q);
    }
    i_num_nonzero
}

fn decode_qp_index(io: &mut BitIO, c_bits: u8) -> u8 {
    if io.get_bit16(1) == 0 {
        return 0;
    }
    (io.get_bit16(u32::from(c_bits)) + 1) as u8
}

const LP_REMAP: [usize; 7] = [4, 1, 2, 3, 5, 6, 7];

impl StrCodec {
    pub(super) fn decode_cbp(&mut self, ctx: &mut CodingContext, ios: &mut IoSet<'_>) {
        let io = &mut ios.v[ctx.io_ac];
        let cf = self.param.cf_color_format;
        let mut i_channel = 1;
        if cf == N_COMPONENT || cf == CMYK {
            i_channel = self.param.c_num_channels;
        }
        let ah_cbp = &mut ctx.ah_cbpcy;
        let ah_cbp1 = &mut ctx.ah_cbpcy1;
        let ah_ex1 = &ctx.ah_expt[1];

        for i in 0..i_channel {
            let (mut cy, mut cu, mut cv) = (0i32, 0i32, 0i32);
            let mut n = get_huff_short(&ah_cbp1.huf_dec_table, io);
            ah_cbp1.disc += ah_cbp1.delta[n as usize];
            match n {
                2 => {
                    n = io.get_bit16(2) as i32;
                    n = match n {
                        0 => 3,
                        1 => 5,
                        _ => CBP_TAB[(n * 2 + io.get_bool16() as i32 - 4) as usize],
                    };
                }
                1 => n = 1 << io.get_bit16(2),
                3 => n = 0xf ^ (1 << io.get_bit16(2)),
                4 => n = 0xf,
                _ => {}
            }

            for i_block in 0..4 {
                if n & (1 << i_block) == 0 {
                    continue;
                }
                let mut nb = get_huff(&ah_cbp.huf_dec_table, io);
                let mut val = (nb as u32).wrapping_add(1);
                ah_cbp.disc += ah_cbp.delta[nb as usize];
                nb = 0;
                if val >= 6 {
                    if io.get_bool16() != 0 {
                        nb = 0x10;
                    } else if io.get_bool16() != 0 {
                        nb = 0x20;
                    } else {
                        nb = 0x30;
                    }
                    if val == 9 && io.get_bool16() == 0 {
                        if io.get_bool16() != 0 {
                            val = 10;
                        } else {
                            val = 11;
                        }
                    }
                    val = val.wrapping_sub(6);
                }
                if val >= 6 {
                    self.err = Some(Error::Codestream);
                    return;
                }
                let mut code1 = G_OFF0[val as usize];
                if G_FLC0[val as usize] != 0 {
                    code1 += io.get_bit16(G_FLC0[val as usize]);
                }
                nb += G_OUT0[code1 as usize];

                match cf {
                    YUV444 => {
                        cy |= (nb & 0xf) << (i_block * 4);
                        for k in 0..2 {
                            if (nb >> (k + 4)) & 1 == 0 {
                                continue;
                            }
                            let mut code = get_huff_short(&ah_ex1.huf_dec_table, io);
                            match code {
                                1 => {
                                    code = io.get_bit16(2) as i32;
                                    code = match code {
                                        0 => 3,
                                        1 => 5,
                                        _ => {
                                            CBP_TAB
                                                [(code * 2 + io.get_bool16() as i32 - 4) as usize]
                                        }
                                    };
                                }
                                0 => code = 1 << io.get_bit16(2),
                                2 => code = 0xf ^ (1 << io.get_bit16(2)),
                                3 => code = 0xf,
                                _ => {}
                            }
                            if k == 0 {
                                cu |= code << (i_block * 4);
                            } else {
                                cv |= code << (i_block * 4);
                            }
                        }
                    }
                    YUV420 => {
                        cy |= (nb & 0xf) << (i_block * 4);
                        cu |= ((nb >> 4) & 1) << i_block;
                        cv |= ((nb >> 5) & 1) << i_block;
                    }
                    YUV422 => {
                        cy |= (nb & 0xf) << (i_block * 4);
                        const SHIFT: [i32; 4] = [0, 1, 4, 5];
                        for k in 0..2 {
                            let mut code = 5i32;
                            if (nb >> (k + 4)) & 1 != 0 {
                                if io.get_bool16() != 0 {
                                    code = 1;
                                } else if io.get_bool16() != 0 {
                                    code = 4;
                                }
                                code <<= SHIFT[i_block as usize];
                                if k == 0 {
                                    cu |= code;
                                } else {
                                    cv |= code;
                                }
                            }
                        }
                    }
                    _ => cy |= nb << (i_block * 4),
                }
            }

            self.mb.i_diff_cbp[i] = cy;
            if cf == YUV420 || cf == YUV444 || cf == YUV422 {
                self.mb.i_diff_cbp[1] = cu;
                self.mb.i_diff_cbp[2] = cv;
            }
        }
    }

    pub(super) fn decode_coeffs(
        &mut self,
        ctx: &mut CodingContext,
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        let tc = self.c_tile_column as usize;
        let cf = self.param.cf_color_format;
        let i_channels = self.param.c_num_channels;
        let mut i_planes = i_channels;
        if cf == YUV420 || cf == YUV422 {
            i_planes = 1;
        }
        let mut i_model_bits = ctx.model_ac.flc_bits[0];
        let mut lm = [0i32; 2];
        let mut p_lm = 0usize;
        let order = &DCT_INDEX[0];
        let mut chroma = false;
        let io_pair = (ctx.io_ac, ctx.io_fl);
        let trim = ctx.trim_flex_bits;
        let scan = if self.mb.i_orientation == 1 {
            &mut ctx.scan_vert
        } else {
            &mut ctx.scan_horiz
        };
        let ah = &mut ctx.ah_expt;
        let i_cbpcu = self.mb.i_cbp[1];
        let i_cbpcv = self.mb.i_cbp[2];
        let mut i_cbpcy = self.mb.i_cbp[0];
        let mut i_n_blocks = 4;
        match cf {
            YUV420 => {
                i_n_blocks = 6;
                i_cbpcy += (i_cbpcu << 16) + (i_cbpcv << 20);
            }
            YUV422 => {
                i_n_blocks = 8;
                i_cbpcy += (i_cbpcu << 16) + (i_cbpcv << 24);
            }
            _ => {}
        }

        for i in 0..i_planes {
            let mut i_index = 0usize;
            for i_block in 0..i_n_blocks {
                let mut qch = 0usize;
                if i_planes > 1 {
                    qch = i;
                } else if i_block > 3 {
                    if cf == YUV420 {
                        qch = i_block - 3;
                    } else {
                        qch = i_block / 2 - 1;
                    }
                }
                let i_qp = self.tiles[tc].q_hp[qch][self.mb.i_q_index_hp as usize].i_qp;

                for i_sub in 0..4 {
                    let start = if i_block >= 4 {
                        if cf == YUV420 {
                            self.p1(i_block - 3) + BLK_OFFSET_UV[i_sub]
                        } else {
                            let ch = 1 + (1 & (i_block >> 1));
                            self.p1(ch) + (i_block & 1) * 32 + BLK_OFFSET_UV422[i_sub]
                        }
                    } else {
                        self.p1(i) + BLK_OFFSET[i_index & 0xf]
                    };
                    let Some(blk) = self.mb_buf[start..].first_chunk_mut::<16>() else {
                        return Err(Error::Codestream);
                    };
                    let n = decode_block_adaptive(
                        i_cbpcy & 1 != 0,
                        chroma,
                        ah,
                        ios,
                        io_pair,
                        blk,
                        scan,
                        (i_model_bits, trim, i_qp),
                        order,
                        self.dp.b_skip_flexbits,
                    );
                    if n > 16 {
                        return Err(Error::Codestream);
                    }
                    lm[p_lm] = lm[p_lm].wrapping_add(n as i32);
                    i_index += 1;
                    i_cbpcy >>= 1;
                }
                if i_block == 3 {
                    i_model_bits = ctx.model_ac.flc_bits[1];
                    p_lm = 1;
                    chroma = true;
                }
            }
            i_cbpcy = self.mb.i_cbp[(i + 1) & 0xf];
        }
        update_model_mb(cf, i_channels, &mut lm, &mut ctx.model_ac);
        Ok(())
    }

    pub(super) fn decode_macroblock_lowpass(
        &mut self,
        ctx: &mut CodingContext,
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        let cf = self.param.cf_color_format;
        let i_channels = self.param.c_num_channels;
        let mut i_full_planes = i_channels;
        if cf == YUV420 || cf == YUV422 {
            i_full_planes = 2;
        }
        let io = &mut ios.v[ctx.io_lp];
        let mut i_model_bits = ctx.model_lp.flc_bits[0];
        let mut rl = [0i32; 32];
        let mut lm = [0i32; 2];
        let mut p_lm = 0usize;
        let mut i_cbp: i32 = 0;
        let tc = self.c_tile_column as usize;

        if self.wmiscp.bf_bitstream_format != SPATIAL && self.tiles[tc].c_bits_lp > 0 {
            self.mb.i_q_index_lp = decode_qp_index(io, self.tiles[tc].c_bits_lp);
            if self.mb.i_q_index_lp >= self.tiles[tc].c_num_qp_lp {
                // jxrlib reads past the table
                return Err(Error::Codestream);
            }
        }

        if self.reset_rgi_totals {
            const I_SCALE: u32 = 2;
            let mut i_weight = I_SCALE * 16;
            ctx.scan_lowpass[0].u_total = MAX_TOTAL;
            for k in 1..16 {
                ctx.scan_lowpass[k].u_total = i_weight;
                i_weight = i_weight.wrapping_sub(I_SCALE);
            }
        }

        if cf == YUV420 || cf == YUV422 || cf == YUV444 {
            let (mut i_count_m, mut i_count_z) = (ctx.cbp_count_max, ctx.cbp_count_zero);
            let i_max = (i_full_planes * 4) as i32 - 5;
            if i_count_z <= 0 || i_count_m < 0 {
                i_cbp = 0;
                if io.get_bool16() != 0 {
                    i_cbp = 1;
                    let k = io.get_bit16((i_full_planes - 1) as u32) as i32;
                    if k != 0 {
                        i_cbp = k * 2 + io.get_bit16(1) as i32;
                    }
                }
                if i_count_m < i_count_z {
                    i_cbp = i_max - i_cbp;
                }
            } else {
                i_cbp = io.get_bit16(i_full_planes as u32) as i32;
            }
            if i_cbp == i_max {
                i_count_m += 1 - 4;
            } else {
                i_count_m += 1;
            }
            if i_cbp == 0 {
                i_count_z += 1 - 4;
            } else {
                i_count_z += 1;
            }
            i_count_m = i_count_m.clamp(-8, 7);
            ctx.cbp_count_max = i_count_m;
            i_count_z = i_count_z.clamp(-8, 7);
            ctx.cbp_count_zero = i_count_z;
        } else {
            for ch in 0..i_channels {
                i_cbp |= (io.get_bit16(1) as i32) << ch;
            }
        }

        for ch in 0..i_full_planes {
            if i_cbp & 1 != 0 {
                let mut loc = 1isize;
                if cf == YUV420 && ch == 1 {
                    loc += 9;
                }
                if cf == YUV422 && ch == 1 {
                    loc += 1;
                }
                let n = decode_block(ch > 0, &mut rl, &mut ctx.ah_expt, CT_DC, io, loc);
                if n > 16 {
                    return Err(Error::Codestream);
                }
                lm[p_lm] = lm[p_lm].wrapping_add(n as i32);
                if (cf == YUV420 || cf == YUV422) && ch != 0 {
                    let mut tmp = [0i32; 16];
                    let mut remap: &[usize] = &LP_REMAP;
                    let mut count = 14;
                    if cf == YUV420 {
                        remap = &LP_REMAP[1..];
                        count = 6;
                    }
                    let mut idx = 0isize;
                    for k in 0..n {
                        idx += rl[k * 2] as isize;
                        tmp[(idx & 0xf) as usize] = rl[k * 2 + 1];
                        idx += 1;
                    }
                    for k in 0..count {
                        self.mb.i_block_dc[(k & 1) + 1][remap[k >> 1]] = tmp[k];
                    }
                } else {
                    let coef = &mut self.mb.i_block_dc[ch];
                    let scan = &mut ctx.scan_lowpass;
                    let mut idx = 1isize;
                    for k in 0..n {
                        idx += rl[k * 2] as isize;
                        if !(1..=15).contains(&idx) {
                            return Err(Error::Codestream);
                        }
                        let u = idx as usize;
                        coef[scan[u].u_scan as usize] = rl[k * 2 + 1];
                        scan[u].u_total += 1;
                        if scan[u].u_total > scan[u - 1].u_total {
                            scan.swap(u, u - 1);
                        }
                        idx += 1;
                    }
                }
            }

            if i_model_bits != 0 {
                let mbits = i_model_bits as u32;
                if (cf == YUV420 || cf == YUV422) && ch != 0 {
                    let kmax = if cf == YUV420 { 4 } else { 8 };
                    for k in 1..kmax {
                        for c in 1..=2 {
                            let d = &mut self.mb.i_block_dc[c][k];
                            if *d > 0 {
                                *d <<= mbits;
                                *d = d.wrapping_add(io.get_bit32(mbits) as i32);
                            } else if *d < 0 {
                                *d <<= mbits;
                                *d = d.wrapping_sub(io.get_bit32(mbits) as i32);
                            } else {
                                *d = io.get_bit32(mbits) as i32;
                                if *d != 0 && io.get_bool16() != 0 {
                                    *d = d.wrapping_neg();
                                }
                            }
                        }
                    }
                } else {
                    let coef = &mut self.mb.i_block_dc[ch];
                    for k in 1..16 {
                        if coef[k] > 0 {
                            coef[k] <<= mbits;
                            coef[k] = coef[k].wrapping_add(io.get_bit32(mbits) as i32);
                        } else if coef[k] < 0 {
                            coef[k] <<= mbits;
                            coef[k] = coef[k].wrapping_sub(io.get_bit32(mbits) as i32);
                        } else {
                            coef[k] = io.get_bit16s(mbits);
                        }
                    }
                }
            }
            p_lm = 1;
            i_model_bits = ctx.model_lp.flc_bits[1];
            i_cbp >>= 1;
        }

        update_model_mb(cf, i_channels, &mut lm, &mut ctx.model_lp);
        if self.reset_context {
            adapt_lowpass_dec(ctx);
        }
        Ok(())
    }

    pub(super) fn decode_macroblock_dc(
        &mut self,
        ctx: &mut CodingContext,
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        let tc = self.c_tile_column as usize;
        let cf = self.param.cf_color_format;
        let i_channels = self.param.c_num_channels;
        let io = &mut ios.v[ctx.io_dc];
        let mut lm = [0i32; 2];
        let mut p_lm = 0usize;
        let mut i_model_bits = ctx.model_dc.flc_bits[0];

        for i in 0..i_channels {
            self.mb.i_block_dc[i] = [0; 16];
        }
        self.mb.i_q_index_lp = 0;
        self.mb.i_q_index_hp = 0;

        let (c_bits_lp, c_bits_hp, c_num_qp_lp, c_num_qp_hp) = {
            let t = &self.tiles[tc];
            (t.c_bits_lp, t.c_bits_hp, t.c_num_qp_lp, t.c_num_qp_hp)
        };
        if self.wmiscp.bf_bitstream_format == SPATIAL && self.wmiscp.sb_subband != SB_DC_ONLY {
            if c_bits_lp > 0 {
                self.mb.i_q_index_lp = decode_qp_index(io, c_bits_lp);
            }
            if self.wmiscp.sb_subband != SB_NO_HIGHPASS && c_bits_hp > 0 {
                self.mb.i_q_index_hp = decode_qp_index(io, c_bits_hp);
            }
        }
        if c_bits_hp == 0 && c_num_qp_hp > 1 {
            self.mb.i_q_index_hp = self.mb.i_q_index_lp;
        }
        if self.mb.i_q_index_lp >= c_num_qp_lp || self.mb.i_q_index_hp >= c_num_qp_hp {
            return Err(Error::Codestream);
        }

        fn read_dc(
            io: &mut BitIO,
            nonzero: bool,
            ah: &mut AdaptiveHuffman,
            lm: &mut [i32; 2],
            p_lm: usize,
            i_model_bits: i32,
        ) -> i32 {
            let mut q = 0i32;
            if nonzero {
                q = decode_significant_abs_level(ah, io) - 1;
                lm[p_lm] = lm[p_lm].wrapping_add(1);
            }
            if i_model_bits != 0 {
                q = (q << (i_model_bits as u32 & 31)) | io.get_bit16(i_model_bits as u32) as i32;
            }
            if q != 0 && io.get_bool16() != 0 {
                q = q.wrapping_neg();
            }
            q
        }

        if cf == Y_ONLY || cf == CMYK || cf == N_COMPONENT {
            for i in 0..i_channels {
                let nz = io.get_bool16() != 0;
                self.mb.i_block_dc[i][0] =
                    read_dc(io, nz, &mut ctx.ah_expt[3], &mut lm, p_lm, i_model_bits);
                p_lm = 1;
                i_model_bits = ctx.model_dc.flc_bits[1];
            }
        } else {
            let idx = get_huff(&ctx.ah_expt[2].huf_dec_table, io);
            let qy = idx >> 2;
            let qu = (idx >> 1) & 1;
            let qv = idx & 1;
            self.mb.i_block_dc[0][0] = read_dc(
                io,
                qy != 0,
                &mut ctx.ah_expt[3],
                &mut lm,
                p_lm,
                i_model_bits,
            );
            p_lm = 1;
            i_model_bits = ctx.model_dc.flc_bits[1];
            self.mb.i_block_dc[1][0] = read_dc(
                io,
                qu != 0,
                &mut ctx.ah_expt[4],
                &mut lm,
                p_lm,
                i_model_bits,
            );
            self.mb.i_block_dc[2][0] = read_dc(
                io,
                qv != 0,
                &mut ctx.ah_expt[4],
                &mut lm,
                p_lm,
                i_model_bits,
            );
        }

        update_model_mb(cf, i_channels, &mut lm, &mut ctx.model_dc);

        if self.wmiscp.sb_subband == SB_DC_ONLY && self.reset_context {
            for kk in 2..5 {
                ctx.ah_expt[kk].adapt_discriminant();
            }
        }
        Ok(())
    }

    pub(super) fn decode_macroblock_highpass(
        &mut self,
        ctx: &mut CodingContext,
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        if self.reset_rgi_totals {
            const I_SCALE: u32 = 2;
            let mut i_weight = I_SCALE * 16;
            ctx.scan_horiz[0].u_total = MAX_TOTAL;
            ctx.scan_vert[0].u_total = MAX_TOTAL;
            for k in 1..16 {
                ctx.scan_horiz[k].u_total = i_weight;
                ctx.scan_vert[k].u_total = i_weight;
                i_weight = i_weight.wrapping_sub(I_SCALE);
            }
        }
        let tc = self.c_tile_column as usize;
        let (c_bits_hp, c_num_qp_hp) = (self.tiles[tc].c_bits_hp, self.tiles[tc].c_num_qp_hp);
        if self.wmiscp.bf_bitstream_format != SPATIAL && c_bits_hp > 0 {
            self.mb.i_q_index_hp = decode_qp_index(&mut ios.v[ctx.io_ac], c_bits_hp);
            if self.mb.i_q_index_hp >= c_num_qp_hp {
                return Err(Error::Codestream);
            }
        } else if c_bits_hp == 0 && c_num_qp_hp > 1 {
            self.mb.i_q_index_hp = self.mb.i_q_index_lp;
        }

        self.decode_cbp(ctx, ios);
        if let Some(e) = self.err.clone() {
            return Err(e);
        }
        self.pred_cbp_dec(&mut ctx.cbp_model);
        self.decode_coeffs(ctx, ios)?;
        if self.reset_context {
            adapt_highpass_dec(ctx);
        }
        Ok(())
    }
}
