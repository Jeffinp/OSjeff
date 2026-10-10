//! Huffman tables and the entropy-coded bit reader.

use super::JpegError;
use alloc::vec::Vec;

/// A JPEG Huffman table (canonical code, up to 16 bits), with a 9-bit lookup for the common
/// short codes.
pub(super) struct Huff {
    /// `fast[next 9 bits] = (length << 8) | symbol`, 0 when the code is longer than 9 bits.
    fast: [u16; 512],
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
}

impl Huff {
    /// `counts[i]` codes of length `i + 1`, then the symbols in code order.
    pub(super) fn new(counts: &[u8; 16], vals: &[u8]) -> Result<Huff, JpegError> {
        let total: usize = counts.iter().map(|&c| c as usize).sum();
        if total == 0 || total > 256 || vals.len() != total {
            return Err(JpegError::BadHuffman);
        }
        let mut h = Huff {
            fast: [0; 512],
            maxcode: [-1; 18],
            valptr: [0; 17],
            mincode: [0; 17],
            vals: vals.to_vec(),
        };
        let mut code = 0i32;
        let mut k = 0i32;
        for len in 1..=16usize {
            let n = counts[len - 1] as i32;
            h.valptr[len] = k;
            h.mincode[len] = code;
            if n > 0 {
                code += n;
                // More codes than fit in `len` bits: the lengths are impossible.
                if code > (1 << len) {
                    return Err(JpegError::BadHuffman);
                }
                h.maxcode[len] = code - 1;
            }
            if len <= 9 {
                for i in 0..n {
                    let c = (h.mincode[len] + i) as usize;
                    let sym = h.vals[(k + i) as usize] as u16;
                    let base = c << (9 - len);
                    for f in 0..(1usize << (9 - len)) {
                        h.fast[base + f] = ((len as u16) << 8) | sym;
                    }
                }
            }
            k += n;
            code <<= 1;
        }
        h.maxcode[17] = i32::MAX;
        Ok(h)
    }
}

/// Reads bits from entropy-coded data: undoes `FF 00` byte stuffing and stops at a marker.
pub(super) struct Bits<'a> {
    data: &'a [u8],
    pub(super) pos: usize,
    acc: u32,
    n: u32,
    /// A marker (`FF xx`, `xx != 0`) was reached; no more data bytes come after it.
    pub(super) marker: Option<u8>,
    /// Zero bits supplied after the data ran out, still unconsumed in `acc`.
    fake: u32,
    /// Bits were consumed that were not in the file: the entropy data is exhausted.
    pub(super) overrun: bool,
}

impl<'a> Bits<'a> {
    pub(super) fn new(data: &'a [u8], pos: usize) -> Bits<'a> {
        Bits {
            data,
            pos,
            acc: 0,
            n: 0,
            marker: None,
            fake: 0,
            overrun: false,
        }
    }

    fn fill(&mut self) {
        while self.n <= 24 {
            let mut real = true;
            let b = if self.marker.is_some() {
                real = false;
                0
            } else {
                match self.data.get(self.pos) {
                    None => {
                        real = false;
                        0
                    }
                    Some(&0xFF) => match self.data.get(self.pos + 1) {
                        Some(&0) => {
                            self.pos += 2;
                            0xFF
                        }
                        Some(&m) => {
                            // Leave `pos` on the 0xFF so the caller can read the marker.
                            self.marker = Some(m);
                            real = false;
                            0
                        }
                        None => {
                            real = false;
                            0
                        }
                    },
                    Some(&b) => {
                        self.pos += 1;
                        b
                    }
                }
            };
            self.acc |= (b as u32) << (24 - self.n);
            self.n += 8;
            if !real {
                self.fake += 8;
            }
        }
    }

    /// Account for `k` consumed bits: if they dipped into the zero padding, the data is exhausted.
    fn used(&mut self, k: u32) {
        self.n -= k;
        if self.n < self.fake {
            self.overrun = true;
            self.fake = self.n;
        }
    }

    pub(super) fn bits(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        if self.n < k {
            self.fill();
        }
        let v = self.acc >> (32 - k);
        self.acc <<= k;
        self.used(k);
        v
    }

    /// Decode one Huffman symbol.
    pub(super) fn symbol(&mut self, h: &Huff) -> Result<u8, JpegError> {
        if self.n < 16 {
            self.fill();
        }
        let e = h.fast[(self.acc >> 23) as usize];
        if e != 0 {
            let len = (e >> 8) as u32;
            self.acc <<= len;
            self.used(len);
            return Ok(e as u8);
        }
        // Longer than 9 bits: canonical decode, one bit at a time.
        let mut code = (self.acc >> 16) as i32;
        let mut len = 10usize;
        while len <= 16 {
            let c = code >> (16 - len);
            if h.maxcode[len] >= 0 && c <= h.maxcode[len] && c >= h.mincode[len] {
                let idx = (h.valptr[len] + c - h.mincode[len]) as usize;
                self.acc <<= len;
                self.used(len as u32);
                return h.vals.get(idx).copied().ok_or(JpegError::BadHuffman);
            }
            len += 1;
        }
        let _ = &mut code;
        Err(JpegError::BadHuffman)
    }

    /// Between restart intervals: drop the padding bits, find the next marker and, if it is a
    /// restart marker (`RST0..RST7`), step over it. Any other marker, or the end of the data,
    /// is an error (the caller then keeps what it has).
    pub(super) fn restart(&mut self) -> Result<(), JpegError> {
        self.acc = 0;
        self.n = 0;
        self.fake = 0;
        self.overrun = false;
        self.marker = None;
        let mut p = self.pos;
        while p + 1 < self.data.len() {
            if self.data[p] == 0xFF && self.data[p + 1] != 0 && self.data[p + 1] != 0xFF {
                let m = self.data[p + 1];
                if (0xD0..=0xD7).contains(&m) {
                    self.pos = p + 2;
                    return Ok(());
                }
                self.pos = p;
                return Err(JpegError::BadScan);
            }
            p += 1;
        }
        self.pos = self.data.len();
        Err(JpegError::Truncated)
    }
}

/// The sign-extension step of the JPEG spec (F.2.2.1): `v` is a `t`-bit value.
pub(super) fn extend(v: u32, t: u32) -> i32 {
    if t == 0 {
        return 0;
    }
    if v < (1 << (t - 1)) {
        v as i32 - (1 << t) + 1
    } else {
        v as i32
    }
}
