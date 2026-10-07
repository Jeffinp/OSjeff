//! A small DEFLATE / zlib *encoder* for saving screenshots.
//!
//! It is deliberately simple and bounded:
//!
//! * [`deflate_stored`]: no compression, blocks of up to 65535 bytes.
//! * [`deflate_fixed`]: greedy LZ77 (3-byte hash, chains of at most
//!   [`MAX_CHAIN`] candidates, 32 KiB window) coded with the fixed Huffman
//!   tables of RFC 1951. Large flat areas (screenshots, icons) shrink a lot;
//!   if the result would not be smaller than stored blocks, stored blocks are
//!   emitted instead, so the output is never more than ~0.01% larger than the
//!   input.
//!
//! Memory is `O(window)` (two 32K-entry tables) plus the output. All output is
//! verified by the decoder in [`crate::inflate`] in the tests.

use crate::inflate::{Adler32, LEN_BASE, LEN_EXTRA};
use alloc::vec;
use alloc::vec::Vec;

/// Longest hash chain searched per position (speed/ratio trade-off).
pub const MAX_CHAIN: usize = 24;

const WINDOW: usize = 32768;
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;

/// LSB-first bit writer.
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn new(capacity: usize) -> Self {
        Self {
            out: Vec::with_capacity(capacity),
            acc: 0,
            n: 0,
        }
    }

    #[inline]
    fn put(&mut self, value: u32, bits: u32) {
        self.acc |= (value as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

const fn rev(code: u32, len: u32) -> u32 {
    code.reverse_bits() >> (32 - len)
}

/// `(reversed code, length)` for each literal/length symbol of the fixed code.
const fn fixed_lit_table() -> [(u16, u8); 288] {
    let mut t = [(0u16, 0u8); 288];
    let mut s = 0;
    while s < 288 {
        let (code, len) = if s < 144 {
            (0x30 + s as u32, 8)
        } else if s < 256 {
            (0x190 + (s as u32 - 144), 9)
        } else if s < 280 {
            (s as u32 - 256, 7)
        } else {
            (0xC0 + (s as u32 - 280), 8)
        };
        t[s] = (rev(code, len) as u16, len as u8);
        s += 1;
    }
    t
}

const fn len_symbol_table() -> [u8; MAX_MATCH + 1] {
    let mut t = [0u8; MAX_MATCH + 1];
    let mut l = MIN_MATCH;
    while l <= MAX_MATCH {
        let mut s = 28;
        while LEN_BASE[s] as usize > l {
            s -= 1;
        }
        t[l] = s as u8;
        l += 1;
    }
    t
}

static FIXED_LIT: [(u16, u8); 288] = fixed_lit_table();
static LEN_SYM: [u8; MAX_MATCH + 1] = len_symbol_table();

/// Distance symbol, extra bit count and extra value for `1 <= dist <= 32768`.
#[inline]
fn dist_code(dist: usize) -> (u32, u32, u32) {
    let d = (dist - 1) as u32;
    if d < 4 {
        return (d, 0, 0);
    }
    let nb = 32 - d.leading_zeros(); // bits in d
    let extra = nb - 2;
    let sym = 2 * (nb - 1) + ((d >> extra) & 1);
    (sym, extra, d & ((1 << extra) - 1))
}

#[inline]
fn put_lit(w: &mut BitWriter, sym: usize) {
    let (code, len) = FIXED_LIT[sym];
    w.put(code as u32, len as u32);
}

/// Compresses `data` as raw DEFLATE using stored blocks only.
pub fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 6);
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
        return out;
    }
    let mut chunks = data.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        out.push(chunks.peek().is_none() as u8);
        out.extend_from_slice(&(c.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        out.extend_from_slice(c);
    }
    out
}

#[inline]
fn hash3(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) | ((d[i + 1] as u32) << 8) | ((d[i + 2] as u32) << 16);
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Compresses `data` as raw DEFLATE: LZ77 + the fixed Huffman code, falling
/// back to stored blocks when that is smaller.
pub fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    if data.len() < MIN_MATCH || data.len() >= u32::MAX as usize - 1 {
        return deflate_stored(data);
    }
    let mut w = BitWriter::new(data.len() / 2 + 16);
    w.put(1, 1); // final block
    w.put(1, 2); // fixed Huffman
    // head[h] = most recent position + 1 with that hash (0 = none);
    // prev[p % WINDOW] = previous position + 1 in the same chain.
    let mut head = vec![0u32; HASH_SIZE];
    let mut prev = vec![0u32; WINDOW];
    let n = data.len();
    let mut i = 0usize;
    let insert = |head: &mut [u32], prev: &mut [u32], p: usize| {
        if p + MIN_MATCH <= n {
            let h = hash3(data, p);
            prev[p & (WINDOW - 1)] = head[h];
            head[h] = p as u32 + 1;
        }
    };
    while i < n {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if i + MIN_MATCH <= n {
            let h = hash3(data, i);
            let mut cand = head[h] as usize;
            let max_len = MAX_MATCH.min(n - i);
            let mut chain = MAX_CHAIN;
            while cand != 0 && chain > 0 {
                let c = cand - 1;
                if c >= i || i - c > WINDOW {
                    break;
                }
                // Quick reject on the byte that would extend the best match.
                if best_len == 0
                    || data[c + best_len.min(max_len - 1)] == data[i + best_len.min(max_len - 1)]
                {
                    let mut l = 0;
                    while l < max_len && data[c + l] == data[i + l] {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        best_dist = i - c;
                        if l == max_len {
                            break;
                        }
                    }
                }
                let next = prev[c & (WINDOW - 1)] as usize;
                if next != 0 && next > c {
                    break; // the ring entry was overwritten: chain is stale
                }
                cand = next;
                chain -= 1;
            }
        }
        if best_len >= MIN_MATCH {
            let ls = LEN_SYM[best_len] as usize;
            put_lit(&mut w, 257 + ls);
            w.put(
                (best_len - LEN_BASE[ls] as usize) as u32,
                LEN_EXTRA[ls] as u32,
            );
            let (ds, eb, ev) = dist_code(best_dist);
            w.put(rev(ds, 5), 5);
            w.put(ev, eb);
            for p in i..i + best_len {
                insert(&mut head, &mut prev, p);
            }
            i += best_len;
        } else {
            put_lit(&mut w, data[i] as usize);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    put_lit(&mut w, 256);
    let packed = w.finish();
    let stored_size = n + n.div_ceil(65535) * 5;
    if packed.len() >= stored_size {
        return deflate_stored(data);
    }
    packed
}

fn zlib_wrap(raw: Vec<u8>, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 6);
    out.extend_from_slice(&[0x78, 0x01]); // 32K window, fastest; (0x78*256+1) % 31 == 0
    out.extend_from_slice(&raw);
    let mut a = Adler32::new();
    a.update(data);
    out.extend_from_slice(&a.finish().to_be_bytes());
    out
}

/// zlib stream using stored blocks.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    zlib_wrap(deflate_stored(data), data)
}

/// zlib stream using LZ77 + fixed Huffman (see [`deflate_fixed`]).
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    zlib_wrap(deflate_fixed(data), data)
}

#[cfg(test)]
mod tests;
