use super::*;
use crate::ui::raster::{premul, unpremul};

const SIZES: [usize; 6] = [16, 24, 32, 48, 64, 128];
const S: Scheme = Scheme::KITSUNE;

/// Twice the signed area of a polygon (positive = clockwise on a y-down grid).
fn area2(pts: &[(i32, i32)]) -> i64 {
    let mut a = 0i64;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.0 as i64 * q.1 as i64 - q.0 as i64 * p.1 as i64;
    }
    a
}

fn sorted(pts: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut v = pts.to_vec();
    v.sort();
    v
}

fn marks() -> Vec<(&'static str, Mark)> {
    alloc::vec![
        ("tile", tile(&S)),
        ("mono", mono(0xFFFFFF)),
        ("halo", halo(&S)),
        ("halo9", halo_n(&S, 9)),
    ]
}

#[test]
fn head_has_a_dozen_polygons_or_fewer() {
    let n = fox_head(&S).len();
    assert!((6..=12).contains(&n), "{n} polygons");
    assert_eq!(tile(&S).polys.len(), n);
}

#[test]
fn polygons_stay_inside_the_grid_and_are_not_degenerate() {
    for (name, m) in marks() {
        for p in &m.polys {
            assert!(p.pts.len() >= 3, "{name}: {:?}", p.role);
            for &(x, y) in &p.pts {
                assert!(
                    (0..=EXTENT).contains(&x) && (0..=EXTENT).contains(&y),
                    "{name} {:?}: ({x},{y}) outside the grid",
                    p.role
                );
            }
            for i in 0..p.pts.len() {
                assert_ne!(
                    p.pts[i],
                    p.pts[(i + 1) % p.pts.len()],
                    "{name} {:?}: repeated corner",
                    p.role
                );
            }
            // Not a sliver: at least 1/4 of a grid unit squared of area.
            assert!(
                area2(&p.pts).abs() >= (UNIT * UNIT) as i64 * 8,
                "{name} {:?}: area {}",
                p.role,
                area2(&p.pts)
            );
        }
    }
}

#[test]
fn winding_is_clockwise_and_knockouts_are_counter_clockwise() {
    for (name, m) in marks() {
        for p in &m.polys {
            let a = area2(&p.pts);
            if p.role == Role::Knockout {
                assert!(a < 0, "{name}: knockout winds clockwise");
            } else {
                assert!(a > 0, "{name} {:?}: winds counter-clockwise", p.role);
            }
        }
    }
}

#[test]
fn the_head_is_mirror_symmetric() {
    let head = fox_head(&S);
    for p in &head {
        let mirror = sorted(
            &p.pts
                .iter()
                .map(|&(x, y)| (EXTENT - x, y))
                .collect::<Vec<_>>(),
        );
        let twin = head
            .iter()
            .find(|q| sorted(&q.pts) == mirror)
            .unwrap_or_else(|| panic!("{:?} has no mirror image", p.role));
        // The two halves of the face differ in colour on purpose; everything else matches.
        if !matches!(p.role, Role::LightFace | Role::ShadeFace) {
            assert_eq!(twin.color, p.color, "{:?}", p.role);
        }
    }
}

#[test]
fn the_silhouette_is_symmetric() {
    let m = mono(0xFFFFFF);
    let outline = &m.polys[0];
    assert_eq!(outline.role, Role::Mark);
    let mirror = sorted(
        &outline
            .pts
            .iter()
            .map(|&(x, y)| (EXTENT - x, y))
            .collect::<Vec<_>>(),
    );
    assert_eq!(sorted(&outline.pts), mirror);
    // The rendered silhouette is symmetric to within the anti-aliasing of the half pixel.
    for px in SIZES {
        let s = render(&m, px);
        for y in 0..px {
            for x in 0..px / 2 {
                let (a, b) = (
                    (s.get(x, y) >> 24) as i32,
                    (s.get(px - 1 - x, y) >> 24) as i32,
                );
                assert!((a - b).abs() <= 12, "{px}px ({x},{y}): {a} vs {b}");
            }
        }
    }
}

#[test]
fn mono_is_a_single_colour() {
    let m = mono(0x336699);
    assert!(m.tile.is_none());
    assert!(m.polys.iter().all(|p| p.color == 0x336699));
    assert_eq!(m.polys.len(), 4, "outline + two eyes + nose");
    for px in SIZES {
        let s = render(&m, px);
        let mut seen = 0;
        for &p in &s.px {
            if p >> 24 == 0 {
                continue;
            }
            seen += 1;
            let c = unpremul(p);
            // Straight colour is the same everywhere (up to the rounding of premultiplying).
            let want = premul((c & 0xFF00_0000) | 0x336699);
            let d =
                |a: u32, b: u32, sh: u32| ((a >> sh & 255) as i32 - (b >> sh & 255) as i32).abs();
            assert!(
                d(p, want, 16) <= 1 && d(p, want, 8) <= 1 && d(p, want, 0) <= 1,
                "{px}px: pixel {p:08x}"
            );
        }
        assert!(seen > px * px / 4, "{px}px: only {seen} pixels inked");
    }
}

fn centre_of(m: &Mark, role: Role, nth: usize) -> (i32, i32) {
    let p = m.polys.iter().filter(|p| p.role == role).nth(nth).unwrap();
    centroid(&p.pts)
}

fn lum(p: u32) -> i32 {
    let c = unpremul(p);
    ((c >> 16 & 255) * 3 + (c >> 8 & 255) * 6 + (c & 255)) as i32 / 10
}

fn at(s: &Surface, px: usize, c: (i32, i32)) -> u32 {
    s.get(
        (c.0 as usize * px / EXTENT as usize).min(px - 1),
        (c.1 as usize * px / EXTENT as usize).min(px - 1),
    )
}

#[test]
fn the_tile_renders_at_every_size_with_visible_eyes_and_nose() {
    let m = tile(&S);
    for px in SIZES {
        let s = render(&m, px);
        assert_eq!((s.w, s.h), (px, px));
        // Rounded corners are transparent, the rest of the tile is opaque.
        assert_eq!(s.get(0, 0) >> 24, 0, "{px}px corner");
        assert_eq!(s.get(px / 2, px - 1) >> 24, 255, "{px}px bottom edge");
        let covered: usize = s.px.iter().map(|&p| (p >> 24) as usize).sum();
        let frac = covered * 1000 / (255 * px * px);
        // A 22 % rounded square covers 95.8 % of its box.
        assert!(
            (900..=980).contains(&frac),
            "{px}px: {frac} permille opaque"
        );
        // Eyes and nose are dark, the face next to them is bright.
        let face = at(&s, px, (EXTENT / 2 - 40, 215));
        for (role, nth) in [(Role::Eye, 0), (Role::Eye, 1), (Role::Nose, 0)] {
            let c = centre_of(&m, role, nth);
            let v = at(&s, px, c);
            assert!(
                lum(v) + 70 < lum(face),
                "{px}px {role:?}#{nth}: {:06x} is not darker than the face {:06x}",
                v & 0xFFFFFF,
                face & 0xFFFFFF
            );
        }
        // How much ink a window holds: the darkness of each pixel relative to the face
        // behind it (the two halves of the face differ in brightness), in pixels.
        let ink = |x0: usize, x1: usize, y0: usize, y1: usize, bg: u32| {
            let (bg, fg) = (lum(bg | 0xFF00_0000), lum(S.eye | 0xFF00_0000));
            let mut sum = 0i32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += ((bg - lum(s.get(x, y))).max(0) * 256 / (bg - fg)).min(256);
                }
            }
            sum as f32 / 256.0
        };
        let e = EXTENT as usize;
        let (x0, x1) = (px * 150 / e, (px * 235).div_ceil(e));
        let (y0, y1) = (px * 265 / e, (px * 325).div_ceil(e));
        let eye_l = ink(x0, x1, y0, y1, S.light_face);
        let eye_r = ink(px - x1, px - x0, y0, y1, S.shade_face);
        let nose = ink(
            px * 215 / e,
            (px * 297).div_ceil(e),
            px * 385 / e,
            (px * 440).div_ceil(e),
            S.muzzle,
        );
        assert!(
            eye_l >= 2.0 && eye_r >= 2.0 && nose >= 2.0,
            "{px}px: ink {eye_l} {eye_r} {nose}"
        );
        // The eyes are mirror images.
        assert!(
            (eye_l - eye_r).abs() <= eye_l * 0.15 + 0.3,
            "{px}px: the eyes are not symmetric ({eye_l} vs {eye_r})"
        );
    }
}

#[test]
fn the_ink_of_the_head_grows_with_the_size_it_is_drawn_at() {
    // Orange (light face) pixels: more of them as the icon grows, and about the same share.
    let m = tile(&S);
    let mut last = 0;
    for px in SIZES {
        let s = render(&m, px);
        let n = s
            .px
            .iter()
            .filter(|&&p| {
                let c = unpremul(p);
                p >> 24 == 255 && (c >> 16 & 255) > 200 && (c >> 8 & 255) < 150 && (c & 255) < 90
            })
            .count();
        assert!(n > last, "{px}px: {n} <= {last}");
        last = n;
        let share = n * 1000 / (px * px);
        assert!(
            (60..=330).contains(&share),
            "{px}px: {share} permille orange"
        );
    }
}

#[test]
fn small_sizes_are_hinted() {
    assert!(hint(16).eye_pct > hint(24).eye_pct);
    assert!(hint(24).eye_pct > hint(48).eye_pct);
    assert_eq!(hint(128).eye_pct, 100);
    assert!(!hint(16).ear_inner && hint(32).ear_inner);
    let m = tile(&S);
    assert_eq!(hinted(&m, 16).polys.len(), m.polys.len() - 2);
    assert_eq!(hinted(&m, 128), m);
    // A hinted eye is bigger than the designed one.
    let w = |m: &Mark| {
        let p = m.polys.iter().find(|p| p.role == Role::Eye).unwrap();
        let (a, b) = (
            p.pts.iter().map(|q| q.0).min().unwrap(),
            p.pts.iter().map(|q| q.0).max().unwrap(),
        );
        b - a
    };
    assert!(w(&hinted(&m, 16)) > w(&m) * 3 / 2);
}

#[test]
fn halo_draws_seven_tails_behind_the_head_and_nine_when_asked() {
    let tails = |m: &Mark| {
        m.polys
            .iter()
            .filter(|p| matches!(p.role, Role::Tail | Role::TailDark))
            .count()
    };
    assert_eq!(tails(&halo(&S)), HALO_TAILS);
    assert_eq!(tails(&halo_n(&S, 9)), 9);
    assert_eq!(tails(&halo_n(&S, 0)), 1);
    assert_eq!(tails(&halo_n(&S, 99)), 9);
    // The head comes last (in front) and the tails are rust with cream tips.
    let h = halo(&S);
    assert_eq!(h.polys.last().unwrap().role, Role::Nose);
    assert!(
        h.polys
            .iter()
            .any(|p| p.role == Role::TailTip && p.color == S.tail_tip)
    );
    // The crown is symmetric about the axis: every tail but the middle one (which curls to
    // the right, like the others on that side) has a mirror image.
    for n in [6usize, 7, 8, 9] {
        let m = halo_n(&S, n);
        let t: Vec<_> = m
            .polys
            .iter()
            .filter(|p| matches!(p.role, Role::Tail | Role::TailDark))
            .collect();
        let paired = t
            .iter()
            .filter(|p| {
                let b = centroid(
                    &p.pts
                        .iter()
                        .map(|&(x, y)| (EXTENT - x, y))
                        .collect::<Vec<_>>(),
                );
                t.iter().any(|q| {
                    let c = centroid(&q.pts);
                    (c.0 - b.0).abs() <= 2 && (c.1 - b.1).abs() <= 2
                })
            })
            .count();
        assert_eq!(paired, n - n % 2, "{n} tails");
    }
    for px in [48usize, 64, 112, 128] {
        let s = render(&h, px);
        assert_eq!(s.get(0, 0) >> 24, 0, "transparent background");
        let ink = s.px.iter().filter(|&&p| p >> 24 > 128).count();
        assert!(ink * 100 / (px * px) > 15, "{px}px: crown too thin");
        // The crown reaches far to both sides of the (small) head.
        let wing =
            |x0: usize, x1: usize| (0..px).any(|y| (x0..x1).any(|x| s.get(x, y) >> 24 > 128));
        assert!(
            wing(0, px / 8) && wing(px - px / 8, px),
            "{px}px: crown too narrow"
        );
    }
}

#[test]
fn svg_mirrors_the_polygons() {
    for (name, m) in marks() {
        let svg = to_svg(&m, name);
        assert!(svg.starts_with("<svg "));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 128 128\""));
        if m.polys[0].role == Role::Mark {
            assert_eq!(svg.matches("<path ").count(), 1);
            assert!(svg.contains("evenodd"));
        } else {
            assert_eq!(svg.matches("<polygon ").count(), m.polys.len(), "{name}");
        }
        assert_eq!(svg.contains("<rect "), m.tile.is_some());
    }
    assert!(to_svg(&tile(&S), "x").contains("points=\"23.25,11 51.5,38.25"));
}

/// FNV-1a over the premultiplied pixels.
fn fnv(px: &[u32]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for p in px {
        for b in p.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

/// Golden hash of the 32 px tile: it changes when the polygons, the palette, the hints or the
/// rasteriser change. If the change is intended, look at the new sheet
/// (`BRAND_SHEET=/tmp/brand.ppm cargo test -p kitsune_core dump_brand -- --ignored`), then
/// paste the hash printed by this test's failure message into `GOLDEN_TILE_32` and regenerate
/// `docs/brand/` (`cargo run -p kitsune_core --example brand_svg`).
const GOLDEN_TILE_32: u64 = 0xcedb_cc1c_c2bd_fb16;

#[test]
fn the_32px_tile_matches_the_golden_hash() {
    let h = fnv(&render(&tile(&S), 32).px);
    assert_eq!(
        h, GOLDEN_TILE_32,
        "the 32 px tile changed: new hash {h:#018x}. See the doc comment of GOLDEN_TILE_32."
    );
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(render(&tile(&S), 48).px, render(&tile(&S), 48).px);
}

/// Contact sheet: every variant at every size on a dark and a light background.
#[test]
#[ignore = "writes a contact sheet; set BRAND_SHEET=/tmp/brand.ppm"]
fn dump_brand() {
    let Ok(path) = std::env::var("BRAND_SHEET") else {
        return;
    };
    let (w, h) = (1100usize, 620usize);
    let mut sheet = Surface::new(w, h);
    sheet.fill(0xFF14141F);
    let mut light = Surface::new(w, h / 2);
    light.fill(0xFFF3F3F7);
    sheet.blit(&light, 0, (h / 2) as i32, 256);
    let mut y0 = 10;
    for (row, bg) in [(0usize, 0), (1, 1)] {
        let _ = (row, bg);
        let mut x = 10i32;
        for px in SIZES {
            let s = render(&tile(&S), px);
            sheet.blit(&s, x, y0, 256);
            let m = render(&mono(if bg == 0 { 0xFFFFFF } else { 0x20204A }), px);
            sheet.blit(&m, x, y0 + 150, 256);
            x += px as i32 + 16;
        }
        let hl = render(&halo(&S), 192);
        sheet.blit(&hl, x + 20, y0, 256);
        let hl9 = render(&halo_n(&S, 9), 192);
        sheet.blit(&hl9, x + 20 + 200, y0, 256);
        let hl2 = render(&halo(&S), 64);
        sheet.blit(&hl2, x + 20, y0 + 200, 256);
        y0 += h as i32 / 2;
    }
    let mut out = std::format!("P6\n{w} {h}\n255\n").into_bytes();
    for &p in &sheet.px {
        let c = unpremul(p);
        out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    }
    std::fs::write(path, out).unwrap();
}

/// The small sizes magnified 8x (nearest neighbour), side by side, to tune the hints by eye.
#[test]
#[ignore = "writes a magnified strip; set BRAND_SMALL=/tmp/small.ppm"]
fn dump_small() {
    let Ok(path) = std::env::var("BRAND_SMALL") else {
        return;
    };
    let sizes = [16usize, 24, 32, 48];
    let w: usize = sizes.iter().map(|p| p * 8 + 16).sum::<usize>() + 16;
    let h = 48 * 8 + 32;
    let mut sheet = Surface::new(w, h);
    sheet.fill(0xFF14141F);
    let mut x0 = 16;
    for px in sizes {
        let s = render(&tile(&S), px);
        for y in 0..px * 8 {
            for x in 0..px * 8 {
                let p =
                    crate::ui::raster::over(sheet.px[(y + 16) * w + x0 + x], s.get(x / 8, y / 8));
                sheet.px[(y + 16) * w + x0 + x] = p;
            }
        }
        x0 += px * 8 + 16;
    }
    let mut out = std::format!("P6\n{w} {h}\n255\n").into_bytes();
    for &p in &sheet.px {
        let c = unpremul(p);
        out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    }
    std::fs::write(path, out).unwrap();
}

/// The tile at 512 px on white, to lay over the reference art (`BRAND_BIG=/tmp/big.ppm`).
#[test]
#[ignore = "writes a 512 px tile; set BRAND_BIG=/tmp/big.ppm"]
fn dump_big() {
    let Ok(path) = std::env::var("BRAND_BIG") else {
        return;
    };
    let s = render(&tile(&S), 512);
    let mut out = b"P6\n512 512\n255\n".to_vec();
    for &p in &s.px {
        let c = unpremul(crate::ui::raster::over(0xFFFF_FFFF, p));
        out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    }
    std::fs::write(path, out).unwrap();
}
