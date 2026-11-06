//! MPEG-2 video (ISO/IEC 13818-2) for the disc's movies
//! (docs/decisions/0007-native-movie-video.md). The decoder covers what all
//! 36 MPEG/*.PSS and MPEG2/*.PSS streams use: Main profile, 4:2:0,
//! progressive frame pictures with frame prediction and frame DCT only
//! (frame_pred_frame_dct = 1), I, P and B pictures. Field pictures, field or
//! dual-prime prediction and other chroma formats are refused, so such a
//! stream ends the movie as an InitMovie failure would.
//!
//! The inverse DCT is FFmpeg's integer "simple" IDCT (THIRD_PARTY.md), so
//! the pictures equal FFmpeg's C decoder bit for bit
//! (tools/python/movie_check.py). The VLC tables are the standard's Tables
//! B.1 to B.15; tools/python/mpeg2_tables_check.py compares them with
//! PCSX2's libmpeg2-derived lookup tables.
use std::collections::VecDeque;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mpeg2Error(pub String);

impl std::fmt::Display for Mpeg2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MPEG-2: {}", self.0)
    }
}

impl std::error::Error for Mpeg2Error {}

type R<T> = Result<T, Mpeg2Error>;

fn err<T>(s: impl Into<String>) -> R<T> {
    Err(Mpeg2Error(s.into()))
}

/// A decoded picture in display order: 8-bit Y, Cb and Cr planes, the
/// chroma planes at half width and height.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub cb: Vec<u8>,
    pub cr: Vec<u8>,
}

// macroblock_type flags (the libmpeg2/PCSX2 values).
const INTRA: i16 = 1;
const PATTERN: i16 = 2;
const BWD: i16 = 4;
const FWD: i16 = 8;
const QUANT: i16 = 16;

const MBA_ESCAPE: i16 = 34;
const MBA_STUFFING: i16 = 35;

/// Table B.1, macroblock_address_increment (with macroblock_escape and the
/// MPEG-1 stuffing code, which FFmpeg also skips in MPEG-2 streams).
const MBA: &[(&str, i16)] = &[
    ("1", 1),
    ("011", 2),
    ("010", 3),
    ("0011", 4),
    ("0010", 5),
    ("0001 1", 6),
    ("0001 0", 7),
    ("0000 111", 8),
    ("0000 110", 9),
    ("0000 1011", 10),
    ("0000 1010", 11),
    ("0000 1001", 12),
    ("0000 1000", 13),
    ("0000 0111", 14),
    ("0000 0110", 15),
    ("0000 0101 11", 16),
    ("0000 0101 10", 17),
    ("0000 0101 01", 18),
    ("0000 0101 00", 19),
    ("0000 0100 11", 20),
    ("0000 0100 10", 21),
    ("0000 0100 011", 22),
    ("0000 0100 010", 23),
    ("0000 0100 001", 24),
    ("0000 0100 000", 25),
    ("0000 0011 111", 26),
    ("0000 0011 110", 27),
    ("0000 0011 101", 28),
    ("0000 0011 100", 29),
    ("0000 0011 011", 30),
    ("0000 0011 010", 31),
    ("0000 0011 001", 32),
    ("0000 0011 000", 33),
    ("0000 0001 000", MBA_ESCAPE),
    ("0000 0001 111", MBA_STUFFING),
];

/// Tables B.2 to B.4, macroblock_type in I, P and B pictures.
const MB_I: &[(&str, i16)] = &[("1", INTRA), ("01", INTRA | QUANT)];
const MB_P: &[(&str, i16)] = &[
    ("1", FWD | PATTERN),
    ("01", PATTERN),
    ("001", FWD),
    ("0001 1", INTRA),
    ("0001 0", FWD | PATTERN | QUANT),
    ("0000 1", PATTERN | QUANT),
    ("0000 01", INTRA | QUANT),
];
const MB_B: &[(&str, i16)] = &[
    ("10", FWD | BWD),
    ("11", FWD | BWD | PATTERN),
    ("010", BWD),
    ("011", BWD | PATTERN),
    ("0010", FWD),
    ("0011", FWD | PATTERN),
    ("0001 1", INTRA),
    ("0001 0", FWD | BWD | PATTERN | QUANT),
    ("0000 11", FWD | PATTERN | QUANT),
    ("0000 10", BWD | PATTERN | QUANT),
    ("0000 01", INTRA | QUANT),
];

/// Table B.9, coded_block_pattern (4:2:0; block 0 is the high bit).
const CBP: &[(&str, i16)] = &[
    ("111", 60),
    ("1101", 4),
    ("1100", 8),
    ("1011", 16),
    ("1010", 32),
    ("1001 1", 12),
    ("1001 0", 48),
    ("1000 1", 20),
    ("1000 0", 40),
    ("0111 1", 28),
    ("0111 0", 44),
    ("0110 1", 52),
    ("0110 0", 56),
    ("0101 1", 1),
    ("0101 0", 61),
    ("0100 1", 2),
    ("0100 0", 62),
    ("0011 11", 24),
    ("0011 10", 36),
    ("0011 01", 3),
    ("0011 00", 63),
    ("0010 111", 5),
    ("0010 110", 9),
    ("0010 101", 17),
    ("0010 100", 33),
    ("0010 011", 6),
    ("0010 010", 10),
    ("0010 001", 18),
    ("0010 000", 34),
    ("0001 1111", 7),
    ("0001 1110", 11),
    ("0001 1101", 19),
    ("0001 1100", 35),
    ("0001 1011", 13),
    ("0001 1010", 49),
    ("0001 1001", 21),
    ("0001 1000", 41),
    ("0001 0111", 14),
    ("0001 0110", 50),
    ("0001 0101", 22),
    ("0001 0100", 42),
    ("0001 0011", 15),
    ("0001 0010", 51),
    ("0001 0001", 23),
    ("0001 0000", 43),
    ("0000 1111", 25),
    ("0000 1110", 37),
    ("0000 1101", 26),
    ("0000 1100", 38),
    ("0000 1011", 29),
    ("0000 1010", 45),
    ("0000 1001", 53),
    ("0000 1000", 57),
    ("0000 0111", 30),
    ("0000 0110", 46),
    ("0000 0101", 54),
    ("0000 0100", 58),
    ("0000 0011 1", 31),
    ("0000 0011 0", 47),
    ("0000 0010 1", 55),
    ("0000 0010 0", 59),
    ("0000 0001 1", 27),
    ("0000 0001 0", 39),
    ("0000 0000 1", 0),
];

/// Table B.10, motion_code magnitude (the sign bit follows a nonzero code).
const MOTION_CODE: &[(&str, i16)] = &[
    ("1", 0),
    ("01", 1),
    ("001", 2),
    ("0001", 3),
    ("0000 11", 4),
    ("0000 101", 5),
    ("0000 100", 6),
    ("0000 011", 7),
    ("0000 0101 1", 8),
    ("0000 0101 0", 9),
    ("0000 0100 1", 10),
    ("0000 0100 01", 11),
    ("0000 0100 00", 12),
    ("0000 0011 11", 13),
    ("0000 0011 10", 14),
    ("0000 0011 01", 15),
    ("0000 0011 00", 16),
];

/// Tables B.12 and B.13, dct_dc_size_luminance and _chrominance.
const DC_LUMA: &[(&str, i16)] = &[
    ("100", 0),
    ("00", 1),
    ("01", 2),
    ("101", 3),
    ("110", 4),
    ("1110", 5),
    ("1111 0", 6),
    ("1111 10", 7),
    ("1111 110", 8),
    ("1111 1110", 9),
    ("1111 1111 0", 10),
    ("1111 1111 1", 11),
];
const DC_CHROMA: &[(&str, i16)] = &[
    ("00", 0),
    ("01", 1),
    ("10", 2),
    ("110", 3),
    ("1110", 4),
    ("1111 0", 5),
    ("1111 10", 6),
    ("1111 110", 7),
    ("1111 1110", 8),
    ("1111 1111 0", 9),
    ("1111 1111 10", 10),
    ("1111 1111 11", 11),
];

const EOB: u8 = 64;
const ESCAPE: u8 = 65;

/// Tables B.14 and B.15, DCT coefficients: (code without the sign bit,
/// run, level). Codes of 12 bits and more are common to both tables except
/// where B.15 gives the pair a shorter code.
const DCT_B14: &[(&str, u8, u8)] = &[
    ("10", EOB, 0),
    ("11", 0, 1), // not the first coefficient (that is "1")
    ("011", 1, 1),
    ("0100", 0, 2),
    ("0101", 2, 1),
    ("0010 1", 0, 3),
    ("0011 1", 3, 1),
    ("0011 0", 4, 1),
    ("0001 10", 1, 2),
    ("0001 11", 5, 1),
    ("0001 01", 6, 1),
    ("0001 00", 7, 1),
    ("0000 110", 0, 4),
    ("0000 100", 2, 2),
    ("0000 111", 8, 1),
    ("0000 101", 9, 1),
    ("0000 01", ESCAPE, 0),
    ("0010 0110", 0, 5),
    ("0010 0001", 0, 6),
    ("0010 0101", 1, 3),
    ("0010 0100", 3, 2),
    ("0010 0111", 10, 1),
    ("0010 0011", 11, 1),
    ("0010 0010", 12, 1),
    ("0010 0000", 13, 1),
    ("0000 0010 10", 0, 7),
    ("0000 0011 00", 1, 4),
    ("0000 0010 11", 2, 3),
    ("0000 0011 11", 4, 2),
    ("0000 0010 01", 5, 2),
    ("0000 0011 10", 14, 1),
    ("0000 0011 01", 15, 1),
    ("0000 0010 00", 16, 1),
    ("0000 0001 1101", 0, 8),
    ("0000 0001 1000", 0, 9),
    ("0000 0001 0011", 0, 10),
    ("0000 0001 0000", 0, 11),
    ("0000 0001 1011", 1, 5),
    ("0000 0001 0100", 2, 4),
    ("0000 0000 1101 0", 0, 12),
    ("0000 0000 1100 1", 0, 13),
    ("0000 0000 1100 0", 0, 14),
    ("0000 0000 1011 1", 0, 15),
];
const DCT_B15: &[(&str, u8, u8)] = &[
    ("0110", EOB, 0),
    ("10", 0, 1),
    ("010", 1, 1),
    ("110", 0, 2),
    ("0010 1", 2, 1),
    ("0111", 0, 3),
    ("0011 1", 3, 1),
    ("0001 10", 4, 1),
    ("0011 0", 1, 2),
    ("0001 11", 5, 1),
    ("0000 110", 6, 1),
    ("0000 100", 7, 1),
    ("1110 0", 0, 4),
    ("0000 111", 2, 2),
    ("0000 101", 8, 1),
    ("1111 000", 9, 1),
    ("0000 01", ESCAPE, 0),
    ("1110 1", 0, 5),
    ("0001 01", 0, 6),
    ("1111 001", 1, 3),
    ("0010 0110", 3, 2),
    ("1111 010", 10, 1),
    ("0010 0001", 11, 1),
    ("0010 0101", 12, 1),
    ("0010 0100", 13, 1),
    ("0001 00", 0, 7),
    ("0010 0111", 1, 4),
    ("1111 1100", 2, 3),
    ("1111 1101", 4, 2),
    ("0000 0010 0", 5, 2),
    ("0000 0010 1", 14, 1),
    ("0000 0011 1", 15, 1),
    ("0000 0011 01", 16, 1),
    ("1111 011", 0, 8),
    ("1111 100", 0, 9),
    ("0010 0011", 0, 10),
    ("0010 0010", 0, 11),
    ("0010 0000", 1, 5),
    ("0000 0011 00", 2, 4),
    ("1111 1010", 0, 12),
    ("1111 1011", 0, 13),
    ("1111 1110", 0, 14),
    ("1111 1111", 0, 15),
];
const DCT_COMMON: &[(&str, u8, u8)] = &[
    ("0000 0001 1100", 3, 3),
    ("0000 0001 0010", 4, 3),
    ("0000 0001 1110", 6, 2),
    ("0000 0001 0101", 7, 2),
    ("0000 0001 0001", 8, 2),
    ("0000 0001 1111", 17, 1),
    ("0000 0001 1010", 18, 1),
    ("0000 0001 1001", 19, 1),
    ("0000 0001 0111", 20, 1),
    ("0000 0001 0110", 21, 1),
    ("0000 0000 1011 0", 1, 6),
    ("0000 0000 1010 1", 1, 7),
    ("0000 0000 1010 0", 2, 5),
    ("0000 0000 1001 1", 3, 4),
    ("0000 0000 1001 0", 5, 3),
    ("0000 0000 1000 1", 9, 2),
    ("0000 0000 1000 0", 10, 2),
    ("0000 0000 1111 1", 22, 1),
    ("0000 0000 1111 0", 23, 1),
    ("0000 0000 1110 1", 24, 1),
    ("0000 0000 1110 0", 25, 1),
    ("0000 0000 1101 1", 26, 1),
    ("0000 0000 0111 11", 0, 16),
    ("0000 0000 0111 10", 0, 17),
    ("0000 0000 0111 01", 0, 18),
    ("0000 0000 0111 00", 0, 19),
    ("0000 0000 0110 11", 0, 20),
    ("0000 0000 0110 10", 0, 21),
    ("0000 0000 0110 01", 0, 22),
    ("0000 0000 0110 00", 0, 23),
    ("0000 0000 0101 11", 0, 24),
    ("0000 0000 0101 10", 0, 25),
    ("0000 0000 0101 01", 0, 26),
    ("0000 0000 0101 00", 0, 27),
    ("0000 0000 0100 11", 0, 28),
    ("0000 0000 0100 10", 0, 29),
    ("0000 0000 0100 01", 0, 30),
    ("0000 0000 0100 00", 0, 31),
    ("0000 0000 0011 000", 0, 32),
    ("0000 0000 0010 111", 0, 33),
    ("0000 0000 0010 110", 0, 34),
    ("0000 0000 0010 101", 0, 35),
    ("0000 0000 0010 100", 0, 36),
    ("0000 0000 0010 011", 0, 37),
    ("0000 0000 0010 010", 0, 38),
    ("0000 0000 0010 001", 0, 39),
    ("0000 0000 0010 000", 0, 40),
    ("0000 0000 0011 111", 1, 8),
    ("0000 0000 0011 110", 1, 9),
    ("0000 0000 0011 101", 1, 10),
    ("0000 0000 0011 100", 1, 11),
    ("0000 0000 0011 011", 1, 12),
    ("0000 0000 0011 010", 1, 13),
    ("0000 0000 0011 001", 1, 14),
    ("0000 0000 0001 0011", 1, 15),
    ("0000 0000 0001 0010", 1, 16),
    ("0000 0000 0001 0001", 1, 17),
    ("0000 0000 0001 0000", 1, 18),
    ("0000 0000 0001 0100", 6, 3),
    ("0000 0000 0001 1010", 11, 2),
    ("0000 0000 0001 1001", 12, 2),
    ("0000 0000 0001 1000", 13, 2),
    ("0000 0000 0001 0111", 14, 2),
    ("0000 0000 0001 0110", 15, 2),
    ("0000 0000 0001 0101", 16, 2),
    ("0000 0000 0001 1111", 27, 1),
    ("0000 0000 0001 1110", 28, 1),
    ("0000 0000 0001 1101", 29, 1),
    ("0000 0000 0001 1100", 30, 1),
    ("0000 0000 0001 1011", 31, 1),
];

/// Coefficient scan orders (raster index of each scan position).
const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
const ALTERNATE: [u8; 64] = [
    0, 8, 16, 24, 1, 9, 2, 10, 17, 25, 32, 40, 48, 56, 57, 49, 41, 33, 26, 18, 3, 11, 4, 12, 19, 27, 34, 42, 50, 58, 35, 43, 51, 59, 20, 28, 5, 13,
    6, 14, 21, 29, 36, 44, 52, 60, 37, 45, 53, 61, 22, 30, 7, 15, 23, 31, 38, 46, 54, 62, 39, 47, 55, 63,
];

/// The default intra quantiser matrix (raster order); the default
/// non-intra matrix is all 16.
const DEFAULT_INTRA: [u8; 64] = [
    8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32,
    35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58, 26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

/// quantiser_scale for q_scale_type 1 (Table 7-6).
const NON_LINEAR_Q: [u8; 32] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 40, 44, 48, 52, 56, 64, 72, 80, 88, 96, 104, 112,
];

/// MSB-first reader; reads past the end give zero bits.
struct Bits {
    d: Vec<u8>,
    pos: usize,
}

impl Bits {
    fn peek(&self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let at = self.pos >> 3;
        let mut w = [0u8; 8];
        if at < self.d.len() {
            let take = (self.d.len() - at).min(8);
            w[..take].copy_from_slice(&self.d[at..at + take]);
        }
        ((u64::from_be_bytes(w) << (self.pos & 7)) >> (64 - n)) as u32
    }

    fn skip(&mut self, n: u32) {
        self.pos += n as usize;
    }

    fn read(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.skip(n);
        v
    }

    fn past_end(&self) -> bool {
        self.pos > self.d.len() * 8
    }

    /// Byte-align and find the next start code; returns its value with the
    /// reader after it.
    fn next_start_code(&mut self) -> Option<u8> {
        let mut at = self.pos.div_ceil(8);
        while at + 4 <= self.d.len() {
            if self.d[at + 2] > 1 {
                at += 3;
            } else if self.d[at] == 0 && self.d[at + 1] == 0 && self.d[at + 2] == 1 {
                self.pos = (at + 4) * 8;
                return Some(self.d[at + 3]);
            } else {
                at += 1;
            }
        }
        self.pos = self.d.len() * 8;
        None
    }
}

fn code_bits(code: &str) -> (u32, u32) {
    let c: String = code.chars().filter(|c| *c != ' ').collect();
    (u32::from_str_radix(&c, 2).expect("binary code"), c.len() as u32)
}

/// A lookup table indexed by the next `bits` bits; len 0 marks an invalid
/// code.
struct Vlc<T: Copy> {
    bits: u32,
    table: Vec<(T, u8)>,
}

impl<T: Copy + Default> Vlc<T> {
    fn new(codes: impl Iterator<Item = (&'static str, T)> + Clone) -> Self {
        let bits = codes.clone().map(|(c, _)| code_bits(c).1).max().unwrap_or(1);
        let mut table = vec![(T::default(), 0u8); 1 << bits];
        for (code, value) in codes {
            let (v, len) = code_bits(code);
            let shift = bits - len;
            for k in 0..(1u32 << shift) {
                let e = &mut table[((v << shift) | k) as usize];
                assert_eq!(e.1, 0, "overlapping VLC code {code}");
                *e = (value, len as u8);
            }
        }
        Self { bits, table }
    }

    fn decode(&self, b: &mut Bits, what: &str) -> R<T> {
        let (v, len) = self.table[b.peek(self.bits) as usize];
        if len == 0 {
            return err(format!("invalid {what} code at bit {}", b.pos));
        }
        b.skip(u32::from(len));
        Ok(v)
    }
}

struct Tables {
    mba: Vlc<i16>,
    mb: [Vlc<i16>; 3],
    cbp: Vlc<i16>,
    motion: Vlc<i16>,
    dc: [Vlc<i16>; 2],
    /// B.14 for the first coefficient of a non-intra block, B.14 for the
    /// others, B.15.
    dct: [Vlc<(u8, u8)>; 3],
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let plain = |t: &'static [(&'static str, i16)]| Vlc::new(t.iter().copied());
        let dct = |t: &'static [(&'static str, u8, u8)], first: bool| {
            let t = t.iter().chain(DCT_COMMON).filter(move |e| !first || e.0 != "10" && e.0 != "11");
            Vlc::new(t.map(|&(c, r, l)| (c, (r, l))).chain(first.then_some(("1", (0, 1)))))
        };
        Tables {
            mba: plain(MBA),
            mb: [plain(MB_I), plain(MB_P), plain(MB_B)],
            cbp: plain(CBP),
            motion: plain(MOTION_CODE),
            dc: [plain(DC_LUMA), plain(DC_CHROMA)],
            dct: [dct(DCT_B14, true), dct(DCT_B14, false), dct(DCT_B15, false)],
        }
    })
}

// FFmpeg's simple IDCT, 8-bit (libavcodec/simple_idct_template.c of
// FFmpeg n8.0.1: "Simple IDCT, Copyright (c) 2001 Michael Niedermayer",
// LGPL-2.1-or-later, used here under GPL-3.0-or-later as LGPL 2.1 section 3
// permits; THIRD_PARTY.md): rows with a DC-only shortcut, then columns.
const W1: i32 = 22725;
const W2: i32 = 21407;
const W3: i32 = 19266;
const W4: i32 = 16383;
const W5: i32 = 12873;
const W6: i32 = 8867;
const W7: i32 = 4520;

fn idct_row(r: &mut [i16]) {
    if r[1..8].iter().all(|&v| v == 0) {
        let v = (i32::from(r[0]) << 3) as i16;
        r[..8].fill(v);
        return;
    }
    let x: [i32; 8] = std::array::from_fn(|k| i32::from(r[k]));
    let a = W4 * x[0] + (1 << 10);
    let (mut a0, mut a1, mut a2, mut a3) = (a + W2 * x[2], a + W6 * x[2], a - W6 * x[2], a - W2 * x[2]);
    let mut b0 = W1 * x[1] + W3 * x[3];
    let mut b1 = W3 * x[1] - W7 * x[3];
    let mut b2 = W5 * x[1] - W1 * x[3];
    let mut b3 = W7 * x[1] - W5 * x[3];
    a0 += W4 * x[4] + W6 * x[6];
    a1 += -W4 * x[4] - W2 * x[6];
    a2 += -W4 * x[4] + W2 * x[6];
    a3 += W4 * x[4] - W6 * x[6];
    b0 += W5 * x[5] + W7 * x[7];
    b1 += -W1 * x[5] - W5 * x[7];
    b2 += W7 * x[5] + W3 * x[7];
    b3 += W3 * x[5] - W1 * x[7];
    r[0] = ((a0 + b0) >> 11) as i16;
    r[7] = ((a0 - b0) >> 11) as i16;
    r[1] = ((a1 + b1) >> 11) as i16;
    r[6] = ((a1 - b1) >> 11) as i16;
    r[2] = ((a2 + b2) >> 11) as i16;
    r[5] = ((a2 - b2) >> 11) as i16;
    r[3] = ((a3 + b3) >> 11) as i16;
    r[4] = ((a3 - b3) >> 11) as i16;
}

/// The column pass for column `c`: the eight output values (before the
/// pixel clip), top to bottom.
fn idct_col(b: &[i16; 64], c: usize) -> [i64; 8] {
    let x: [i64; 8] = std::array::from_fn(|k| i64::from(b[8 * k + c]));
    let (w1, w2, w3, w4, w5, w6, w7) = (W1 as i64, W2 as i64, W3 as i64, W4 as i64, W5 as i64, W6 as i64, W7 as i64);
    let a = w4 * (x[0] + ((1 << 19) / w4));
    let a0 = a + w2 * x[2] + w4 * x[4] + w6 * x[6];
    let a1 = a + w6 * x[2] - w4 * x[4] - w2 * x[6];
    let a2 = a - w6 * x[2] - w4 * x[4] + w2 * x[6];
    let a3 = a - w2 * x[2] + w4 * x[4] - w6 * x[6];
    let b0 = w1 * x[1] + w3 * x[3] + w5 * x[5] + w7 * x[7];
    let b1 = w3 * x[1] - w7 * x[3] - w1 * x[5] - w5 * x[7];
    let b2 = w5 * x[1] - w1 * x[3] + w7 * x[5] + w3 * x[7];
    let b3 = w7 * x[1] - w5 * x[3] + w3 * x[5] - w1 * x[7];
    [a0 + b0, a1 + b1, a2 + b2, a3 + b3, a3 - b3, a2 - b2, a1 - b1, a0 - b0].map(|v| v >> 20)
}

/// IDCT the block into the plane at `at` (stride `stride`): replacing the
/// pixels (intra) or adding to the prediction.
fn idct_store(block: &mut [i16; 64], plane: &mut [u8], at: usize, stride: usize, add: bool) {
    for r in block.chunks_exact_mut(8) {
        idct_row(r);
    }
    for c in 0..8 {
        for (k, v) in idct_col(block, c).into_iter().enumerate() {
            let p = &mut plane[at + k * stride + c];
            let base = if add { i64::from(*p) } else { 0 };
            *p = (base + v).clamp(0, 255) as u8;
        }
    }
}

/// Half-pel prediction of an n×n block at `src` position (x, y) in
/// half-pels into `dst` (stride n), averaged with what is there for the
/// second direction of a bidirectional prediction. Positions outside the
/// reference repeat its edge pixels.
#[allow(clippy::too_many_arguments)]
fn predict(dst: &mut [u8], n: usize, src: &[u8], sw: usize, sh: usize, hx: isize, hy: isize, avg: bool) {
    let (x0, y0, fx, fy) = (hx >> 1, hy >> 1, (hx & 1) as usize, (hy & 1) as usize);
    let inside = x0 >= 0 && y0 >= 0 && x0 as usize + n + fx <= sw && y0 as usize + n + fy <= sh;
    let px = |x: isize, y: isize| -> u32 {
        if inside {
            u32::from(src[y as usize * sw + x as usize])
        } else {
            u32::from(src[y.clamp(0, sh as isize - 1) as usize * sw + x.clamp(0, sw as isize - 1) as usize])
        }
    };
    for j in 0..n {
        let y = y0 + j as isize;
        for i in 0..n {
            let x = x0 + i as isize;
            let v = match (fx, fy) {
                (0, 0) => px(x, y),
                (1, 0) => (px(x, y) + px(x + 1, y) + 1) >> 1,
                (0, _) => (px(x, y) + px(x, y + 1) + 1) >> 1,
                _ => (px(x, y) + px(x + 1, y) + px(x, y + 1) + px(x + 1, y + 1) + 2) >> 2,
            };
            let d = &mut dst[j * n + i];
            *d = if avg { ((u32::from(*d) + v + 1) >> 1) as u8 } else { v as u8 };
        }
    }
}

/// A frame in macroblock-aligned planes.
#[derive(Clone)]
struct Frame {
    w: usize,
    h: usize,
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}

impl Frame {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, y: vec![0; w * h], cb: vec![0; w * h / 4], cr: vec![0; w * h / 4] }
    }
}

#[derive(Clone)]
struct Sequence {
    width: usize,
    height: usize,
    /// sequence_extension seen (MPEG-2) and its chroma_format.
    extension: bool,
    chroma_format: u32,
}

#[derive(Clone, Default)]
struct PictureHeader {
    /// picture_coding_type: 1 I, 2 P, 3 B.
    kind: u32,
    /// picture_coding_extension seen.
    extension: bool,
    f_code: [[u32; 2]; 2],
    dc_precision: u32,
    structure: u32,
    frame_pred_frame_dct: bool,
    concealment_mvs: bool,
    q_scale_type: bool,
    intra_vlc: bool,
    alternate_scan: bool,
}

/// Decodes an MPEG-2 video elementary stream into pictures in display
/// order (an iterator; the first error ends it).
pub struct Decoder {
    bits: Bits,
    seq: Option<Sequence>,
    intra_q: [u8; 64],
    non_intra_q: [u8; 64],
    pic: PictureHeader,
    closed_gop: bool,
    /// The picture being decoded (None between pictures or while a
    /// leading B picture of an open GOP is skipped).
    cur: Option<Frame>,
    skipping: bool,
    /// The older and the newer reference picture.
    past: Option<Frame>,
    recent: Option<Frame>,
    out: VecDeque<Picture>,
    done: bool,
    // Slice state.
    qscale: i32,
    dc_pred: [i32; 3],
    pmv: [[i32; 2]; 2],
    prev_flags: i16,
}

impl Decoder {
    pub fn new(es: Vec<u8>) -> Self {
        Self {
            bits: Bits { d: es, pos: 0 },
            seq: None,
            intra_q: DEFAULT_INTRA,
            non_intra_q: [16; 64],
            pic: PictureHeader::default(),
            closed_gop: false,
            cur: None,
            skipping: false,
            past: None,
            recent: None,
            out: VecDeque::new(),
            done: false,
            qscale: 0,
            dc_pred: [0; 3],
            pmv: [[0; 2]; 2],
            prev_flags: 0,
        }
    }

    fn picture(&self, f: &Frame) -> Picture {
        let seq = self.seq.as_ref().expect("a frame implies a sequence");
        let (w, h) = (seq.width, seq.height);
        let crop = |p: &[u8], stride: usize, w: usize, h: usize| p.chunks_exact(stride).take(h).flat_map(|r| &r[..w]).copied().collect();
        Picture { width: w, height: h, y: crop(&f.y, f.w, w, h), cb: crop(&f.cb, f.w / 2, w / 2, h / 2), cr: crop(&f.cr, f.w / 2, w / 2, h / 2) }
    }

    /// The current picture is complete: B pictures are shown now, a
    /// reference picture when the next one arrives.
    fn finish(&mut self) {
        self.skipping = false;
        let Some(f) = self.cur.take() else { return };
        if self.pic.kind == 3 {
            self.out.push_back(self.picture(&f));
        } else {
            if let Some(r) = &self.recent {
                self.out.push_back(self.picture(r));
            }
            self.past = self.recent.replace(f);
        }
    }

    fn sequence_header(&mut self) -> R<()> {
        let b = &mut self.bits;
        let width = b.read(12) as usize;
        let height = b.read(12) as usize;
        b.skip(4 + 4 + 18 + 1 + 10 + 1); // aspect, frame rate, bit rate, marker, VBV, constrained
        self.intra_q = DEFAULT_INTRA;
        self.non_intra_q = [16; 64];
        if b.read(1) == 1 {
            for &j in &ZIGZAG {
                self.intra_q[usize::from(j)] = b.read(8) as u8;
            }
        }
        if b.read(1) == 1 {
            for &j in &ZIGZAG {
                self.non_intra_q[usize::from(j)] = b.read(8) as u8;
            }
        }
        if width == 0 || height == 0 || width > 4096 || height > 2800 {
            return err(format!("picture size {width}x{height}"));
        }
        if self.seq.as_ref().is_some_and(|s| (s.width, s.height) != (width, height)) {
            return err("the picture size changes within the stream");
        }
        let extension = self.seq.as_ref().is_some_and(|s| s.extension);
        let chroma_format = self.seq.as_ref().map_or(0, |s| s.chroma_format);
        self.seq = Some(Sequence { width, height, extension, chroma_format });
        Ok(())
    }

    fn extension(&mut self) -> R<()> {
        let b = &mut self.bits;
        match b.read(4) {
            1 => {
                b.skip(8 + 1); // profile_and_level, progressive_sequence
                let chroma_format = b.read(2);
                if b.read(2) != 0 || b.read(2) != 0 {
                    return err("size extension bits");
                }
                let Some(seq) = self.seq.as_mut() else { return err("sequence extension before a sequence header") };
                seq.extension = true;
                seq.chroma_format = chroma_format;
            }
            3 => {
                // quant_matrix_extension (4:2:0 uses the luma matrices for chroma).
                if b.read(1) == 1 {
                    for &j in &ZIGZAG {
                        self.intra_q[usize::from(j)] = b.read(8) as u8;
                    }
                }
                if b.read(1) == 1 {
                    for &j in &ZIGZAG {
                        self.non_intra_q[usize::from(j)] = b.read(8) as u8;
                    }
                }
            }
            8 => {
                let p = &mut self.pic;
                for s in 0..2 {
                    for t in 0..2 {
                        p.f_code[s][t] = b.read(4);
                    }
                }
                p.dc_precision = b.read(2);
                p.structure = b.read(2);
                b.skip(1); // top_field_first
                p.frame_pred_frame_dct = b.read(1) == 1;
                p.concealment_mvs = b.read(1) == 1;
                p.q_scale_type = b.read(1) == 1;
                p.intra_vlc = b.read(1) == 1;
                p.alternate_scan = b.read(1) == 1;
                p.extension = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn picture_header(&mut self) -> R<()> {
        let b = &mut self.bits;
        b.skip(10); // temporal_reference
        let kind = b.read(3);
        b.skip(16); // vbv_delay
        if kind == 2 || kind == 3 {
            b.skip(4); // full_pel_forward_vector, forward_f_code (MPEG-1)
        }
        if kind == 3 {
            b.skip(4);
        }
        if !(1..=3).contains(&kind) {
            return err(format!("picture_coding_type {kind}"));
        }
        self.pic = PictureHeader { kind, ..Default::default() };
        Ok(())
    }

    /// The first slice of a picture: check that the stream is one this
    /// decoder covers and that its reference pictures exist.
    fn begin(&mut self) -> R<()> {
        let Some(seq) = &self.seq else { return err("a slice before a sequence header") };
        if !seq.extension {
            return err("MPEG-1 video");
        }
        if seq.chroma_format != 1 {
            return err(format!("chroma_format {} (only 4:2:0)", seq.chroma_format));
        }
        let p = &self.pic;
        if p.kind == 0 || !p.extension {
            return err("a slice without a picture header and its coding extension");
        }
        if p.structure != 3 || !p.frame_pred_frame_dct {
            return err("field pictures or field prediction");
        }
        let need = |dirs: &[usize]| dirs.iter().all(|&s| (1..=9).contains(&p.f_code[s][0]) && (1..=9).contains(&p.f_code[s][1]));
        match p.kind {
            2 if self.recent.is_none() => return err("a P picture without a reference"),
            2 if !need(&[0]) => return err("f_code"),
            3 if self.recent.is_none() || self.past.is_none() && !self.closed_gop => {
                // A leading B picture of an open GOP at the start of the
                // stream: its forward reference is not there (FFmpeg skips
                // it too).
                self.skipping = true;
                return Ok(());
            }
            3 if !need(&[0, 1]) => return err("f_code"),
            _ => {}
        }
        let (w, h) = (seq.width.div_ceil(16) * 16, seq.height.div_ceil(16) * 16);
        self.cur = Some(Frame::new(w, h));
        Ok(())
    }

    fn slice(&mut self, row: usize) -> R<()> {
        let (mb_w, mb_h) = {
            let f = self.cur.as_ref().expect("begin made the frame");
            (f.w / 16, f.h / 16)
        };
        if row >= mb_h {
            return err(format!("slice row {row}"));
        }
        let code = self.bits.read(5);
        self.set_qscale(code)?;
        if self.bits.read(1) == 1 {
            self.bits.skip(1 + 7); // intra_slice, reserved_bits
            while self.bits.read(1) == 1 {
                self.bits.skip(8);
            }
        }
        self.reset_dc();
        self.pmv = [[0; 2]; 2];
        let mut addr = row * mb_w;
        let mut first = true;
        loop {
            let mut inc = 0usize;
            loop {
                match tables().mba.decode(&mut self.bits, "macroblock_address_increment")? {
                    MBA_ESCAPE => inc += 33,
                    MBA_STUFFING => {}
                    v => {
                        inc += v as usize;
                        break;
                    }
                }
            }
            if first {
                addr += inc - 1;
                first = false;
            } else {
                for k in 1..inc {
                    self.skipped(addr + k, mb_w)?;
                }
                addr += inc;
            }
            if addr >= mb_w * mb_h {
                return err("macroblock address past the picture");
            }
            self.macroblock(addr % mb_w, addr / mb_w)?;
            if self.bits.peek(23) == 0 {
                return Ok(());
            }
            if self.bits.past_end() {
                return err("a slice past the end of the stream");
            }
        }
    }

    fn set_qscale(&mut self, code: u32) -> R<()> {
        if code == 0 {
            return err("quantiser_scale_code 0");
        }
        self.qscale = if self.pic.q_scale_type { i32::from(NON_LINEAR_Q[code as usize]) } else { 2 * code as i32 };
        Ok(())
    }

    fn reset_dc(&mut self) {
        self.dc_pred = [128 << self.pic.dc_precision; 3];
    }

    /// A skipped macroblock: P pictures copy the forward reference, B
    /// pictures repeat the previous macroblock's prediction.
    fn skipped(&mut self, addr: usize, mb_w: usize) -> R<()> {
        self.reset_dc();
        let flags = match self.pic.kind {
            2 => {
                self.pmv[0] = [0, 0];
                FWD
            }
            3 if self.prev_flags & INTRA == 0 => self.prev_flags & (FWD | BWD),
            _ => return err("a skipped macroblock in an I picture or after an intra one"),
        };
        let mvs = self.pmv;
        self.predict_mb(addr % mb_w, addr / mb_w, flags, mvs)
    }

    fn motion_vector(&mut self, s: usize) -> R<()> {
        for t in 0..2 {
            let f_code = self.pic.f_code[s][t];
            if !(1..=9).contains(&f_code) {
                return err(format!("f_code {f_code}"));
            }
            let r_size = f_code - 1;
            let code = i32::from(tables().motion.decode(&mut self.bits, "motion_code")?);
            let mut delta = code;
            if code != 0 {
                let negative = self.bits.read(1) == 1;
                if r_size > 0 {
                    delta = ((code - 1) << r_size) + self.bits.read(r_size) as i32 + 1;
                }
                if negative {
                    delta = -delta;
                }
            }
            let f = 1 << r_size;
            let mut v = self.pmv[s][t] + delta;
            if v < -16 * f {
                v += 32 * f;
            }
            if v > 16 * f - 1 {
                v -= 32 * f;
            }
            self.pmv[s][t] = v;
        }
        Ok(())
    }

    fn macroblock(&mut self, mb_x: usize, mb_y: usize) -> R<()> {
        let kind = self.pic.kind as usize;
        let mut flags = tables().mb[kind - 1].decode(&mut self.bits, "macroblock_type")?;
        if flags & QUANT != 0 {
            let code = self.bits.read(5);
            self.set_qscale(code)?;
        }
        let intra = flags & INTRA != 0;
        let concealment = intra && self.pic.concealment_mvs;
        if flags & FWD != 0 || concealment {
            self.motion_vector(0)?;
        }
        if flags & BWD != 0 {
            self.motion_vector(1)?;
        }
        if concealment {
            self.bits.skip(1); // marker_bit
        }
        let cbp = if flags & PATTERN != 0 {
            tables().cbp.decode(&mut self.bits, "coded_block_pattern")?
        } else if intra {
            63
        } else {
            0
        };
        if intra {
            if !concealment {
                self.pmv = [[0; 2]; 2];
            }
        } else {
            self.reset_dc();
            if kind == 2 && flags & FWD == 0 {
                // P "no motion compensation": the zero vector forward.
                self.pmv[0] = [0, 0];
                flags |= FWD;
            }
            let mvs = self.pmv;
            self.predict_mb(mb_x, mb_y, flags, mvs)?;
        }
        self.prev_flags = flags;
        let mut block = [0i16; 64];
        for i in 0..6usize {
            if cbp & (32 >> i) == 0 {
                continue;
            }
            let comp = i.saturating_sub(3);
            self.block(intra, comp, &mut block)?;
            let f = self.cur.as_mut().expect("begin made the frame");
            let (plane, stride, at) = match i {
                0..=3 => (&mut f.y, f.w, (mb_y * 16 + (i / 2) * 8) * f.w + mb_x * 16 + (i % 2) * 8),
                4 => (&mut f.cb, f.w / 2, mb_y * 8 * (f.w / 2) + mb_x * 8),
                _ => (&mut f.cr, f.w / 2, mb_y * 8 * (f.w / 2) + mb_x * 8),
            };
            idct_store(&mut block, plane, at, stride, !intra);
        }
        Ok(())
    }

    /// Motion-compensated prediction of the macroblock from the forward
    /// and/or backward reference, written into the current frame.
    fn predict_mb(&mut self, mb_x: usize, mb_y: usize, flags: i16, mvs: [[i32; 2]; 2]) -> R<()> {
        let (fwd_ref, bwd_ref) = if self.pic.kind == 2 { (self.recent.as_ref(), None) } else { (self.past.as_ref(), self.recent.as_ref()) };
        let mut y = [0u8; 256];
        let mut cb = [0u8; 64];
        let mut cr = [0u8; 64];
        let mut any = false;
        for (s, flag, reference) in [(0, FWD, fwd_ref), (1, BWD, bwd_ref)] {
            if flags & flag == 0 {
                continue;
            }
            let Some(r) = reference else { return err("prediction from a missing reference picture") };
            let [vx, vy] = mvs[s];
            let (lx, ly) = ((mb_x * 32) as isize + vx as isize, (mb_y * 32) as isize + vy as isize);
            predict(&mut y, 16, &r.y, r.w, r.h, lx, ly, any);
            // Chroma vectors: half the luma vector, rounded toward zero.
            let (cx, cy) = ((mb_x * 16) as isize + (vx / 2) as isize, (mb_y * 16) as isize + (vy / 2) as isize);
            predict(&mut cb, 8, &r.cb, r.w / 2, r.h / 2, cx, cy, any);
            predict(&mut cr, 8, &r.cr, r.w / 2, r.h / 2, cx, cy, any);
            any = true;
        }
        if !any {
            return err("a non-intra macroblock without prediction");
        }
        let f = self.cur.as_mut().expect("begin made the frame");
        let cw = f.w / 2;
        for j in 0..16 {
            let at = (mb_y * 16 + j) * f.w + mb_x * 16;
            f.y[at..at + 16].copy_from_slice(&y[j * 16..j * 16 + 16]);
        }
        for j in 0..8 {
            let at = (mb_y * 8 + j) * cw + mb_x * 8;
            f.cb[at..at + 8].copy_from_slice(&cb[j * 8..j * 8 + 8]);
            f.cr[at..at + 8].copy_from_slice(&cr[j * 8..j * 8 + 8]);
        }
        Ok(())
    }

    /// One block's coefficients, dequantised with saturation and mismatch
    /// control, in raster order.
    fn block(&mut self, intra: bool, comp: usize, out: &mut [i16; 64]) -> R<()> {
        *out = [0; 64];
        let t = tables();
        let scan = if self.pic.alternate_scan { &ALTERNATE } else { &ZIGZAG };
        let b = &mut self.bits;
        let mut sum: i32;
        let mut next: usize;
        let mut table;
        let matrix;
        if intra {
            let size = t.dc[usize::from(comp > 0)].decode(b, "dct_dc_size")? as u32;
            let diff = if size == 0 {
                0
            } else {
                let v = b.read(size) as i32;
                if v < 1 << (size - 1) { v + 1 - (1 << size) } else { v }
            };
            self.dc_pred[comp] = self.dc_pred[comp].wrapping_add(diff);
            let dc = self.dc_pred[comp] << (3 - self.pic.dc_precision);
            out[0] = dc as i16;
            sum = dc;
            next = 1;
            table = if self.pic.intra_vlc { &t.dct[2] } else { &t.dct[1] };
            matrix = &self.intra_q;
        } else {
            sum = 0;
            next = 0;
            table = &t.dct[0];
            matrix = &self.non_intra_q;
        }
        loop {
            let (run, level) = table.decode(b, "DCT coefficient")?;
            table = if intra && self.pic.intra_vlc { &t.dct[2] } else { &t.dct[1] };
            let (run, level) = match run {
                EOB => break,
                ESCAPE => {
                    let run = b.read(6) as usize;
                    let l = b.read(12) as i32;
                    let l = if l >= 2048 { l - 4096 } else { l };
                    if l == 0 || l == -2048 {
                        return err("escape level");
                    }
                    (run, l)
                }
                _ => {
                    let l = i32::from(level);
                    (usize::from(run), if b.read(1) == 1 { -l } else { l })
                }
            };
            next += run;
            if next > 63 {
                return err("more than 64 coefficients");
            }
            let j = usize::from(scan[next]);
            let w = i32::from(matrix[j]);
            let v = if intra { level * w * self.qscale / 16 } else { (2 * level + level.signum()) * w * self.qscale / 32 };
            let v = v.clamp(-2048, 2047);
            out[j] = v as i16;
            sum += v;
            next += 1;
        }
        if sum & 1 == 0 {
            out[63] ^= 1;
        }
        Ok(())
    }

    fn step(&mut self) -> R<()> {
        let Some(code) = self.bits.next_start_code() else {
            self.finish();
            if let Some(r) = self.recent.take() {
                let p = self.picture(&r);
                self.out.push_back(p);
            }
            self.done = true;
            return Ok(());
        };
        match code {
            0x00 => {
                self.finish();
                self.picture_header()
            }
            0x01..=0xaf => {
                if self.cur.is_none() && !self.skipping {
                    self.begin()?;
                }
                if self.skipping {
                    return Ok(());
                }
                self.slice(usize::from(code) - 1)
            }
            0xb3 => {
                self.finish();
                self.sequence_header()
            }
            0xb5 => self.extension(),
            0xb7 => {
                self.finish();
                Ok(())
            }
            0xb8 => {
                self.finish();
                self.bits.skip(25); // time_code
                self.closed_gop = self.bits.read(1) == 1;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Iterator for Decoder {
    type Item = Result<Picture, Mpeg2Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(p) = self.out.pop_front() {
                return Some(Ok(p));
            }
            if self.done {
                return None;
            }
            if let Err(e) = self.step() {
                self.done = true;
                self.out.clear();
                return Some(Err(e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MSB-first writer for synthetic streams.
    #[derive(Default)]
    struct W {
        bytes: Vec<u8>,
        bits: u32,
    }

    impl W {
        fn put(&mut self, n: u32, v: u32) {
            for k in (0..n).rev() {
                if self.bits.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (v >> k) & 1 == 1 {
                    *self.bytes.last_mut().unwrap() |= 0x80 >> (self.bits % 8);
                }
                self.bits += 1;
            }
        }

        fn code(&mut self, c: &str) {
            let (v, n) = code_bits(c);
            self.put(n, v);
        }

        fn start(&mut self, code: u8) {
            while !self.bits.is_multiple_of(8) {
                self.put(1, 0);
            }
            self.put(32, 0x100 | u32::from(code));
        }

        fn sequence(&mut self, w: u32, h: u32) {
            self.start(0xb3);
            self.put(12, w);
            self.put(12, h);
            self.put(4, 2);
            self.put(4, 5);
            self.put(18, 0x3ffff);
            self.put(1, 1);
            self.put(10, 112);
            self.put(1, 0);
            self.put(2, 0); // default matrices
            self.start(0xb5);
            self.put(4, 1);
            self.put(8, 0x48);
            self.put(1, 1);
            self.put(2, 1); // 4:2:0
            self.put(2 + 2 + 12, 0);
            self.put(1, 1);
            self.put(8 + 1 + 2 + 5, 0);
        }

        fn picture(&mut self, kind: u32, f_code: u32, structure: u32) {
            self.start(0x00);
            self.put(10, 0);
            self.put(3, kind);
            self.put(16, 0xffff);
            if kind >= 2 {
                self.put(4, 0b0111);
            }
            if kind == 3 {
                self.put(4, 0b0111);
            }
            self.put(1, 0);
            self.start(0xb5);
            self.put(4, 8);
            for _ in 0..4 {
                self.put(4, f_code);
            }
            self.put(2, 0); // intra_dc_precision
            self.put(2, structure);
            self.put(1, 1);
            self.put(1, 1); // frame_pred_frame_dct
            self.put(1, 0);
            self.put(1, 0);
            self.put(1, 1); // intra_vlc_format
            self.put(1, 0);
            self.put(1, 0);
            self.put(1, 0);
            self.put(1, 1); // progressive_frame
            self.put(1, 0);
        }

        /// A slice of DC-only intra macroblocks with the given luma DC
        /// differences (chroma differences 0).
        fn intra_slice(&mut self, row: u8, dcs: &[i32]) {
            self.start(row);
            self.put(5, 8);
            self.put(1, 0);
            for &dc in dcs {
                self.code("1"); // address increment 1
                self.code("1"); // intra
                for blk in 0..6 {
                    let d = if blk == 0 { dc } else { 0 };
                    if d == 0 {
                        self.code(if blk < 4 { "100" } else { "00" });
                    } else {
                        // Size 4 (luma "110"), |d| in 8..15.
                        assert!((8..16).contains(&d.abs()));
                        self.code("110");
                        self.put(4, if d > 0 { d as u32 } else { (d + 15) as u32 });
                    }
                    self.code("0110"); // EOB (B.15)
                }
            }
        }
    }

    #[test]
    fn tables_are_prefix_free_and_complete_where_the_standard_is() {
        let t = tables();
        // Kraft sums: the address increment, CBP and DCT tables leave only
        // the standard's forbidden codes unused.
        let kraft = |v: &Vlc<i16>| v.table.iter().filter(|e| e.1 > 0).count() as f64 / v.table.len() as f64;
        assert_eq!(kraft(&t.mb[0]), 0.75);
        assert_eq!(kraft(&t.mb[2]), 63.0 / 64.0);
        assert_eq!(kraft(&t.cbp), 511.0 / 512.0);
        assert_eq!(kraft(&t.dc[0]), 1.0);
        assert_eq!(kraft(&t.dc[1]), 1.0);
        let dct_used = |v: &Vlc<(u8, u8)>| v.table.iter().filter(|e| e.1 > 0).count();
        // Everything but the 16 codes 0000 0000 0000 xxxx.
        assert_eq!(dct_used(&t.dct[0]), 65536 - 16);
        assert_eq!(dct_used(&t.dct[1]), 65536 - 16);
        // B.15 leaves B.14's long codes for its shortened pairs unused.
        assert!(dct_used(&t.dct[2]) < 65536 - 16);
        assert_eq!(DCT_B14.len() + DCT_COMMON.len(), 111 + 2);
        assert_eq!(DCT_B15.len(), DCT_B14.len());
    }

    #[test]
    fn idct_of_a_dc_block_is_flat() {
        let mut b = [0i16; 64];
        b[0] = 1024;
        let mut plane = [0u8; 64];
        idct_store(&mut b, &mut plane, 0, 8, false);
        assert!(plane.iter().all(|&p| p == 128));
    }

    #[test]
    fn intra_then_skipped_p_and_b_pictures() {
        let mut w = W::default();
        w.sequence(48, 16);
        w.start(0xb8);
        w.put(25, 0);
        w.put(1, 1); // closed GOP
        w.put(1, 0);
        w.picture(1, 15, 3);
        w.intra_slice(1, &[8, -9, 8]);
        // P: macroblocks 0 and 2 "MC, not coded" with a zero vector,
        // macroblock 1 skipped.
        w.picture(2, 1, 3);
        w.start(1);
        w.put(5, 8);
        w.put(1, 0);
        w.code("1");
        w.code("001");
        w.code("1");
        w.code("1");
        w.code("011"); // increment 2: one skipped macroblock
        w.code("001");
        w.code("1");
        w.code("1");
        // B: macroblocks 0 and 2 backward only with a zero vector,
        // macroblock 1 skipped (repeats that).
        w.picture(3, 1, 3);
        w.start(1);
        w.put(5, 8);
        w.put(1, 0);
        w.code("1");
        w.code("010");
        w.code("1");
        w.code("1");
        w.code("011");
        w.code("010");
        w.code("1");
        w.code("1");
        w.start(0xb7);
        let pics: Vec<_> = Decoder::new(w.bytes).collect::<Result<_, _>>().unwrap();
        // Display order: I, B, P.
        assert_eq!(pics.len(), 3);
        let i = &pics[0];
        assert_eq!((i.width, i.height, i.y.len(), i.cb.len()), (48, 16, 768, 192));
        // The luma DC predictor runs 128 + 8, - 9, + 8 over the macroblocks
        // (the mismatch-control coefficient moves pixels by at most one).
        for (x, dc) in [(0, 136), (8, 136), (24, 127), (40, 135)] {
            assert!(i.y[x].abs_diff(dc) <= 1, "x {x}: {}", i.y[x]);
        }
        assert!(i.cb[0].abs_diff(128) <= 1 && i.cr[23].abs_diff(128) <= 1);
        assert_eq!(pics[1], pics[0]);
        assert_eq!(pics[2], pics[0]);
    }

    #[test]
    fn field_pictures_are_refused() {
        let mut w = W::default();
        w.sequence(16, 16);
        w.picture(1, 15, 1);
        w.intra_slice(1, &[8]);
        let first = Decoder::new(w.bytes).next().unwrap();
        assert!(first.unwrap_err().0.contains("field"));
    }

    #[test]
    fn a_leading_b_picture_of_an_open_gop_is_skipped() {
        let mut w = W::default();
        w.sequence(16, 16);
        w.start(0xb8);
        w.put(25, 0);
        w.put(2, 0); // open GOP
        w.picture(1, 15, 3);
        w.intra_slice(1, &[8]);
        w.picture(3, 1, 3);
        w.start(1);
        w.put(5, 8);
        w.put(1, 0);
        w.code("1");
        w.code("10"); // interpolated: needs the missing forward picture
        w.code("1");
        w.code("1");
        w.code("1");
        w.code("1");
        let pics: Vec<_> = Decoder::new(w.bytes).collect::<Result<_, _>>().unwrap();
        assert_eq!(pics.len(), 1);
    }
}
