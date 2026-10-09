use super::*;

fn masks() -> CornerMasks {
    CornerMasks::new()
}

#[test]
fn premultiply_roundtrip_and_over_identities() {
    let c = argb(128, 200, 100, 50);
    let p = premul(c);
    assert_eq!(p >> 24, 128);
    let u = unpremul(p);
    for sh in [16, 8, 0] {
        assert!(((u >> sh) & 0xFF).abs_diff((c >> sh) & 0xFF) <= 2);
    }
    let dst = premul(rgb(0x336699));
    assert_eq!(over(dst, 0), dst);
    assert_eq!(over(dst, rgb(0xFF0000)), rgb(0xFF0000));
    // Half white over black is mid grey.
    let m = over(premul(rgb(0)), premul(rgba(0xFFFFFF, 128)));
    assert!(((m >> 16) & 0xFF).abs_diff(128) <= 1);
    assert!((m >> 24) >= 254);
}

#[test]
fn corner_masks_are_monotonic_and_symmetric_in_xy() {
    let m = masks();
    for style in [Corner::Circle, Corner::Squircle] {
        for r in 1..=MAX_RADIUS {
            let k = m.get(style, r);
            assert_eq!(k.len(), r * r);
            // Fully outside at the very corner, fully inside next to the straight edges.
            if r >= 6 {
                assert!(k[0] < 40, "r={r} corner {}", k[0]);
            }
            if r >= 6 {
                assert!(k[r * r - 1] > 215, "r={r} centre {}", k[r * r - 1]);
            }
            for y in 0..r {
                for x in 0..r {
                    let a = k[y * r + x] as i32;
                    let b = k[x * r + y] as i32;
                    assert!((a - b).abs() <= 2, "r={r} ({x},{y}) {a} {b}");
                    // Coverage never decreases going inwards.
                    if x + 1 < r {
                        assert!(k[y * r + x + 1] as i32 >= a - 2);
                    }
                    if y + 1 < r {
                        assert!(k[(y + 1) * r + x] as i32 >= a - 2);
                    }
                }
            }
        }
    }
    assert!(m.get(Corner::Circle, 0).is_empty());
}

#[test]
fn circle_mask_area_matches_pi_r_squared() {
    let m = masks();
    for r in [4usize, 8, 12, 20, 40] {
        let sum: u32 = m.get(Corner::Circle, r).iter().map(|&v| v as u32).sum();
        let area = sum as f64 / 255.0;
        // The mask is the quarter inside the circle of radius r.
        let want = core::f64::consts::PI * (r * r) as f64 / 4.0;
        assert!((area - want).abs() / want < 0.02, "r={r} {area} {want}");
    }
}

#[test]
fn squircle_is_squarer_than_the_circle() {
    let m = masks();
    let c: u32 = m.get(Corner::Circle, 24).iter().map(|&v| v as u32).sum();
    let s: u32 = m.get(Corner::Squircle, 24).iter().map(|&v| v as u32).sum();
    assert!(s > c + c / 20, "squircle {s} circle {c}");
}

#[test]
fn rrect_fill_is_solid_inside_and_clear_outside() {
    let m = masks();
    let mut s = Surface::new(40, 30);
    s.fill_rrect(
        4,
        4,
        32,
        22,
        8,
        Corner::Circle,
        Paint::Solid(rgb(0x2080FF)),
        &m,
    );
    assert_eq!(s.get(20, 15), rgb(0x2080FF));
    assert_eq!(s.get(0, 0), 0);
    assert!(s.get(4, 4) >> 24 < 60);
    assert_eq!(s.get(3, 15), 0);
    assert_eq!(s.get(20, 4), rgb(0x2080FF));
    // Partially covered corner pixels exist.
    let mid =
        s.px.iter()
            .filter(|&&p| (p >> 24) > 20 && (p >> 24) < 235)
            .count();
    assert!(mid >= 8, "{mid}");
}

#[test]
fn vertical_gradient_interpolates() {
    let m = masks();
    let mut s = Surface::new(8, 100);
    s.fill_rrect(
        0,
        0,
        8,
        100,
        0,
        Corner::Circle,
        Paint::Vertical(rgb(0), rgb(0xFFFFFF)),
        &m,
    );
    let top = (s.get(4, 0) >> 16) & 0xFF;
    let mid = (s.get(4, 50) >> 16) & 0xFF;
    let bot = (s.get(4, 99) >> 16) & 0xFF;
    assert!(
        top < 5 && (120..136).contains(&mid) && bot > 250,
        "{top} {mid} {bot}"
    );
}

#[test]
fn stroke_is_a_ring() {
    let m = masks();
    let mut s = Surface::new(40, 40);
    s.stroke_rrect(
        0,
        0,
        40,
        40,
        10,
        1,
        Corner::Circle,
        Paint::Solid(rgb(0xFF0000)),
        &m,
    );
    assert_eq!(s.get(20, 0), rgb(0xFF0000));
    assert_eq!(s.get(0, 20), rgb(0xFF0000));
    assert_eq!(s.get(20, 20), 0);
    assert_eq!(s.get(20, 1), 0);
    // The ring follows the rounded corner: the corner pixel itself is empty.
    assert!(s.get(0, 0) >> 24 < 40);
}

#[test]
fn path_circle_area_and_polygon_orientation() {
    let mut s = Surface::new(64, 64);
    let mut p = Path::new();
    p.ellipse(32 * 256, 32 * 256, 20 * 256, 20 * 256);
    s.fill_path(&p, Paint::Solid(rgb(0xFFFFFF)));
    let area: u32 = s.px.iter().map(|&p| p >> 24).sum::<u32>() / 255;
    let want = (core::f64::consts::PI * 400.0) as u32;
    assert!(area.abs_diff(want) < 14, "{area} {want}");
    assert_eq!(s.get(32, 32), rgb(0xFFFFFF));
    assert_eq!(s.get(2, 2), 0);
}

#[test]
fn blur_keeps_constant_images_and_total_energy() {
    let mut s = Surface::new(32, 32);
    s.fill(rgb(0x808080));
    s.blur(5, 3);
    assert!(s.px.iter().all(|&p| p == rgb(0x808080)));
    // A dot in the middle spreads out but the energy stays (away from the edges).
    let mut d = Surface::new(64, 64);
    d.px[32 * 64 + 32] = 255 << 24;
    let before: u32 = d.px.iter().map(|&p| p >> 24).sum();
    d.blur(3, 2);
    let after: u32 = d.px.iter().map(|&p| p >> 24).sum();
    assert!(before.abs_diff(after) < 40, "{before} {after}");
    assert!(d.px[32 * 64 + 32] >> 24 < 60);
    assert!(d.px[32 * 64 + 33] >> 24 > 0);
}

#[test]
fn resize_preserves_flat_colour_and_averages() {
    let mut s = Surface::new(16, 16);
    s.fill(rgb(0x204060));
    for (w, h) in [(8, 8), (5, 7), (16, 16), (40, 40), (3, 30)] {
        let r = s.resized(w, h);
        assert!(r.px.iter().all(|&p| p == rgb(0x204060)), "{w}x{h}");
    }
    // 2x2 checkerboard of black/white averages to grey when halved to 1x1.
    let mut c = Surface::new(2, 2);
    c.px = vec![rgb(0), rgb(0xFFFFFF), rgb(0xFFFFFF), rgb(0)];
    let one = c.resized(1, 1);
    assert!(((one.px[0] >> 16) & 0xFF).abs_diff(128) <= 1);
    // Upscaling a gradient stays monotonic.
    let mut g = Surface::new(4, 1);
    g.px = vec![rgb(0), rgb(0x404040), rgb(0x808080), rgb(0xFFFFFF)];
    let up = g.resized(16, 1);
    let ch: Vec<u32> = up.px.iter().map(|p| (p >> 16) & 0xFF).collect();
    assert!(ch.windows(2).all(|w| w[0] <= w[1]), "{ch:?}");
}

#[test]
fn shadow_profile_is_a_soft_box() {
    let p = shadow_profile(100, 12);
    assert_eq!(p.len(), 124);
    assert_eq!(p[0], 0);
    assert!(p[12 + 50] >= 254);
    // Symmetric and monotonic up to the middle.
    for i in 0..p.len() {
        assert!(p[i].abs_diff(p[p.len() - 1 - i]) <= 3, "{i}");
    }
    assert!(p[..62].windows(2).all(|w| w[0] <= w[1]));
    // Half intensity at the edge.
    assert!(p[12].abs_diff(127) < 12, "{}", p[12]);
    // A segment shorter than the blur never reaches full opacity.
    let q = shadow_profile(4, 12);
    assert!(*q.iter().max().unwrap() < 200);
    assert_eq!(shadow_profile(10, 0)[..10], [255u8; 10]);
    assert_eq!(shadow_alpha(255, 255, 256), (255 * 255 * 256) >> 16);
}

#[test]
fn drop_shadow_darkens_below_the_shape() {
    let m = masks();
    let mut s = Surface::new(32, 32);
    s.fill_rrect(
        0,
        0,
        32,
        32,
        8,
        Corner::Circle,
        Paint::Solid(rgb(0xFFFFFF)),
        &m,
    );
    let sh = s.with_shadow(12, 4, 4, 160);
    assert_eq!((sh.w, sh.h), (56, 56));
    // Below the tile (inside the margin) there is faint shadow, above less.
    let below = sh.get(28, 12 + 32 + 4) >> 24;
    let above = sh.get(28, 6) >> 24;
    assert!(below > above, "{below} {above}");
    assert_eq!(sh.get(28, 28), rgb(0xFFFFFF));
}

#[test]
fn glow_lut_falls_off() {
    let t = glow_lut();
    assert!(t[0] >= 254 && t[255] == 0);
    assert!(t.windows(2).all(|w| w[0] >= w[1]));
}

#[test]
fn mask_rrect_rounds_the_corners() {
    let m = masks();
    let mut s = Surface::new(24, 24);
    s.fill(rgb(0xFF0000));
    s.mask_rrect(8, Corner::Circle, &m);
    assert!(s.get(0, 0) >> 24 < 40);
    assert_eq!(s.get(12, 12), rgb(0xFF0000));
    assert_eq!(s.get(12, 0), rgb(0xFF0000));
}
