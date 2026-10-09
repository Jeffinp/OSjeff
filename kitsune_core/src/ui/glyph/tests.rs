use super::*;

const REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.subset.ttf");

fn total(b: &Bitmap) -> u32 {
    b.data.iter().map(|&v| v as u32).sum()
}

#[test]
fn space_is_empty() {
    let f = Font::parse(REGULAR).unwrap();
    let b = rasterize(&f, f.glyph_index(' '), 13);
    assert_eq!((b.w, b.h), (0, 0));
    assert!(b.data.is_empty());
}

#[test]
fn capital_i_is_a_vertical_bar_with_the_right_height() {
    let f = Font::parse(REGULAR).unwrap();
    let b = rasterize(&f, f.glyph_index('I'), 26);
    // Cap height of Inter is 0.7275 em: 18.9 px.
    assert!((18..=21).contains(&(b.h as i32)), "h={}", b.h);
    assert_eq!(b.top as i32, b.h as i32, "I sits on the baseline");
    // A solid stem: the middle row has one run of full coverage.
    let row = &b.data[(b.h as usize / 2) * b.w as usize..][..b.w as usize];
    // Inter's stem is about 0.09 em: 2.3 px at 26 px.
    let width = row.iter().map(|&v| v as u32).sum::<u32>() as f32 / 255.0;
    assert!((1.8..3.2).contains(&width), "stem {width}");
    assert!(row.iter().filter(|&&v| v > 0).count() <= 4);
}

#[test]
fn ink_scales_with_the_square_of_the_size() {
    let f = Font::parse(REGULAR).unwrap();
    let g = f.glyph_index('o');
    let a = total(&rasterize(&f, g, 20)) as f32;
    let b = total(&rasterize(&f, g, 40)) as f32;
    let ratio = b / a;
    assert!((3.7..4.3).contains(&ratio), "ratio {ratio}");
}

#[test]
fn counters_stay_open() {
    // The inside of an 'o' must be empty: opposite winding cuts the hole.
    let f = Font::parse(REGULAR).unwrap();
    let b = rasterize(&f, f.glyph_index('o'), 40);
    let (w, h) = (b.w as usize, b.h as usize);
    assert_eq!(b.data[(h / 2) * w + w / 2], 0);
    assert!(b.data[(h / 2) * w + 1] > 100 || b.data[(h / 2) * w + 2] > 100);
}

#[test]
fn coverage_is_anti_aliased_not_binary() {
    let f = Font::parse(REGULAR).unwrap();
    let b = rasterize(&f, f.glyph_index('S'), 13);
    let mid = b.data.iter().filter(|&&v| v > 16 && v < 240).count();
    assert!(
        mid * 4 > b.data.iter().filter(|&&v| v > 0).count(),
        "mid {mid}"
    );
}

#[test]
fn accents_sit_above_and_cedilla_below() {
    let f = Font::parse(REGULAR).unwrap();
    let a = rasterize(&f, f.glyph_index('a'), 20);
    let acute = rasterize(&f, f.glyph_index('á'), 20);
    assert!(acute.top > a.top + 3);
    let c = rasterize(&f, f.glyph_index('c'), 20);
    let cedilla = rasterize(&f, f.glyph_index('ç'), 20);
    assert!(cedilla.top - (cedilla.h as i16) < c.top - (c.h as i16) - 2);
}

#[test]
fn synthetic_square_covers_exactly() {
    // Build a 1x1 px square by hand through the accumulator.
    let mut acc = vec![0i32; 3];
    // Square from (0,0) to (256,256): left edge down, right edge up.
    add_line(&mut acc, 3, 0, 0, 0, 256);
    add_line(&mut acc, 3, 256, 256, 256, 0);
    let c0 = acc[0].unsigned_abs().min(65536);
    assert_eq!(c0, 65536);
    // Half-covered column.
    let mut acc = vec![0i32; 3];
    add_line(&mut acc, 3, 128, 0, 128, 256);
    add_line(&mut acc, 3, 256, 256, 256, 0);
    let s0 = acc[0];
    assert_eq!(s0.unsigned_abs(), 32768);
}

#[test]
fn slanted_edges_telescope_to_full_coverage() {
    // A wide parallelogram: the area right of the edges must sum to exactly 1.
    let mut acc = vec![0i32; 40 * 4];
    add_line(&mut acc, 40, 0, 0, 0, 4 * 256);
    add_line(&mut acc, 40, 37 * 256, 4 * 256, 3 * 256 + 77, 0); // slanted
    for row in 0..4 {
        let mut sum = 0i64;
        for x in 0..38 {
            sum += acc[row * 40 + x] as i64;
        }
        assert!(sum.abs() <= 8, "row {row} residue {sum}");
    }
}

#[test]
fn all_glyphs_at_all_ui_sizes_rasterise() {
    let f = Font::parse(REGULAR).unwrap();
    for px in [11u16, 12, 13, 15, 17, 22, 28] {
        for g in 0..f.glyph_count() {
            let b = rasterize(&f, g, px);
            assert_eq!(b.data.len(), b.w as usize * b.h as usize);
            assert!(b.w <= 64 && b.h <= 64, "gid {g} {px}px {}x{}", b.w, b.h);
        }
    }
}
