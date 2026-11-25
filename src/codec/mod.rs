// Ported from jxrlib strcodec.c/.h, windowsmediaphoto.h (BSD-2, see NOTICE).

//! Decodes a JPEG XR codestream. A port of jxrlib's decode path (through the Go port); names
//! follow the C source, in snake case.
// The port keeps jxrlib's field and constant set, including entries the decode path never reads,
// and its index-heavy loops and long argument lists.
#![allow(
    dead_code,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::manual_range_contains
)]

mod adapthuff;
#[cfg(target_arch = "x86_64")]
mod bgr8;
mod output;
mod predquant;
mod segdec;
#[allow(unsafe_code)]
pub(crate) mod simd;
mod strdec;
mod transform;
mod wrap;
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
mod x86;

#[cfg(test)]
mod tests;

pub(crate) use strdec::{Format, Header, Scratch, TileTables, decode};

use crate::Error;
use crate::limits::{Ledger, refill};
use crate::par::{Ctx, OnRows};
use adapthuff::AdaptiveHuffman;

// Colour formats (COLORFORMAT).
pub(crate) const Y_ONLY: usize = 0;
pub(crate) const YUV420: usize = 1;
pub(crate) const YUV422: usize = 2;
pub(crate) const YUV444: usize = 3;
pub(crate) const CMYK: usize = 4;
pub(crate) const N_COMPONENT: usize = 6;
pub(crate) const CF_RGB: usize = 7;
pub(crate) const CF_RGBE: usize = 8;

// Bit depths (BITDEPTH_BITS).
pub(crate) const BD1: usize = 0;
pub(crate) const BD8: usize = 1;
pub(crate) const BD16: usize = 2;
pub(crate) const BD16S: usize = 3;
pub(crate) const BD16F: usize = 4;
pub(crate) const BD32: usize = 5;
pub(crate) const BD32S: usize = 6;
pub(crate) const BD32F: usize = 7;
pub(crate) const BD5: usize = 8;
pub(crate) const BD10: usize = 9;
pub(crate) const BD565: usize = 10;
pub(crate) const BD1_ALT: usize = 0xf;

const OL_NONE: usize = 0;
const OL_ONE: usize = 1;
const OL_TWO: usize = 2;

const SPATIAL: usize = 0;
const FREQUENCY: usize = 1;

const SB_ALL: usize = 0;
const SB_NO_FLEXBITS: usize = 1;
const SB_NO_HIGHPASS: usize = 2;
const SB_DC_ONLY: usize = 3;
const SB_ISOLATED: usize = 4;

const MAX_CHANNELS: usize = 16;
const LOG_MAX_TILES: u32 = 12;
const MAX_TILES: usize = 1 << LOG_MAX_TILES;

const CODEC_SUBVERSION: u32 = 0;
const CODEC_SUBVERSION_SOFT_TILE: u32 = 1;
const CODEC_SUBVERSION_HARD_TILE: u32 = 9;

const CONTEXT_X: usize = 8;
const CT_DC: usize = 5;
const NUM_VLC_TABLES: usize = 21;

const SHIFT_ZERO: i32 = 1;
const QP_FRAC_BITS: i32 = 2;

const BAND_DC: i32 = 1;
const BAND_LP: i32 = 2;
const BAND_AC: i32 = 3;

const BLK_OFFSET: [usize; 16] = [
    0, 64, 16, 80, 128, 192, 144, 208, 32, 96, 48, 112, 160, 224, 176, 240,
];
const BLK_OFFSET_UV: [usize; 4] = [0, 32, 16, 48];
const BLK_OFFSET_UV422: [usize; 8] = [0, 64, 16, 80, 32, 96, 48, 112];

const DCT_INDEX: [[usize; 16]; 3] = [
    [0, 5, 1, 6, 10, 12, 8, 14, 2, 4, 3, 7, 9, 13, 11, 15],
    [0, 5, 1, 6, 10, 12, 8, 14, 2, 4, 3, 7, 9, 13, 11, 15],
    [
        0, 128, 64, 208, 32, 240, 48, 224, 16, 192, 80, 144, 112, 176, 96, 160,
    ],
];

const IDX_CC: [[u8; 16]; 16] = [
    [
        0x00, 0x01, 0x05, 0x04, 0x40, 0x41, 0x45, 0x44, 0x80, 0x81, 0x85, 0x84, 0xc0, 0xc1, 0xc5,
        0xc4,
    ],
    [
        0x02, 0x03, 0x07, 0x06, 0x42, 0x43, 0x47, 0x46, 0x82, 0x83, 0x87, 0x86, 0xc2, 0xc3, 0xc7,
        0xc6,
    ],
    [
        0x0a, 0x0b, 0x0f, 0x0e, 0x4a, 0x4b, 0x4f, 0x4e, 0x8a, 0x8b, 0x8f, 0x8e, 0xca, 0xcb, 0xcf,
        0xce,
    ],
    [
        0x08, 0x09, 0x0d, 0x0c, 0x48, 0x49, 0x4d, 0x4c, 0x88, 0x89, 0x8d, 0x8c, 0xc8, 0xc9, 0xcd,
        0xcc,
    ],
    [
        0x10, 0x11, 0x15, 0x14, 0x50, 0x51, 0x55, 0x54, 0x90, 0x91, 0x95, 0x94, 0xd0, 0xd1, 0xd5,
        0xd4,
    ],
    [
        0x12, 0x13, 0x17, 0x16, 0x52, 0x53, 0x57, 0x56, 0x92, 0x93, 0x97, 0x96, 0xd2, 0xd3, 0xd7,
        0xd6,
    ],
    [
        0x1a, 0x1b, 0x1f, 0x1e, 0x5a, 0x5b, 0x5f, 0x5e, 0x9a, 0x9b, 0x9f, 0x9e, 0xda, 0xdb, 0xdf,
        0xde,
    ],
    [
        0x18, 0x19, 0x1d, 0x1c, 0x58, 0x59, 0x5d, 0x5c, 0x98, 0x99, 0x9d, 0x9c, 0xd8, 0xd9, 0xdd,
        0xdc,
    ],
    [
        0x20, 0x21, 0x25, 0x24, 0x60, 0x61, 0x65, 0x64, 0xa0, 0xa1, 0xa5, 0xa4, 0xe0, 0xe1, 0xe5,
        0xe4,
    ],
    [
        0x22, 0x23, 0x27, 0x26, 0x62, 0x63, 0x67, 0x66, 0xa2, 0xa3, 0xa7, 0xa6, 0xe2, 0xe3, 0xe7,
        0xe6,
    ],
    [
        0x2a, 0x2b, 0x2f, 0x2e, 0x6a, 0x6b, 0x6f, 0x6e, 0xaa, 0xab, 0xaf, 0xae, 0xea, 0xeb, 0xef,
        0xee,
    ],
    [
        0x28, 0x29, 0x2d, 0x2c, 0x68, 0x69, 0x6d, 0x6c, 0xa8, 0xa9, 0xad, 0xac, 0xe8, 0xe9, 0xed,
        0xec,
    ],
    [
        0x30, 0x31, 0x35, 0x34, 0x70, 0x71, 0x75, 0x74, 0xb0, 0xb1, 0xb5, 0xb4, 0xf0, 0xf1, 0xf5,
        0xf4,
    ],
    [
        0x32, 0x33, 0x37, 0x36, 0x72, 0x73, 0x77, 0x76, 0xb2, 0xb3, 0xb7, 0xb6, 0xf2, 0xf3, 0xf7,
        0xf6,
    ],
    [
        0x3a, 0x3b, 0x3f, 0x3e, 0x7a, 0x7b, 0x7f, 0x7e, 0xba, 0xbb, 0xbf, 0xbe, 0xfa, 0xfb, 0xff,
        0xfe,
    ],
    [
        0x38, 0x39, 0x3d, 0x3c, 0x78, 0x79, 0x7d, 0x7c, 0xb8, 0xb9, 0xbd, 0xbc, 0xf8, 0xf9, 0xfd,
        0xfc,
    ],
];

const IDX_CC420: [[u8; 8]; 8] = [
    [0x00, 0x01, 0x05, 0x04, 0x20, 0x21, 0x25, 0x24],
    [0x02, 0x03, 0x07, 0x06, 0x22, 0x23, 0x27, 0x26],
    [0x0a, 0x0b, 0x0f, 0x0e, 0x2a, 0x2b, 0x2f, 0x2e],
    [0x08, 0x09, 0x0d, 0x0c, 0x28, 0x29, 0x2d, 0x2c],
    [0x10, 0x11, 0x15, 0x14, 0x30, 0x31, 0x35, 0x34],
    [0x12, 0x13, 0x17, 0x16, 0x32, 0x33, 0x37, 0x36],
    [0x1a, 0x1b, 0x1f, 0x1e, 0x3a, 0x3b, 0x3f, 0x3e],
    [0x18, 0x19, 0x1d, 0x1c, 0x38, 0x39, 0x3d, 0x3c],
];

const CBLK_CHROMAS: [usize; 9] = [0, 4, 8, 16, 16, 16, 16, 0, 0];

//================================================================
// Bit I/O
//================================================================

/// Reads MSB first from `buf`. Reads past the end yield zero bits.
#[derive(Clone, Copy)]
pub(crate) struct BitIO<'a> {
    buf: &'a [u8],
    /// Unread bits, MSB aligned.
    acc: u64,
    /// Valid bits in `acc`.
    nb: u32,
    /// Next byte to load into `acc`.
    off: u64,
}

impl<'a> BitIO<'a> {
    fn new() -> Self {
        BitIO {
            buf: &[],
            acc: 0,
            nb: 0,
            off: 0,
        }
    }

    fn attach(&mut self, buf: &'a [u8], byte_off: u64) {
        self.buf = buf;
        self.acc = 0;
        self.nb = 0;
        self.off = byte_off;
    }

    /// Tops `acc` up to at least 56 bits.
    fn refill(&mut self) {
        let mut v: u64 = 0;
        let len = self.buf.len() as u64;
        if self.off + 8 <= len {
            let o = self.off as usize;
            let mut a = [0u8; 8];
            a.copy_from_slice(&self.buf[o..o + 8]);
            v = u64::from_be_bytes(a);
        } else {
            for i in 0..8u64 {
                v <<= 8;
                if self.off + i < len {
                    v |= u64::from(self.buf[(self.off + i) as usize]);
                }
            }
        }
        self.acc |= v >> (self.nb & 63);
        self.off += u64::from(63u32.wrapping_sub(self.nb) >> 3);
        self.nb |= 56;
    }

    /// `refill` for callers holding `acc` and `nb` in locals.
    fn fill(&mut self, acc: u64, nb: u32) -> (u64, u32) {
        self.acc = acc;
        self.nb = nb;
        self.refill();
        (self.acc, self.nb)
    }

    /// Returns the next `n` (1..=32) bits.
    fn peek_bit16(&mut self, n: u32) -> u32 {
        if self.nb < n {
            self.refill();
        }
        (self.acc >> (64u32.wrapping_sub(n) & 63)) as u32
    }

    fn flush_bit16(&mut self, n: u32) {
        self.acc <<= n & 63;
        self.nb = self.nb.wrapping_sub(n);
    }

    fn get_bit16(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek_bit16(n);
        self.flush_bit16(n);
        v
    }

    fn get_bit32(&mut self, n: u32) -> u32 {
        self.get_bit16(n)
    }

    fn get_bool16(&mut self) -> u32 {
        self.get_bit16(1)
    }

    /// Returns 0 or -1.
    fn get_sign(&mut self) -> i32 {
        (self.get_bit16(1) as i32).wrapping_neg()
    }

    /// `_getBit16s`: a signed value, sign bit only if nonzero.
    fn get_bit16s(&mut self, n: u32) -> i32 {
        let mut r = self.peek_bit16(n + 1) as i32;
        r = ((r >> 1) ^ (-(r & 1))) + (r & 1);
        let mut adv = n;
        if r != 0 {
            adv += 1;
        }
        self.flush_bit16(adv);
        r
    }

    fn flush_to_byte(&mut self) {
        self.flush_bit16(self.nb & 7);
    }

    /// `getPosRead`.
    fn byte_pos(&self) -> u64 {
        self.off.wrapping_sub(u64::from((self.nb + 7) >> 3))
    }
}

/// The bit readers of one decode: index 0 is the header reader, 1.. are the per-tile-column
/// (spatial) or per-subband (frequency) readers. The primary and alpha planes share them, which
/// is why they live outside either plane.
pub(crate) struct IoSet<'a> {
    v: Vec<BitIO<'a>>,
    stream: &'a [u8],
}

impl IoSet<'_> {
    /// Points reader `k` at byte `off` of the stream.
    fn attach(&mut self, k: usize, off: u64) {
        let s = self.stream;
        self.v[k].attach(s, off);
    }
}

const HEADER_IO: usize = 0;

/// jxrlib's `SimpleBitIO`, used for the image headers.
struct SimpleBitIO<'a> {
    buf: &'a [u8],
    cb_read: usize,
    acc: u8,
    left: u32,
    err: bool,
}

impl SimpleBitIO<'_> {
    fn get_bit32(&mut self, mut c_bits: u32) -> u32 {
        let mut rc: u32 = 0;
        while self.left < c_bits {
            rc <<= self.left;
            rc |= u32::from(self.acc) >> (8 - self.left);
            c_bits -= self.left;
            if self.cb_read < self.buf.len() {
                self.acc = self.buf[self.cb_read];
            } else {
                self.err = true;
                self.acc = 0;
            }
            self.cb_read += 1;
            self.left = 8;
        }
        rc <<= c_bits;
        rc |= u32::from(self.acc) >> (8 - c_bits);
        // Go shifts everything out at 8; Rust would mask the count to 0.
        self.acc = self.acc.checked_shl(c_bits).unwrap_or(0);
        self.left -= c_bits;
        rc
    }

    fn flush_to_byte(&mut self) {
        self.acc = 0;
        self.left = 0;
    }
}

//================================================================
// Structures
//================================================================

#[derive(Clone, Copy, Default)]
struct Quantizer {
    i_index: u8,
    i_qp: i32,
    i_offset: i32,
    i_man: i32,
    i_exp: i32,
}

#[derive(Clone, Copy, Default)]
struct MbInfo {
    i_block_dc: [[i32; 16]; MAX_CHANNELS],
    i_orientation: i32,
    i_cbp: [i32; MAX_CHANNELS],
    i_diff_cbp: [i32; MAX_CHANNELS],
    i_q_index_lp: u8,
    i_q_index_hp: u8,
}

#[derive(Clone, Copy, Default)]
struct AdaptiveScan {
    u_total: u32,
    u_scan: u32,
}

#[derive(Clone, Copy, Default)]
struct AdaptiveModel {
    flc_state: [i32; 2],
    flc_bits: [i32; 2],
    band: i32,
}

#[derive(Clone, Copy, Default)]
struct CbpModel {
    count0: [i32; 2],
    count1: [i32; 2],
    state: [i32; 2],
}

/// `io_*` index the [`IoSet`].
#[derive(Clone, Default)]
struct CodingContext {
    io_dc: usize,
    io_lp: usize,
    io_ac: usize,
    io_fl: usize,

    ah_cbpcy: AdaptiveHuffman,
    ah_cbpcy1: AdaptiveHuffman,
    ah_expt: [AdaptiveHuffman; NUM_VLC_TABLES],

    scan_lowpass: [AdaptiveScan; 16],
    scan_horiz: [AdaptiveScan; 16],
    scan_vert: [AdaptiveScan; 16],

    model_ac: AdaptiveModel,
    model_lp: AdaptiveModel,
    model_dc: AdaptiveModel,

    cbp_count_zero: i32,
    cbp_count_max: i32,

    cbp_model: CbpModel,

    trim_flex_bits: i32,
    in_roi: bool,
}

#[derive(Clone, Copy, Default)]
struct PredInfo {
    i_qp_index: i32,
    i_cbp: i32,
    i_dc: i32,
    i_ad: [i32; 6],
}

#[derive(Clone, Copy, Default)]
struct CoreParameters {
    c_version: u32,
    c_sub_version: u32,
    cf_color_format: usize,
    rb_swapped: bool,
    alpha_channel: bool,
    scaled_arith: bool,
    index_table: bool,
    trim_flexbits_flag: bool,
    use_hard_tile_boundaries: bool,
    c_num_channels: usize,
    c_extra_pixels_top: usize,
    c_extra_pixels_left: usize,
    c_extra_pixels_bottom: usize,
    c_extra_pixels_right: usize,
    qp_mode: u32,
    ui_qp_index_dc: [u8; MAX_CHANNELS],
    ui_qp_index_lp: [u8; MAX_CHANNELS],
    ui_qp_index_hp: [u8; MAX_CHANNELS],
}

#[derive(Clone, Default)]
struct WmiTile {
    q_dc: [Vec<Quantizer>; MAX_CHANNELS],
    q_lp: [Vec<Quantizer>; MAX_CHANNELS],
    q_hp: [Vec<Quantizer>; MAX_CHANNELS],
    c_num_qp_lp: u8,
    c_num_qp_hp: u8,
    c_bits_lp: u8,
    c_bits_hp: u8,
    b_use_dc: bool,
    b_use_lp: bool,
    c_ch_mode_dc: u8,
    c_ch_mode_lp: [u8; 16],
    c_ch_mode_hp: [u8; 16],
}

impl WmiTile {
    /// Makes this tile a fresh one for `nch` channels, keeping the capacity of the quantizer
    /// vectors it will fill again.
    fn reset(&mut self, nch: usize) {
        let mut q = [
            std::mem::take(&mut self.q_dc),
            std::mem::take(&mut self.q_lp),
            std::mem::take(&mut self.q_hp),
        ];
        for chans in q.iter_mut() {
            for (i, v) in chans.iter_mut().enumerate() {
                if i < nch {
                    v.clear();
                } else {
                    *v = Vec::new();
                }
            }
        }
        let [q_dc, q_lp, q_hp] = q;
        *self = WmiTile {
            q_dc,
            q_lp,
            q_hp,
            c_num_qp_lp: 1,
            c_num_qp_hp: 1,
            ..WmiTile::default()
        };
    }
}

/// `CWMImageInfo`, reduced to what decoding uses.
#[derive(Clone, Copy, Default)]
struct ImageInfo {
    c_width: usize,
    c_height: usize,
    cf_color_format: usize,
    bd_bit_depth: usize,
    c_bits_per_unit: usize,
    b_rgb: bool,
    c_chroma_centering_x: u8,
    c_chroma_centering_y: u8,
    c_roi_left_x: usize,
    c_roi_width: usize,
    c_roi_top_y: usize,
    c_roi_height: usize,
    o_orientation: usize,
}

/// `CWMIStrCodecParam`, reduced.
#[derive(Clone, Default)]
struct CodecParam {
    cf_color_format: usize,
    ol_overlap: usize,
    bf_bitstream_format: usize,
    c_channel: usize,
    sb_subband: usize,
    c_num_of_slice_minus1_v: u32,
    /// Length `c_num_of_slice_minus1_v + 1`.
    ui_tile_x: Vec<u32>,
    c_num_of_slice_minus1_h: u32,
    /// Length `c_num_of_slice_minus1_h + 1`.
    ui_tile_y: Vec<u32>,
    n_len_mantissa_or_shift: u8,
    n_exp_bias: i8,
    b_black_white: bool,
    b_use_hard_tile_boundaries: bool,
}

impl CodecParam {
    /// A copy of `self` whose tile vectors are `x` and `y`, refilled in their capacity. With
    /// `led` they are charged, as they grow with the header.
    fn copy_to(
        &self,
        mut x: Vec<u32>,
        mut y: Vec<u32>,
        led: Option<&Ledger>,
    ) -> Result<Self, Error> {
        let (nx, ny) = (self.ui_tile_x.len(), self.ui_tile_y.len());
        if let Some(led) = led {
            led.charge(4 * (nx + ny) as u64)?;
        }
        refill(&mut x, nx, 0)?;
        refill(&mut y, ny, 0)?;
        if let Some(led) = led {
            led.keep(&x);
            led.keep(&y);
        }
        x.copy_from_slice(&self.ui_tile_x);
        y.copy_from_slice(&self.ui_tile_y);
        // Every field but the vectors, named so that a new one cannot be left out.
        let CodecParam {
            cf_color_format,
            ol_overlap,
            bf_bitstream_format,
            c_channel,
            sb_subband,
            c_num_of_slice_minus1_v,
            c_num_of_slice_minus1_h,
            n_len_mantissa_or_shift,
            n_exp_bias,
            b_black_white,
            b_use_hard_tile_boundaries,
            ui_tile_x: _,
            ui_tile_y: _,
        } = *self;
        Ok(CodecParam {
            cf_color_format,
            ol_overlap,
            bf_bitstream_format,
            c_channel,
            sb_subband,
            c_num_of_slice_minus1_v,
            ui_tile_x: x,
            c_num_of_slice_minus1_h,
            ui_tile_y: y,
            n_len_mantissa_or_shift,
            n_exp_bias,
            b_black_white,
            b_use_hard_tile_boundaries,
        })
    }
}

#[derive(Clone, Default)]
struct DecoderParameters {
    b_decode_full_frame: bool,
    b_decode_full_width: bool,
    b_skip_flexbits: bool,
    b_decode_hp: bool,
    b_decode_lp: bool,
    c_roi_left_x: isize,
    c_roi_right_x: isize,
    c_roi_top_y: isize,
    c_roi_bottom_y: isize,
    p_offset_x: Vec<usize>,
    p_offset_y: Vec<usize>,
}

/// jxrlib reads outside its malloc here.
const MB_PAD: usize = 256;

/// An interleaved alpha plane and its coding contexts.
#[derive(Default)]
struct AlphaPlane {
    sc: StrCodec,
    ctxs: Vec<CodingContext>,
}

/// `CWMImageStrCodec`. The coding contexts (one per tile column) live outside it, so a macroblock
/// step can hold one while it calls the plane's methods.
#[derive(Default)]
struct StrCodec {
    wmii: ImageInfo,
    wmiscp: CodecParam,
    mb: MbInfo,
    param: CoreParameters,
    dp: DecoderParameters,

    c_sb: usize,
    uv_resolution_change: bool,
    /// `Format::plane`.
    plane: usize,
    p_res_u: Vec<i32>,
    p_res_v: Vec<i32>,
    index_table: Vec<u64>,
    c_tile_row: u32,
    c_tile_column: u32,
    ctx_left: bool,
    ctx_top: bool,
    reset_rgi_totals: bool,
    reset_context: bool,
    tiles: Vec<WmiTile>,
    /// Number of bit readers besides the header reader (0 without an index table).
    n_bit_io: usize,
    c_header_size: u64,
    c_row: usize,
    c_column: usize,
    cmb_width: usize,
    cmb_height: usize,

    // hard tile state kept across macroblocks
    mb_x: usize,
    mb_y: usize,
    tile_x: u32,
    tile_y: u32,
    vert_tile_boundary: bool,
    hori_tile_boundary: bool,
    one_mb_left_vert_tb: bool,
    one_mb_right_vert_tb: bool,
    i_pred_before: [[i32; 2]; 2],
    i_pred_after: [[i32; 2]; 2],

    /// Every channel's two macroblock rows, as in jxrlib: `mb_pad`, then per channel the
    /// previous and current row, then `mb_pad`. Absolute offsets reach the neighbours. The alpha
    /// plane has its own single-channel buffer.
    mb_buf: Vec<i32>,
    /// Offsets of the previous and current row of each channel in `mb_buf`.
    a0_off: [usize; MAX_CHANNELS],
    a1_off: [usize; MAX_CHANNELS],
    pred_info: [Vec<PredInfo>; MAX_CHANNELS],
    pred_info_prev: [Vec<PredInfo>; MAX_CHANNELS],
    stream_len: usize,

    // Interleaved alpha (`m_pNextSC`): alpha is the Y_ONLY companion plane decoded in lockstep
    // with this one; `secondary` marks alpha itself, which shares the primary's bit readers.
    alpha: Option<Box<AlphaPlane>>,
    secondary: bool,
    out_stride: usize,
    err: Option<Error>,
}

//================================================================
// Tile position and quantizers
//================================================================

impl StrCodec {
    fn get_tile_pos(&mut self, mb_x: usize, mb_y: usize) {
        let p = &self.wmiscp;
        if mb_x == 0 {
            self.c_tile_column = 0;
        } else if self.c_tile_column < p.c_num_of_slice_minus1_v
            && mb_x as u32 == p.ui_tile_x[self.c_tile_column as usize + 1]
        {
            self.c_tile_column += 1;
        }
        if mb_y == 0 {
            self.c_tile_row = 0;
        } else if self.c_tile_row < p.c_num_of_slice_minus1_h
            && mb_y as u32 == p.ui_tile_y[self.c_tile_row as usize + 1]
        {
            self.c_tile_row += 1;
        }
        let tc = self.c_tile_column as usize;
        let tr = self.c_tile_row as usize;
        self.ctx_left = mb_x as u32 == p.ui_tile_x[tc];
        self.ctx_top = mb_y as u32 == p.ui_tile_y[tr];
        self.reset_context = (mb_x as u32).wrapping_sub(p.ui_tile_x[tc]) & 0xf == 0;
        self.reset_rgi_totals = self.reset_context;
        if self.c_tile_column == p.c_num_of_slice_minus1_v {
            if mb_x + 1 == self.cmb_width {
                self.reset_context = true;
            }
        } else if (mb_x + 1) as u32 == p.ui_tile_x[tc + 1] {
            self.reset_context = true;
        }
    }

    /// Copies tile 0's quantizers of subband `sb` to the other tile columns. Go aliased the
    /// slices, so tiles 1.. saw whatever tile 0 held when the uniform setup finished.
    fn set_uniform_quantizer(&mut self, sb: usize) {
        let nch = self.param.c_num_channels;
        let (first, rest) = self.tiles.split_at_mut(1);
        for t in rest.iter_mut() {
            for ch in 0..nch {
                let (dst, src) = match sb {
                    0 => (&mut t.q_dc[ch], &first[0].q_dc[ch]),
                    1 => (&mut t.q_lp[ch], &first[0].q_lp[ch]),
                    _ => (&mut t.q_hp[ch], &first[0].q_hp[ch]),
                };
                dst.clear();
                dst.reserve_exact(src.len());
                dst.extend_from_slice(src);
            }
        }
    }

    fn use_dc_quantizer(&mut self, i_tile: usize) {
        for ch in 0..self.param.c_num_channels {
            let dc = self.tiles[i_tile].q_dc[ch][0];
            self.tiles[i_tile].q_lp[ch][0] = dc;
        }
    }

    fn use_lp_quantizer(&mut self, c_qp: usize, i_tile: usize) {
        for ch in 0..self.param.c_num_channels {
            for i_qp in 0..c_qp {
                let lp = self.tiles[i_tile].q_lp[ch][i_qp];
                self.tiles[i_tile].q_hp[ch][i_qp] = lp;
            }
        }
    }

    fn allocate_bit_io_info(
        &mut self,
        stream_len: usize,
        io: &mut IoSet<'_>,
        led: &Ledger,
    ) -> Result<(), Error> {
        self.c_sb = match self.wmiscp.sb_subband {
            SB_DC_ONLY => 1,
            SB_NO_HIGHPASS => 2,
            SB_NO_FLEXBITS => 3,
            _ => 4,
        };
        let n = if !self.param.index_table {
            0
        } else if self.wmiscp.bf_bitstream_format == SPATIAL {
            self.wmiscp.c_num_of_slice_minus1_v as usize + 1
        } else {
            (self.wmiscp.c_num_of_slice_minus1_v as usize + 1) * self.c_sb
        };
        if n > MAX_TILES * 4 {
            return Err(Error::Codestream);
        }
        io.v.truncate(1);
        if n > 0 {
            if self.wmiscp.c_num_of_slice_minus1_h as usize >= MAX_TILES {
                return Err(Error::Codestream);
            }
            // Each entry takes at least one byte.
            if n * (self.wmiscp.c_num_of_slice_minus1_h as usize + 1) > stream_len {
                return Err(Error::Codestream);
            }
            led.charge(n as u64 * std::mem::size_of::<BitIO>() as u64)?;
            io.v.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
            io.v.resize(n + 1, BitIO::new());
            let len = n * (self.wmiscp.c_num_of_slice_minus1_h as usize + 1);
            led.reuse(&mut self.index_table, len, 0)?;
        } else {
            self.index_table.clear();
            led.keep(&self.index_table);
        }
        led.keep(&io.v);
        self.n_bit_io = n;
        Ok(())
    }

    fn set_bit_io_pointers(&self, ctxs: &mut [CodingContext]) {
        if self.n_bit_io > 0 {
            for i in 0..=self.wmiscp.c_num_of_slice_minus1_v as usize {
                let c = &mut ctxs[i];
                if self.wmiscp.bf_bitstream_format == SPATIAL {
                    let k = 1 + i;
                    c.io_dc = k;
                    c.io_lp = k;
                    c.io_ac = k;
                    c.io_fl = k;
                } else {
                    let j = self.c_sb;
                    c.io_dc = 1 + i * j;
                    if j > 1 {
                        c.io_lp = 1 + i * j + 1;
                    }
                    if j > 2 {
                        c.io_ac = 1 + i * j + 2;
                    }
                    if j > 3 {
                        c.io_fl = 1 + i * j + 3;
                    }
                }
            }
        } else {
            let c = &mut ctxs[0];
            c.io_dc = HEADER_IO;
            c.io_lp = HEADER_IO;
            c.io_ac = HEADER_IO;
            c.io_fl = HEADER_IO;
        }
    }
}

fn format_quantizer(
    q: &mut [Vec<Quantizer>; MAX_CHANNELS],
    c_ch_mode: u8,
    c_ch: usize,
    i_pos: usize,
    b_shifted_uv: bool,
    b_scaled_arith: bool,
) {
    for i_ch in 0..c_ch {
        if i_ch > 0 {
            match c_ch_mode {
                0 => q[i_ch][i_pos] = q[0][i_pos],
                1 => q[i_ch][i_pos] = q[1][i_pos],
                _ => {}
            }
        }
        let mut shift = SHIFT_ZERO;
        if i_ch > 0 && b_shifted_uv {
            shift = SHIFT_ZERO - 1;
        }
        predquant::remap_qp(&mut q[i_ch][i_pos], shift, b_scaled_arith);
    }
}

/// Fills `q` with `c_qp` zeroed quantizers for each of `c_channel` channels.
fn allocate_quantizer(q: &mut [Vec<Quantizer>; MAX_CHANNELS], c_channel: usize, c_qp: usize) {
    for v in q.iter_mut().take(c_channel) {
        v.clear();
        v.reserve_exact(c_qp);
        v.resize(c_qp, Quantizer::default());
    }
}

fn dquant_bits(c_qp: u8) -> u8 {
    match c_qp {
        0..=1 => 0,
        2..=3 => 1,
        4..=5 => 2,
        6..=9 => 3,
        _ => 4,
    }
}
