//! decoder (split out of `png.rs`).

use super::*;

pub(super) fn read_full(inf: &mut Inflater<'_>, buf: &mut [u8]) -> Result<(), PngError> {
    let mut n = 0;
    while n < buf.len() {
        match inf.read(&mut buf[n..])? {
            0 => return Err(PngError::ImageDataTooShort),
            k => n += k,
        }
    }
    Ok(())
}

/// Decodes a PNG into an RGBA8 [`Image`].
pub fn decode(data: &[u8]) -> Result<Image, PngError> {
    if data.get(..8) != Some(&SIGNATURE[..]) {
        return Err(PngError::BadSignature);
    }
    let mut pos = 8usize;
    let first = next_chunk(data, &mut pos)?;
    if &first.ty != b"IHDR" {
        return Err(PngError::MissingIhdr);
    }
    let hdr = parse_ihdr(first.data)?;

    let mut plte: Option<&[u8]> = None;
    let mut trns: Option<&[u8]> = None;
    let mut idat: Vec<&[u8]> = Vec::new();
    let mut idat_done = false; // a non-IDAT chunk followed the IDATs
    let mut seen_end = false;
    while pos < data.len() {
        let c = next_chunk(data, &mut pos)?;
        match &c.ty {
            b"IHDR" => return Err(PngError::ChunkOrder),
            b"PLTE" => {
                if plte.is_some() || !idat.is_empty() || trns.is_some() {
                    return Err(PngError::ChunkOrder);
                }
                plte = Some(c.data);
            }
            b"tRNS" => {
                if trns.is_some() || !idat.is_empty() {
                    return Err(PngError::ChunkOrder);
                }
                trns = Some(c.data);
            }
            b"IDAT" => {
                if idat_done {
                    return Err(PngError::ChunkOrder);
                }
                idat.push(c.data);
            }
            b"IEND" => {
                if !c.data.is_empty() {
                    return Err(PngError::BadChunk);
                }
                seen_end = true;
                break;
            }
            ty => {
                if ty[0] & 0x20 == 0 {
                    return Err(PngError::UnknownCriticalChunk);
                }
            }
        }
        if &c.ty != b"IDAT" && !idat.is_empty() {
            idat_done = true;
        }
    }
    if !seen_end {
        return Err(PngError::Truncated);
    }
    if idat.is_empty() {
        return Err(PngError::MissingIdat);
    }

    // Palette / transparency.
    let mut ctx = Ctx {
        color_type: hdr.color_type,
        depth: hdr.bit_depth,
        palette: [0xFF00_0000; 256],
        palette_len: 0,
        key: None,
    };
    match hdr.color_type {
        ColorType::Palette => {
            let p = plte.ok_or(PngError::MissingPalette)?;
            let n = p.len() / 3;
            if p.len() % 3 != 0 || n == 0 || n > 256 || n > (1usize << hdr.bit_depth) {
                return Err(PngError::BadPalette);
            }
            for (slot, e) in ctx.palette.iter_mut().zip(p.as_chunks::<3>().0) {
                *slot = rgba(e[0], e[1], e[2], 255);
            }
            ctx.palette_len = n;
            if let Some(t) = trns {
                if t.len() > n {
                    return Err(PngError::BadPalette);
                }
                for (slot, &a) in ctx.palette.iter_mut().zip(t) {
                    *slot = (*slot & 0x00FF_FFFF) | ((a as u32) << 24);
                }
            }
        }
        ColorType::Gray | ColorType::Rgb => {
            if plte.is_some() && hdr.color_type == ColorType::Gray {
                return Err(PngError::BadPalette); // PLTE is illegal for greyscale
            }
            if let Some(t) = trns {
                let want = if hdr.color_type == ColorType::Gray {
                    2
                } else {
                    6
                };
                if t.len() != want {
                    return Err(PngError::BadPalette);
                }
                let mask = if hdr.bit_depth == 16 {
                    0xFFFF
                } else {
                    (1u16 << hdr.bit_depth) - 1
                };
                let rd = |i: usize| u16::from_be_bytes([t[2 * i], t[2 * i + 1]]) & mask;
                ctx.key = Some(if want == 2 {
                    [rd(0), 0, 0]
                } else {
                    [rd(0), rd(1), rd(2)]
                });
            }
        }
        ColorType::GrayAlpha => {
            if plte.is_some() {
                return Err(PngError::BadPalette);
            }
        }
        ColorType::Rgba => {}
    }

    // Size checks before any large allocation.
    let (w, h) = (hdr.width as usize, hdr.height as usize);
    let expected = usize::try_from(raw_size(&hdr)).map_err(|_| ImageError::TooLarge)?;
    let compressed: usize = idat.iter().map(|c| c.len()).sum();
    // Deflate expands at most ~1032:1; a header promising more than that was
    // lying (and would make us allocate the image for nothing).
    if expected as u64 > (compressed as u64 + 16) * 1100 {
        return Err(PngError::ImageDataTooShort);
    }
    let stream: Cow<[u8]> = if idat.len() == 1 {
        Cow::Borrowed(idat[0])
    } else {
        let mut v = Vec::new();
        v.try_reserve_exact(compressed)
            .map_err(|_| ImageError::OutOfMemory)?;
        for c in &idat {
            v.extend_from_slice(c);
        }
        Cow::Owned(v)
    };

    let mut img = Image::new(w, h, 0)?;
    let mut inf = Inflater::new_zlib(&stream, expected)?;
    let bpp_bits = hdr.bits_per_pixel();
    let fbpp = hdr.filter_bpp();
    let max_rb = row_bytes(w, bpp_bits);
    let mut prev = try_vec(max_rb, 0u8)?;
    let mut cur = try_vec(max_rb + 1, 0u8)?;
    let mut tmp = if hdr.interlaced {
        try_vec(w, 0u32)?
    } else {
        Vec::new()
    };
    let (ps, np) = passes(w, h, hdr.interlaced);
    for p in ps.iter().take(np) {
        if p.w == 0 || p.h == 0 {
            continue;
        }
        let rb = row_bytes(p.w, bpp_bits);
        prev[..rb].fill(0);
        for j in 0..p.h {
            read_full(&mut inf, &mut cur[..rb + 1])?;
            let (ft, row) = (cur[0], &mut cur[1..rb + 1]);
            unfilter(ft, row, &prev[..rb], fbpp)?;
            let y = p.y0 + j * p.dy;
            if hdr.interlaced {
                convert_row(&ctx, row, &mut tmp[..p.w])?;
                let dst = img.row_mut(y);
                for (i, &px) in tmp[..p.w].iter().enumerate() {
                    if let Some(slot) = dst.get_mut(p.x0 + i * p.dx) {
                        *slot = px;
                    }
                }
            } else {
                convert_row(&ctx, row, img.row_mut(y))?;
            }
            prev[..rb].copy_from_slice(row);
        }
    }
    // The stream must end exactly here (and its Adler-32 must check out).
    let mut probe = [0u8; 1];
    match inf.read(&mut probe) {
        Ok(0) => {}
        Ok(_) | Err(InflateError::OutputLimit) => return Err(PngError::ImageDataTooLong),
        Err(e) => return Err(PngError::Inflate(e)),
    }
    Ok(img)
}
