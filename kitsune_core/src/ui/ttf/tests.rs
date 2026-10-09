use super::*;

const REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.subset.ttf");

#[test]
fn parses_the_embedded_font() {
    let f = Font::parse(REGULAR).expect("font");
    assert_eq!(f.units_per_em, 2048);
    assert!(f.ascent > 1800 && f.descent < -300);
    assert!(f.cap_height > 1300 && f.cap_height < 1600);
    assert!(f.glyph_count() > 200);
}

#[test]
fn maps_ascii_and_accents() {
    let f = Font::parse(REGULAR).unwrap();
    for c in ' '..='~' {
        let g = f.glyph_index(c);
        assert!(g != 0, "missing {c:?}");
    }
    for c in "çãõáéíóúâêôüÇÃÕÁÉÍÓÚ·•…–—“”‘’→←↑↓⌘⇧⌥⌃✓×÷°©®€".chars()
    {
        assert!(f.glyph_index(c) != 0, "missing {c:?}");
    }
    assert_eq!(f.glyph_index('\u{4E2D}'), 0);
    assert_eq!(f.glyph_index('\u{1F600}'), 0);
}

#[test]
fn advances_are_plausible() {
    let f = Font::parse(REGULAR).unwrap();
    let w = f.advance(f.glyph_index('W'));
    let i = f.advance(f.glyph_index('i'));
    let sp = f.advance(f.glyph_index(' '));
    assert!(w > i * 2, "W {w} i {i}");
    assert!(sp > 300 && sp < 900);
}

#[test]
fn kerning_pairs_exist() {
    let f = Font::parse(REGULAR).unwrap();
    let (a, v) = (f.glyph_index('A'), f.glyph_index('V'));
    assert!(f.kern(a, v) < 0, "AV = {}", f.kern(a, v));
    let (o, n) = (f.glyph_index('o'), f.glyph_index('n'));
    assert!(f.kern(o, n).abs() < 200);
    assert_eq!(f.kern(0, v), 0);
}

fn outline_bbox(f: &Font, c: char) -> (i32, i32, i32, i32, usize) {
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN, 0);
    f.outline(f.glyph_index(c), &mut |s| {
        n += 1;
        let mut p = |x: i32, y: i32| {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        };
        match s {
            Seg::Move(x, y) | Seg::Line(x, y) => p(x, y),
            Seg::Quad(a, b, x, y) => {
                p(a, b);
                p(x, y)
            }
            Seg::Close => {}
        }
    });
    (x0, y0, x1, y1, n)
}

#[test]
fn outlines_have_sane_boxes() {
    let f = Font::parse(REGULAR).unwrap();
    let (x0, y0, x1, y1, n) = outline_bbox(&f, 'H');
    assert!(n > 8);
    assert!(x0 >= 0 && x1 < 1800);
    assert!(y0 == 0 && (y1 - f.cap_height as i32).abs() < 40, "{y1}");
    // A space has no outline.
    assert_eq!(outline_bbox(&f, ' ').4, 0);
}

#[test]
fn composite_accents_extend_above_the_base() {
    let f = Font::parse(REGULAR).unwrap();
    let (_, _, _, ya, na) = outline_bbox(&f, 'a');
    let (_, _, _, yb, nb) = outline_bbox(&f, 'á');
    assert!(nb > na && yb > ya + 200, "{ya} {yb}");
    // Cedilla hangs below the baseline.
    let (_, y0, _, _, _) = outline_bbox(&f, 'ç');
    assert!(y0 < -100, "{y0}");
}

#[test]
fn every_glyph_outline_is_well_formed() {
    let f = Font::parse(REGULAR).unwrap();
    for g in 0..f.glyph_count() {
        let mut open = false;
        f.outline(g, &mut |s| match s {
            Seg::Move(..) => {
                assert!(!open);
                open = true;
            }
            Seg::Close => {
                assert!(open);
                open = false;
            }
            _ => assert!(open),
        });
        assert!(!open);
    }
}

#[test]
fn hostile_fonts_never_panic() {
    // Truncations and byte flips of the real font must only fail soft.
    let mut seed = 0x1234_5678u32;
    for round in 0..300 {
        let mut data = REGULAR.to_vec();
        if round % 3 == 0 {
            let cut = (seed as usize) % data.len();
            data.truncate(cut);
        }
        for _ in 0..8 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let at = (seed as usize >> 4) % data.len().max(1);
            if at < data.len() {
                data[at] ^= (seed >> 24) as u8 | 1;
            }
        }
        if let Some(f) = Font::parse(&data) {
            for g in (0..f.glyph_count().min(300)).step_by(7) {
                f.outline(g, &mut |_| {});
                let _ = f.advance(g);
                let _ = f.kern(g, g.wrapping_add(3));
            }
            let _ = f.glyph_index('é');
        }
    }
    assert!(Font::parse(&[]).is_none());
    assert!(Font::parse(b"not a font at all").is_none());
}
