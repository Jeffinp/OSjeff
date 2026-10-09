use super::*;

#[test]
fn drag_selects_a_range_of_characters() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    // "The quick brown fox": 8 px per character; from inside "quick" to inside "fox".
    let a = (x0 + 4 * 8 + 1, y0 + 2);
    let b = (x0 + 19 * 8 - 1, y0 + 2);
    let r = p.select(a, b, &FixedAdvance).unwrap();
    assert_eq!(p.selection_text(&r), "quick brown fox");
    let spans = p.selection_spans(&r, &FixedAdvance);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].x, x0 + 4 * 8);
    assert_eq!(spans[0].w, 15 * 8);
}

#[test]
fn a_drag_snaps_to_the_nearer_character_boundary() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    // 3 px into the 'T' (8 px wide) is nearer its left edge; 5 px into the fourth character
    // is nearer its right edge.
    let r = p
        .select((x0 + 3, y0 + 2), (x0 + 3 * 8 + 5, y0 + 2), &FixedAdvance)
        .unwrap();
    assert_eq!(p.selection_text(&r), "The ");
}

#[test]
fn dragging_backwards_is_the_same_selection() {
    let p = text_page();
    let (_, y0) = first_run(&p);
    let a = (400, y0 + 2);
    let b = (30, y0 + 2);
    let r = p.select(a, b, &FixedAdvance).unwrap();
    assert_eq!(p.select(b, a, &FixedAdvance), Some(r));
}

#[test]
fn selection_across_lines_has_a_newline() {
    let p = text_page();
    let r = p.select((0, 0), (5000, 5000), &FixedAdvance).unwrap();
    let t = p.selection_text(&r);
    assert!(t.starts_with("The quick brown fox\njumps"), "{t}");
    assert!(t.ends_with("The end."));
}

#[test]
fn a_click_without_movement_selects_nothing() {
    let p = text_page();
    assert_eq!(p.select((50, 40), (50, 40), &FixedAdvance), None);
}

#[test]
fn selection_on_a_page_without_text_is_none() {
    let p = render(b"<div></div>", 600);
    assert_eq!(p.select((1, 1), (50, 50), &FixedAdvance), None);
    assert_eq!(p.word_count(), 0);
}

#[test]
fn dragging_below_the_page_selects_to_the_end() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    let r = p.select((x0, y0 + 2), (20, 5000), &FixedAdvance).unwrap();
    assert!(p.selection_text(&r).ends_with("The end."));
}

#[test]
fn dragging_above_the_page_selects_from_the_start() {
    let p = text_page();
    let r = p.select((20, 0), (400, 5000), &FixedAdvance).unwrap();
    assert_eq!(r.start, CharPos { run: 0, off: 0 });
}

#[test]
fn double_click_picks_a_word() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    let r = p.select_word(x0 + 6 * 8, y0 + 2, &FixedAdvance).unwrap();
    assert_eq!(p.selection_text(&r), "quick");
    assert!(p.select_word(2, 2, &FixedAdvance).is_none());
}
