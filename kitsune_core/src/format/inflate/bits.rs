//! bits (split out of `inflate.rs`).

use super::*;

pub(super) struct Bits<'a> {
    pub(super) data: &'a [u8],
    /// Next byte to load into `buf`.
    pub(super) pos: usize,
    /// Bit buffer, LSB first. Bits at and above `cnt` are either zero or the
    /// real upcoming input bits (never anything else), so refilling by OR is
    /// idempotent.
    pub(super) buf: u64,
    pub(super) cnt: u32,
}

impl<'a> Bits<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        }
    }

    /// Tops the buffer up to at least 56 valid bits, or to the end of input.
    #[inline(always)]
    pub(super) fn refill(&mut self) {
        if self.cnt >= 56 {
            return;
        }
        if let Some(c) = self.data.get(self.pos..).and_then(|s| s.first_chunk::<8>()) {
            self.buf |= u64::from_le_bytes(*c) << self.cnt;
            self.pos += ((63 - self.cnt) >> 3) as usize;
            self.cnt |= 56;
        } else {
            while self.cnt <= 56 {
                let Some(&b) = self.data.get(self.pos) else {
                    break;
                };
                self.buf |= (b as u64) << self.cnt;
                self.pos += 1;
                self.cnt += 8;
            }
        }
    }

    /// Reads `n` (<= 32) bits.
    #[inline(always)]
    pub(super) fn bits(&mut self, n: u32) -> Result<u32, InflateError> {
        if self.cnt < n {
            self.refill();
            if self.cnt < n {
                return Err(InflateError::Truncated);
            }
        }
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.cnt -= n;
        Ok(v)
    }

    /// Drops the bits up to the next byte boundary, then gives the whole
    /// bytes still in the buffer back to the input slice.
    pub(super) fn align_and_unread(&mut self) {
        let drop = self.cnt & 7;
        self.cnt -= drop;
        self.pos -= (self.cnt >> 3) as usize;
        self.buf = 0;
        self.cnt = 0;
    }

    /// Bytes of input fully or partially consumed.
    pub(super) fn consumed(&self) -> usize {
        self.pos - (self.cnt >> 3) as usize
    }
}
