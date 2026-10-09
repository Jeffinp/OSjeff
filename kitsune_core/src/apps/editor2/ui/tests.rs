//! Tests of the editor window geometry.

use super::*;

fn win() -> Rect {
    Rect::new(100, 80, 640, 420)
}

const M: Metrics = Metrics { cw: 9, lh: 20 };

#[test]
fn the_grid_fits_between_the_title_and_the_status_bar() {
    for bar in [0, FIND_H, REPLACE_H] {
        for gutter in [0usize, 3, 7] {
            let l = layout(win(), M, bar, gutter);
            assert!(l.top >= win().y + TITLE_H + bar);
            assert!(
                l.top + l.rows as i32 * M.lh <= l.status.y,
                "bar {bar} gutter {gutter}"
            );
            assert!(l.rows >= 1 && l.cols >= 2);
            // Whole cells: the text never reaches past the right padding.
            let right = l.text_x + (l.cols as i32 - gutter as i32) * M.cw;
            assert!(
                right <= win().right() - PAD + 1 || gutter + 2 > l.cols,
                "{right}"
            );
            assert_eq!(l.bar.is_some(), bar > 0);
            if let Some(b) = l.bar {
                assert_eq!(b.y, win().y + TITLE_H);
                assert!(b.bottom() <= l.top);
            }
            assert_eq!(l.status.bottom(), win().bottom());
            assert!(l.gutter.right() <= l.text_x);
        }
    }
}

#[test]
fn a_taller_bar_leaves_fewer_rows() {
    let none = layout(win(), M, 0, 3).rows;
    let find = layout(win(), M, FIND_H, 3).rows;
    let repl = layout(win(), M, REPLACE_H, 3).rows;
    assert!(none > find && find > repl, "{none} {find} {repl}");
}

#[test]
fn bigger_text_gives_fewer_rows_and_columns() {
    let small = layout(win(), Metrics { cw: 7, lh: 16 }, 0, 3);
    let big = layout(win(), Metrics { cw: 14, lh: 31 }, 0, 3);
    assert!(small.rows > big.rows && small.cols > big.cols);
}

#[test]
fn layout_survives_tiny_windows() {
    let l = layout(Rect::new(0, 0, 40, 30), M, REPLACE_H, 9);
    assert!(l.rows >= 1 && l.cols >= 2);
    assert!(l.area.h >= 0 && l.gutter.w >= 0);
    let l = layout(Rect::new(0, 0, 0, 0), M, 0, 0);
    assert!(l.rows >= 1);
}

#[test]
fn clicks_map_to_cells_with_the_gutter_offset() {
    let l = layout(win(), M, 0, 3);
    // The first text column is the engine's column `gutter_cols`.
    let (x, y) = l.cell_xy(2, 3);
    assert_eq!((x, y), (l.text_x, l.top + 2 * M.lh));
    assert_eq!(l.cell_at(x + 1, y + 1), (2, 3));
    assert_eq!(l.cell_at(x + M.cw * 5 + 2, y + 5), (2, 8));
    // A press in the gutter lands on the first text column; left of it too.
    assert_eq!(l.cell_at(l.window.x + 4, y + 1), (2, 3));
    assert_eq!(l.cell_at(l.text_x - 2, y + 1), (2, 3));
    // Below and above the grid clamp to its edge rows; far right clamps to the last column.
    assert_eq!(l.cell_at(x, l.top + 10_000).0, l.rows - 1);
    assert_eq!(l.cell_at(x, -50).0, 0);
    assert_eq!(l.cell_at(10_000, y).1, l.cols - 1);
    // Without a gutter the text starts at column 0.
    let l0 = layout(win(), M, 0, 0);
    assert_eq!(l0.cell_at(l0.text_x + 1, l0.top).1, 0);
    assert!(l.in_text(l.text_x + 5, l.top + 5));
    assert!(!l.in_text(l.text_x + 5, l.top - 3));
    assert!(!l.in_text(l.text_x + 5, l.status.y + 1));
}

#[test]
fn cells_and_pixels_round_trip() {
    let l = layout(win(), Metrics { cw: 11, lh: 24 }, FIND_H, 4);
    for row in [0usize, 1, l.rows - 1] {
        for col in [4usize, 5, 20, l.cols - 1] {
            let (x, y) = l.cell_xy(row, col);
            assert_eq!(l.cell_at(x + 3, y + 3), (row, col), "{row},{col}");
        }
    }
    assert!(l.number_right() <= l.text_x);
    assert!(l.number_right() > l.window.x + PAD);
}

#[test]
fn selection_runs_are_found() {
    let cells = [false, true, true, false, true, false, false, true];
    assert_eq!(
        selection_runs(cells.iter().copied()),
        vec![(1, 2), (4, 1), (7, 1)]
    );
    assert!(selection_runs([false, false].into_iter()).is_empty());
    assert_eq!(selection_runs([true, true, true].into_iter()), vec![(0, 3)]);
    assert!(selection_runs(core::iter::empty()).is_empty());
}

#[test]
fn indent_is_counted_in_columns() {
    assert_eq!(leading_indent("fn x()", 4), 0);
    assert_eq!(leading_indent("    let a;", 4), 4);
    assert_eq!(leading_indent("\tlet a;", 4), 4);
    assert_eq!(leading_indent("  \tlet a;", 4), 4);
    assert_eq!(leading_indent("\t\t x", 4), 9);
    assert_eq!(leading_indent("        ", 4), 8);
    assert_eq!(leading_indent("", 4), 0);
    assert_eq!(leading_indent("  x", 0), 2); // a zero tab width is treated as 1
}

#[test]
fn indent_guides_sit_at_each_level_past_the_text_edge() {
    assert!(indent_guides(0, 4).is_empty());
    assert!(indent_guides(4, 4).is_empty()); // the text itself starts at 4: no guide inside
    assert_eq!(indent_guides(5, 4), vec![4]);
    assert_eq!(indent_guides(13, 4), vec![4, 8, 12]);
    assert_eq!(indent_guides(7, 2), vec![2, 4, 6]);
    assert_eq!(indent_guides(3, 0), vec![1, 2]); // tab width 0 acts as 1
}

#[test]
fn the_picker_sheet_has_room_for_everything() {
    for (w, h) in [(560, 350), (400, 280), (900, 700)] {
        let r = Rect::new(0, 0, w, h);
        for save in [false, true] {
            let (pw, ph) = picker_size(r, save);
            assert!((380..=600).contains(&pw) && (260..=400).contains(&ph));
            let panel = Rect::new((w - pw) / 2, TITLE_H, pw, ph);
            let l = picker_layout(panel, save);
            for part in [l.title, l.sidebar, l.path, l.list, l.hint] {
                assert!(
                    part.x >= panel.x && part.right() <= panel.right(),
                    "{part:?}"
                );
                assert!(
                    part.y >= panel.y && part.bottom() <= panel.bottom(),
                    "{part:?}"
                );
            }
            assert!(l.rows >= 1);
            assert!(l.list.bottom() <= l.hint.y);
            assert!(l.sidebar.right() < l.list.x);
            if save {
                assert!(
                    l.field.h > 0 && l.list.bottom() <= l.field.y && l.field.bottom() <= l.hint.y
                );
            } else {
                assert_eq!(l.field.h, 0);
            }
            // The buttons live below the hint line.
            let btn_top = panel.bottom() - SHEET_PAD - BUTTON_H;
            assert!(l.hint.bottom() <= btn_top);
        }
    }
}

#[test]
fn picker_hits() {
    let panel = Rect::new(50, 40, 520, 340);
    let l = picker_layout(panel, true);
    assert_eq!(l.row_at(0, l.list.x + 5, l.list.y + 2), Some(0));
    assert_eq!(
        l.row_at(7, l.list.x + 5, l.list.y + PICK_ROW_H * 2 + 1),
        Some(9)
    );
    assert_eq!(l.row_at(0, l.list.x - 2, l.list.y + 2), None);
    assert_eq!(l.row_at(0, l.list.x + 5, l.list.bottom() + 2), None);
    for i in 0..PLACES.len() {
        let r = l.place_rect(i);
        assert!(
            l.sidebar.contains(r.x + 2, r.y + 2)
                && r.bottom() <= l.sidebar.bottom().max(r.bottom())
        );
        assert_eq!(l.place_at(r.x + 4, r.y + 4), Some(i));
    }
    assert_eq!(l.place_at(l.list.x + 5, l.list.y + 5), None);
}

#[test]
fn places_hold_their_subfolders() {
    assert_eq!(place_of("/"), Some(3));
    assert_eq!(place_of("/Documentos"), Some(1));
    assert_eq!(place_of("/Documentos/a/b"), Some(1));
    assert_eq!(place_of("/DocumentosX"), None);
    assert_eq!(place_of("/home"), Some(0));
    assert_eq!(place_of("/home/x"), Some(0));
    assert_eq!(place_of("/Imagens/ferias"), Some(2));
    assert_eq!(place_of("/outra"), None);
    assert_eq!(place_of(""), None);
}

#[test]
fn the_close_buttons_sit_apart_inside_the_panel() {
    let panel = Rect::new(100, 60, 440, 170);
    let [d, c, s] = close_buttons(panel, [96, 86, 80]);
    assert!(d.right() < c.x && c.right() < s.x);
    assert_eq!(s.right(), panel.right() - SHEET_PAD);
    assert_eq!(d.x, panel.x + SHEET_PAD);
    for b in [d, c, s] {
        assert!(b.bottom() == panel.bottom() - SHEET_PAD && b.h == BUTTON_H);
    }
}

#[test]
fn the_find_bar_controls_do_not_overlap() {
    for (w, replace) in [
        (640, false),
        (640, true),
        (420, false),
        (420, true),
        (300, true),
    ] {
        let bar = Rect::new(0, 32, w, if replace { REPLACE_H } else { FIND_H });
        let f = find_layout(bar, replace, 96, 72);
        let row = [f.find, f.notice, f.prev, f.next, f.case, f.close];
        for p in row.windows(2) {
            assert!(
                p[0].right() <= p[1].x,
                "{w} {replace}: {:?} {:?}",
                p[0],
                p[1]
            );
        }
        for r in row {
            assert!(r.y >= bar.y && r.bottom() <= bar.bottom());
        }
        assert!(f.find.w >= 60);
        assert_eq!(f.replace.is_some(), replace);
        if let (Some(rep), Some(one), Some(all)) = (f.replace, f.replace_one, f.replace_all) {
            assert!(rep.right() <= one.x && one.right() <= all.x);
            assert!(rep.y >= f.find.bottom() && rep.bottom() <= bar.bottom());
            assert_eq!(all.right(), f.close.right());
        }
    }
}

#[test]
fn find_bar_presses_land_on_their_controls() {
    let bar = Rect::new(0, 32, 640, REPLACE_H);
    let f = find_layout(bar, true, 96, 72);
    let mid = |r: Rect| (r.x + r.w / 2, r.y + r.h / 2);
    for (r, want) in [
        (f.find, FindHit::Find),
        (f.prev, FindHit::Prev),
        (f.next, FindHit::Next),
        (f.case, FindHit::Case),
        (f.close, FindHit::Close),
        (f.replace.unwrap(), FindHit::Replace),
        (f.replace_one.unwrap(), FindHit::ReplaceOne),
        (f.replace_all.unwrap(), FindHit::ReplaceAll),
    ] {
        let (x, y) = mid(r);
        assert_eq!(f.hit(x, y, false), Some(want));
    }
    assert_eq!(f.hit(f.notice.x + 1, f.notice.y + 1, false), None);
    // The go-to-line bar answers only on its field and the close button.
    let g = find_layout(Rect::new(0, 32, 640, FIND_H), false, 96, 72);
    let (x, y) = mid(g.next);
    assert_eq!(g.hit(x, y, true), None);
    let (x, y) = mid(g.find);
    assert_eq!(g.hit(x, y, true), Some(FindHit::Find));
    let (x, y) = mid(g.close);
    assert_eq!(g.hit(x, y, true), Some(FindHit::Close));
}
