use super::*;
use std::collections::HashMap;
use std::vec;

/// A real LZW encoder (GIF flavour), so the decoder is checked against streams that grow the table,
/// bump the code width and hit the 4096-entry limit.
fn lzw_encode(idx: &[u8], min: u8) -> Vec<u8> {
    let clear = 1u32 << min;
    let eoi = clear + 1;
    let mut out: Vec<u8> = Vec::new();
    let (mut acc, mut nbits) = (0u32, 0u32);
    let mut put = |code: u32, size: u32, out: &mut Vec<u8>| {
        acc |= code << nbits;
        nbits += size;
        while nbits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            nbits -= 8;
        }
    };
    let mut size = min as u32 + 1;
    let mut table: HashMap<(u32, u8), u32> = HashMap::new();
    let mut next = eoi + 1;
    put(clear, size, &mut out);
    let mut it = idx.iter();
    let Some(&first) = it.next() else {
        put(eoi, size, &mut out);
        if nbits > 0 {
            out.push(acc as u8);
        }
        return out;
    };
    let mut cur = first as u32;
    for &b in it {
        if let Some(&c) = table.get(&(cur, b)) {
            cur = c;
        } else {
            put(cur, size, &mut out);
            if next < 4096 {
                table.insert((cur, b), next);
                if next == (1 << size) && size < 12 {
                    size += 1;
                }
                next += 1;
            } else {
                put(clear, size, &mut out);
                table.clear();
                size = min as u32 + 1;
                next = eoi + 1;
            }
            cur = b as u32;
        }
    }
    put(cur, size, &mut out);
    put(eoi, size, &mut out);
    if nbits > 0 {
        out.push(acc as u8);
    }
    out
}

fn blocks(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    for ch in data.chunks(255) {
        v.push(ch.len() as u8);
        v.extend_from_slice(ch);
    }
    v.push(0);
    v
}

struct Gif {
    w: u16,
    h: u16,
    global: Option<Vec<[u8; 3]>>,
    frame: (u16, u16, u16, u16),
    local: Option<Vec<[u8; 3]>>,
    interlace: bool,
    transparent: Option<u8>,
    idx: Vec<u8>,
    min: u8,
}

fn table_flag(t: &[[u8; 3]]) -> u8 {
    let n = t.len().next_power_of_two().max(2);
    0x80 | (n.trailing_zeros() as u8 - 1)
}

fn pad(t: &[[u8; 3]]) -> Vec<u8> {
    let n = t.len().next_power_of_two().max(2);
    let mut v: Vec<u8> = t.iter().flatten().copied().collect();
    v.resize(n * 3, 0);
    v
}

impl Gif {
    fn simple(w: u16, h: u16, colors: &[[u8; 3]], idx: Vec<u8>) -> Gif {
        Gif {
            w,
            h,
            global: Some(colors.to_vec()),
            frame: (0, 0, w, h),
            local: None,
            interlace: false,
            transparent: None,
            idx,
            min: (colors.len().next_power_of_two().max(4).trailing_zeros() as u8).max(2),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&self.w.to_le_bytes());
        v.extend_from_slice(&self.h.to_le_bytes());
        match &self.global {
            Some(g) => {
                v.push(table_flag(g));
                v.extend_from_slice(&[0, 0]);
                v.extend(pad(g));
            }
            None => v.extend_from_slice(&[0, 0, 0]),
        }
        if let Some(t) = self.transparent {
            v.extend_from_slice(&[0x21, 0xF9, 4, 1, 0, 0, t, 0]);
        }
        // A comment extension the decoder must skip.
        v.extend_from_slice(&[0x21, 0xFE, 3, b'h', b'i', b'!', 0]);
        v.push(0x2C);
        for n in [self.frame.0, self.frame.1, self.frame.2, self.frame.3] {
            v.extend_from_slice(&n.to_le_bytes());
        }
        let mut fl = if self.interlace { 0x40 } else { 0 };
        if let Some(l) = &self.local {
            fl |= table_flag(l);
        }
        v.push(fl);
        if let Some(l) = &self.local {
            v.extend(pad(l));
        }
        v.push(self.min);
        v.extend(blocks(&lzw_encode(&self.idx, self.min)));
        v.push(0x3B);
        v
    }
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

fn px(img: &Image, x: usize, y: usize) -> u32 {
    img.pixels()[y * img.width() + x]
}

#[test]
fn the_smallest_transparent_gif() {
    // The well-known 1x1 transparent GIF.
    let g = [
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0xff,
        0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];
    let img = decode(&g).unwrap();
    assert_eq!((img.width(), img.height()), (1, 1));
    assert_eq!(img.pixels()[0] >> 24, 0, "index 0 is transparent");
}

#[test]
fn colours_and_dimensions() {
    let g = Gif::simple(
        4,
        2,
        &[RED, GREEN, BLUE, WHITE],
        vec![0, 1, 2, 3, 3, 2, 1, 0],
    );
    let img = decode(&g.bytes()).unwrap();
    assert_eq!((img.width(), img.height()), (4, 2));
    assert_eq!(px(&img, 0, 0), rgba(255, 0, 0, 255));
    assert_eq!(px(&img, 1, 0), rgba(0, 255, 0, 255));
    assert_eq!(px(&img, 2, 0), rgba(0, 0, 255, 255));
    assert_eq!(px(&img, 3, 0), rgba(255, 255, 255, 255));
    assert_eq!(px(&img, 0, 1), rgba(255, 255, 255, 255));
    assert_eq!(px(&img, 3, 1), rgba(255, 0, 0, 255));
}

#[test]
fn big_images_exercise_every_code_width_and_the_full_table() {
    // Pseudo-random 8-bit data with runs: the table fills up, the encoder clears and starts over.
    let (w, h) = (200usize, 150usize);
    let mut x = 12345u32;
    let idx: Vec<u8> = (0..w * h)
        .map(|i| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            if i % 7 < 3 {
                (i / 7) as u8
            } else {
                (x >> 24) as u8
            }
        })
        .collect();
    let colors: Vec<[u8; 3]> = (0..256)
        .map(|i| [i as u8, (255 - i) as u8, (i * 7) as u8])
        .collect();
    let g = Gif::simple(w as u16, h as u16, &colors, idx.clone());
    assert_eq!(g.min, 8);
    let img = decode(&g.bytes()).unwrap();
    for (i, &v) in idx.iter().enumerate() {
        let c = colors[v as usize];
        assert_eq!(img.pixels()[i], rgba(c[0], c[1], c[2], 255), "pixel {i}");
    }
}

#[test]
fn long_runs_use_the_kwkwk_case() {
    let idx = vec![1u8; 5000];
    let g = Gif::simple(100, 50, &[RED, GREEN, BLUE, WHITE], idx);
    let img = decode(&g.bytes()).unwrap();
    assert!(img.pixels().iter().all(|&p| p == rgba(0, 255, 0, 255)));
}

#[test]
fn every_minimum_code_size() {
    for bits in 1..=8u32 {
        let n = 1usize << bits;
        let colors: Vec<[u8; 3]> = (0..n)
            .map(|i| [(i as u8).wrapping_mul(3), i as u8, 255 - i as u8])
            .collect();
        let idx: Vec<u8> = (0..64).map(|i| (i % n) as u8).collect();
        let mut g = Gif::simple(8, 8, &colors, idx.clone());
        g.min = (bits as u8).max(2);
        let img = decode(&g.bytes()).unwrap();
        for (i, &v) in idx.iter().enumerate() {
            let c = colors[v as usize];
            assert_eq!(
                img.pixels()[i],
                rgba(c[0], c[1], c[2], 255),
                "bits {bits} pixel {i}"
            );
        }
    }
}

#[test]
fn interlaced_rows_land_where_they_belong() {
    let h = 11u16;
    // Row r is filled with colour r % 4, sent in interlace order.
    let colors = [RED, GREEN, BLUE, WHITE];
    let mut order: Vec<usize> = Vec::new();
    for (s, st) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        order.extend((s..h as usize).step_by(st));
    }
    let mut idx = Vec::new();
    for &r in &order {
        idx.extend(std::iter::repeat_n((r % 4) as u8, 3));
    }
    let mut g = Gif::simple(3, h, &colors, idx);
    g.interlace = true;
    let img = decode(&g.bytes()).unwrap();
    for r in 0..h as usize {
        let c = colors[r % 4];
        assert_eq!(px(&img, 1, r), rgba(c[0], c[1], c[2], 255), "row {r}");
    }
}

#[test]
fn a_local_palette_overrides_the_global_one_and_a_frame_can_be_offset() {
    let mut g = Gif::simple(4, 4, &[RED, GREEN, BLUE, WHITE], vec![0, 1, 1, 0]);
    g.frame = (1, 1, 2, 2);
    g.local = Some(vec![BLUE, WHITE]);
    let img = decode(&g.bytes()).unwrap();
    assert_eq!(px(&img, 0, 0) >> 24, 0, "outside the frame is transparent");
    assert_eq!(px(&img, 1, 1), rgba(0, 0, 255, 255));
    assert_eq!(px(&img, 2, 1), rgba(255, 255, 255, 255));
    assert_eq!(px(&img, 2, 2), rgba(0, 0, 255, 255));
    assert_eq!(px(&img, 3, 3) >> 24, 0);
}

#[test]
fn a_frame_hanging_off_the_screen_is_clipped() {
    let mut g = Gif::simple(3, 3, &[RED, GREEN, BLUE, WHITE], vec![1; 16]);
    g.frame = (2, 2, 4, 4);
    let img = decode(&g.bytes()).unwrap();
    assert_eq!((img.width(), img.height()), (3, 3));
    assert_eq!(px(&img, 2, 2), rgba(0, 255, 0, 255));
    assert_eq!(px(&img, 1, 1) >> 24, 0);
}

#[test]
fn the_transparent_index_is_clear() {
    let mut g = Gif::simple(2, 1, &[RED, GREEN, BLUE, WHITE], vec![0, 1]);
    g.transparent = Some(0);
    let img = decode(&g.bytes()).unwrap();
    assert_eq!(px(&img, 0, 0) >> 24, 0);
    assert_eq!(px(&img, 1, 0), rgba(0, 255, 0, 255));
}

#[test]
fn an_index_past_the_palette_is_black() {
    // Three colours in a four-entry table, index 3 is the padding (black from `pad`); use a
    // frame index beyond the table through a 2-colour palette with min code size 3.
    let mut g = Gif::simple(2, 1, &[RED, GREEN], vec![1, 5]);
    g.min = 3;
    let img = decode(&g.bytes()).unwrap();
    assert_eq!(px(&img, 0, 0), rgba(0, 255, 0, 255));
    assert_eq!(px(&img, 1, 0), rgba(0, 0, 0, 255));
}

#[test]
fn a_cut_off_file_gives_the_pixels_so_far() {
    let g = Gif::simple(
        40,
        40,
        &[RED, GREEN, BLUE, WHITE],
        (0..1600).map(|i| (i % 4) as u8).collect(),
    );
    let full = g.bytes();
    let mut shown = 0;
    for cut in (full.len() / 2..full.len() - 2).step_by(7) {
        // The cut file may lose the terminator and part of the data: never a panic.
        if let Ok(img) = decode(&full[..cut]) {
            assert_eq!(img.width(), 40);
            shown += 1;
        }
    }
    assert!(shown > 0, "at least some cuts still show a partial picture");
}

#[test]
fn errors() {
    assert_eq!(decode(b"").unwrap_err(), GifError::BadSignature);
    assert_eq!(decode(b"GIF90a").unwrap_err(), GifError::BadSignature);
    assert_eq!(decode(b"GIF89a").unwrap_err(), GifError::Truncated);
    assert_eq!(
        decode(b"GIF89a\0\0\0\0\0\0\0").unwrap_err(),
        GifError::BadSize
    );
    // Valid header and palette, then the trailer: no image.
    let mut v = b"GIF89a\x01\x00\x01\x00\x80\x00\x00".to_vec();
    v.extend_from_slice(&[0, 0, 0, 255, 255, 255, 0x3B]);
    assert_eq!(decode(&v).unwrap_err(), GifError::NoImage);
    // Unknown block introducer.
    let mut v = b"GIF89a\x01\x00\x01\x00\x80\x00\x00".to_vec();
    v.extend_from_slice(&[0, 0, 0, 255, 255, 255, 0x77]);
    assert_eq!(decode(&v).unwrap_err(), GifError::BadBlock(0x77));
    // No palette anywhere.
    let mut g = Gif::simple(2, 2, &[RED, GREEN], vec![0; 4]);
    g.global = None;
    assert_eq!(decode(&g.bytes()).unwrap_err(), GifError::NoPalette);
    // Bad LZW code size.
    let mut g = Gif::simple(2, 2, &[RED, GREEN], vec![0; 4]);
    let mut b = g.bytes();
    let at = b.len() - 1 - blocks(&lzw_encode(&g.idx, g.min)).len() - 1;
    b[at] = 9;
    assert_eq!(decode(&b).unwrap_err(), GifError::BadLzw);
    g.min = 2;
    // A frame with no data.
    let mut v = b"GIF89a\x01\x00\x01\x00\x80\x00\x00".to_vec();
    v.extend_from_slice(&[
        0, 0, 0, 255, 255, 255, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 0, 0x3B,
    ]);
    assert_eq!(decode(&v).unwrap_err(), GifError::Truncated);
}

#[test]
fn the_size_limit_is_checked_before_allocating() {
    // 65535 x 65535 would be 4 G pixels.
    let mut v = b"GIF89a".to_vec();
    v.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0x80, 0, 0]);
    v.extend_from_slice(&[
        0, 0, 0, 255, 255, 255, 0x2C, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0, 2, 2, 0x44, 1, 0, 0x3B,
    ]);
    assert_eq!(decode(&v).unwrap_err(), GifError::BadSize);
}

#[test]
fn hostile_lzw_never_overruns() {
    // A code from the future, a clear in the middle, only end codes, garbage: no panic, and the
    // output never exceeds the frame.
    for data in [
        vec![0xFFu8; 64],
        vec![0u8; 64],
        vec![0x04, 0x00, 0x00, 0x00],
        (0..255u8).collect::<Vec<_>>(),
        vec![
            0x8C, 0x2D, 0x99, 0x87, 0x2A, 0x1C, 0xDC, 0x33, 0xA0, 0x02, 0x75, 0xEC, 0x95, 0xFA,
        ],
    ] {
        for min in 2..=8u8 {
            let out = lzw(&data, min, 100);
            assert!(out.len() <= 100);
        }
    }
}

#[test]
fn arbitrary_mutations_of_a_valid_file_never_panic() {
    let g = Gif::simple(
        8,
        8,
        &[RED, GREEN, BLUE, WHITE],
        (0..64).map(|i| (i % 4) as u8).collect(),
    );
    let good = g.bytes();
    for i in 0..good.len() {
        for repl in [0u8, 1, 0x2C, 0x3B, 0x21, 0x80, 0xFF] {
            let mut m = good.clone();
            m[i] = repl;
            let _ = decode(&m);
        }
    }
}

#[test]
fn is_gif_checks_the_signature_only() {
    assert!(is_gif(b"GIF87a"));
    assert!(is_gif(b"GIF89a....."));
    assert!(!is_gif(b"GIF88a"));
    assert!(!is_gif(b"GIF8"));
}
