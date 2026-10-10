//! huffman (split out of `inflate.rs`).

use super::*;

pub(super) const FAST_BITS: u32 = 10;

pub(super) const FAST_SIZE: usize = 1 << FAST_BITS;

pub(super) const FAST_MASK: u64 = (FAST_SIZE as u64) - 1;

pub(super) const MAX_CODE_LEN: usize = 15;

pub(super) const MAX_SYMBOLS: usize = 288;

pub(super) struct Huffman {
    /// `symbol << 4 | length` for codes of at most `FAST_BITS` bits, indexed
    /// by the next `FAST_BITS` input bits; 0 = not a short code.
    pub(super) fast: [u16; FAST_SIZE],
    /// Number of codes of each length.
    pub(super) count: [u16; MAX_CODE_LEN + 1],
    /// Symbols ordered by (length, symbol): the canonical order.
    pub(super) symbols: [u16; MAX_SYMBOLS],
}

impl Huffman {
    pub(super) fn new() -> Self {
        Self {
            fast: [0; FAST_SIZE],
            count: [0; MAX_CODE_LEN + 1],
            symbols: [0; MAX_SYMBOLS],
        }
    }

    /// Builds the canonical code for `lens`. `complete` demands a complete
    /// code (used for the code-length code); otherwise the only incomplete
    /// codes accepted are "no codes at all" and "one code of length 1", the
    /// two cases RFC 1951 and zlib allow for distance (and literal) trees.
    pub(super) fn build(&mut self, lens: &[u8], complete: bool) -> Result<(), InflateError> {
        if lens.len() > MAX_SYMBOLS {
            return Err(InflateError::BadCodeLengths);
        }
        self.count = [0; MAX_CODE_LEN + 1];
        for &l in lens {
            let l = l as usize;
            if l > MAX_CODE_LEN {
                return Err(InflateError::BadCodeLengths);
            }
            self.count[l] += 1;
        }
        let total = lens.len() as u32 - self.count[0] as u32;
        // Kraft inequality.
        let mut left: i32 = 1;
        for &c in &self.count[1..] {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return Err(InflateError::BadCodeLengths);
            }
        }
        if left > 0 {
            let single = total == 1 && self.count[1] == 1;
            if complete || !(total == 0 || single) {
                return Err(InflateError::BadCodeLengths);
            }
        }
        // Offsets of each length in `symbols`, and the first code of each.
        let mut offs = [0u16; MAX_CODE_LEN + 2];
        let mut next_code = [0u32; MAX_CODE_LEN + 2];
        let mut code = 0u32;
        for len in 1..=MAX_CODE_LEN {
            offs[len + 1] = offs[len] + self.count[len];
            code = (code + self.count[len - 1] as u32 * (len > 1) as u32) << 1;
            next_code[len] = code;
        }
        self.fast = [0; FAST_SIZE];
        for (sym, &l) in lens.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let l = l as usize;
            let slot = offs[l] as usize;
            offs[l] += 1;
            if let Some(s) = self.symbols.get_mut(slot) {
                *s = sym as u16;
            }
            let c = next_code[l];
            next_code[l] += 1;
            if l as u32 <= FAST_BITS {
                let rev = (c.reverse_bits() >> (32 - l)) as usize;
                let entry = ((sym as u16) << 4) | l as u16;
                let mut i = rev;
                while i < FAST_SIZE {
                    self.fast[i] = entry;
                    i += 1 << l;
                }
            }
        }
        Ok(())
    }

    /// Decodes one symbol.
    #[inline(always)]
    pub(super) fn decode(&self, br: &mut Bits<'_>) -> Result<u16, InflateError> {
        br.refill();
        let e = self.fast[(br.buf & FAST_MASK) as usize];
        if e != 0 {
            let l = (e & 15) as u32;
            if l > br.cnt {
                return Err(InflateError::Truncated);
            }
            br.buf >>= l;
            br.cnt -= l;
            return Ok(e >> 4);
        }
        self.decode_slow(br)
    }

    #[cold]
    pub(super) fn decode_slow(&self, br: &mut Bits<'_>) -> Result<u16, InflateError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAX_CODE_LEN {
            let bit = br.bits(1)? as i32;
            code |= bit;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self
                    .symbols
                    .get((index + (code - first)) as usize)
                    .copied()
                    .unwrap_or(0));
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(InflateError::InvalidCode)
    }
}

pub(crate) const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

pub(crate) const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

pub(crate) const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

pub(crate) const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

pub(super) const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

pub(super) const WINDOW: usize = 32768;

pub(super) const WMASK: usize = WINDOW - 1;
