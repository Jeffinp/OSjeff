//! inflater (split out of `inflate.rs`).

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
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
pub(super) enum Table {
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
    pub(super) br: Bits<'a>,
    pub(super) state: State,
    pub(super) table: Table,
    pub(super) last_block: bool,
    pub(super) zlib: bool,
    pub(super) adler: Adler32,
    pub(super) lit: Box<Huffman>,
    pub(super) dist: Box<Huffman>,
    pub(super) window: Box<[u8; WINDOW]>,
    pub(super) wpos: usize,
    pub(super) produced: usize,
    pub(super) limit: usize,
    pub(super) pend_len: usize,
    pub(super) pend_dist: usize,
}

impl<'a> Inflater<'a> {
    pub(super) fn new_inner(data: &'a [u8], max_output: usize, zlib: bool) -> Self {
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

    pub(super) fn read_inner(
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
    pub(super) fn end_block(
        &mut self,
        out: &[u8],
        n: usize,
        hashed: &mut usize,
    ) -> Result<(), InflateError> {
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

    pub(super) fn start_block(&mut self) -> Result<(), InflateError> {
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

    pub(super) fn read_dynamic(&mut self) -> Result<(), InflateError> {
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
    pub(super) fn decode_codes(
        &mut self,
        out: &mut [u8],
        n: &mut usize,
    ) -> Result<bool, InflateError> {
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
    pub(super) fn read_all(&mut self) -> Result<Vec<u8>, InflateError> {
        match self.read_all_partial() {
            (out, None) => Ok(out),
            (_, Some(e)) => Err(e),
        }
    }

    /// Decodes everything that is left, keeping the decoded prefix when the
    /// stream fails (cut input, corrupt data, output limit, checksum): returns
    /// `(everything produced before the problem, the problem if any)`.
    pub(super) fn read_all_partial(&mut self) -> (Vec<u8>, Option<InflateError>) {
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
