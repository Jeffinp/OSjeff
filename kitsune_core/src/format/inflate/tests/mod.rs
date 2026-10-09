use super::vectors::*;
use super::*;
use crate::testutil::unhex;
use alloc::vec;
use alloc::vec::Vec;

const TEXT_LEN: usize = 731;

fn text() -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..4 {
        v.extend_from_slice(
            b"The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. \
              How vexingly quick daft zebras jump! Sphinx of black quartz, judge my vow. ",
        );
    }
    v.extend_from_slice(
        b"Kitsune image decoder: 0123456789 abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    );
    v
}

fn window_data() -> Vec<u8> {
    let mut pat = Vec::new();
    for k in 0..32u32 {
        pat.extend(core::iter::repeat_n(((k * 37 + 11) % 256) as u8, 1024));
    }
    let mut v = pat.clone();
    v.extend_from_slice(&pat);
    v.extend_from_slice(&pat[..5000]);
    v
}

/// LSB-first bit writer for hand-made streams.
#[derive(Default)]
struct W {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl W {
    fn bits(&mut self, v: u32, n: u32) -> &mut Self {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
        self
    }
    /// A Huffman code (MSB first on the wire).
    fn code(&mut self, code: u32, len: u32) -> &mut Self {
        let rev = code.reverse_bits() >> (32 - len);
        self.bits(rev, len)
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
    fn header(&mut self, last: bool, ty: u32) -> &mut Self {
        self.bits(last as u32, 1).bits(ty, 2)
    }
    fn fixed_lit(&mut self, sym: u32) -> &mut Self {
        match sym {
            0..=143 => self.code(0x30 + sym, 8),
            144..=255 => self.code(0x190 + sym - 144, 9),
            256..=279 => self.code(sym - 256, 7),
            _ => self.code(0xC0 + sym - 280, 8),
        }
    }
    fn fixed_match(&mut self, len: usize, dist: usize) -> &mut Self {
        let li = LEN_BASE.iter().rposition(|&b| b as usize <= len).unwrap();
        self.fixed_lit(257 + li as u32);
        self.bits((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
        let di = DIST_BASE.iter().rposition(|&b| b as usize <= dist).unwrap();
        self.code(di as u32, 5);
        self.bits(
            (dist - DIST_BASE[di] as usize) as u32,
            DIST_EXTRA[di] as u32,
        )
    }
}

/// Canonical codes for `lens`: `(code, len)` per symbol.
fn canon(lens: &[u8]) -> Vec<(u32, u32)> {
    let mut count = [0u32; 16];
    for &l in lens {
        count[l as usize] += 1;
    }
    count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0;
    for b in 1..16 {
        code = (code + count[b - 1]) << 1;
        next[b] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                (0, 0)
            } else {
                let c = next[l as usize];
                next[l as usize] += 1;
                (c, l as u32)
            }
        })
        .collect()
}

/// Starts a dynamic block whose code lengths are sent with a flat 4-bit
/// code-length code (symbols 0..=15 only, no run-length codes).
fn dyn_header(w: &mut W, last: bool, lit: &[u8], dist: &[u8]) {
    w.header(last, 2);
    w.bits(lit.len() as u32 - 257, 5);
    w.bits(dist.len() as u32 - 1, 5);
    w.bits(15, 4); // hclen = 19
    for &order in &CL_ORDER {
        w.bits(if order <= 15 { 4 } else { 0 }, 3);
    }
    for &l in lit.iter().chain(dist) {
        w.code(l as u32, 4);
    }
}

fn fixed_stream(f: impl FnOnce(&mut W)) -> Vec<u8> {
    let mut w = W::default();
    w.header(true, 1);
    f(&mut w);
    w.fixed_lit(256);
    w.finish()
}

fn lit_lens(pairs: &[(usize, u8)], n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    for &(i, l) in pairs {
        v[i] = l;
    }
    v
}

fn zlib_wrap(raw: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = vec![0x78, 0x9C];
    v.extend_from_slice(raw);
    v.extend_from_slice(&adler32(data).to_be_bytes());
    v
}

const BIG: usize = 1 << 24;

fn crc_bitwise(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn read_chunked(mut inf: Inflater<'_>, chunk: usize) -> Result<Vec<u8>, InflateError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; chunk];
    loop {
        let n = inf.read(&mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

mod checksums;
mod corruption;
mod hand_made_streams;
mod output_limit;
mod partial_output_cut;
mod real_zlib_streams;
mod streaming;
mod zlib_wrapper;
