use super::*;
use alloc::vec;
use alloc::vec::Vec;

const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: u32 = 0xFFFF_FFFF;
const BLACK: u32 = 0xFF00_0000;

/// A deterministic non-trivial image (opaque unless `alpha` is set).
fn pattern(w: usize, h: usize, alpha: bool) -> Image {
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let v = (x * 31 + y * 17 + x * y) as u32;
            let a = if alpha { (v * 7 % 256) as u8 } else { 255 };
            px.push(rgba(
                (v * 3) as u8,
                (v * 5 + 11) as u8,
                (v * 13 + 29) as u8,
                a,
            ));
        }
    }
    Image::from_pixels(w, h, px).unwrap()
}

fn img(w: usize, h: usize, px: &[u32]) -> Image {
    Image::from_pixels(w, h, px.to_vec()).unwrap()
}

// ---------------------------------------------------------------- basics

#[test]
fn rgba_and_channels_roundtrip() {
    assert_eq!(rgba(1, 2, 3, 4), 0x0401_0203);
    assert_eq!(channels(0x0401_0203), [1, 2, 3, 4]);
    assert_eq!(alpha(0x8000_0000), 0x80);
    for v in [0u32, 0xFFFF_FFFF, 0x1234_5678, 0x8000_0001] {
        let [r, g, b, a] = channels(v);
        assert_eq!(rgba(r, g, b, a), v);
    }
}

#[test]
fn new_image_is_filled() {
    let i = Image::new(3, 2, 0xAABB_CCDD).unwrap();
    assert_eq!((i.width(), i.height()), (3, 2));
    assert_eq!(i.pixels().len(), 6);
    assert!(i.pixels().iter().all(|&p| p == 0xAABB_CCDD));
    assert!(format!("{i:?}").contains("3x2"));
}

#[test]
fn zero_sizes_are_rejected() {
    assert_eq!(Image::new(0, 5, 0), Err(ImageError::ZeroSize));
    assert_eq!(Image::new(5, 0, 0), Err(ImageError::ZeroSize));
    assert_eq!(pixel_count(0, 0), Err(ImageError::ZeroSize));
}

#[test]
fn absurd_dimensions_are_rejected_without_allocating() {
    // These would need exabytes; the check must fire first (the test would
    // abort on allocation failure otherwise).
    assert_eq!(
        Image::new(usize::MAX, usize::MAX, 0),
        Err(ImageError::TooLarge)
    );
    assert_eq!(Image::new(usize::MAX, 2, 0), Err(ImageError::TooLarge));
    assert_eq!(Image::new(1 << 40, 1 << 40, 0), Err(ImageError::TooLarge));
    assert_eq!(Image::new(100_000, 100_000, 0), Err(ImageError::TooLarge));
    assert_eq!(pixel_count(65_536, 65_536), Err(ImageError::TooLarge));
}

#[test]
fn pixel_limit_is_exact() {
    assert_eq!(pixel_count(MAX_PIXELS, 1), Ok(MAX_PIXELS));
    assert_eq!(pixel_count(1, MAX_PIXELS), Ok(MAX_PIXELS));
    assert_eq!(pixel_count(4096, 4096), Ok(MAX_PIXELS));
    assert_eq!(pixel_count(MAX_PIXELS + 1, 1), Err(ImageError::TooLarge));
    assert_eq!(pixel_count(4097, 4096), Err(ImageError::TooLarge));
    assert_eq!(MAX_PIXELS, 16 * 1024 * 1024);
}

#[test]
fn from_pixels_checks_the_buffer_length() {
    assert_eq!(
        Image::from_pixels(2, 2, vec![0; 3]),
        Err(ImageError::BadBuffer)
    );
    assert_eq!(
        Image::from_pixels(2, 2, vec![0; 5]),
        Err(ImageError::BadBuffer)
    );
    assert!(Image::from_pixels(2, 2, vec![0; 4]).is_ok());
    assert_eq!(
        Image::from_pixels(usize::MAX, 3, vec![]),
        Err(ImageError::TooLarge)
    );
}

#[test]
fn rgba_bytes_roundtrip_and_validate() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
    let i = Image::from_rgba(2, 1, &bytes).unwrap();
    assert_eq!(i.pixels(), &[rgba(1, 2, 3, 4), rgba(5, 6, 7, 8)]);
    assert_eq!(i.to_rgba().unwrap(), bytes);
    assert_eq!(
        Image::from_rgba(2, 1, &bytes[..7]),
        Err(ImageError::BadBuffer)
    );
    assert_eq!(Image::from_rgba(0, 1, &[]), Err(ImageError::ZeroSize));
}

#[test]
fn get_set_and_rows_are_bounds_safe() {
    let mut i = Image::new(3, 2, 0).unwrap();
    assert!(i.set(2, 1, 7));
    assert_eq!(i.get(2, 1), Some(7));
    assert_eq!(i.get(3, 0), None);
    assert_eq!(i.get(0, 2), None);
    assert!(!i.set(3, 0, 1));
    assert!(!i.set(0, 2, 1));
    assert!(!i.set(usize::MAX, usize::MAX, 1));
    assert_eq!(i.row(1), &[0, 0, 7]);
    assert!(i.row(2).is_empty());
    assert!(i.row(usize::MAX).is_empty());
    assert!(i.row_mut(usize::MAX).is_empty());
    i.row_mut(0)[1] = 9;
    assert_eq!(i.pixels(), &[0, 9, 0, 0, 0, 7]);
    assert_eq!(i.clone().into_pixels().len(), 6);
    i.pixels_mut()[0] = 1;
    assert_eq!(i.get(0, 0), Some(1));
}

#[test]
fn opacity_detection() {
    assert!(Image::new(2, 2, BLACK).unwrap().is_opaque());
    let mut i = Image::new(2, 2, BLACK).unwrap();
    i.pixels_mut()[3] = 0xFE00_0000;
    assert!(!i.is_opaque());
}

// ---------------------------------------------------------------- crop

#[test]
fn crop_extracts_the_rectangle() {
    let i = img(3, 3, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(i.crop(1, 1, 2, 2).unwrap().pixels(), &[5, 6, 8, 9]);
    assert_eq!(i.crop(0, 0, 3, 3).unwrap(), i);
    assert_eq!(i.crop(2, 0, 1, 3).unwrap().pixels(), &[3, 6, 9]);
    assert_eq!(i.crop(0, 2, 3, 1).unwrap().pixels(), &[7, 8, 9]);
    let c = i.crop(1, 0, 1, 1).unwrap();
    assert_eq!((c.width(), c.height(), c.pixels()), (1, 1, &[2][..]));
}

#[test]
fn crop_rejects_bad_rectangles() {
    let i = img(3, 3, &[0; 9]);
    assert_eq!(i.crop(2, 0, 2, 1), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(0, 2, 1, 2), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(4, 0, 1, 1), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(0, 0, 0, 1), Err(ImageError::ZeroSize));
    assert_eq!(i.crop(usize::MAX, 0, 2, 1), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(1, usize::MAX, 1, 2), Err(ImageError::OutOfBounds));
    assert_eq!(i.crop(0, 0, usize::MAX, 1), Err(ImageError::TooLarge));
}

// ---------------------------------------------------------------- nearest

#[test]
fn nearest_upscale_replicates_pixels() {
    let i = img(2, 2, &[RED, GREEN, BLUE, WHITE]);
    let o = i.resize_nearest(4, 4).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(o.get(x, y), i.get(x / 2, y / 2), "({x},{y})");
        }
    }
}

#[test]
fn nearest_downscale_samples_centres() {
    let i = pattern(8, 8, false);
    let o = i.resize_nearest(4, 4).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            // Centre of destination pixel x maps to source x*2+1 (2x2 block, round up).
            assert_eq!(o.get(x, y), i.get(x * 2 + 1, y * 2 + 1));
        }
    }
}

#[test]
fn nearest_same_size_is_identity_and_keeps_alpha() {
    let i = pattern(7, 5, true);
    assert_eq!(i.resize_nearest(7, 5).unwrap(), i);
    let o = i.resize_nearest(13, 9).unwrap();
    assert_eq!(o.width(), 13);
    assert!(o.pixels().iter().all(|p| i.pixels().contains(p)));
}

#[test]
fn nearest_to_one_pixel_and_odd_ratios() {
    let i = pattern(5, 3, false);
    let o = i.resize_nearest(1, 1).unwrap();
    assert_eq!(o.get(0, 0), i.get(2, 1));
    let o = i.resize_nearest(7, 2).unwrap();
    assert_eq!((o.width(), o.height()), (7, 2));
}

// ---------------------------------------------------------------- bilinear

#[test]
fn bilinear_constant_image_stays_constant() {
    for &(sw, sh, dw, dh) in &[
        (1, 1, 5, 5),
        (3, 2, 10, 7),
        (10, 7, 3, 2),
        (4, 4, 4, 9),
        (9, 1, 2, 6),
        (64, 48, 100, 75),
    ] {
        for c in [
            0xFF12_3456u32,
            0x8000_00FF,
            0x00FF_FFFF,
            0xFFFF_FFFF,
            0x0102_0304,
        ] {
            let i = Image::new(sw, sh, c).unwrap();
            let o = i.resize_bilinear(dw, dh).unwrap();
            let want = if alpha(c) == 0 { 0 } else { c };
            assert!(
                o.pixels().iter().all(|&p| p == want || (alpha(c) == 0)),
                "{sw}x{sh}->{dw}x{dh} {c:#x}"
            );
        }
    }
}

#[test]
fn bilinear_two_by_two_to_four_by_four_known_values() {
    // Gray ramp 0 | 255 over 2 pixels, upscaled 2x: sample positions are
    // -0.25, 0.25, 0.75, 1.25 -> clamp, 64, 191, clamp (8-bit fractions).
    let i = img(2, 1, &[BLACK, WHITE]);
    let o = i.resize_bilinear(4, 1).unwrap();
    let g: Vec<u8> = o.pixels().iter().map(|&p| p as u8).collect();
    assert_eq!(g, [0, 64, 191, 255]);
}

#[test]
fn bilinear_is_symmetric_for_a_symmetric_input() {
    let i = img(4, 1, &[BLACK, WHITE, WHITE, BLACK]);
    let o = i.resize_bilinear(9, 1).unwrap();
    // Sample positions are quantised to 1/256 pixel, so mirror images agree to
    // within one level per channel.
    for x in 0..9 {
        let (a, b) = (
            channels(o.get(x, 0).unwrap()),
            channels(o.get(8 - x, 0).unwrap()),
        );
        for k in 0..4 {
            assert!((a[k] as i32 - b[k] as i32).abs() <= 1, "x {x} ch {k}");
        }
    }
}

#[test]
fn bilinear_same_size_is_identity() {
    let i = pattern(11, 6, true);
    assert_eq!(i.resize_bilinear(11, 6).unwrap(), i);
}

#[test]
fn bilinear_one_pixel_wide_and_tall() {
    let col = img(1, 3, &[BLACK, WHITE, BLACK]);
    let o = col.resize_bilinear(4, 3).unwrap();
    for y in 0..3 {
        for x in 0..4 {
            assert_eq!(o.get(x, y), col.get(0, y));
        }
    }
    let row = img(3, 1, &[RED, GREEN, BLUE]);
    let o = row.resize_bilinear(3, 5).unwrap();
    for x in 0..3 {
        for y in 0..5 {
            assert_eq!(o.get(x, y), row.get(x, 0));
        }
    }
}

#[test]
fn bilinear_channels_stay_in_range_of_inputs() {
    let i = pattern(9, 7, false);
    let o = i.resize_bilinear(31, 23).unwrap();
    let (mut lo, mut hi) = ([255u8; 3], [0u8; 3]);
    for &p in i.pixels() {
        let c = channels(p);
        for k in 0..3 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    for &p in o.pixels() {
        let c = channels(p);
        assert_eq!(c[3], 255);
        for k in 0..3 {
            assert!(c[k] >= lo[k] && c[k] <= hi[k]);
        }
    }
}

#[test]
fn bilinear_does_not_bleed_colour_from_transparent_pixels() {
    // Opaque red next to fully transparent *blue*: the result must stay red,
    // only its alpha changes.
    let i = img(2, 1, &[RED, 0x0000_00FF]);
    let o = i.resize_bilinear(8, 1).unwrap();
    for &p in o.pixels() {
        if alpha(p) != 0 {
            assert_eq!(channels(p)[..3], [255, 0, 0], "{p:#x}");
        }
    }
    let mid = o.get(4, 0).unwrap();
    assert!(alpha(mid) > 0 && alpha(mid) < 255);
    assert_eq!(o.get(0, 0), Some(RED));
}

#[test]
fn bilinear_alpha_interpolates_linearly() {
    let i = img(2, 1, &[0x00FF_FFFF, 0xFFFF_FFFF]);
    let o = i.resize_bilinear(4, 1).unwrap();
    let a: Vec<u8> = o.pixels().iter().map(|&p| alpha(p)).collect();
    assert_eq!(a, [0, 64, 191, 255]);
}

// ---------------------------------------------------------------- box

#[test]
fn box_uniform_image_is_preserved_for_any_ratio() {
    for &(sw, sh, dw, dh) in &[
        (8, 8, 3, 3),
        (10, 7, 3, 2),
        (100, 3, 7, 1),
        (5, 5, 5, 2),
        (3, 3, 7, 7),
        (1, 1, 4, 4),
    ] {
        let c = 0xFF30_90C0;
        let i = Image::new(sw, sh, c).unwrap();
        let o = i.resize_box(dw, dh).unwrap();
        assert!(o.pixels().iter().all(|&p| p == c), "{sw}x{sh}->{dw}x{dh}");
    }
}

#[test]
fn box_integer_ratio_is_the_block_mean() {
    // 4x4 -> 2x2: each output pixel is the mean of a 2x2 block.
    let mut px = Vec::new();
    for y in 0..4u32 {
        for x in 0..4u32 {
            let v = (x * 16 + y * 64) as u8;
            px.push(rgba(v, 255 - v, v / 2, 255));
        }
    }
    let i = img(4, 4, &px);
    let o = i.resize_box(2, 2).unwrap();
    for by in 0..2 {
        for bx in 0..2 {
            let mut sum = [0u32; 3];
            for dy in 0..2 {
                for dx in 0..2 {
                    let c = channels(i.get(bx * 2 + dx, by * 2 + dy).unwrap());
                    for k in 0..3 {
                        sum[k] += c[k] as u32;
                    }
                }
            }
            let got = channels(o.get(bx, by).unwrap());
            for k in 0..3 {
                let want = (sum[k] + 2) / 4; // rounded mean
                assert!(
                    (got[k] as i32 - want as i32).abs() <= 1,
                    "block ({bx},{by}) ch {k}: {} vs {want}",
                    got[k]
                );
            }
        }
    }
}

#[test]
fn box_to_one_pixel_is_the_global_mean() {
    let i = img(2, 2, &[0xFF00_0000, 0xFF64_6464, 0xFFC8_C8C8, 0xFF00_0000]);
    let o = i.resize_box(1, 1).unwrap();
    let want = (100 + 200) / 4;
    assert_eq!(channels(o.get(0, 0).unwrap())[0], want as u8);
}

#[test]
fn box_fractional_weights_are_exact() {
    // 3 -> 2 columns: dst0 = (1*s0 + 0.5*s1)/1.5, dst1 = (0.5*s1 + s2)/1.5.
    let i = img(3, 1, &[0xFF00_0000, 0xFF60_6060, 0xFFFF_FFFF]);
    let o = i.resize_box(2, 1).unwrap();
    // dst0 = 0.5*96/1.5 = 32 ; dst1 = (0.5*96 + 255)/1.5 = 202
    assert_eq!(channels(o.get(0, 0).unwrap())[0], 32);
    assert_eq!(channels(o.get(1, 0).unwrap())[0], 202);
}

#[test]
fn box_uses_premultiplied_alpha() {
    // Half the pixels opaque white, half transparent *black*: average stays
    // white with alpha ~ 128 (a naive channel average would give gray).
    let i = img(2, 2, &[WHITE, 0, 0, WHITE]);
    let o = i.resize_box(1, 1).unwrap();
    let c = channels(o.get(0, 0).unwrap());
    assert_eq!(c[..3], [255, 255, 255]);
    assert!((127..=129).contains(&c[3]), "alpha {}", c[3]);
    // Fully transparent area stays transparent black.
    let t = Image::new(4, 4, 0x0012_3456)
        .unwrap()
        .resize_box(2, 2)
        .unwrap();
    assert!(t.pixels().iter().all(|&p| p == 0));
}

#[test]
fn box_upscale_and_same_size() {
    let i = pattern(4, 3, true);
    assert_eq!(i.resize_box(4, 3).unwrap(), i);
    let o = i.resize_box(8, 6).unwrap();
    // An exact 2x enlargement replicates pixels (each dst covers half a src).
    for y in 0..6 {
        for x in 0..8 {
            let s = i.get(x / 2, y / 2).unwrap();
            let d = o.get(x, y).unwrap();
            assert!(
                (alpha(s) as i32 - alpha(d) as i32).abs() <= 1,
                "({x},{y}) alpha"
            );
            if alpha(s) == 0 {
                continue; // colour of a fully transparent pixel is not preserved
            }
            for k in 0..3 {
                assert!(
                    (channels(s)[k] as i32 - channels(d)[k] as i32).abs() <= 1,
                    "({x},{y}) ch {k}"
                );
            }
        }
    }
}

#[test]
fn box_streaming_handles_tall_and_wide_extremes() {
    let tall = pattern(2, 1000, false);
    let o = tall.resize_box(1, 7).unwrap();
    assert_eq!((o.width(), o.height()), (1, 7));
    let wide = pattern(1000, 2, true);
    let o = wide.resize_box(13, 1).unwrap();
    assert_eq!((o.width(), o.height()), (13, 1));
    let line = pattern(300, 1, false).resize_box(1, 1).unwrap();
    assert_eq!(line.pixels().len(), 1);
}

#[test]
fn resize_rejects_bad_targets() {
    let i = pattern(4, 4, false);
    for f in [Filter::Nearest, Filter::Bilinear, Filter::Box, Filter::Auto] {
        assert_eq!(i.resize(0, 4, f), Err(ImageError::ZeroSize));
        assert_eq!(i.resize(4, 0, f), Err(ImageError::ZeroSize));
        assert_eq!(i.resize(1 << 20, 1 << 20, f), Err(ImageError::TooLarge));
        assert_eq!(i.resize(usize::MAX, 2, f), Err(ImageError::TooLarge));
        assert_eq!(i.resize(4, 4, f).unwrap(), i);
    }
}

#[test]
fn auto_filter_picks_box_to_shrink_and_bilinear_to_grow() {
    let i = pattern(8, 8, false);
    assert_eq!(
        i.resize(4, 4, Filter::Auto).unwrap(),
        i.resize_box(4, 4).unwrap()
    );
    assert_eq!(
        i.resize(16, 16, Filter::Auto).unwrap(),
        i.resize_bilinear(16, 16).unwrap()
    );
    assert_eq!(
        i.resize(4, 16, Filter::Auto).unwrap(),
        i.resize_box(4, 16).unwrap()
    );
    assert_eq!(
        i.resize(8, 16, Filter::Nearest).unwrap(),
        i.resize_nearest(8, 16).unwrap()
    );
}

// ---------------------------------------------------------------- fit

#[test]
fn fit_dims_keeps_aspect_ratio() {
    let i = Image::new(1024, 768, 0).unwrap();
    assert_eq!(i.fit_dims(320, 240, false), Ok((320, 240)));
    assert_eq!(i.fit_dims(500, 100, false), Ok((133, 100)));
    assert_eq!(i.fit_dims(100, 500, false), Ok((100, 75)));
    assert_eq!(i.fit_dims(64, 64, false), Ok((64, 48)));
    let tall = Image::new(768, 1024, 0).unwrap();
    assert_eq!(tall.fit_dims(64, 64, false), Ok((48, 64)));
}

#[test]
fn fit_dims_only_upscales_on_request() {
    let i = Image::new(100, 50, 0).unwrap();
    assert_eq!(i.fit_dims(400, 400, false), Ok((100, 50)));
    assert_eq!(i.fit_dims(400, 400, true), Ok((400, 200)));
    assert_eq!(i.fit_dims(100, 50, false), Ok((100, 50)));
    // Too big in one dimension only: shrinks.
    assert_eq!(i.fit_dims(50, 400, false), Ok((50, 25)));
}

#[test]
fn fit_dims_extreme_aspect_ratios_stay_at_least_one() {
    let line = Image::new(1000, 1, 0).unwrap();
    assert_eq!(line.fit_dims(10, 10, false), Ok((10, 1)));
    let col = Image::new(1, 1000, 0).unwrap();
    assert_eq!(col.fit_dims(10, 10, false), Ok((1, 10)));
    assert_eq!(line.fit_dims(0, 10, false), Err(ImageError::ZeroSize));
    assert_eq!(line.fit_dims(10, 0, true), Err(ImageError::ZeroSize));
}

#[test]
fn fit_dims_never_exceeds_the_box() {
    for (sw, sh) in [
        (1, 1),
        (3, 7),
        (640, 480),
        (1920, 1080),
        (17, 1000),
        (999, 2),
    ] {
        let i = Image::new(sw, sh, 0).unwrap();
        for (bw, bh) in [(1, 1), (2, 5), (64, 64), (200, 100), (1000, 3), (7, 999)] {
            let (w, h) = i.fit_dims(bw, bh, true).unwrap();
            assert!(
                w >= 1 && h >= 1 && w <= bw && h <= bh,
                "{sw}x{sh} in {bw}x{bh} -> {w}x{h}"
            );
            // One of the two dimensions touches the box.
            assert!(w == bw || h == bh, "{sw}x{sh} in {bw}x{bh} -> {w}x{h}");
        }
    }
}

#[test]
fn fit_resamples_to_the_fitted_size() {
    let i = pattern(40, 30, false);
    let t = i.fit(16, 16, false, Filter::Auto).unwrap();
    assert_eq!((t.width(), t.height()), (16, 12));
    let same = i.fit(100, 100, false, Filter::Auto).unwrap();
    assert_eq!(same, i);
    let big = i.fit(80, 80, true, Filter::Bilinear).unwrap();
    assert_eq!((big.width(), big.height()), (80, 60));
}

// ---------------------------------------------------------------- orientation

#[test]
fn rotate90_known_values() {
    // 1 2 3        4 1
    // 4 5 6   ->   5 2
    //              6 3
    let i = img(3, 2, &[1, 2, 3, 4, 5, 6]);
    let r = i.rotate90().unwrap();
    assert_eq!((r.width(), r.height()), (2, 3));
    assert_eq!(r.pixels(), &[4, 1, 5, 2, 6, 3]);
}

#[test]
fn rotate270_known_values() {
    let i = img(3, 2, &[1, 2, 3, 4, 5, 6]);
    let r = i.rotate270().unwrap();
    assert_eq!((r.width(), r.height()), (2, 3));
    assert_eq!(r.pixels(), &[3, 6, 2, 5, 1, 4]);
}

#[test]
fn four_quarter_turns_are_the_identity() {
    let i = pattern(7, 4, true);
    let mut r = i.clone();
    for _ in 0..4 {
        r = r.rotate90().unwrap();
    }
    assert_eq!(r, i);
    let mut l = i.clone();
    for _ in 0..4 {
        l = l.rotate270().unwrap();
    }
    assert_eq!(l, i);
    assert_eq!(i.rotate90().unwrap().rotate270().unwrap(), i);
}

#[test]
fn rotate180_equals_two_quarter_turns_and_two_flips() {
    let i = pattern(5, 3, false);
    let mut a = i.clone();
    a.rotate180();
    assert_eq!(a, i.rotate90().unwrap().rotate90().unwrap());
    let mut b = i.clone();
    b.flip_horizontal();
    b.flip_vertical();
    assert_eq!(a, b);
    a.rotate180();
    assert_eq!(a, i);
}

#[test]
fn flips_are_involutions_with_known_values() {
    let i = img(3, 2, &[1, 2, 3, 4, 5, 6]);
    let mut h = i.clone();
    h.flip_horizontal();
    assert_eq!(h.pixels(), &[3, 2, 1, 6, 5, 4]);
    let mut v = i.clone();
    v.flip_vertical();
    assert_eq!(v.pixels(), &[4, 5, 6, 1, 2, 3]);
    h.flip_horizontal();
    v.flip_vertical();
    assert_eq!(h, i);
    assert_eq!(v, i);
}

#[test]
fn flip_vertical_handles_odd_and_single_rows() {
    let mut i = img(2, 3, &[1, 2, 3, 4, 5, 6]);
    i.flip_vertical();
    assert_eq!(i.pixels(), &[5, 6, 3, 4, 1, 2]);
    let mut one = img(3, 1, &[1, 2, 3]);
    one.flip_vertical();
    assert_eq!(one.pixels(), &[1, 2, 3]);
    let mut col = img(1, 4, &[1, 2, 3, 4]);
    col.flip_vertical();
    assert_eq!(col.pixels(), &[4, 3, 2, 1]);
    col.flip_horizontal();
    assert_eq!(col.pixels(), &[4, 3, 2, 1]);
}

#[test]
fn rotating_a_single_pixel_or_line() {
    let p = img(1, 1, &[42]);
    assert_eq!(p.rotate90().unwrap(), p);
    assert_eq!(p.rotate270().unwrap(), p);
    let row = img(4, 1, &[1, 2, 3, 4]);
    let r = row.rotate90().unwrap();
    assert_eq!(
        (r.width(), r.height(), r.pixels()),
        (1, 4, &[1, 2, 3, 4][..])
    );
    let l = row.rotate270().unwrap();
    assert_eq!(l.pixels(), &[4, 3, 2, 1]);
}

// ---------------------------------------------------------------- compositing

#[test]
fn over_fast_paths() {
    assert_eq!(over(RED, BLUE), RED); // opaque source wins
    assert_eq!(over(0x00FF_FFFF, BLUE), BLUE); // transparent source
    assert_eq!(over(0x0000_0000, 0), 0);
}

#[test]
fn over_half_alpha_on_opaque() {
    let o = over(0x80FF_0000, BLACK);
    assert_eq!(alpha(o), 255);
    assert_eq!(channels(o)[0], 128);
    let o = over(0x80FF_FFFF, BLACK);
    assert_eq!(channels(o)[..3], [128, 128, 128]);
    // 255 - a of the destination plus a of the source, rounded.
    let o = over(0x4000_00FF, WHITE);
    assert_eq!(channels(o), [191, 191, 255, 255]);
}

#[test]
fn over_onto_transparent_destination_keeps_source_colour() {
    let o = over(0x80C8_6414, 0x0000_0000);
    assert_eq!(channels(o), [200, 100, 20, 128]);
    // Two half-transparent layers: alpha 0.5 + 0.5*0.5 = 0.75.
    let o = over(0x80FF_0000, 0x800000FF);
    assert!((190..=192).contains(&alpha(o)), "alpha {}", alpha(o));
    let [r, _, b, _] = channels(o);
    assert!(r > b); // the top layer dominates
}

#[test]
fn over_never_overflows_a_channel() {
    for sa in (0..=255u32).step_by(15) {
        for da in (0..=255u32).step_by(15) {
            for c in [0u8, 1, 127, 128, 254, 255] {
                let s = rgba(c, c, c, sa as u8);
                let d = rgba(255 - c, c, 255, da as u8);
                let o = over(s, d);
                let [r, g, b, a] = channels(o);
                // Result colour lies between the two inputs per channel.
                let lo = |x: u8, y: u8| x.min(y);
                let hi = |x: u8, y: u8| x.max(y);
                if a > 0 && sa > 0 && da > 0 {
                    assert!(
                        r >= lo(c, 255 - c).saturating_sub(1)
                            && r <= hi(c, 255 - c).saturating_add(1)
                    );
                    assert!(g >= c.saturating_sub(1) && g <= c.saturating_add(1));
                    assert!(b >= lo(c, 255).saturating_sub(1));
                }
            }
        }
    }
}

#[test]
fn flatten_composites_over_a_background() {
    let mut i = img(3, 1, &[RED, 0x0000_FF00, 0x80FF_FFFF]);
    i.flatten(0x0000_00FF); // background alpha is ignored: treated as opaque
    assert_eq!(i.pixels()[0], RED);
    assert_eq!(i.pixels()[1], BLUE);
    let [r, g, b, a] = channels(i.pixels()[2]);
    assert_eq!(a, 255);
    assert_eq!((r, g), (128, 128));
    assert_eq!(b, 255);
    assert!(i.is_opaque());
}

#[test]
fn blit_over_clips_on_every_side() {
    let mut dst = Image::new(4, 4, BLACK).unwrap();
    let src = Image::new(2, 2, WHITE).unwrap();
    dst.blit_over(&src, 1, 1);
    assert_eq!(dst.get(1, 1), Some(WHITE));
    assert_eq!(dst.get(2, 2), Some(WHITE));
    assert_eq!(dst.get(0, 0), Some(BLACK));
    assert_eq!(dst.get(3, 3), Some(BLACK));
    // Hanging off the top-left corner: only the bottom-right source pixel lands.
    let mut d = Image::new(4, 4, BLACK).unwrap();
    d.blit_over(&src, -1, -1);
    assert_eq!(d.get(0, 0), Some(WHITE));
    assert_eq!(d.get(1, 0), Some(BLACK));
    // Off the bottom-right.
    let mut d = Image::new(4, 4, BLACK).unwrap();
    d.blit_over(&src, 3, 3);
    assert_eq!(d.get(3, 3), Some(WHITE));
    assert_eq!(d.get(2, 3), Some(BLACK));
    // Completely outside, including extreme offsets.
    let before = d.clone();
    d.blit_over(&src, 4, 0);
    d.blit_over(&src, 0, 4);
    d.blit_over(&src, -2, 0);
    d.blit_over(&src, i32::MIN, i32::MIN);
    d.blit_over(&src, i32::MAX, i32::MAX);
    assert_eq!(d, before);
}

#[test]
fn blit_over_blends_alpha() {
    let mut dst = Image::new(2, 1, BLACK).unwrap();
    let src = img(2, 1, &[0x80FF_FFFF, 0x0000_0000]);
    dst.blit_over(&src, 0, 0);
    assert_eq!(channels(dst.get(0, 0).unwrap())[0], 128);
    assert_eq!(dst.get(1, 0), Some(BLACK));
}

#[test]
fn blit_larger_source_into_smaller_destination() {
    let mut dst = Image::new(2, 2, BLACK).unwrap();
    let src = Image::new(10, 10, RED).unwrap();
    dst.blit_over(&src, -3, -3);
    assert!(dst.pixels().iter().all(|&p| p == RED));
}

#[test]
fn error_messages_are_nonempty() {
    use alloc::string::ToString;
    for e in [
        ImageError::ZeroSize,
        ImageError::TooLarge,
        ImageError::BadBuffer,
        ImageError::OutOfBounds,
        ImageError::OutOfMemory,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

// ---------------------------------------------------------------- format detection / decode

mod detect_decode {
    use super::super::*;
    use crate::format::bmp::BmpError;
    use crate::format::png::PngError;
    use crate::format::ppm::PpmError;
    use alloc::vec;

    fn sample() -> Image {
        let mut px = Vec::new();
        for y in 0..6usize {
            for x in 0..7usize {
                px.push(rgba((x * 30) as u8, (y * 40) as u8, (x * y) as u8, 255));
            }
        }
        Image::from_pixels(7, 6, px).unwrap()
    }

    #[test]
    fn detects_each_format_by_signature() {
        assert_eq!(detect(b"\x89PNG\r\n\x1a\n...."), Some(Format::Png));
        assert_eq!(detect(b"BM\x00\x00"), Some(Format::Bmp));
        assert_eq!(detect(b"P6\n1 1\n255\n"), Some(Format::Ppm));
        assert_eq!(detect(b"P3 1 1 255 0 0 0"), Some(Format::Ppm));
        assert_eq!(detect(b"P3#c\n"), Some(Format::Ppm));
        assert_eq!(detect(b"P6\t"), Some(Format::Ppm));
        assert_eq!(Format::Png.name(), "png");
        assert_eq!(Format::Bmp.name(), "bmp");
        assert_eq!(Format::Ppm.name(), "ppm");
    }

    #[test]
    fn rejects_unknown_and_short_signatures() {
        for d in [
            &b""[..],
            b"B",
            b"P",
            b"P6",
            b"P3",
            b"P1 1 1\n0",
            b"P5\n",
            b"P6x",
            b"GIF89a",
            b"\xFF\xD8\xFF\xE0",
            b"RIFF....WEBP",
            b"\x89PN",
            b"bm",
            b"\0\0\0\0",
        ] {
            assert_eq!(detect(d), None, "{d:?}");
            assert_eq!(decode(d), Err(DecodeError::UnknownFormat), "{d:?}");
        }
    }

    #[test]
    fn decode_dispatches_to_the_right_decoder() {
        let img = sample();
        for fmt in [Format::Png, Format::Bmp, Format::Ppm] {
            let bytes = encode(&img, fmt).unwrap();
            assert_eq!(detect(&bytes), Some(fmt));
            assert_eq!(decode(&bytes).unwrap(), img, "{fmt:?}");
        }
    }

    #[test]
    fn encode_picks_sensible_variants() {
        let opaque = sample();
        let png = encode(&opaque, Format::Png).unwrap();
        assert_eq!(
            crate::format::png::read_header(&png).unwrap().color_type,
            crate::format::png::ColorType::Rgb
        );
        let bmp = encode(&opaque, Format::Bmp).unwrap();
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 24);
        let mut alpha = sample();
        alpha.set(0, 0, 0x4000_00FF);
        let png = encode(&alpha, Format::Png).unwrap();
        assert_eq!(
            crate::format::png::read_header(&png).unwrap().color_type,
            crate::format::png::ColorType::Rgba
        );
        let bmp = encode(&alpha, Format::Bmp).unwrap();
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 32);
        assert_eq!(decode(&bmp).unwrap(), alpha);
        // PPM has no alpha: it is flattened over black.
        let ppm = encode(&alpha, Format::Ppm).unwrap();
        let back = decode(&ppm).unwrap();
        assert_eq!(back.get(0, 0), Some(rgba(0, 0, 64, 255)));
    }

    #[test]
    fn errors_are_typed_per_format() {
        assert_eq!(
            decode(b"\x89PNG\r\n\x1a\n"),
            Err(DecodeError::Png(PngError::Truncated))
        );
        assert_eq!(
            decode(b"\x89PNG\x00\x00\x00\x00"),
            Err(DecodeError::Png(PngError::BadSignature))
        );
        assert_eq!(decode(b"BM"), Err(DecodeError::Bmp(BmpError::Truncated)));
        assert_eq!(
            decode(b"P6 1 1 255\n\x01"),
            Err(DecodeError::Ppm(PpmError::Truncated))
        );
        assert_eq!(
            decode(b"P3 0 0 255\n"),
            Err(DecodeError::Ppm(PpmError::BadHeader))
        );
    }

    #[test]
    fn from_impls_wrap_the_module_errors() {
        assert_eq!(
            DecodeError::from(PngError::BadChunk),
            DecodeError::Png(PngError::BadChunk)
        );
        assert_eq!(
            DecodeError::from(BmpError::BadHeader),
            DecodeError::Bmp(BmpError::BadHeader)
        );
        assert_eq!(
            DecodeError::from(PpmError::BadSample),
            DecodeError::Ppm(PpmError::BadSample)
        );
    }

    #[test]
    fn decode_messages_name_the_format() {
        use alloc::string::ToString;
        assert!(DecodeError::UnknownFormat.to_string().contains("unknown"));
        assert!(
            DecodeError::Png(PngError::BadChunk)
                .to_string()
                .starts_with("png:")
        );
        assert!(
            DecodeError::Bmp(BmpError::BadHeader)
                .to_string()
                .starts_with("bmp:")
        );
        assert!(
            DecodeError::Ppm(PpmError::BadSample)
                .to_string()
                .starts_with("ppm:")
        );
    }

    #[test]
    fn decoded_images_respect_the_pixel_limit_in_every_format() {
        // Headers that claim 100000 x 100000 pixels, one per format.
        let mut png = SIGNATURE_PNG.to_vec();
        let mut ihdr = vec![];
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut chunk = (ihdr.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"IHDR");
        chunk.extend_from_slice(&ihdr);
        let mut crc = crate::format::inflate::Crc32::new();
        crc.update(b"IHDR");
        crc.update(&ihdr);
        chunk.extend_from_slice(&crc.finish().to_be_bytes());
        png.extend(chunk);
        assert_eq!(
            decode(&png),
            Err(DecodeError::Png(PngError::Image(ImageError::TooLarge)))
        );
        assert_eq!(
            decode(b"P6 100000 100000 255\n"),
            Err(DecodeError::Ppm(PpmError::Image(ImageError::TooLarge)))
        );
        let mut bmp = crate::format::bmp::encode_24(&sample(), 0).unwrap();
        bmp[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&100_000i32.to_le_bytes());
        assert_eq!(
            decode(&bmp),
            Err(DecodeError::Bmp(BmpError::Image(ImageError::TooLarge)))
        );
    }

    const SIGNATURE_PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn decode_then_resize_pipeline() {
        // What the viewer does: decode, fit to a thumbnail box, rotate.
        let bytes = encode(&sample(), Format::Png).unwrap();
        let img = decode(&bytes).unwrap();
        let thumb = img.fit(4, 4, false, Filter::Auto).unwrap();
        assert!(thumb.width() <= 4 && thumb.height() <= 4);
        let rot = thumb.rotate90().unwrap();
        assert_eq!((rot.width(), rot.height()), (thumb.height(), thumb.width()));
    }

    #[test]
    fn decode_never_panics_on_garbage_with_valid_signatures() {
        let mut x = 0xDEAD_BEEFu32;
        for sig in [&b"\x89PNG\r\n\x1a\n"[..], b"BM", b"P6 ", b"P3 "] {
            for round in 0..150 {
                let mut d = sig.to_vec();
                for _ in 0..round {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    d.push(x as u8);
                }
                let _ = decode(&d);
            }
        }
    }
}
