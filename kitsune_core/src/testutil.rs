//! Helpers shared by the image/codec tests.

use alloc::vec::Vec;

/// Decodes a lowercase/uppercase hex string.
pub(crate) fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}
