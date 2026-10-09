use super::*;

fn masks() -> CornerMasks {
    CornerMasks::with_max_radius(64)
}

fn opaque(s: &Surface) -> usize {
    s.px.iter().filter(|&&p| p >> 24 > 16).count()
}

#[test]
fn every_file_icon_has_content_at_every_size() {
    let m = masks();
    for k in FILE_KINDS {
        for side in [16usize, 20, 32, 48, 64, 96] {
            let s = file_icon(k, side, 0x5B5CF6, &m);
            assert_eq!((s.w, s.h), (side, side));
            let n = opaque(&s);
            assert!(n > side * side / 8, "{k:?} at {side}: {n} px");
            assert!(n < side * side, "{k:?} at {side} fills the whole box");
        }
    }
}

#[test]
fn file_icons_are_distinct_and_deterministic() {
    let m = masks();
    let mut seen: Vec<Vec<u32>> = Vec::new();
    for k in FILE_KINDS {
        let a = file_icon(k, 32, 0x5B5CF6, &m);
        let b = file_icon(k, 32, 0x5B5CF6, &m);
        assert_eq!(a.px, b.px, "{k:?} is not deterministic");
        assert!(!seen.contains(&a.px), "{k:?} duplicates another icon");
        seen.push(a.px);
    }
}

#[test]
fn the_accent_tints_folders_and_apps_but_not_documents() {
    let m = masks();
    for k in [FileKind::Folder, FileKind::App] {
        let a = file_icon(k, 48, 0x5B5CF6, &m);
        let b = file_icon(k, 48, 0xF59E0B, &m);
        assert_ne!(a.px, b.px, "{k:?} ignores the accent");
    }
    let a = file_icon(FileKind::Text, 48, 0x5B5CF6, &m);
    let b = file_icon(FileKind::Text, 48, 0xF59E0B, &m);
    // Only the heading line of the text icon follows the accent.
    let diff = a.px.iter().zip(&b.px).filter(|(x, y)| x != y).count();
    assert!(diff < 48 * 48 / 10, "{diff}");
}

#[test]
fn corners_of_a_file_icon_stay_transparent() {
    let m = masks();
    for k in FILE_KINDS {
        let s = file_icon(k, 64, 0x5B5CF6, &m);
        assert_eq!(s.get(0, 0) >> 24, 0, "{k:?} top-left");
        assert_eq!(s.get(63, 63) >> 24, 0, "{k:?} bottom-right");
    }
}

#[test]
fn every_tool_glyph_draws_in_the_colour_asked() {
    for t in TOOLS {
        for side in [12usize, 14, 16, 20, 24] {
            let s = tool_glyph(t, side, 0xFF11_80F0);
            let n = opaque(&s);
            assert!(n >= side * side / 20, "{t:?} at {side}: {n}");
            assert!(n < side * side * 9 / 10, "{t:?} at {side} is a blob: {n}");
            // Premultiplied pixels never carry more colour than alpha.
            for &p in &s.px {
                let a = p >> 24;
                assert!((p >> 16) & 0xFF <= a && (p >> 8) & 0xFF <= a && p & 0xFF <= a);
            }
        }
    }
}

#[test]
fn tool_glyphs_are_distinct() {
    let mut seen: Vec<Vec<u32>> = Vec::new();
    for t in TOOLS {
        let s = tool_glyph(t, 16, 0xFF00_0000);
        assert!(!seen.contains(&s.px), "{t:?} duplicates another glyph");
        seen.push(s.px);
    }
}

#[test]
fn mirrored_pairs_are_mirrors() {
    let l = tool_glyph(Tool::ChevronLeft, 16, 0xFF00_0000);
    let r = tool_glyph(Tool::ChevronRight, 16, 0xFF00_0000);
    // The two chevrons have the same ink (within rounding of the anti-aliasing).
    let (a, b) = (opaque(&l) as i32, opaque(&r) as i32);
    assert!((a - b).abs() <= 6, "{a} vs {b}");
    let up = tool_glyph(Tool::ChevronUp, 16, 0xFF00_0000);
    let down = tool_glyph(Tool::ChevronDown, 16, 0xFF00_0000);
    assert!((opaque(&up) as i32 - opaque(&down) as i32).abs() <= 6);
}

#[test]
fn sizes_are_clamped_not_rejected() {
    let m = masks();
    assert_eq!(file_icon(FileKind::Folder, 0, 0, &m).w, 8);
    assert_eq!(file_icon(FileKind::Folder, 10_000, 0, &m).w, 256);
    assert_eq!(tool_glyph(Tool::Eye, 1, 0xFF00_0000).w, 8);
}

#[test]
#[ignore = "writes a contact sheet: APPART_SHEET=/tmp/appart.ppm"]
fn dump_sheet() {
    let m = masks();
    let Ok(path) = std::env::var("APPART_SHEET") else {
        return;
    };
    let (w, h) = (6 * 72 + 8, 100 + 6 * 40 + 8);
    let mut px = alloc::vec![0xFFF6F6F8u32; w * h];
    let paste = |px: &mut Vec<u32>, s: &Surface, x: usize, y: usize| {
        for sy in 0..s.h {
            for sx in 0..s.w {
                let p = s.px[sy * s.w + sx];
                let a = p >> 24;
                let d = &mut px[(y + sy) * w + x + sx];
                let ch = |sh: u32| {
                    let sc = (p >> sh) & 0xFF;
                    let dc = (*d >> sh) & 0xFF;
                    sc + dc * (255 - a) / 255
                };
                *d = 0xFF00_0000 | (ch(16).min(255) << 16) | (ch(8).min(255) << 8) | ch(0).min(255);
            }
        }
    };
    for (i, k) in FILE_KINDS.iter().enumerate() {
        paste(&mut px, &file_icon(*k, 64, 0x5B5CF6, &m), 4 + i * 72, 4);
        paste(&mut px, &file_icon(*k, 20, 0x5B5CF6, &m), 4 + i * 72, 72);
    }
    for (i, t) in TOOLS.iter().enumerate() {
        paste(
            &mut px,
            &tool_glyph(*t, 32, 0xFF1D1D1F),
            4 + (i % 6) * 72,
            100 + (i / 6) * 40,
        );
    }
    let mut out = alloc::format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, out).ok();
}
