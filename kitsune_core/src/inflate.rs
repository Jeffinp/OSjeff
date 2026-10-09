//! DEFLATE (RFC 1951) and zlib (RFC 1950) decoder, plus CRC-32 and Adler-32.
//!
//! Everything is safe, total and bounded:
//!
//! * No function panics on any input. Every malformed stream maps to an
//!   [`InflateError`].
//! * The caller **must** pass `max_output`, the largest number of bytes the
//!   stream may expand to. Exceeding it is [`InflateError::OutputLimit`]. The
//!   decoder never allocates ahead of the data it actually produces (the
//!   output `Vec` grows in steps bounded by what was already produced), so a
//!   tiny "zip bomb" input cannot make it reserve `max_output` up front.
//! * Memory beyond the output is constant: a 32 KiB history window and two
//!   Huffman tables.
//!
//! Two front ends share one decoder:
//!
//! * [`inflate`] / [`zlib_decompress`]: whole input -> `Vec<u8>`.
//! * [`Inflater`]: streaming *output*. The input is a complete slice, but
//!   the output is produced in caller-sized pieces by [`Inflater::read`], so a
//!   consumer (the PNG decoder) can process scanlines without ever holding the
//!   whole decompressed image.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

/// Why a stream was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InflateError {
    /// The input ended before the stream did.
    Truncated,
    /// Block type 3 (reserved).
    BadBlockType,
    /// A stored block whose `LEN` is not the complement of `NLEN`.
    StoredLenMismatch,
    /// The Huffman code lengths are over-subscribed, or incomplete in a way
    /// RFC 1951 does not allow.
    BadCodeLengths,
    /// A dynamic block with no end-of-block code.
    MissingEndOfBlock,
    /// A length/distance symbol that is reserved (286, 287, 30, 31).
    InvalidSymbol,
    /// A bit pattern that matches no code of the current table.
    InvalidCode,
    /// A back-reference farther than the data produced so far (or 32 KiB).
    InvalidDistance,
    /// The output would exceed `max_output`.
    OutputLimit,
    /// The zlib header is malformed (method, window size, or check bits).
    BadZlibHeader,
    /// The zlib header asks for a preset dictionary, which is unsupported.
    DictionaryUnsupported,
    /// The Adler-32 trailer does not match the decompressed data.
    ChecksumMismatch,
    /// The allocator could not provide the output buffer.
    OutOfMemory,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            InflateError::Truncated => "compressed data is truncated",
            InflateError::BadBlockType => "reserved deflate block type",
            InflateError::StoredLenMismatch => "stored block length check failed",
            InflateError::BadCodeLengths => "invalid huffman code lengths",
            InflateError::MissingEndOfBlock => "block has no end-of-block code",
            InflateError::InvalidSymbol => "reserved length/distance symbol",
            InflateError::InvalidCode => "invalid huffman code",
            InflateError::InvalidDistance => "back-reference too far",
            InflateError::OutputLimit => "decompressed data exceeds the limit",
            InflateError::BadZlibHeader => "bad zlib header",
            InflateError::DictionaryUnsupported => "zlib preset dictionary unsupported",
            InflateError::ChecksumMismatch => "adler-32 mismatch",
            InflateError::OutOfMemory => "out of memory",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Checksums
// ---------------------------------------------------------------------------

const fn make_crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut s = 1;
        while s < 8 {
            let prev = t[s - 1][i];
            t[s][i] = t[0][(prev & 0xFF) as usize] ^ (prev >> 8);
            s += 1;
        }
        i += 1;
    }
    t
}

static CRC_TABLES: [[u32; 256]; 8] = make_crc_tables();

/// Incremental CRC-32 (IEEE 802.3, as used by PNG and zlib's `crc32`).
#[derive(Clone, Copy, Debug)]
pub struct Crc32 {
    state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    /// Feeds `data` (slicing-by-8).
    pub fn update(&mut self, data: &[u8]) {
        let t = &CRC_TABLES;
        let mut c = self.state;
        let (chunks, rest) = data.as_chunks::<8>();
        for ch in chunks {
            let lo = c ^ u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]);
            let hi = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
            c = t[7][(lo & 0xFF) as usize]
                ^ t[6][((lo >> 8) & 0xFF) as usize]
                ^ t[5][((lo >> 16) & 0xFF) as usize]
                ^ t[4][(lo >> 24) as usize]
                ^ t[3][(hi & 0xFF) as usize]
                ^ t[2][((hi >> 8) & 0xFF) as usize]
                ^ t[1][((hi >> 16) & 0xFF) as usize]
                ^ t[0][(hi >> 24) as usize];
        }
        for &b in rest {
            c = t[0][((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        self.state = c;
    }

    /// The CRC of everything fed so far (the hasher stays usable).
    pub const fn finish(&self) -> u32 {
        !self.state
    }
}

/// One-shot CRC-32.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(data);
    c.finish()
}

/// Incremental Adler-32 (RFC 1950).
#[derive(Clone, Copy, Debug)]
pub struct Adler32 {
    a: u32,
    b: u32,
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Adler32 {
    pub const fn new() -> Self {
        Self { a: 1, b: 0 }
    }

    pub fn update(&mut self, data: &[u8]) {
        // 5552 is the largest block for which `b` cannot overflow a u32
        // before the modulo (zlib's NMAX).
        for block in data.chunks(5552) {
            let (mut a, mut b) = (self.a, self.b);
            for &x in block {
                a += x as u32;
                b += a;
            }
            self.a = a % 65521;
            self.b = b % 65521;
        }
    }

    pub const fn finish(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

/// One-shot Adler-32.
pub fn adler32(data: &[u8]) -> u32 {
    let mut a = Adler32::new();
    a.update(data);
    a.finish()
}

// ---------------------------------------------------------------------------
// Bit reader
// ---------------------------------------------------------------------------

struct Bits<'a> {
    data: &'a [u8],
    /// Next byte to load into `buf`.
    pos: usize,
    /// Bit buffer, LSB first. Bits at and above `cnt` are either zero or the
    /// real upcoming input bits (never anything else), so refilling by OR is
    /// idempotent.
    buf: u64,
    cnt: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        }
    }

    /// Tops the buffer up to at least 56 valid bits, or to the end of input.
    #[inline(always)]
    fn refill(&mut self) {
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
    fn bits(&mut self, n: u32) -> Result<u32, InflateError> {
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
    fn align_and_unread(&mut self) {
        let drop = self.cnt & 7;
        self.cnt -= drop;
        self.pos -= (self.cnt >> 3) as usize;
        self.buf = 0;
        self.cnt = 0;
    }

    /// Bytes of input fully or partially consumed.
    fn consumed(&self) -> usize {
        self.pos - (self.cnt >> 3) as usize
    }
}

// ---------------------------------------------------------------------------
// Huffman tables
// ---------------------------------------------------------------------------

const FAST_BITS: u32 = 10;
const FAST_SIZE: usize = 1 << FAST_BITS;
const FAST_MASK: u64 = (FAST_SIZE as u64) - 1;
const MAX_CODE_LEN: usize = 15;
const MAX_SYMBOLS: usize = 288;

struct Huffman {
    /// `symbol << 4 | length` for codes of at most `FAST_BITS` bits, indexed
    /// by the next `FAST_BITS` input bits; 0 = not a short code.
    fast: [u16; FAST_SIZE],
    /// Number of codes of each length.
    count: [u16; MAX_CODE_LEN + 1],
    /// Symbols ordered by (length, symbol): the canonical order.
    symbols: [u16; MAX_SYMBOLS],
}

impl Huffman {
    fn new() -> Self {
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
    fn build(&mut self, lens: &[u8], complete: bool) -> Result<(), InflateError> {
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
    fn decode(&self, br: &mut Bits<'_>) -> Result<u16, InflateError> {
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
    fn decode_slow(&self, br: &mut Bits<'_>) -> Result<u16, InflateError> {
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
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

const WINDOW: usize = 32768;
const WMASK: usize = WINDOW - 1;

// ---------------------------------------------------------------------------
// Inflater
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Expecting a block header.
    Header,
    /// Inside a stored block with this many bytes left.
    Stored(usize),
    /// Inside a Huffman block.
    Codes,
    /// Final block done (and zlib trailer verified).
    Done,
    Failed(InflateError),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    None,
    Fixed,
    Dynamic,
}

/// A resumable DEFLATE / zlib decoder over a complete input slice.
///
/// ```
/// use kitsune_core::inflate::Inflater;
/// // zlib stream of "hi" stored in one block.
/// let z = [0x78, 0x01, 0x01, 0x02, 0x00, 0xFD, 0xFF, b'h', b'i', 0x01, 0x3B, 0x00, 0xD2];
/// let mut inf = Inflater::new_zlib(&z, 16).unwrap();
/// let mut buf = [0u8; 8];
/// let n = inf.read(&mut buf).unwrap();
/// assert_eq!(&buf[..n], b"hi");
/// assert_eq!(inf.read(&mut buf).unwrap(), 0); // end of stream
/// ```
pub struct Inflater<'a> {
    br: Bits<'a>,
    state: State,
    table: Table,
    last_block: bool,
    zlib: bool,
    adler: Adler32,
    lit: Box<Huffman>,
    dist: Box<Huffman>,
    window: Box<[u8; WINDOW]>,
    wpos: usize,
    produced: usize,
    limit: usize,
    pend_len: usize,
    pend_dist: usize,
}

impl<'a> Inflater<'a> {
    fn new_inner(data: &'a [u8], max_output: usize, zlib: bool) -> Self {
        let window: Box<[u8; WINDOW]> = alloc::vec![0u8; WINDOW]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| Box::new([0u8; WINDOW]));
        Self {
            br: Bits::new(data),
            state: State::Header,
            table: Table::None,
            last_block: false,
            zlib,
            adler: Adler32::new(),
            lit: Box::new(Huffman::new()),
            dist: Box::new(Huffman::new()),
            window,
            wpos: 0,
            produced: 0,
            limit: max_output,
            pend_len: 0,
            pend_dist: 0,
        }
    }

    /// A decoder for a raw DEFLATE stream.
    pub fn new_raw(data: &'a [u8], max_output: usize) -> Self {
        Self::new_inner(data, max_output, false)
    }

    /// A decoder for a zlib stream. The 2-byte header is validated here; the
    /// Adler-32 trailer is verified when the stream ends.
    pub fn new_zlib(data: &'a [u8], max_output: usize) -> Result<Self, InflateError> {
        let (&cmf, &flg) = match (data.first(), data.get(1)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(InflateError::Truncated),
        };
        if cmf & 0x0F != 8 || cmf >> 4 > 7 {
            return Err(InflateError::BadZlibHeader);
        }
        if !(((cmf as u32) << 8) | flg as u32).is_multiple_of(31) {
            return Err(InflateError::BadZlibHeader);
        }
        if flg & 0x20 != 0 {
            return Err(InflateError::DictionaryUnsupported);
        }
        let mut s = Self::new_inner(data, max_output, true);
        s.br.pos = 2;
        Ok(s)
    }

    /// True once the final block (and, for zlib, the checksum) was read.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Total bytes produced so far.
    pub fn total_out(&self) -> usize {
        self.produced
    }

    /// Input bytes consumed. After [`is_done`](Self::is_done) this is the
    /// exact length of the stream (including the zlib trailer), so trailing
    /// data starts here.
    pub fn consumed(&self) -> usize {
        self.br.consumed()
    }

    /// Decodes up to `out.len()` bytes. Returns how many were written; `0`
    /// means the stream ended (or `out` is empty). A short read only happens
    /// at the end of the stream; after an error every later call repeats it.
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, InflateError> {
        match self.read_partial(out) {
            (n, None) => Ok(n),
            (_, Some(e)) => Err(e),
        }
    }

    /// Like [`read`](Self::read) but an error does not throw away what was
    /// decoded before it: returns `(bytes written to out, error)`. The bytes
    /// are valid output of the stream (the decoder wrote them before it hit
    /// the problem), which is what lets a truncated download still render its
    /// decoded prefix. After an error every later call returns `(0, error)`.
    pub fn read_partial(&mut self, out: &mut [u8]) -> (usize, Option<InflateError>) {
        let mut hashed = 0usize;
        let mut n = 0usize;
        let r = self.read_inner(out, &mut hashed, &mut n);
        if self.zlib {
            self.adler.update(out.get(hashed..n).unwrap_or(&[]));
        }
        match r {
            Ok(()) => (n, None),
            Err(e) => {
                self.state = State::Failed(e);
                (n, Some(e))
            }
        }
    }

    fn read_inner(
        &mut self,
        out: &mut [u8],
        hashed: &mut usize,
        n: &mut usize,
    ) -> Result<(), InflateError> {
        loop {
            if *n == out.len() {
                return Ok(());
            }
            match self.state {
                State::Failed(e) => return Err(e),
                State::Done => return Ok(()),
                State::Header => self.start_block()?,
                State::Stored(rem) => {
                    if rem == 0 {
                        self.end_block(out, *n, hashed)?;
                        continue;
                    }
                    // Deliver what fits under the limit before refusing the rest.
                    let room = self.limit - self.produced;
                    let take = rem.min(out.len() - *n).min(room);
                    if take == 0 {
                        return Err(InflateError::OutputLimit);
                    }
                    // A cut input still yields the bytes that did arrive.
                    let avail = self.br.data.get(self.br.pos..).unwrap_or(&[]);
                    let src = &avail[..take.min(avail.len())];
                    let take = src.len();
                    if take == 0 {
                        return Err(InflateError::Truncated);
                    }
                    let dst = &mut out[*n..*n + take];
                    dst.copy_from_slice(src);
                    for &b in src {
                        self.window[self.wpos & WMASK] = b;
                        self.wpos = (self.wpos + 1) & WMASK;
                    }
                    self.br.pos += take;
                    self.produced += take;
                    *n += take;
                    self.state = State::Stored(rem - take);
                }
                State::Codes => {
                    let eob = self.decode_codes(out, n)?;
                    if eob {
                        self.end_block(out, *n, hashed)?;
                    }
                }
            }
        }
    }

    /// The block just ended: go to the next header, or finish the stream.
    fn end_block(&mut self, out: &[u8], n: usize, hashed: &mut usize) -> Result<(), InflateError> {
        if !self.last_block {
            self.state = State::Header;
            return Ok(());
        }
        self.br.align_and_unread();
        if self.zlib {
            self.adler.update(out.get(*hashed..n).unwrap_or(&[]));
            *hashed = n;
            let t = self
                .br
                .data
                .get(self.br.pos..)
                .and_then(|s| s.first_chunk::<4>())
                .ok_or(InflateError::Truncated)?;
            if u32::from_be_bytes(*t) != self.adler.finish() {
                return Err(InflateError::ChecksumMismatch);
            }
            self.br.pos += 4;
        }
        self.state = State::Done;
        Ok(())
    }

    fn start_block(&mut self) -> Result<(), InflateError> {
        let h = self.br.bits(3)?;
        self.last_block = h & 1 != 0;
        match h >> 1 {
            0 => {
                let drop = self.br.cnt & 7;
                self.br.buf >>= drop;
                self.br.cnt -= drop;
                let len = self.br.bits(16)?;
                let nlen = self.br.bits(16)?;
                if len != !nlen & 0xFFFF {
                    return Err(InflateError::StoredLenMismatch);
                }
                // Hand the buffered whole bytes back: the payload is copied
                // straight from the input slice.
                self.br.align_and_unread();
                self.state = State::Stored(len as usize);
            }
            1 => {
                if self.table != Table::Fixed {
                    let mut lens = [8u8; 288];
                    for l in lens.iter_mut().take(256).skip(144) {
                        *l = 9;
                    }
                    for l in lens.iter_mut().take(280).skip(256) {
                        *l = 7;
                    }
                    self.lit.build(&lens, true)?;
                    self.dist.build(&[5u8; 32], false)?;
                    self.table = Table::Fixed;
                }
                self.state = State::Codes;
            }
            2 => {
                self.read_dynamic()?;
                self.table = Table::Dynamic;
                self.state = State::Codes;
            }
            _ => return Err(InflateError::BadBlockType),
        }
        Ok(())
    }

    fn read_dynamic(&mut self) -> Result<(), InflateError> {
        let hlit = self.br.bits(5)? as usize + 257;
        let hdist = self.br.bits(5)? as usize + 1;
        let hclen = self.br.bits(4)? as usize + 4;
        if hlit > 286 || hdist > 30 {
            return Err(InflateError::BadCodeLengths);
        }
        let mut cl_lens = [0u8; 19];
        for &idx in CL_ORDER.iter().take(hclen) {
            cl_lens[idx] = self.br.bits(3)? as u8;
        }
        let mut cl = Huffman::new();
        cl.build(&cl_lens, true)?;
        let mut lens = [0u8; 286 + 30];
        let total = hlit + hdist;
        let mut i = 0usize;
        while i < total {
            let sym = cl.decode(&mut self.br)?;
            let (val, rep) = match sym {
                0..=15 => (sym as u8, 1usize),
                16 => {
                    if i == 0 {
                        return Err(InflateError::BadCodeLengths);
                    }
                    (lens[i - 1], 3 + self.br.bits(2)? as usize)
                }
                17 => (0, 3 + self.br.bits(3)? as usize),
                _ => (0, 11 + self.br.bits(7)? as usize),
            };
            if i + rep > total {
                return Err(InflateError::BadCodeLengths);
            }
            for l in &mut lens[i..i + rep] {
                *l = val;
            }
            i += rep;
        }
        if lens[256] == 0 {
            return Err(InflateError::MissingEndOfBlock);
        }
        self.lit.build(&lens[..hlit], false)?;
        self.dist.build(&lens[hlit..total], false)?;
        Ok(())
    }

    /// Decodes symbols into `out[*n..]` until it is full or the block ends,
    /// advancing `*n` as bytes are written (so they survive an error).
    /// Returns whether the block ended.
    fn decode_codes(&mut self, out: &mut [u8], n: &mut usize) -> Result<bool, InflateError> {
        loop {
            if *n == out.len() {
                return Ok(false);
            }
            if self.pend_len > 0 {
                let room = self.limit - self.produced;
                if room == 0 {
                    return Err(InflateError::OutputLimit);
                }
                // A match that would cross the limit delivers its head first.
                let take = self.pend_len.min(out.len() - *n).min(room);
                let dist = self.pend_dist;
                let src = self.wpos.wrapping_sub(dist) & WMASK;
                if dist >= take && src + take <= WINDOW && self.wpos + take <= WINDOW {
                    // Source and destination are disjoint, contiguous ring
                    // ranges: a plain memmove, no per-byte work.
                    let dst = self.wpos;
                    self.window.copy_within(src..src + take, dst);
                    out[*n..*n + take].copy_from_slice(&self.window[dst..dst + take]);
                    self.wpos = (dst + take) & WMASK;
                } else {
                    // Overlapping (dist < len, e.g. run-length) or wrapping.
                    let mut w = self.wpos;
                    for o in &mut out[*n..*n + take] {
                        let b = self.window[w.wrapping_sub(dist) & WMASK];
                        self.window[w & WMASK] = b;
                        *o = b;
                        w = (w + 1) & WMASK;
                    }
                    self.wpos = w;
                }
                self.produced += take;
                self.pend_len -= take;
                *n += take;
                continue;
            }
            let sym = self.lit.decode(&mut self.br)? as usize;
            if sym < 256 {
                if self.produced >= self.limit {
                    return Err(InflateError::OutputLimit);
                }
                out[*n] = sym as u8;
                self.window[self.wpos & WMASK] = sym as u8;
                self.wpos = (self.wpos + 1) & WMASK;
                self.produced += 1;
                *n += 1;
            } else if sym == 256 {
                return Ok(true);
            } else {
                let li = sym - 257;
                let (Some(&lbase), Some(&lextra)) = (LEN_BASE.get(li), LEN_EXTRA.get(li)) else {
                    return Err(InflateError::InvalidSymbol);
                };
                let len = lbase as usize + self.br.bits(lextra as u32)? as usize;
                let ds = self.dist.decode(&mut self.br)? as usize;
                let (Some(&dbase), Some(&dextra)) = (DIST_BASE.get(ds), DIST_EXTRA.get(ds)) else {
                    return Err(InflateError::InvalidSymbol);
                };
                let dist = dbase as usize + self.br.bits(dextra as u32)? as usize;
                if dist > self.produced || dist > WINDOW {
                    return Err(InflateError::InvalidDistance);
                }
                self.pend_len = len;
                self.pend_dist = dist;
            }
        }
    }

    /// Decodes everything that is left into a new `Vec`.
    fn read_all(&mut self) -> Result<Vec<u8>, InflateError> {
        match self.read_all_partial() {
            (out, None) => Ok(out),
            (_, Some(e)) => Err(e),
        }
    }

    /// Decodes everything that is left, keeping the decoded prefix when the
    /// stream fails (cut input, corrupt data, output limit, checksum): returns
    /// `(everything produced before the problem, the problem if any)`.
    fn read_all_partial(&mut self) -> (Vec<u8>, Option<InflateError>) {
        let mut out: Vec<u8> = Vec::new();
        loop {
            let room = self.limit - self.produced;
            // Grow geometrically from what was already produced; one extra
            // byte over `room` is never requested, so the limit is exact.
            let want = room.min(out.len().max(4096)).max(1);
            let start = out.len();
            if out.try_reserve(want).is_err() {
                out.shrink_to_fit();
                return (out, Some(InflateError::OutOfMemory));
            }
            out.resize(start + want, 0);
            let (got, err) = self.read_partial(&mut out[start..]);
            out.truncate(start + got);
            if err.is_some() || got == 0 {
                out.shrink_to_fit();
                return (out, err);
            }
        }
    }
}

/// Decompresses a raw DEFLATE stream. `max_output` is the hard cap on the
/// output size. Bytes after the final block are ignored.
pub fn inflate(data: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    Inflater::new_raw(data, max_output).read_all()
}

/// Like [`inflate`] but a failing stream (cut input, corrupt data, output limit)
/// still returns what was decoded before the problem: `(prefix, problem)`.
/// `problem` is `None` only for a stream that ended cleanly.
pub fn inflate_partial(data: &[u8], max_output: usize) -> (Vec<u8>, Option<InflateError>) {
    Inflater::new_raw(data, max_output).read_all_partial()
}

/// [`inflate_partial`] for a zlib stream. A bad zlib header is `Err` (nothing
/// was decoded); an Adler-32 mismatch after the last block is reported as
/// `Some(ChecksumMismatch)` next to the complete output.
pub fn zlib_partial(
    data: &[u8],
    max_output: usize,
) -> Result<(Vec<u8>, Option<InflateError>), InflateError> {
    Ok(Inflater::new_zlib(data, max_output)?.read_all_partial())
}

/// Like [`inflate_partial`], also returning how many input bytes the stream
/// used (only meaningful when `problem` is `None`).
pub fn inflate_consumed_partial(
    data: &[u8],
    max_output: usize,
) -> (Vec<u8>, Option<InflateError>, usize) {
    let mut inf = Inflater::new_raw(data, max_output);
    let (v, e) = inf.read_all_partial();
    (v, e, inf.consumed())
}

/// Like [`inflate`], also returning how many input bytes the stream used.
pub fn inflate_consumed(data: &[u8], max_output: usize) -> Result<(Vec<u8>, usize), InflateError> {
    let mut inf = Inflater::new_raw(data, max_output);
    let v = inf.read_all()?;
    Ok((v, inf.consumed()))
}

/// Decompresses a zlib stream (RFC 1950), verifying the Adler-32 trailer.
pub fn zlib_decompress(data: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    Inflater::new_zlib(data, max_output)?.read_all()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;
