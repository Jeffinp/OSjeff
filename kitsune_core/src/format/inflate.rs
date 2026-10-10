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

mod bits;
mod checksums;
mod huffman;
mod inflater;
use bits::*;
pub use checksums::*;
pub(crate) use huffman::*;
pub use inflater::*;

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

// ---------------------------------------------------------------------------
// Bit reader
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Huffman tables
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Inflater
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;
