use super::*;
use crate::ui::textlayout::{ellipsize, measure, wrap};

const R: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.subset.ttf");
const M: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Medium.subset.ttf");
const S: &[u8] = include_bytes!("../../../../assets/fonts/Inter-SemiBold.subset.ttf");
const MONO: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.subset.ttf");

fn eng() -> TextEngine {
    TextEngine::new([R, M, S, MONO]).unwrap()
}

#[test]
fn the_monospace_face_has_a_fixed_pitch_and_a_sane_line() {
    let e = eng();
    for px in [12u16, 13, 15, 20] {
        let w = e.advance_q8(Weight::Mono, px, 'i');
        for c in ['W', 'm', '0', ' ', '.', 'ç', '│'] {
            assert_eq!(e.advance_q8(Weight::Mono, px, c), w, "{c:?} at {px}");
        }
    }
    // 600/1000 em: 9 px at 15 px, exactly.
    assert_eq!(
        e.mono_cell(15),
        (9, e.vmetrics(Weight::Mono, 15).line_height)
    );
    let (pitch, line) = e.mono_cell(15);
    assert_eq!(pitch, 9);
    assert!((18..=22).contains(&line), "{line}");
    assert_eq!(e.mono_cell(20).0, 12);
}

#[test]
fn widths_scale_with_size_and_weight() {
    let e = eng();
    let w13 = measure(&e.face(Weight::Regular, 13), "Configurações do Sistema");
    let w26 = measure(&e.face(Weight::Regular, 26), "Configurações do Sistema");
    assert!((w26 - 2 * w13).abs() <= 3, "{w13} {w26}");
    let semi = measure(&e.face(Weight::Semibold, 13), "Configurações do Sistema");
    assert!(semi >= w13, "semibold is not narrower");
    // About 7 px per character at 13 px: far from the 12 px of the old font.
    assert!((130..190).contains(&w13), "{w13}");
}

#[test]
fn kerning_tightens_av() {
    let e = eng();
    let f = e.face(Weight::Regular, 40);
    let kerned = crate::ui::textlayout::measure_q8(&f, "AV");
    let plain = f.advance_q8('A') + f.advance_q8('V');
    assert!(kerned < plain, "{kerned} {plain}");
}

#[test]
fn vmetrics_are_consistent() {
    let e = eng();
    let v = e.vmetrics(Weight::Regular, 13);
    assert_eq!(v.ascent, 13);
    assert_eq!(v.descent, 4);
    assert!(v.cap_height == 9 || v.cap_height == 10);
    assert!(v.line_height >= v.ascent + v.descent);
    let v28 = e.vmetrics(Weight::Semibold, 28);
    assert!(v28.line_height > 2 * v.line_height - 4);
}

#[test]
fn glyph_cache_is_lazy_and_idempotent() {
    let mut e = eng();
    assert_eq!(e.stats().glyphs, 0);
    let a = e.glyph(Weight::Regular, 13, 'e');
    assert_eq!(e.stats().glyphs, 1);
    let b = e.glyph(Weight::Regular, 13, 'e');
    assert_eq!(a, b);
    assert_eq!(e.stats().glyphs, 1);
    assert!(!e.coverage(&a).is_empty());
    assert_eq!(e.coverage(&a).len(), a.w as usize * a.h as usize);
    // Space has no ink but a real advance.
    let sp = e.glyph(Weight::Regular, 13, ' ');
    assert_eq!(sp.w, 0);
    assert!(sp.adv_q8 > 2 * 256);
    // Unknown characters render as a question mark.
    let q = e.glyph(Weight::Regular, 13, '?');
    let u = e.glyph(Weight::Regular, 13, '\u{4E2D}');
    assert_eq!((q.w, q.h), (u.w, u.h));
}

#[test]
fn prewarm_stays_small() {
    let mut e = eng();
    for px in [11u16, 12, 13, 15, 17, 22, 28] {
        e.prewarm(Weight::Regular, px);
    }
    let st = e.stats();
    assert!(st.glyphs > 600, "{}", st.glyphs);
    assert!(st.arena_bytes < 160 * 1024, "{}", st.arena_bytes);
}

#[test]
fn ellipsis_and_wrap_use_the_real_font() {
    let e = eng();
    let f = e.face(Weight::Regular, 13);
    let (s, cut) = ellipsize(&f, "Gerenciador de Tarefas do Sistema Operacional", 120);
    assert!(cut && measure(&f, &s) <= 120, "{s:?}");
    let t = "Um texto razoavelmente longo que precisa quebrar em varias linhas";
    let lines = wrap(&f, t, 150, 8);
    assert!(lines.len() >= 3);
    for (a, b) in lines {
        assert!(measure(&f, &t[a..b]) <= 150, "{:?}", &t[a..b]);
    }
}
