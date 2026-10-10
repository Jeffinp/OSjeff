//! pixels (split out of `png.rs`).

use super::*;

pub(super) struct Ctx {
    pub(super) color_type: ColorType,
    pub(super) depth: u8,
    pub(super) palette: [u32; 256],
    pub(super) palette_len: usize,
    /// `tRNS` colour key for grey (`[g, 0, 0]`) and RGB images.
    pub(super) key: Option<[u16; 3]>,
}

#[inline]
pub(super) fn s16(v: u16) -> u8 {
    ((v as u32 + 128) / 257) as u8
}

/// The `i`th sample of a packed row of `depth` (1, 2 or 4) bit samples.
#[inline]
pub(super) fn packed_sample(raw: &[u8], i: usize, depth: usize) -> u8 {
    let bit = i * depth;
    let b = raw.get(bit >> 3).copied().unwrap_or(0);
    (b >> (8 - depth - (bit & 7))) & ((1u8 << depth) - 1)
}

pub(super) fn convert_row(c: &Ctx, raw: &[u8], out: &mut [u32]) -> Result<(), PngError> {
    let depth = c.depth as usize;
    match (c.color_type, depth) {
        (ColorType::Gray, 8) => {
            let k = c.key.map_or(-1, |k| k[0] as i32);
            for (o, &g) in out.iter_mut().zip(raw) {
                *o = rgba(g, g, g, if g as i32 == k { 0 } else { 255 });
            }
        }
        (ColorType::Gray, 16) => {
            let k = c.key.map_or(-1, |k| k[0] as i32);
            for (o, v) in out.iter_mut().zip(raw.as_chunks::<2>().0) {
                let v = u16::from_be_bytes(*v);
                let g = s16(v);
                *o = rgba(g, g, g, if v as i32 == k { 0 } else { 255 });
            }
        }
        (ColorType::Gray, d) => {
            let k = c.key.map_or(-1, |k| k[0] as i32);
            let mult = 255 / ((1u32 << d) - 1);
            for (i, o) in out.iter_mut().enumerate() {
                let v = packed_sample(raw, i, d);
                let g = (v as u32 * mult) as u8;
                *o = rgba(g, g, g, if v as i32 == k { 0 } else { 255 });
            }
        }
        (ColorType::Rgb, 8) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<3>().0) {
                let a = match c.key {
                    Some(k) if k == [p[0] as u16, p[1] as u16, p[2] as u16] => 0,
                    _ => 255,
                };
                *o = rgba(p[0], p[1], p[2], a);
            }
        }
        (ColorType::Rgb, _) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<6>().0) {
                let v = [
                    u16::from_be_bytes([p[0], p[1]]),
                    u16::from_be_bytes([p[2], p[3]]),
                    u16::from_be_bytes([p[4], p[5]]),
                ];
                let a = if c.key == Some(v) { 0 } else { 255 };
                *o = rgba(s16(v[0]), s16(v[1]), s16(v[2]), a);
            }
        }
        (ColorType::Palette, 8) => {
            for (o, &i) in out.iter_mut().zip(raw) {
                if i as usize >= c.palette_len {
                    return Err(PngError::BadPaletteIndex);
                }
                *o = c.palette[i as usize];
            }
        }
        (ColorType::Palette, d) => {
            for (i, o) in out.iter_mut().enumerate() {
                let idx = packed_sample(raw, i, d) as usize;
                if idx >= c.palette_len {
                    return Err(PngError::BadPaletteIndex);
                }
                *o = c.palette[idx];
            }
        }
        (ColorType::GrayAlpha, 8) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<2>().0) {
                *o = rgba(p[0], p[0], p[0], p[1]);
            }
        }
        (ColorType::GrayAlpha, _) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<4>().0) {
                let g = s16(u16::from_be_bytes([p[0], p[1]]));
                *o = rgba(g, g, g, s16(u16::from_be_bytes([p[2], p[3]])));
            }
        }
        (ColorType::Rgba, 8) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<4>().0) {
                *o = rgba(p[0], p[1], p[2], p[3]);
            }
        }
        (ColorType::Rgba, _) => {
            for (o, p) in out.iter_mut().zip(raw.as_chunks::<8>().0) {
                *o = rgba(
                    s16(u16::from_be_bytes([p[0], p[1]])),
                    s16(u16::from_be_bytes([p[2], p[3]])),
                    s16(u16::from_be_bytes([p[4], p[5]])),
                    s16(u16::from_be_bytes([p[6], p[7]])),
                );
            }
        }
    }
    Ok(())
}
