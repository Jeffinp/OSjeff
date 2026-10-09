//! ChaCha20 (RFC 8439): the block function and a keystream helper.
//!
//! This is the stream cipher core of the DRBG ([`super::drbg`]). It is written
//! out here (about 60 lines) instead of pulling `chacha20`/`cipher` into the
//! kernel: nothing else in the tree needs them, and the block function is
//! checked against the RFC's test vectors in `tests.rs`.

/// "expand 32-byte k".
const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

/// Bytes in one ChaCha20 block.
pub const BLOCK_LEN: usize = 64;

/// One quarter round (RFC 8439 section 2.1).
#[inline]
pub fn quarter_round(a: u32, b: u32, c: u32, d: u32) -> (u32, u32, u32, u32) {
    let a = a.wrapping_add(b);
    let d = (d ^ a).rotate_left(16);
    let c = c.wrapping_add(d);
    let b = (b ^ c).rotate_left(12);
    let a = a.wrapping_add(b);
    let d = (d ^ a).rotate_left(8);
    let c = c.wrapping_add(d);
    let b = (b ^ c).rotate_left(7);
    (a, b, c, d)
}

#[inline]
fn qr(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    let (x, y, z, w) = quarter_round(s[a], s[b], s[c], s[d]);
    s[a] = x;
    s[b] = y;
    s[c] = z;
    s[d] = w;
}

/// The ChaCha20 block function: 64 bytes of keystream for `key`, block
/// `counter` and `nonce` (RFC 8439 section 2.3).
pub fn block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; BLOCK_LEN] {
    let mut init = [0u32; 16];
    init[..4].copy_from_slice(&SIGMA);
    for (i, w) in init[4..12].iter_mut().enumerate() {
        *w = u32::from_le_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
    }
    init[12] = counter;
    for (i, w) in init[13..16].iter_mut().enumerate() {
        *w = u32::from_le_bytes([
            nonce[4 * i],
            nonce[4 * i + 1],
            nonce[4 * i + 2],
            nonce[4 * i + 3],
        ]);
    }
    let mut s = init;
    for _ in 0..10 {
        // Column rounds, then diagonal rounds.
        qr(&mut s, 0, 4, 8, 12);
        qr(&mut s, 1, 5, 9, 13);
        qr(&mut s, 2, 6, 10, 14);
        qr(&mut s, 3, 7, 11, 15);
        qr(&mut s, 0, 5, 10, 15);
        qr(&mut s, 1, 6, 11, 12);
        qr(&mut s, 2, 7, 8, 13);
        qr(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; BLOCK_LEN];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&s[i].wrapping_add(init[i]).to_le_bytes());
    }
    out
}

/// Fill `out` with keystream starting at block `counter`. Returns the counter
/// of the next unused block, or `None` if the request would wrap the 32-bit
/// block counter (a keystream block must never repeat under one key and nonce).
pub fn keystream(
    key: &[u8; 32],
    nonce: &[u8; 12],
    mut counter: u32,
    out: &mut [u8],
) -> Option<u32> {
    for chunk in out.chunks_mut(BLOCK_LEN) {
        let b = block(key, counter, nonce);
        chunk.copy_from_slice(&b[..chunk.len()]);
        counter = counter.checked_add(1)?;
    }
    Some(counter)
}
