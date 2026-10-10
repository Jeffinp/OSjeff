//! unfilter (split out of `png.rs`).

use super::*;

#[inline]
pub(super) fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i16, b as i16, c as i16);
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Reverses filter `ft` in place. `prev` is the previous (already
/// unfiltered) scanline of the same length, or zeros for the first row.
pub(super) fn unfilter(ft: u8, row: &mut [u8], prev: &[u8], bpp: usize) -> Result<(), PngError> {
    let n = row.len();
    let prev = &prev[..n];
    match ft {
        0 => {}
        1 => {
            for i in bpp..n {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        2 => {
            for (r, &p) in row.iter_mut().zip(prev) {
                *r = r.wrapping_add(p);
            }
        }
        3 => {
            for i in 0..bpp.min(n) {
                row[i] = row[i].wrapping_add(prev[i] >> 1);
            }
            for i in bpp..n {
                let avg = ((row[i - bpp] as u16 + prev[i] as u16) >> 1) as u8;
                row[i] = row[i].wrapping_add(avg);
            }
        }
        4 => {
            for i in 0..bpp.min(n) {
                row[i] = row[i].wrapping_add(prev[i]); // paeth(0, b, 0) == b
            }
            for i in bpp..n {
                let p = paeth(row[i - bpp], prev[i], prev[i - bpp]);
                row[i] = row[i].wrapping_add(p);
            }
        }
        _ => return Err(PngError::BadFilter),
    }
    Ok(())
}
