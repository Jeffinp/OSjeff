use super::*;

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
