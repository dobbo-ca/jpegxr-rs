// Ported from jxrlib strdec.c (BSD-2, see NOTICE).

use super::segdec::{allocate_coding_context_dec, reset_coding_context_dec};
use super::*;
use crate::limits::filled;
use crate::par;
use std::sync::atomic::{AtomicBool, Ordering};

/// The output pixel layout, from the container pixel format.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Format {
    /// `CF_RGB` or `Y_ONLY`, or one of the other colour formats.
    pub color_format: usize,
    /// `BD8` or `BD16`, or one of the other bit depths.
    pub bit_depth: usize,
    /// Bits per pixel.
    pub bits_per_unit: usize,
    pub rgb: bool,
    /// `uAlphaMode > 0`: decode an interleaved alpha plane.
    pub alpha: bool,
    /// `plane > 0` writes only channel `plane - 1` of an `N_COMPONENT` `BD16` image, one 16-bit
    /// sample a pixel.
    pub plane: usize,
}

/// The image header fields callers need.
#[derive(Clone, Default)]
pub(crate) struct Header {
    /// Inscribed image size.
    pub width: usize,
    pub height: usize,
    /// Source colour format field.
    pub color_format: usize,
    /// Source bit depth field.
    pub bit_depth: usize,
    /// `bAlphaChannel`: an interleaved alpha plane follows the primary.
    pub alpha: bool,

    pub(super) param: CoreParameters,
    pub(super) ii: ImageInfo,
    pub(super) scp: CodecParam,
    /// Header bytes, up to the index table.
    pub(super) size: usize,

    /// Interleaved alpha's own plane header, read straight after the primary's
    /// (strdec.c:3346-3350); zero when `!alpha`.
    pub(super) a_param: CoreParameters,
    pub(super) a_scp: CodecParam,
}

/// The header's tile tables (the primary plane's and alpha's), kept between decodes.
#[derive(Default)]
pub(crate) struct TileTables {
    x: Vec<u32>,
    y: Vec<u32>,
    a_x: Vec<u32>,
    a_y: Vec<u32>,
}

/// The working buffers of a plane that outlive one decode.
#[derive(Default)]
struct PlaneBufs {
    mb_buf: Vec<i32>,
    pred_info: [Vec<PredInfo>; MAX_CHANNELS],
    pred_info_prev: [Vec<PredInfo>; MAX_CHANNELS],
    tiles: Vec<WmiTile>,
    index_table: Vec<u64>,
    p_res_u: Vec<i32>,
    p_res_v: Vec<i32>,
    p_offset_x: Vec<usize>,
    p_offset_y: Vec<usize>,
    tile_x: Vec<u32>,
    tile_y: Vec<u32>,
}

/// What the transform half of a pipelined decode keeps: its copy of the tile tables, and alpha's
/// (in the tile tables of the plane in the box).
#[derive(Default)]
struct TransformKept {
    tile_x: Vec<u32>,
    tile_y: Vec<u32>,
    alpha: Option<Box<AlphaPlane>>,
}

/// The buffers a decode works in, kept between decodes: the next decode takes them out, makes
/// them as fresh ones, and puts them back when it ends, so a decode of a stream of the same
/// shape allocates nothing (Go's `Decoder` reuse).
#[derive(Default)]
pub(crate) struct Scratch {
    /// The header's tile tables, for a decoder that parses into them.
    pub(crate) tiles: TileTables,
    plane: PlaneBufs,
    alpha: Option<Box<AlphaPlane>>,
    ctxs: Vec<CodingContext>,
    /// Always empty readers: they point into the stream of a decode.
    ios: Vec<BitIO<'static>>,
    slots: Vec<Slot>,
    tr: TransformKept,
}

#[cfg(test)]
impl Scratch {
    /// Whether every kept quantizer vector is within the bound `str_dec_init` charges: 1 for DC,
    /// 16 for LP and HP.
    pub(super) fn quantizers_are_exact(&self) -> bool {
        self.plane.tiles.iter().all(|t| {
            t.q_dc.iter().all(|v| v.capacity() <= 1)
                && [&t.q_lp, &t.q_hp]
                    .into_iter()
                    .flatten()
                    .all(|v| v.capacity() <= 16)
        })
    }
}

/// Moves `from` into `to` when it has more capacity.
fn keep_larger(to: &mut Vec<u32>, from: &mut Vec<u32>) {
    if from.capacity() > to.capacity() {
        *to = std::mem::take(from);
    }
}

fn read_quantizer_sb(q: &mut [u8; MAX_CHANNELS], sb: &mut SimpleBitIO<'_>, c_channel: usize) -> u8 {
    let mut mode = 0u8;
    if c_channel >= MAX_CHANNELS {
        return 0;
    }
    if c_channel > 1 {
        mode = sb.get_bit32(2) as u8;
    }
    q[0] = sb.get_bit32(8) as u8;
    if mode == 1 {
        q[1] = sb.get_bit32(8) as u8;
    } else if mode > 0 {
        for v in q.iter_mut().take(c_channel).skip(1) {
            *v = sb.get_bit32(8) as u8;
        }
    }
    mode
}

fn read_image_plane_header(
    ii: &mut ImageInfo,
    scp: &mut CodecParam,
    p: &mut CoreParameters,
    sb: &mut SimpleBitIO<'_>,
) -> Result<(), Error> {
    p.cf_color_format = sb.get_bit32(3) as usize;
    if p.cf_color_format > N_COMPONENT || p.cf_color_format == 5 {
        return Err(Error::Codestream);
    }
    scp.cf_color_format = p.cf_color_format;
    p.scaled_arith = sb.get_bit32(1) != 0;
    scp.sb_subband = sb.get_bit32(4) as usize;

    match p.cf_color_format {
        Y_ONLY => p.c_num_channels = 1,
        YUV420 => {
            p.c_num_channels = 3;
            sb.get_bit32(1);
            ii.c_chroma_centering_x = sb.get_bit32(3) as u8;
            sb.get_bit32(1);
            ii.c_chroma_centering_y = sb.get_bit32(3) as u8;
        }
        YUV422 => {
            p.c_num_channels = 3;
            sb.get_bit32(1);
            ii.c_chroma_centering_x = sb.get_bit32(3) as u8;
            sb.get_bit32(4);
        }
        YUV444 => {
            p.c_num_channels = 3;
            sb.get_bit32(4);
            sb.get_bit32(4);
        }
        N_COMPONENT => {
            p.c_num_channels = sb.get_bit32(4) as usize + 1;
            sb.get_bit32(4);
        }
        CMYK => p.c_num_channels = 4,
        _ => {}
    }

    match ii.bd_bit_depth {
        BD16 | BD16S | BD32 | BD32S => scp.n_len_mantissa_or_shift = sb.get_bit32(8) as u8,
        BD32F => {
            scp.n_len_mantissa_or_shift = sb.get_bit32(8) as u8;
            scp.n_exp_bias = sb.get_bit32(8) as u8 as i8;
        }
        _ => {}
    }

    p.qp_mode = 0;
    if sb.get_bit32(1) == 1 {
        p.qp_mode += u32::from(read_quantizer_sb(
            &mut p.ui_qp_index_dc,
            sb,
            p.c_num_channels,
        )) << 3;
    } else {
        p.qp_mode += 1;
    }
    if scp.sb_subband != SB_DC_ONLY {
        if sb.get_bit32(1) == 0 {
            p.qp_mode += 0x200;
            if sb.get_bit32(1) == 1 {
                p.qp_mode += u32::from(read_quantizer_sb(
                    &mut p.ui_qp_index_lp,
                    sb,
                    p.c_num_channels,
                )) << 5;
            } else {
                p.qp_mode += 2;
            }
        } else {
            p.qp_mode += ((p.qp_mode & 1) << 1) + ((p.qp_mode & 0x18) << 2);
        }
        if scp.sb_subband != SB_NO_HIGHPASS {
            if sb.get_bit32(1) == 0 {
                p.qp_mode += 0x400;
                if sb.get_bit32(1) == 1 {
                    p.qp_mode += u32::from(read_quantizer_sb(
                        &mut p.ui_qp_index_hp,
                        sb,
                        p.c_num_channels,
                    )) << 7;
                } else {
                    p.qp_mode += 4;
                }
            } else {
                p.qp_mode += ((p.qp_mode & 2) << 1) + ((p.qp_mode & 0x60) << 2);
            }
        }
    }
    match scp.sb_subband {
        SB_DC_ONLY => p.qp_mode |= 0x200,
        SB_NO_HIGHPASS => p.qp_mode |= 0x400,
        _ => {}
    }
    if p.qp_mode & 0x600 == 0 {
        return Err(Error::Codestream);
    }
    sb.flush_to_byte();
    Ok(())
}

/// `Header.Estimate`'s constants in Go: a coding context with its Huffman tables, and the sizes of
/// `predInfo`, `bitIO` and `wmiTile`.
pub(super) const CTX_BYTES: u64 = 16 << 10;
pub(super) const PRED_INFO_BYTES: u64 = 36;
pub(super) const BIT_IO_BYTES: u64 = 56;
pub(super) const WMI_TILE_BYTES: u64 = 1264;

impl Header {
    /// Parses the image header and first plane header (`ReadWMIHeader`). The tile vectors are
    /// the one allocation here that grows with the input: `max_alloc` caps them (new; Go had no
    /// cap, but its largest, 4095 + 4095 entries of 4 bytes, is under the parity floor).
    pub(crate) fn parse(
        stream: &[u8],
        max_alloc: u64,
        kept: &mut TileTables,
    ) -> Result<Header, Error> {
        let mut h = Header::default();
        if stream.len() < 8 || &stream[..7] != b"WMPHOTO" {
            return Err(Error::Codestream);
        }
        let mut sb = SimpleBitIO {
            buf: &stream[8..],
            cb_read: 0,
            acc: 0,
            left: 0,
            err: false,
        };
        let sb = &mut sb;
        let mut p = CoreParameters::default();
        let mut ii = ImageInfo::default();
        let mut scp = CodecParam::default();

        if sb.get_bit32(4) != 1 {
            return Err(Error::Codestream);
        }
        p.c_version = 1;
        let mut i = sb.get_bit32(4);
        if i != CODEC_SUBVERSION
            && i != CODEC_SUBVERSION_SOFT_TILE
            && i != CODEC_SUBVERSION_HARD_TILE
        {
            return Err(Error::Codestream);
        }
        p.c_sub_version = i;
        p.use_hard_tile_boundaries = i == CODEC_SUBVERSION_HARD_TILE;
        scp.b_use_hard_tile_boundaries = p.use_hard_tile_boundaries;

        let tiling = sb.get_bit32(1) != 0;
        scp.bf_bitstream_format = sb.get_bit32(1) as usize;
        ii.o_orientation = sb.get_bit32(3) as usize;
        p.index_table = sb.get_bit32(1) != 0;
        i = sb.get_bit32(2);
        if i == 3 {
            return Err(Error::Codestream);
        }
        scp.ol_overlap = i as usize;

        let short = sb.get_bit32(1) != 0;
        sb.get_bit32(1); // long word flag
        let inscribed = sb.get_bit32(1) != 0;
        p.trim_flexbits_flag = sb.get_bit32(1) != 0;
        let tile_stretch = sb.get_bit32(1) != 0;
        p.rb_swapped = sb.get_bit32(1) != 0;
        sb.get_bit32(1);
        p.alpha_channel = sb.get_bit32(1) != 0;

        ii.cf_color_format = sb.get_bit32(4) as usize;
        ii.bd_bit_depth = sb.get_bit32(4) as usize;
        if ii.bd_bit_depth == BD1_ALT {
            ii.bd_bit_depth = BD1;
            scp.b_black_white = true;
        }

        let (size_bits, tile_bits) = if short { (16, 8) } else { (32, 16) };
        ii.c_width = sb.get_bit32(size_bits) as usize + 1;
        ii.c_height = sb.get_bit32(size_bits) as usize + 1;
        if !inscribed && ii.c_width & 0xf != 0 {
            p.c_extra_pixels_right = 0x10 - (ii.c_width & 0xf);
        }
        if !inscribed && ii.c_height & 0xf != 0 {
            p.c_extra_pixels_bottom = 0x10 - (ii.c_height & 0xf);
        }

        if tiling {
            scp.c_num_of_slice_minus1_v = sb.get_bit32(LOG_MAX_TILES);
            scp.c_num_of_slice_minus1_h = sb.get_bit32(LOG_MAX_TILES);
        }
        if !p.index_table
            && (scp.bf_bitstream_format == FREQUENCY
                || scp.c_num_of_slice_minus1_v + scp.c_num_of_slice_minus1_h > 0)
        {
            return Err(Error::Codestream);
        }
        // Slices: headers are copied per decode.
        let nv = scp.c_num_of_slice_minus1_v as usize;
        let nh = scp.c_num_of_slice_minus1_h as usize;
        // The fields below take `tile_bits` each: a count the stream cannot hold is a truncated
        // header, which Go reports as `Codestream` too (its bit reader's error, below).
        let bits_left = sb.buf.len().saturating_sub(sb.cb_read) as u64 * 8 + u64::from(sb.left);
        if (nv + nh) as u64 * u64::from(tile_bits) > bits_left {
            return Err(Error::Codestream);
        }
        // The tables, and the alpha plane's copy of them (`a_scp` below), are both held.
        let need = 4 * (nv + nh + 2) as u64 * if p.alpha_channel { 2 } else { 1 };
        if need > max_alloc {
            return Err(Error::AllocLimit {
                need,
                max: max_alloc,
            });
        }
        scp.ui_tile_x = std::mem::take(&mut kept.x);
        refill(&mut scp.ui_tile_x, nv + 1, 0)?;
        scp.ui_tile_x.shrink_to_fit();
        scp.ui_tile_y = std::mem::take(&mut kept.y);
        refill(&mut scp.ui_tile_y, nh + 1, 0)?;
        scp.ui_tile_y.shrink_to_fit();
        for i in 0..nv {
            scp.ui_tile_x[i + 1] = sb.get_bit32(tile_bits).wrapping_add(scp.ui_tile_x[i]);
        }
        for i in 0..nh {
            scp.ui_tile_y[i + 1] = sb.get_bit32(tile_bits).wrapping_add(scp.ui_tile_y[i]);
        }
        if tile_stretch {
            for _ in 0..(nv as u32 + 1) * (nh as u32 + 1) {
                if sb.err {
                    break;
                }
                sb.get_bit32(8);
            }
        }
        if inscribed {
            p.c_extra_pixels_top = sb.get_bit32(6) as usize;
            p.c_extra_pixels_left = sb.get_bit32(6) as usize;
            p.c_extra_pixels_bottom = sb.get_bit32(6) as usize;
            p.c_extra_pixels_right = sb.get_bit32(6) as usize;
        }
        if ((ii.c_width + p.c_extra_pixels_left + p.c_extra_pixels_right) & 0xf)
            + ((ii.c_height + p.c_extra_pixels_top + p.c_extra_pixels_bottom) & 0xf)
            != 0
        {
            if (ii.c_width & 0xf)
                + (ii.c_height & 0xf)
                + p.c_extra_pixels_left
                + p.c_extra_pixels_top
                != 0
            {
                return Err(Error::Codestream);
            }
            if ii.c_width <= p.c_extra_pixels_right || ii.c_height <= p.c_extra_pixels_bottom {
                return Err(Error::Codestream);
            }
            ii.c_width -= p.c_extra_pixels_right;
            ii.c_height -= p.c_extra_pixels_bottom;
        }
        sb.flush_to_byte();

        read_image_plane_header(&mut ii, &mut scp, &mut p, sb)?;
        if sb.err {
            return Err(Error::Codestream);
        }
        scp.c_channel = p.c_num_channels;
        if (ii.bd_bit_depth == BD5 || ii.bd_bit_depth == BD10 || ii.bd_bit_depth == BD565)
            && scp.cf_color_format != YUV444
            && scp.cf_color_format != YUV422
            && scp.cf_color_format != YUV420
            && scp.cf_color_format != Y_ONLY
        {
            return Err(Error::Codestream);
        }
        if p.alpha_channel {
            // Alpha's plane header follows the primary's in the same bitstream
            // (strdec.c:3346-3350); its own errors are ignored like jxrlib does, but a
            // truncated stream still fails below.
            let mut a_ii = ii;
            h.a_param = p;
            h.a_scp = scp.copy_to(
                std::mem::take(&mut kept.a_x),
                std::mem::take(&mut kept.a_y),
                None,
            )?;
            h.a_scp.ui_tile_x.shrink_to_fit();
            h.a_scp.ui_tile_y.shrink_to_fit();
            let _ = read_image_plane_header(&mut a_ii, &mut h.a_scp, &mut h.a_param, sb);
            h.a_param.cf_color_format = Y_ONLY;
            h.a_param.c_num_channels = 1;
            h.a_scp.c_channel = 1;
        }
        if sb.err {
            return Err(Error::Codestream);
        }
        h.size = 8 + sb.cb_read;
        h.width = ii.c_width;
        h.height = ii.c_height;
        h.color_format = ii.cf_color_format;
        h.bit_depth = ii.bd_bit_depth;
        h.alpha = p.alpha_channel;
        h.param = p;
        h.ii = ii;
        h.scp = scp;
        Ok(h)
    }

    /// Gives the tile tables back to `to`, for the next header to parse into.
    pub(crate) fn give_tiles(&mut self, to: &mut TileTables) {
        keep_larger(&mut to.x, &mut self.scp.ui_tile_x);
        keep_larger(&mut to.y, &mut self.scp.ui_tile_y);
        keep_larger(&mut to.a_x, &mut self.a_scp.ui_tile_x);
        keep_larger(&mut to.a_y, &mut self.a_scp.ui_tile_y);
    }

    /// An upper bound on the heap a decode of this header allocates, following the buffer sizes
    /// `decode` creates: macroblock rows, the per-tile-column contexts and bit readers, and the
    /// index table, which the stream length bounds.
    pub(crate) fn estimate(&self, stream_len: usize) -> u64 {
        // Go's figures (`Header.Estimate`), not the sizes of the Rust structs, so one memory limit
        // gives one class on both sides; `estimate_constants` checks the structs fit them.
        let p = &self.param;
        let w = (self.ii.c_width + p.c_extra_pixels_left + p.c_extra_pixels_right) as u64;
        let mbw = w.div_ceil(16) + 1;
        let mut ch = p.c_num_channels as u64;
        if p.alpha_channel {
            ch += 1;
        }
        let cols = u64::from(self.scp.c_num_of_slice_minus1_v) + 1;
        let rows = ch * mbw * (2 * 256 * 4 + 2 * PRED_INFO_BYTES) + 2 * 256 * 4 * mbw;
        let offsets = 8
            * (w + (self.ii.c_height + p.c_extra_pixels_top + p.c_extra_pixels_bottom) as u64 + 64);
        let mut tiles = cols * (CTX_BYTES + 4 * (BIT_IO_BYTES + 8) + WMI_TILE_BYTES);
        if p.alpha_channel {
            tiles += cols * (CTX_BYTES + WMI_TILE_BYTES);
        }
        rows + offsets + tiles + 8 * stream_len as u64 + CTX_BYTES
    }
}

/// `ReadQuantizer`.
fn read_quantizer(
    q: &mut [Vec<Quantizer>; MAX_CHANNELS],
    io: &mut BitIO,
    c_channel: usize,
    i_pos: usize,
) -> u8 {
    let mut mode = 0u8;
    if c_channel > 1 {
        mode = io.get_bit16(2) as u8;
    }
    q[0][i_pos].i_index = io.get_bit16(8) as u8;
    if mode == 1 {
        q[1][i_pos].i_index = io.get_bit16(8) as u8;
    } else if mode > 0 {
        for v in q.iter_mut().take(c_channel).skip(1) {
            v[i_pos].i_index = io.get_bit16(8) as u8;
        }
    }
    mode
}

fn read_packet_header(io: &mut BitIO) -> bool {
    if io.get_bit16(8) != 0 {
        return false;
    }
    if io.get_bit16(8) != 0 {
        return false;
    }
    if io.get_bit16(8) != 1 {
        return false;
    }
    io.get_bit16(8);
    true
}

impl StrCodec {
    fn read_tile_header_dc(&mut self, io: &mut BitIO) {
        if self.param.qp_mode & 1 != 0 {
            let tc = self.c_tile_column as usize;
            let nch = self.param.c_num_channels;
            if self.c_tile_row + self.c_tile_column == 0 {
                for t in self.tiles.iter_mut() {
                    allocate_quantizer(&mut t.q_dc, nch, 1);
                }
            }
            let t = &mut self.tiles[tc];
            t.c_ch_mode_dc = read_quantizer(&mut t.q_dc, io, nch, 0);
            format_quantizer(
                &mut t.q_dc,
                t.c_ch_mode_dc,
                nch,
                0,
                true,
                self.param.scaled_arith,
            );
        }
    }

    fn read_tile_header_lp(&mut self, io: &mut BitIO) {
        if self.wmiscp.sb_subband != SB_DC_ONLY && self.param.qp_mode & 2 != 0 {
            let tc = self.c_tile_column as usize;
            let nch = self.param.c_num_channels;
            let scaled = self.param.scaled_arith;
            let t = &mut self.tiles[tc];
            t.b_use_dc = io.get_bit16(1) == 1;
            t.c_bits_lp = 0;
            t.c_num_qp_lp = 1;
            if t.b_use_dc {
                allocate_quantizer(&mut t.q_lp, nch, usize::from(t.c_num_qp_lp));
                self.use_dc_quantizer(tc);
            } else {
                t.c_num_qp_lp = io.get_bit16(4) as u8 + 1;
                t.c_bits_lp = dquant_bits(t.c_num_qp_lp);
                allocate_quantizer(&mut t.q_lp, nch, usize::from(t.c_num_qp_lp));
                for i in 0..usize::from(t.c_num_qp_lp) {
                    t.c_ch_mode_lp[i] = read_quantizer(&mut t.q_lp, io, nch, i);
                    format_quantizer(&mut t.q_lp, t.c_ch_mode_lp[i], nch, i, true, scaled);
                }
            }
        }
    }

    fn read_tile_header_hp(&mut self, io: &mut BitIO) {
        if self.wmiscp.sb_subband != SB_DC_ONLY
            && self.wmiscp.sb_subband != SB_NO_HIGHPASS
            && self.param.qp_mode & 4 != 0
        {
            let tc = self.c_tile_column as usize;
            let nch = self.param.c_num_channels;
            let scaled = self.param.scaled_arith;
            let t = &mut self.tiles[tc];
            t.b_use_lp = io.get_bit16(1) == 1;
            t.c_bits_hp = 0;
            t.c_num_qp_hp = 1;
            if t.b_use_lp {
                t.c_num_qp_hp = t.c_num_qp_lp;
                allocate_quantizer(&mut t.q_hp, nch, usize::from(t.c_num_qp_hp));
                let n = usize::from(t.c_num_qp_hp);
                self.use_lp_quantizer(n, tc);
            } else {
                t.c_num_qp_hp = io.get_bit16(4) as u8 + 1;
                t.c_bits_hp = dquant_bits(t.c_num_qp_hp);
                allocate_quantizer(&mut t.q_hp, nch, usize::from(t.c_num_qp_hp));
                for i in 0..usize::from(t.c_num_qp_hp) {
                    t.c_ch_mode_hp[i] = read_quantizer(&mut t.q_hp, io, nch, i);
                    format_quantizer(&mut t.q_hp, t.c_ch_mode_hp[i], nch, i, false, scaled);
                }
            }
        }
    }

    fn read_packets(
        &mut self,
        ctxs: &mut [CodingContext],
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        let nv = self.wmiscp.c_num_of_slice_minus1_v as usize;
        if self.c_column == 0
            && self.c_row as u32 == self.wmiscp.ui_tile_y[self.c_tile_row as usize]
        {
            if self.secondary {
                // The alpha plane shares the primary's packet/tile headers (strdec.c:206-216);
                // only its coding contexts reset here.
                if self.n_bit_io > 0 {
                    for c in ctxs.iter_mut().take(nv + 1) {
                        reset_coding_context_dec(c);
                    }
                } else {
                    reset_coding_context_dec(&mut ctxs[0]);
                }
            } else {
                let n = self.n_bit_io;
                for k in 0..n {
                    let off = self.index_table[n * self.c_tile_row as usize + k]
                        .wrapping_add(self.c_header_size);
                    ios.attach(1 + k, off);
                }
                if n == 0 {
                    ios.attach(HEADER_IO, self.c_header_size);
                }
                for k in 0..=nv {
                    if self.wmiscp.bf_bitstream_format == SPATIAL {
                        let io = &mut ios.v[if n != 0 { 1 + k } else { HEADER_IO }];
                        if !read_packet_header(io) {
                            return Err(Error::Codestream);
                        }
                        ctxs[k].trim_flex_bits = 0;
                        if self.param.trim_flexbits_flag {
                            ctxs[k].trim_flex_bits = io.get_bit16(4) as i32;
                        }
                    } else {
                        let base = 1 + k * self.c_sb;
                        if !read_packet_header(&mut ios.v[base]) {
                            return Err(Error::Codestream);
                        }
                        if self.c_sb > 1 && !read_packet_header(&mut ios.v[base + 1]) {
                            return Err(Error::Codestream);
                        }
                        if self.c_sb > 2 && !read_packet_header(&mut ios.v[base + 2]) {
                            return Err(Error::Codestream);
                        }
                        if self.c_sb > 3 {
                            let io = &mut ios.v[base + 3];
                            read_packet_header(io); // jxrlib ignores a bad flexbits header
                            ctxs[k].trim_flex_bits = 0;
                            if self.param.trim_flexbits_flag {
                                ctxs[k].trim_flex_bits = io.get_bit16(4) as i32;
                            }
                        }
                    }
                    reset_coding_context_dec(&mut ctxs[k]);
                }
            }
        }

        if self.ctx_left && self.ctx_top && !self.secondary {
            let tc = self.c_tile_column as usize;
            let (io_dc, io_lp, io_ac) = (ctxs[tc].io_dc, ctxs[tc].io_lp, ctxs[tc].io_ac);
            self.read_tile_header_dc(&mut ios.v[io_dc]);
            if let Some(a) = self.alpha.as_mut() {
                a.sc.read_tile_header_dc(&mut ios.v[io_dc]);
            }
            if self.c_sb > 1 {
                self.read_tile_header_lp(&mut ios.v[io_lp]);
                if let Some(a) = self.alpha.as_mut() {
                    a.sc.read_tile_header_lp(&mut ios.v[io_lp]);
                }
            }
            if self.c_sb > 2 {
                self.read_tile_header_hp(&mut ios.v[io_ac]);
                if let Some(a) = self.alpha.as_mut() {
                    a.sc.read_tile_header_hp(&mut ios.v[io_ac]);
                }
            }
        }
        Ok(())
    }

    /// Steps this plane's macroblock, then, when an interleaved alpha plane is present, steps it
    /// too (strdec.c:309-393: the `for(j=0;j<=jend;j++)` loop, unrolled since `jend` is at most 1
    /// here).
    fn process_macroblock_dec(
        &mut self,
        ctxs: &mut [CodingContext],
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        self.mb_step(ctxs, ios)?;
        let (r, c) = (self.c_row, self.c_column);
        if let Some(a) = self.alpha.as_mut() {
            a.sc.c_row = r;
            a.sc.c_column = c;
            a.sc.mb_step(&mut a.ctxs, ios)?;
        }
        Ok(())
    }

    fn mb_step(&mut self, ctxs: &mut [CodingContext], ios: &mut IoSet<'_>) -> Result<(), Error> {
        self.mb_entropy(ctxs, ios)?;
        self.mb_transform();
        Ok(())
    }

    /// The entropy half of `mb_step`: reads this macroblock's coefficients into the current row
    /// (`p1`) and the prediction state. It touches nothing the transform half reads.
    fn mb_entropy(&mut self, ctxs: &mut [CodingContext], ios: &mut IoSet<'_>) -> Result<(), Error> {
        let ol = self.wmiscp.ol_overlap;
        let bottom = self.c_row == self.cmb_height;
        let bottom_or_right = bottom || self.c_column == self.cmb_width;

        if !bottom_or_right {
            self.get_tile_pos(self.c_column, self.c_row);
            self.read_packets(ctxs, ios)?;
            let tc = self.c_tile_column as usize;
            let tr = self.c_tile_row as usize;
            let ctx = &mut ctxs[tc];
            let w = &self.wmiscp;
            if !self.dp.b_decode_full_frame && self.c_column as u32 == w.ui_tile_x[tc] {
                let (r_left, r_right) = (self.dp.c_roi_left_x, self.dp.c_roi_right_x);
                let (r_top, r_bottom) = (self.dp.c_roi_top_y, self.dp.c_roi_bottom_y);
                let r_ext: isize = match ol {
                    OL_ONE => 2,
                    OL_TWO => 10,
                    _ => 0,
                };
                let t_left = (self.c_column * 16) as isize;
                let t_top = w.ui_tile_y[tr] as isize * 16;
                let mut t_right = (self.cmb_width * 16) as isize;
                if self.c_tile_column != w.c_num_of_slice_minus1_v {
                    t_right = w.ui_tile_x[tc + 1] as isize * 16;
                }
                let mut t_bottom = (self.cmb_height * 16) as isize;
                if self.c_tile_row != w.c_num_of_slice_minus1_h {
                    t_bottom = w.ui_tile_y[tr + 1] as isize * 16;
                }
                ctx.in_roi = r_left < t_right + r_ext
                    && r_top < t_bottom + r_ext
                    && t_left <= r_right + r_ext
                    && t_top <= r_bottom + r_ext
                    && (self.c_row * 16) as isize <= r_bottom + r_ext;
            }

            if self.dp.b_decode_full_frame || ctx.in_roi {
                self.decode_macroblock_dc(ctx, ios)?;
                if self.dp.b_decode_lp {
                    self.decode_macroblock_lowpass(ctx, ios)?;
                }
                self.pred_dcac_dec();
                self.dequantize_macroblock();
                if self.dp.b_decode_hp {
                    self.decode_macroblock_highpass(ctx, ios)?;
                    self.pred_ac_dec();
                }
                self.update_pred_info(self.c_column, self.param.cf_color_format);
            }
        }
        Ok(())
    }

    /// The transform half of `mb_step`: the inverse transform and overlap filter of this
    /// macroblock in `mb_buf`.
    fn mb_transform(&mut self) {
        let dp = &self.dp;
        let (col16, row16) = ((self.c_column * 16) as isize, (self.c_row * 16) as isize);
        if !dp.b_decode_full_frame
            && (col16 > dp.c_roi_right_x + 25
                || col16 + 25 < dp.c_roi_left_x
                || row16 > dp.c_roi_bottom_y + 25
                || row16 + 25 < dp.c_roi_top_y)
        {
            return;
        }
        self.inv_transform_macroblock_altered_operators_hard();
    }

    pub(super) fn read_index_table(&mut self, ios: &mut IoSet<'_>) -> Result<(), Error> {
        let io = &mut ios.v[HEADER_IO];
        if self.n_bit_io > 0 {
            if io.get_bit32(16) != 1 {
                return Err(Error::Codestream);
            }
            for v in self.index_table.iter_mut() {
                *v = get_vl_word_esc(io);
            }
        }
        self.c_header_size = get_vl_word_esc(io);
        io.flush_to_byte();
        self.c_header_size = self.c_header_size.wrapping_add(io.byte_pos());
        if self.c_header_size > self.stream_len as u64 {
            return Err(Error::Codestream);
        }
        for &v in &self.index_table {
            if v > self.stream_len as u64 {
                return Err(Error::Codestream);
            }
        }
        Ok(())
    }

    fn set_roi(&mut self) {
        let ii = &mut self.wmii;
        let dp = &mut self.dp;
        let p = &self.param;
        let sb = self.wmiscp.sb_subband;
        dp.b_skip_flexbits = sb == SB_NO_FLEXBITS;
        dp.b_decode_hp = sb == SB_ALL || sb == SB_NO_FLEXBITS;
        dp.b_decode_lp = sb != SB_DC_ONLY;

        // ii holds the padded size here.
        dp.c_roi_left_x = (ii.c_roi_left_x + p.c_extra_pixels_left) as isize;
        dp.c_roi_right_x = dp.c_roi_left_x + ii.c_roi_width as isize - 1;
        dp.c_roi_top_y = (ii.c_roi_top_y + p.c_extra_pixels_top) as isize;
        dp.c_roi_bottom_y = dp.c_roi_top_y + ii.c_roi_height as isize - 1;
        if dp.c_roi_right_x >= ii.c_width as isize {
            dp.c_roi_right_x = ii.c_width as isize - 1;
        }
        if dp.c_roi_bottom_y >= ii.c_height as isize {
            dp.c_roi_bottom_y = ii.c_height as isize - 1;
        }
        dp.b_decode_full_frame = dp.c_roi_left_x + dp.c_roi_top_y == 0
            && (dp.c_roi_right_x + 15) / 16 >= (ii.c_width as isize + 14) / 16
            && (dp.c_roi_bottom_y + 15) / 16 >= (ii.c_height as isize + 14) / 16;
        dp.b_decode_full_width =
            dp.c_roi_left_x == 0 && (dp.c_roi_right_x + 15) / 16 >= (ii.c_width as isize + 14) / 16;

        ii.c_width -= p.c_extra_pixels_left + p.c_extra_pixels_right;
        ii.c_height -= p.c_extra_pixels_top + p.c_extra_pixels_bottom;
        self.c_tile_column = 0;
        self.c_tile_row = 0;
    }

    /// Builds pixel offsets, in output units, for `O_NONE`.
    fn init_lookup_tables(&mut self, led: &Ledger) -> Result<(), Error> {
        let ii = &self.wmii;
        let dp = &mut self.dp;
        let w = (ii.c_width as isize + dp.c_roi_left_x) as usize;
        let h = (ii.c_height as isize + dp.c_roi_top_y) as usize;
        let mut stride_y = self.out_stride;
        match ii.bd_bit_depth {
            BD16 | BD16S | BD5 | BD565 | BD16F => stride_y /= 2,
            BD32 | BD32S | BD32F | BD10 => stride_y /= 4,
            _ => {}
        }
        let mut stride_x = match ii.bd_bit_depth {
            BD1 | BD5 | BD10 | BD565 => 1,
            _ => (ii.c_bits_per_unit >> 3) / CB_CHANNELS[ii.bd_bit_depth],
        };
        if self.plane > 0 {
            stride_x = 1;
        }
        let (mut first_x, mut first_y) = (0usize, 0usize);
        if !dp.b_decode_full_frame {
            first_x = dp.c_roi_left_x as usize;
            first_y = dp.c_roi_top_y as usize;
        }
        led.reuse(&mut dp.p_offset_x, w, 0)?;
        for i in 0..w.saturating_sub(first_x) {
            dp.p_offset_x[i + first_x] = i * stride_x;
        }
        led.reuse(&mut dp.p_offset_y, h, 0)?;
        for i in 0..h.saturating_sub(first_y) {
            dp.p_offset_y[i + first_y] = i * stride_y;
        }
        Ok(())
    }

    fn mb_stride(&self, i: usize) -> usize {
        if i == 0 {
            return 256;
        }
        CBLK_CHROMAS[self.param.cf_color_format] * 16
    }

    pub(super) fn p0(&self, i: usize) -> usize {
        self.a0_off[i] + self.c_column * self.mb_stride(i)
    }

    pub(super) fn p1(&self, i: usize) -> usize {
        self.a1_off[i] + self.c_column * self.mb_stride(i)
    }

    /// `StrDecInit`: per-tile quantizers, coding contexts and bit-reader pointers.
    /// `wmii.cf_color_format` must already hold the validated external colour format.
    fn str_dec_init(&mut self, led: &Ledger, ctxs: &mut Vec<CodingContext>) -> Result<(), Error> {
        let cf_int = self.param.cf_color_format;
        let ext = self.wmii.cf_color_format;
        self.uv_resolution_change = ext != Y_ONLY
            && ((cf_int == YUV420 && ext != YUV420) || (cf_int == YUV422 && ext != YUV422));
        if self.uv_resolution_change {
            led.reuse(&mut self.p_res_u, 256 * self.cmb_width, 0)?;
            led.reuse(&mut self.p_res_v, 256 * self.cmb_width, 0)?;
        } else {
            self.p_res_u.clear();
            self.p_res_v.clear();
        }
        self.allocate_pred_info(led)?;
        if self.wmiscp.c_num_of_slice_minus1_v as usize >= MAX_TILES {
            return Err(Error::Codestream);
        }
        let fresh = WmiTile {
            c_num_qp_lp: 1,
            c_num_qp_hp: 1,
            ..WmiTile::default()
        };
        let cols = self.wmiscp.c_num_of_slice_minus1_v as usize + 1;
        let nch = self.param.c_num_channels;
        // Fresh tiles, in the kept tiles' capacity (the quantizer vectors keep theirs).
        led.charge((cols as u64).saturating_mul(std::mem::size_of::<WmiTile>() as u64))?;
        self.tiles.truncate(cols);
        for t in self.tiles.iter_mut() {
            t.reset(nch);
        }
        self.tiles
            .try_reserve_exact(cols - self.tiles.len())
            .map_err(|_| Error::OutOfMemory)?;
        self.tiles.resize(cols, fresh);
        led.keep(&self.tiles);
        // The quantizers are reallocated per tile inside the macroblock loop; at most one DC, 16
        // low-pass and 16 high-pass entries a channel and column are alive at once.
        led.charge((cols * nch * 33 * std::mem::size_of::<Quantizer>()) as u64)?;
        let scaled = self.param.scaled_arith;
        if self.param.qp_mode & 1 == 0 {
            allocate_quantizer(&mut self.tiles[0].q_dc, nch, 1);
            for i in 0..nch {
                self.tiles[0].q_dc[i][0].i_index = self.param.ui_qp_index_dc[i];
            }
            format_quantizer(
                &mut self.tiles[0].q_dc,
                ((self.param.qp_mode >> 3) & 3) as u8,
                nch,
                0,
                true,
                scaled,
            );
            self.set_uniform_quantizer(0);
        }
        if self.wmiscp.sb_subband != SB_DC_ONLY {
            if self.param.qp_mode & 2 == 0 {
                allocate_quantizer(&mut self.tiles[0].q_lp, nch, 1);
                if self.param.qp_mode & 0x200 == 0 {
                    self.use_dc_quantizer(0);
                } else {
                    for i in 0..nch {
                        self.tiles[0].q_lp[i][0].i_index = self.param.ui_qp_index_lp[i];
                    }
                    format_quantizer(
                        &mut self.tiles[0].q_lp,
                        ((self.param.qp_mode >> 5) & 3) as u8,
                        nch,
                        0,
                        true,
                        scaled,
                    );
                }
                self.set_uniform_quantizer(1);
            }
            if self.wmiscp.sb_subband != SB_NO_HIGHPASS && self.param.qp_mode & 4 == 0 {
                allocate_quantizer(&mut self.tiles[0].q_hp, nch, 1);
                if self.param.qp_mode & 0x400 == 0 {
                    self.use_lp_quantizer(1, 0);
                } else {
                    for i in 0..nch {
                        self.tiles[0].q_hp[i][0].i_index = self.param.ui_qp_index_hp[i];
                    }
                    format_quantizer(
                        &mut self.tiles[0].q_hp,
                        ((self.param.qp_mode >> 7) & 3) as u8,
                        nch,
                        0,
                        false,
                        scaled,
                    );
                }
                self.set_uniform_quantizer(2);
            }
        }
        allocate_coding_context_dec(
            self.param.cf_color_format,
            self.wmiscp.c_num_of_slice_minus1_v as usize + 1,
            led,
            ctxs,
        )?;
        self.set_bit_io_pointers(ctxs);
        Ok(())
    }

    /// Configures `asc` as this plane's interleaved alpha companion (strdec.c:3330-3390): a
    /// `Y_ONLY`, 1-channel plane sharing its tile grid, decoded in lockstep and composited by
    /// `output_mb_row`/`output_alpha`.
    fn set_up_alpha(
        &mut self,
        h: &Header,
        led: &Ledger,
        kept: Option<Box<AlphaPlane>>,
    ) -> Result<(), Error> {
        let mut plane = kept.unwrap_or_default();
        let bufs = plane.sc.take_bufs();
        plane.sc = StrCodec {
            stream_len: self.stream_len,
            secondary: true,
            ..StrCodec::with_bufs(bufs)
        };
        let asc = &mut plane.sc;
        asc.param = h.a_param;
        asc.wmiscp = h.a_scp.copy_to(
            std::mem::take(&mut asc.wmiscp.ui_tile_x),
            std::mem::take(&mut asc.wmiscp.ui_tile_y),
            Some(led),
        )?;
        asc.wmii = self.wmii;
        asc.wmii.cf_color_format = Y_ONLY;
        asc.cmb_width = self.cmb_width;
        asc.cmb_height = self.cmb_height;
        let n = 256 * asc.cmb_width;
        led.reuse(&mut asc.mb_buf, 2 * n + 2 * MB_PAD, 0)?;
        asc.a0_off[0] = MB_PAD;
        asc.a1_off[0] = MB_PAD + n;
        if asc.wmiscp.c_num_of_slice_minus1_v as usize >= MAX_TILES {
            return Err(Error::Codestream);
        }
        self.alpha = Some(plane);
        Ok(())
    }
}

/// Reads a variable length byte aligned integer.
fn get_vl_word_esc(io: &mut BitIO) -> u64 {
    let mut s = u64::from(io.get_bit32(8));
    if s == 0xfd || s == 0xfe || s == 0xff {
        return 0;
    }
    if s < 0xfb {
        return (s << 8) | u64::from(io.get_bit32(8));
    }
    s -= 0xfb;
    if s != 0 {
        s = u64::from(io.get_bit32(16)) << 16;
        s = (s | u64::from(io.get_bit32(16))) << 16;
        s <<= 16;
    }
    s |= u64::from(io.get_bit32(16)) << 16;
    s |= u64::from(io.get_bit32(16));
    s
}

const CB_CHANNELS: [usize; 11] = [1, 1, 2, 2, 2, 4, 4, 4, 0, 0, 0];

fn cancelled(cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

/// The smallest image the pipeline pays for: a pool wake is about 10 us, and one macroblock row
/// 256 pixels wide takes about 37 us to transform and output, so the hand-over per row costs at
/// most 13%. Under 65,536 pixels the wake itself is a visible share of the whole decode.
// ponytail: fixed counts; tune them from the bench's per-size table.
const PIPE_MIN_MB_COLS: usize = 16;
const PIPE_MIN_MBS: usize = 256;
/// Rows in flight: one being made, one waiting, one being consumed.
const PIPE_SLOTS: usize = 3;

/// One macroblock row of entropy-decoded coefficients, on its way from the entropy stage to the
/// transform stage. `prim` holds `MB_PAD`, then each channel's row in `mb_buf` order, then
/// `MB_PAD`; `alpha` holds `MB_PAD`, the alpha row, `MB_PAD`.
struct Slot {
    prim: Vec<i32>,
    alpha: Vec<i32>,
}

impl StrCodec {
    /// The ints of a slot's `prim` and `alpha`.
    fn slot_len(&self) -> (usize, usize) {
        let prim = 2 * MB_PAD
            + (0..self.param.c_num_channels)
                .map(|i| self.mb_stride(i) * self.cmb_width)
                .sum::<usize>();
        let alpha = if self.alpha.is_some() {
            2 * MB_PAD + 256 * self.cmb_width
        } else {
            0
        };
        (prim, alpha)
    }

    /// The bytes a pipelined decode holds beyond a serial one: the ring, and the transform
    /// stage's copy of the tile vectors.
    fn pipe_bytes(&self) -> u64 {
        let (p, a) = self.slot_len();
        let ring = (PIPE_SLOTS * (p + a)) as u64 * 4;
        let tiles = 4 * (self.wmiscp.ui_tile_x.len() + self.wmiscp.ui_tile_y.len()) as u64;
        ring.saturating_add(if self.alpha.is_some() {
            2 * tiles
        } else {
            tiles
        })
    }

    fn new_slot(&self) -> Result<Slot, Error> {
        let (p, a) = self.slot_len();
        Ok(Slot {
            prim: filled(p, 0)?,
            alpha: filled(a, 0)?,
        })
    }

    /// Makes `slots` the ring: kept slots of the right size stay as they are (a row is zeroed
    /// before the entropy stage writes it), the others are built. The ring's bytes are the
    /// pipeline's `hold`.
    fn fill_slots(&self, slots: &mut Vec<Slot>) -> Result<(), Error> {
        let (p, a) = self.slot_len();
        slots.truncate(PIPE_SLOTS);
        slots
            .try_reserve_exact(PIPE_SLOTS - slots.len())
            .map_err(|_| Error::OutOfMemory)?;
        for s in slots.iter_mut() {
            if s.prim.len() != p || s.alpha.len() != a {
                *s = self.new_slot()?;
            }
        }
        while slots.len() < PIPE_SLOTS {
            slots.push(self.new_slot()?);
        }
        Ok(())
    }

    /// A codec holding `b`'s buffers and nothing else.
    fn with_bufs(b: PlaneBufs) -> StrCodec {
        StrCodec {
            wmiscp: CodecParam {
                ui_tile_x: b.tile_x,
                ui_tile_y: b.tile_y,
                ..CodecParam::default()
            },
            dp: DecoderParameters {
                p_offset_x: b.p_offset_x,
                p_offset_y: b.p_offset_y,
                ..DecoderParameters::default()
            },
            p_res_u: b.p_res_u,
            p_res_v: b.p_res_v,
            index_table: b.index_table,
            tiles: b.tiles,
            mb_buf: b.mb_buf,
            pred_info: b.pred_info,
            pred_info_prev: b.pred_info_prev,
            ..StrCodec::default()
        }
    }

    /// The inverse of `with_bufs`: this codec's buffers, which it no longer holds.
    fn take_bufs(&mut self) -> PlaneBufs {
        PlaneBufs {
            mb_buf: std::mem::take(&mut self.mb_buf),
            pred_info: std::mem::take(&mut self.pred_info),
            pred_info_prev: std::mem::take(&mut self.pred_info_prev),
            tiles: std::mem::take(&mut self.tiles),
            index_table: std::mem::take(&mut self.index_table),
            p_res_u: std::mem::take(&mut self.p_res_u),
            p_res_v: std::mem::take(&mut self.p_res_v),
            p_offset_x: std::mem::take(&mut self.dp.p_offset_x),
            p_offset_y: std::mem::take(&mut self.dp.p_offset_y),
            tile_x: std::mem::take(&mut self.wmiscp.ui_tile_x),
            tile_y: std::mem::take(&mut self.wmiscp.ui_tile_y),
        }
    }

    /// Cuts every buffer `Ledger::keep` noted, alpha's too, to its length.
    fn shrink(&mut self) {
        self.mb_buf.shrink_to_fit();
        for v in self.pred_info.iter_mut().chain(&mut self.pred_info_prev) {
            v.shrink_to_fit();
        }
        self.tiles.shrink_to_fit();
        self.index_table.shrink_to_fit();
        self.p_res_u.shrink_to_fit();
        self.p_res_v.shrink_to_fit();
        self.dp.p_offset_x.shrink_to_fit();
        self.dp.p_offset_y.shrink_to_fit();
        self.wmiscp.ui_tile_x.shrink_to_fit();
        self.wmiscp.ui_tile_y.shrink_to_fit();
        if let Some(a) = self.alpha.as_mut() {
            a.sc.shrink();
            a.ctxs.shrink_to_fit();
        }
    }

    /// The transform and output half of this decoder, for the calling thread of a pipelined
    /// decode: it takes `mb_buf`, the output offsets and the chroma rows, and copies what else
    /// those read (the copies are charged by `pipe_bytes`, and made in `kept`). What stays in
    /// `self` is the entropy half: contexts, bit readers, prediction state, tiles and the index
    /// table. `join_transform` takes the buffers back.
    fn split_transform(&mut self, kept: &mut TransformKept) -> Result<StrCodec, Error> {
        let (ox, oy) = (
            std::mem::take(&mut self.dp.p_offset_x),
            std::mem::take(&mut self.dp.p_offset_y),
        );
        let mut dp = self.dp.clone();
        dp.p_offset_x = ox;
        dp.p_offset_y = oy;
        let wmiscp = self.wmiscp.copy_to(
            std::mem::take(&mut kept.tile_x),
            std::mem::take(&mut kept.tile_y),
            None,
        )?;
        let alpha = match self.alpha.as_mut() {
            Some(a) => {
                let mut plane = kept.alpha.take().unwrap_or_default();
                let mut ak = TransformKept {
                    tile_x: std::mem::take(&mut plane.sc.wmiscp.ui_tile_x),
                    tile_y: std::mem::take(&mut plane.sc.wmiscp.ui_tile_y),
                    alpha: None,
                };
                plane.sc = a.sc.split_transform(&mut ak)?;
                Some(plane)
            }
            None => None,
        };
        Ok(StrCodec {
            wmii: self.wmii,
            wmiscp,
            param: self.param,
            dp,
            plane: self.plane,
            uv_resolution_change: self.uv_resolution_change,
            p_res_u: std::mem::take(&mut self.p_res_u),
            p_res_v: std::mem::take(&mut self.p_res_v),
            cmb_width: self.cmb_width,
            cmb_height: self.cmb_height,
            mb_buf: std::mem::take(&mut self.mb_buf),
            a0_off: self.a0_off,
            a1_off: self.a1_off,
            alpha,
            secondary: self.secondary,
            ..StrCodec::default()
        })
    }

    /// Takes back what `split_transform` gave `t`, alpha's too.
    fn join_transform(&mut self, t: &mut StrCodec) {
        self.mb_buf = std::mem::take(&mut t.mb_buf);
        self.p_res_u = std::mem::take(&mut t.p_res_u);
        self.p_res_v = std::mem::take(&mut t.p_res_v);
        self.dp.p_offset_x = std::mem::take(&mut t.dp.p_offset_x);
        self.dp.p_offset_y = std::mem::take(&mut t.dp.p_offset_y);
        if let (Some(a), Some(ta)) = (self.alpha.as_mut(), t.alpha.as_mut()) {
            a.sc.join_transform(&mut ta.sc);
        }
    }

    /// The entropy stage of macroblock row `r`: every macroblock's coefficients, the primary
    /// plane's and then the alpha plane's, in the bit reader's order.
    fn entropy_row(
        &mut self,
        r: usize,
        ctxs: &mut [CodingContext],
        ios: &mut IoSet<'_>,
    ) -> Result<(), Error> {
        self.c_row = r;
        for c in 0..self.cmb_width {
            self.c_column = c;
            self.mb_entropy(ctxs, ios)?;
            if let Some(a) = self.alpha.as_mut() {
                a.sc.c_row = r;
                a.sc.c_column = c;
                a.sc.mb_entropy(&mut a.ctxs, ios)?;
            }
        }
        for i in 0..self.param.c_num_channels {
            std::mem::swap(&mut self.pred_info[i], &mut self.pred_info_prev[i]);
        }
        if let Some(a) = self.alpha.as_mut() {
            std::mem::swap(&mut a.sc.pred_info[0], &mut a.sc.pred_info_prev[0]);
        }
        Ok(())
    }

    /// Puts row `slot` (none: zeros, as at the bottom step) into the current row of `mb_buf`
    /// where the serial loop zero-fills it. `offs` are the channels' offsets in `slot.prim`.
    fn load_row(&mut self, slot: Option<&Slot>, offs: &[usize; MAX_CHANNELS]) {
        for i in 0..self.param.c_num_channels {
            let (a1, n) = (self.a1_off[i], self.mb_stride(i) * self.cmb_width);
            let dst = &mut self.mb_buf[a1..a1 + n];
            match slot {
                Some(s) => dst.copy_from_slice(&s.prim[offs[i]..offs[i] + n]),
                None => dst.fill(0),
            }
        }
        if let Some(a) = self.alpha.as_mut() {
            let (a1, n) = (a.sc.a1_off[0], 256 * a.sc.cmb_width);
            let dst = &mut a.sc.mb_buf[a1..a1 + n];
            match slot {
                Some(s) => dst.copy_from_slice(&s.alpha[MB_PAD..MB_PAD + n]),
                None => dst.fill(0),
            }
        }
    }

    /// Writes the pixels of row `c_row - 1` once the row is transformed, hands the finished
    /// rows to `on_rows`, and moves on to the next row's buffers.
    fn finish_row(&mut self, dst: &mut [u8], on_rows: OnRows<'_>) -> Result<(), Error> {
        let nch = self.param.c_num_channels;
        let (row16, top, bottom) = (
            (self.c_row * 16) as isize,
            self.dp.c_roi_top_y,
            self.dp.c_roi_bottom_y,
        );
        if self.c_row != 0 && (self.dp.b_decode_full_frame || (row16 > top && row16 <= bottom + 16))
        {
            self.output_mb_row(dst)?;
            let y = (self.c_row * 16)
                .saturating_sub(self.param.c_extra_pixels_top)
                .min(self.wmii.c_height);
            on_rows(dst, y)?;
        }
        for i in 0..nch {
            std::mem::swap(&mut self.pred_info[i], &mut self.pred_info_prev[i]);
            std::mem::swap(&mut self.a0_off[i], &mut self.a1_off[i]);
        }
        if let Some(a) = self.alpha.as_mut() {
            std::mem::swap(&mut a.sc.pred_info[0], &mut a.sc.pred_info_prev[0]);
            std::mem::swap(&mut a.sc.a0_off[0], &mut a.sc.a1_off[0]);
        }
        Ok(())
    }
}

/// Decodes `stream` (the codestream from `WMPHOTO` to end of file, whose header is `h`) into
/// `dst`, laid out as jxrlib writes format `f`. `cx` carries the ledger, the thread budget and the
/// cancel flag, which is checked each macroblock row. After each row's pixels are written,
/// `on_rows(dst, y)` is called with `y`, the number of image rows that are final, and once more
/// with the height when the decode ends.
///
/// With a pool and an image of enough macroblocks this runs as a two-stage pipeline: a pool
/// worker entropy-decodes row r + 1 while this thread transforms row r and writes its pixels. The
/// bytes written and the error returned are the same as the serial loop's.
///
/// The working buffers come from `scratch` and go back to it when the call ends, whatever the
/// result, made as fresh ones each time: a decode of a stream of the same shape allocates nothing
/// on one thread.
pub(crate) fn decode(
    stream: &[u8],
    h: &Header,
    f: &Format,
    dst: &mut [u8],
    stride: usize,
    cx: &Ctx<'_>,
    on_rows: OnRows<'_>,
    scratch: &mut Scratch,
) -> Result<(), Error> {
    let led = &cx.ledger;
    if h.param.c_sub_version == CODEC_SUBVERSION
        || (h.param.alpha_channel && !f.alpha)
        || h.scp.sb_subband == SB_ISOLATED
    {
        return Err(Error::Unsupported);
    }
    if h.ii.bd_bit_depth != f.bit_depth {
        return Err(Error::Unsupported);
    }
    // Everything this call charges is given back when it returns: its buffers are in the scratch
    // or dropped by then, and `decode_component` may call it again.
    let mark = led.mark();
    let mut parts = Parts::take(scratch, stream);
    let r = decode_charged(&mut parts, stream, h, f, dst, stride, cx, on_rows);
    parts.give_back(scratch);
    led.rewind(mark);
    r
}

/// What one decode works in, taken from the `Scratch` and put back when it ends.
struct Parts<'a> {
    sc: StrCodec,
    ctxs: Vec<CodingContext>,
    ios: IoSet<'a>,
    slots: Vec<Slot>,
    /// Alpha's plane from the scratch, for `set_up_alpha`.
    alpha: Option<Box<AlphaPlane>>,
    tr: TransformKept,
}

impl<'a> Parts<'a> {
    fn take(scratch: &mut Scratch, stream: &'a [u8]) -> Self {
        let mut v: Vec<BitIO<'a>> = std::mem::take(&mut scratch.ios);
        v.clear();
        v.push(BitIO::new());
        Parts {
            sc: StrCodec::with_bufs(std::mem::take(&mut scratch.plane)),
            ctxs: std::mem::take(&mut scratch.ctxs),
            ios: IoSet { v, stream },
            slots: std::mem::take(&mut scratch.slots),
            alpha: scratch.alpha.take(),
            tr: std::mem::take(&mut scratch.tr),
        }
    }

    /// Puts every buffer back, whatever state the decode left it in: the next decode makes them
    /// as fresh ones.
    fn give_back(self, scratch: &mut Scratch) {
        let Parts {
            mut sc,
            ctxs,
            ios,
            slots,
            alpha,
            tr,
        } = self;
        scratch.plane = sc.take_bufs();
        scratch.alpha = sc.alpha.take().or(alpha);
        scratch.ctxs = ctxs;
        // The readers are reset, which also drops their borrow of the stream; std reuses the
        // allocation for this in place.
        scratch.ios = ios.v.into_iter().map(|_| BitIO::new()).collect();
        scratch.slots = slots;
        scratch.tr = tr;
    }
}

fn decode_charged(
    parts: &mut Parts<'_>,
    stream: &[u8],
    h: &Header,
    f: &Format,
    dst: &mut [u8],
    stride: usize,
    cx: &Ctx<'_>,
    on_rows: OnRows<'_>,
) -> Result<(), Error> {
    let (led, cancel) = (&cx.ledger, cx.cancel);
    let Parts {
        sc,
        ctxs,
        ios,
        slots,
        alpha,
        tr,
    } = parts;
    // The header's own tile tables (and the alpha copy), parsed before this call's ledger existed.
    // `Header::parse` cut them to the size the stream needs, so only their length is held.
    led.charge(
        4 * (h.scp.ui_tile_x.len()
            + h.scp.ui_tile_y.len()
            + h.a_scp.ui_tile_x.len()
            + h.a_scp.ui_tile_y.len()) as u64,
    )?;

    sc.wmiscp = h.scp.copy_to(
        std::mem::take(&mut sc.wmiscp.ui_tile_x),
        std::mem::take(&mut sc.wmiscp.ui_tile_y),
        Some(led),
    )?;
    sc.stream_len = stream.len();
    sc.out_stride = stride;
    sc.param = h.param;
    sc.wmii = h.ii;

    // WMPhotoValidate: output colour format.
    let mut ext = f.color_format;
    let internal = sc.param.cf_color_format;
    if internal == N_COMPONENT {
        ext = N_COMPONENT;
    }
    if internal == CMYK && ext != Y_ONLY && ext != CF_RGB {
        ext = CMYK;
    }
    if h.ii.cf_color_format == CF_RGB && ext != Y_ONLY && ext != N_COMPONENT {
        ext = CF_RGB;
    }
    if h.ii.cf_color_format == CF_RGBE {
        if f.color_format != CF_RGBE {
            // writer needs 32-bit pixels
            return Err(Error::Unsupported);
        }
        ext = CF_RGBE;
    }
    // jxrlib writes past the pixel here.
    if (ext == N_COMPONENT
        && sc.param.c_num_channels * CB_CHANNELS[f.bit_depth] * 8 > f.bits_per_unit)
        || (ext == CMYK && 4 * CB_CHANNELS[f.bit_depth] * 8 > f.bits_per_unit)
    {
        return Err(Error::Unsupported);
    }
    if f.plane > 0
        && (ext != N_COMPONENT
            || f.bit_depth != BD16
            || f.alpha
            || f.plane > sc.param.c_num_channels)
    {
        return Err(Error::Unsupported);
    }
    sc.plane = f.plane;
    sc.wmii.cf_color_format = ext;
    sc.wmii.b_rgb = f.rgb;
    sc.wmii.c_bits_per_unit = f.bits_per_unit;
    sc.wmii.o_orientation = 0;
    sc.wmii.c_roi_left_x = 0;
    sc.wmii.c_roi_top_y = 0;
    sc.wmii.c_roi_width = h.width;
    sc.wmii.c_roi_height = h.height;

    // ImageStrDecInit: padded size.
    sc.wmii.c_width += sc.param.c_extra_pixels_left + sc.param.c_extra_pixels_right;
    sc.wmii.c_height += sc.param.c_extra_pixels_top + sc.param.c_extra_pixels_bottom;
    sc.cmb_width = sc.wmii.c_width.div_ceil(16);
    sc.cmb_height = sc.wmii.c_height.div_ceil(16);
    // jxrlib layout (strdec.c:3320): edge reads cross channels.
    let nch = sc.param.c_num_channels;
    let mut total = 2 * MB_PAD;
    for i in 0..nch {
        total += 2 * sc.mb_stride(i) * sc.cmb_width;
    }
    led.reuse(&mut sc.mb_buf, total, 0)?;
    let mut off = MB_PAD;
    for i in 0..nch {
        let n = sc.mb_stride(i) * sc.cmb_width;
        sc.a0_off[i] = off;
        sc.a1_off[i] = off + n;
        off += 2 * n;
    }

    // StrIODecInit
    sc.allocate_bit_io_info(stream.len(), ios, led)?;
    ios.attach(HEADER_IO, h.size as u64);
    sc.read_index_table(ios)?;

    if h.param.alpha_channel {
        sc.set_up_alpha(h, led, alpha.take())?;
    }

    sc.str_dec_init(led, ctxs)?;
    if let Some(a) = sc.alpha.as_mut() {
        a.sc.n_bit_io = sc.n_bit_io;
        a.sc.c_sb = sc.c_sb;
        a.sc.str_dec_init(led, &mut a.ctxs)?;
    }

    // ImageStrDecDecode
    sc.set_roi();
    if let Some(a) = sc.alpha.as_mut() {
        a.sc.set_roi();
    }
    let mut c_mb_row = sc.cmb_height;
    if !sc.dp.b_decode_full_frame {
        c_mb_row = ((sc.dp.c_roi_bottom_y + 16) >> 4) as usize;
    }
    sc.init_lookup_tables(led)?;
    // Every charge a fresh decode makes is made. The kept buffers' spare capacity is booked now,
    // or cut when it does not fit.
    if !led.settle() {
        sc.shrink();
        ctxs.shrink_to_fit();
        ios.v.shrink_to_fit();
    }

    // The pipeline: worth it for this many macroblocks, a pool with a worker for it, and the
    // ring and copies within `max_alloc` (a thread count never causes an error that one thread
    // would not give).
    if cx.uncapped
        || (sc.cmb_width >= PIPE_MIN_MB_COLS && sc.cmb_width * sc.cmb_height >= PIPE_MIN_MBS)
    {
        if let Some(pool) = par::stage(cx)
            && let Some(held) = led.hold(sc.pipe_bytes())
        {
            if sc.fill_slots(slots).is_ok() {
                let r = pipelined(
                    sc,
                    ctxs,
                    ios,
                    (pool, slots, tr),
                    cx,
                    dst,
                    on_rows,
                    c_mb_row + 1,
                );
                drop(held);
                return r;
            }
        }
    }

    sc.c_row = 0;
    while sc.c_row <= c_mb_row {
        cancelled(cancel)?;
        for i in 0..nch {
            let (a1, n) = (sc.a1_off[i], sc.mb_stride(i) * sc.cmb_width);
            sc.mb_buf[a1..a1 + n].fill(0);
        }
        if let Some(a) = sc.alpha.as_mut() {
            let (a1, n) = (a.sc.a1_off[0], 256 * a.sc.cmb_width);
            a.sc.mb_buf[a1..a1 + n].fill(0);
        }
        sc.c_column = 0;
        while sc.c_column <= sc.cmb_width {
            sc.process_macroblock_dec(ctxs, ios)?;
            sc.c_column += 1;
        }
        sc.finish_row(dst, on_rows)?;
        sc.c_row += 1;
    }
    on_rows(dst, sc.wmii.c_height)
}

/// The decode loop as a pipeline over `n_t` macroblock rows: `sc`'s entropy half runs on a pool
/// worker (and on this thread when the worker is not on the row this thread needs), its
/// transform half and the output here.
fn pipelined(
    sc: &mut StrCodec,
    ctxs: &mut [CodingContext],
    ios: &mut IoSet<'_>,
    (pool, slots, tr): (&par::ThreadPool, &mut Vec<Slot>, &mut TransformKept),
    cx: &Ctx<'_>,
    dst: &mut [u8],
    on_rows: OnRows<'_>,
    n_t: usize,
) -> Result<(), Error> {
    let cancel = cx.cancel;
    let mut t = sc.split_transform(tr)?;
    // The entropy half writes each row into a slot, at compact offsets.
    let mut offs = [0usize; MAX_CHANNELS];
    let mut off = MB_PAD;
    for i in 0..sc.param.c_num_channels {
        offs[i] = off;
        sc.a1_off[i] = off;
        sc.a0_off[i] = off;
        off += sc.mb_stride(i) * sc.cmb_width;
    }
    if let Some(a) = sc.alpha.as_mut() {
        a.sc.a1_off[0] = MB_PAD;
        a.sc.a0_off[0] = MB_PAD;
    }
    let n_e = n_t.min(sc.cmb_height);
    let cmb_width = t.cmb_width;
    let e = |r: usize, slot: &mut Slot| -> Result<(), Error> {
        cancelled(cancel)?;
        std::mem::swap(&mut sc.mb_buf, &mut slot.prim);
        sc.mb_buf.fill(0);
        if let Some(a) = sc.alpha.as_mut() {
            std::mem::swap(&mut a.sc.mb_buf, &mut slot.alpha);
            a.sc.mb_buf.fill(0);
        }
        let res = sc.entropy_row(r, ctxs, ios);
        std::mem::swap(&mut sc.mb_buf, &mut slot.prim);
        if let Some(a) = sc.alpha.as_mut() {
            std::mem::swap(&mut a.sc.mb_buf, &mut slot.alpha);
        }
        res
    };
    let height = t.wmii.c_height;
    let row = |r: usize, slot: Option<&Slot>| -> Result<(), Error> {
        cancelled(cancel)?;
        t.load_row(slot, &offs);
        for c in 0..=cmb_width {
            t.c_row = r;
            t.c_column = c;
            t.mb_transform();
            if let Some(a) = t.alpha.as_mut() {
                a.sc.c_row = r;
                a.sc.c_column = c;
                a.sc.mb_transform();
            }
        }
        t.finish_row(dst, on_rows)
    };
    let r = par::pipe(pool, &cx.gauge, slots, (n_e, n_t), e, row);
    // Whatever the result, the buffers go back to the entropy half and the kept ones to `tr`.
    sc.join_transform(&mut t);
    tr.tile_x = std::mem::take(&mut t.wmiscp.ui_tile_x);
    tr.tile_y = std::mem::take(&mut t.wmiscp.ui_tile_y);
    tr.alpha = t.alpha.take();
    r?;
    on_rows(dst, height)
}
