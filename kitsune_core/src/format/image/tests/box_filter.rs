use super::*;

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
