// Ported from jxrlib adapthuff.c (BSD-2, see NOTICE).

/// Tables are padded to one size for copying.
pub(super) const HUF_TABLE_LEN: usize = 64;

pub(super) const G4_HUFF_LOOKUP_TABLE: [i16; HUF_TABLE_LEN] = [
    19, 19, 19, 19, 27, 27, 27, 27, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0,
];

pub(super) const G5_HUFF_LOOKUP_TABLE: [[i16; HUF_TABLE_LEN]; 2] = [
    [
        28, 28, 36, 36, 19, 19, 19, 19, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        11, 11, 11, 11, 19, 19, 19, 19, 27, 27, 27, 27, 35, 35, 35, 35, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

pub(super) const G6_HUFF_LOOKUP_TABLE: [[i16; HUF_TABLE_LEN]; 4] = [
    [
        13, 29, 44, 44, 19, 19, 19, 19, 34, 34, 34, 34, 34, 34, 34, 34, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        12, 12, 28, 28, 43, 43, 43, 43, 2, 2, 2, 2, 2, 2, 2, 2, 18, 18, 18, 18, 18, 18, 18, 18, 34,
        34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        4, 4, 12, 12, 43, 43, 43, 43, 18, 18, 18, 18, 18, 18, 18, 18, 26, 26, 26, 26, 26, 26, 26,
        26, 34, 34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        5, 13, 36, 36, 43, 43, 43, 43, 18, 18, 18, 18, 18, 18, 18, 18, 25, 25, 25, 25, 25, 25, 25,
        25, 25, 25, 25, 25, 25, 25, 25, 25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

pub(super) const G7_HUFF_LOOKUP_TABLE: [[i16; HUF_TABLE_LEN]; 2] = [
    [
        45, 53, 36, 36, 27, 27, 27, 27, 2, 2, 2, 2, 2, 2, 2, 2, 10, 10, 10, 10, 10, 10, 10, 10, 18,
        18, 18, 18, 18, 18, 18, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 37, 28, 28, 19, 19, 19, 19, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

pub(super) const G8_HUFF_LOOKUP_TABLE: [[i16; HUF_TABLE_LEN]; 2] = [
    [
        53, 21, 28, 28, 11, 11, 11, 11, 43, 43, 43, 43, 59, 59, 59, 59, 2, 2, 2, 2, 2, 2, 2, 2, 34,
        34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        52, 52, 20, 20, 3, 3, 3, 3, 11, 11, 11, 11, 27, 27, 27, 27, 35, 35, 35, 35, 43, 43, 43, 43,
        58, 58, 58, 58, 58, 58, 58, 58, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

pub(super) const G9_HUFF_LOOKUP_TABLE: [[i16; HUF_TABLE_LEN]; 2] = [
    [
        13, 29, 37, 61, 20, 20, 68, 68, 3, 3, 3, 3, 51, 51, 51, 51, 41, 41, 41, 41, 41, 41, 41, 41,
        41, 41, 41, 41, 41, 41, 41, 41, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 53, 28, 28, 11, 11, 11, 11, 19, 19, 19, 19, 43, 43, 43, 43, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, -32734, 4, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

pub(super) const G12_HUFF_LOOKUP_TABLE: [[i16; HUF_TABLE_LEN]; 5] = [
    [
        -32736, 5, 76, 76, 37, 53, 69, 85, 43, 43, 43, 43, 91, 91, 91, 91, 57, 57, 57, 57, 57, 57,
        57, 57, 57, 57, 57, 57, 57, 57, 57, 57, -32734, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 85, 13, 53, 4, 4, 36, 36, 43, 43, 43, 43, 67, 67, 67, 67, 75, 75, 75, 75, 91, 91,
        91, 91, 58, 58, 58, 58, 58, 58, 58, 58, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 37, 92, 92, 11, 11, 11, 11, 43, 43, 43, 43, 59, 59, 59, 59, 67, 67, 67, 67, 75, 75,
        75, 75, 2, 2, 2, 2, 2, 2, 2, 2, -32734, -32732, 2, 3, 6, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 29, 37, 69, 3, 3, 3, 3, 43, 43, 43, 43, 59, 59, 59, 59, 75, 75, 75, 75, 91, 91, 91,
        91, 10, 10, 10, 10, 10, 10, 10, 10, -32734, 10, 2, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    [
        -32736, 93, 28, 28, 60, 60, 76, 76, 3, 3, 3, 3, 43, 43, 43, 43, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        9, 9, 9, 9, 9, 9, 9, -32734, -32732, -32730, 2, 4, 8, 6, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
];

const G5_DELTA_TABLE: [i32; 5] = [0, -1, 0, 1, 1];

const G6_DELTA_TABLE: [i32; 18] = [
    -1, 1, 1, 1, 0, 1, //
    -2, 0, 0, 2, 0, 0, //
    -1, -1, 0, 1, -2, 0,
];

const G7_DELTA_TABLE: [i32; 7] = [1, 0, -1, -1, -1, -1, -1];

const G9_DELTA_TABLE: [i32; 9] = [2, 2, 1, 1, -1, -2, -2, -2, -3];

const G12_DELTA_TABLE: [i32; 48] = [
    1, 1, 1, 1, 1, 0, 0, -1, 2, 1, 0, 0, //
    2, 2, -1, -1, -1, 0, -2, -1, 0, 0, -2, -1, //
    -1, 1, 0, 2, 0, 0, 0, 0, -2, 0, 1, 1, //
    0, 1, 0, 1, -2, 0, -1, -1, -2, -1, -2, -2,
];

/// `CAdaptiveHuffman`; only the decode tables are kept. The tables are copies, so a lookup is one
/// load from the struct.
#[derive(Clone, Copy)]
pub(super) struct AdaptiveHuffman {
    pub(super) huf_dec_table: [i16; HUF_TABLE_LEN],
    pub(super) delta: [i32; 16],
    pub(super) delta1: [i32; 16],
    pub(super) disc: i32,
    pub(super) disc1: i32,
    pub(super) n_symbols: usize,
    pub(super) table_index: i32,
    pub(super) initialize: bool,
    pub(super) upper_bound: i32,
    pub(super) lower_bound: i32,
}

impl Default for AdaptiveHuffman {
    fn default() -> Self {
        AdaptiveHuffman {
            huf_dec_table: [0; HUF_TABLE_LEN],
            delta: [0; 16],
            delta1: [0; 16],
            disc: 0,
            disc1: 0,
            n_symbols: 0,
            table_index: 0,
            initialize: false,
            upper_bound: 0,
            lower_bound: 0,
        }
    }
}

/// Copies the tail of `t`, as the C pointer sees it.
fn set_delta(d: &mut [i32; 16], t: &[i32]) {
    *d = [0; 16];
    let n = t.len().min(16);
    d[..n].copy_from_slice(&t[..n]);
}

const THRESHOLD: i32 = 8;
const MEMORY: i32 = 8;

const G_MAX_TABLES: [i32; 13] = [0, 0, 0, 0, 1, 2, 4, 2, 2, 2, 0, 0, 5];
const G_SECOND_DISC: [i32; 13] = [0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1];

impl AdaptiveHuffman {
    pub(super) fn adapt_discriminant(&mut self) {
        let i_sym = self.n_symbols;
        let mut change = false;
        if !self.initialize {
            self.initialize = true;
            self.disc = 0;
            self.disc1 = 0;
            self.table_index = G_SECOND_DISC[i_sym];
        }
        let d_l = self.disc;
        let mut d_h = self.disc;
        if G_SECOND_DISC[i_sym] != 0 {
            d_h = self.disc1;
        }
        if d_l < self.lower_bound {
            self.table_index -= 1;
            change = true;
        } else if d_h > self.upper_bound {
            self.table_index += 1;
            change = true;
        }
        if change {
            self.disc = 0;
            self.disc1 = 0;
        }
        self.disc = self.disc.clamp(-THRESHOLD * MEMORY, THRESHOLD * MEMORY);
        self.disc1 = self.disc1.clamp(-THRESHOLD * MEMORY, THRESHOLD * MEMORY);

        let t = self.table_index;
        self.lower_bound = if t == 0 { i32::MIN } else { -THRESHOLD };
        self.upper_bound = if t == G_MAX_TABLES[i_sym] - 1 {
            1 << 30
        } else {
            THRESHOLD
        };

        let b2i = |b: bool| -> i32 { i32::from(b) };
        let ts = t as usize;
        let sym = i_sym as i32;
        match i_sym {
            4 => {
                self.delta = [0; 16];
                self.huf_dec_table = G4_HUFF_LOOKUP_TABLE;
            }
            5 => {
                set_delta(&mut self.delta, &G5_DELTA_TABLE);
                self.huf_dec_table = G5_HUFF_LOOKUP_TABLE[ts];
            }
            6 => {
                let s1 = (sym * (t - b2i(t + 1 == G_MAX_TABLES[i_sym]))) as usize;
                set_delta(&mut self.delta1, &G6_DELTA_TABLE[s1..]);
                let s0 = ((t - 1 + b2i(t == 0)) * sym) as usize;
                set_delta(&mut self.delta, &G6_DELTA_TABLE[s0..]);
                self.huf_dec_table = G6_HUFF_LOOKUP_TABLE[ts];
            }
            7 => {
                set_delta(&mut self.delta, &G7_DELTA_TABLE);
                self.huf_dec_table = G7_HUFF_LOOKUP_TABLE[ts];
            }
            8 => {
                self.delta = [0; 16];
                self.huf_dec_table = G8_HUFF_LOOKUP_TABLE[0];
            }
            9 => {
                set_delta(&mut self.delta, &G9_DELTA_TABLE);
                self.huf_dec_table = G9_HUFF_LOOKUP_TABLE[ts];
            }
            12 => {
                let s1 = (sym * (t - b2i(t + 1 == G_MAX_TABLES[i_sym]))) as usize;
                set_delta(&mut self.delta1, &G12_DELTA_TABLE[s1..]);
                let s0 = ((t - 1 + b2i(t == 0)) * sym) as usize;
                set_delta(&mut self.delta, &G12_DELTA_TABLE[s0..]);
                self.huf_dec_table = G12_HUFF_LOOKUP_TABLE[ts];
            }
            _ => {}
        }
    }
}
