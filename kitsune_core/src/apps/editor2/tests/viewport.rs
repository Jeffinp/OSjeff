use super::*;

#[test]
fn view_follows_cursor_down_and_up() {
    let mut e = ed(&numbered(100));
    e.resize(10, 40);
    for _ in 0..30 {
        e.move_vertical(1, false);
    }
    assert_eq!(e.top_line(), 21);
    assert_eq!(e.cursor_screen(), Some((9, 0)));
    for _ in 0..30 {
        e.move_vertical(-1, false);
    }
    assert_eq!(e.top_line(), 0);
    assert_eq!(e.cursor_screen(), Some((0, 0)));
}

#[test]
fn view_scrolls_horizontally() {
    let long = "x".repeat(100);
    let mut e = ed(&long);
    e.resize(5, 20);
    e.move_doc_end(false);
    assert_eq!(e.left_col(), 100 - 20 + 1);
    let (r, c) = e.cursor_screen().unwrap();
    assert_eq!((r, c), (0, 19));
    e.move_doc_start(false);
    assert_eq!(e.left_col(), 0);
}

#[test]
fn horizontal_viewport_rows_show_the_right_slice() {
    let line: String = (0..60).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let mut e = ed(&line);
    e.resize(3, 10);
    e.move_doc_end(false);
    let row = e.visible_rows().next().unwrap();
    let s: String = row.cells().map(|c| c.ch).collect();
    assert_eq!(s.chars().count(), 9);
    assert_eq!(s, line[51..].to_string());
}

#[test]
fn resize_keeps_cursor_visible() {
    let mut e = ed(&numbered(50));
    e.resize(30, 80);
    e.goto_line(40);
    e.resize(5, 20);
    assert!(e.cursor_screen().is_some());
    e.resize(100, 200);
    assert!(e.cursor_screen().is_some());
}

#[test]
fn tiny_windows_do_not_panic() {
    let mut e = ed("héllo\nwörld\n\tx");
    for (r, c) in [(0, 0), (1, 1), (1, 2), (2, 1), (1, 40)] {
        e.resize(r, c);
        e.set_line_numbers(true);
        e.set_soft_wrap(true);
        e.move_doc_end(false);
        for row in e.visible_rows() {
            for _ in row.cells() {}
        }
        e.move_doc_start(false);
        let _ = e.cursor_screen();
        let _ = e.pos_at_screen(0, 0);
        check(&e);
    }
}

#[test]
fn line_number_gutter_width_grows_with_digits() {
    let mut e = ed(&numbered(8));
    e.set_line_numbers(true);
    assert_eq!(e.gutter_width(), 2);
    let mut e2 = ed(&numbered(1000));
    e2.set_line_numbers(true);
    assert_eq!(e2.gutter_width(), 5);
    assert_eq!(e2.text_cols(), 80 - 5);
    e2.set_line_numbers(false);
    assert_eq!(e2.gutter_width(), 0);
}

#[test]
fn visible_rows_report_line_numbers() {
    let mut e = ed("a\nb\nc");
    e.set_line_numbers(true);
    e.resize(2, 20);
    let nums: Vec<Option<usize>> = e.visible_rows().map(|r| r.line_number).collect();
    assert_eq!(nums, vec![Some(1), Some(2)]);
}

#[test]
fn visible_rows_stop_at_document_end() {
    let e = ed("a\nb");
    assert_eq!(e.visible_rows().count(), 2);
}

#[test]
fn visible_rows_limited_by_window() {
    let mut e = ed(&numbered(50));
    e.resize(7, 30);
    assert_eq!(e.visible_rows().count(), 7);
}

#[test]
fn tabs_expand_in_rows_and_selection_marks_cells() {
    let mut e = ed("a\tb");
    e.select_range(1, 2);
    let row = e.visible_rows().next().unwrap();
    let cells: Vec<Cell> = row.cells().collect();
    assert_eq!(cells.len(), 5);
    assert_eq!(
        cells[0],
        Cell {
            ch: 'a',
            selected: false
        }
    );
    assert!(cells[1].selected && cells[2].selected && cells[3].selected);
    assert!(!cells[4].selected);
    assert_eq!(cells[4].ch, 'b');
}

#[test]
fn soft_wrap_splits_long_lines_into_rows() {
    let mut e = ed(&"abcdefghij".repeat(3));
    e.set_soft_wrap(true);
    e.resize(10, 10);
    let rows: Vec<RowView> = e.visible_rows().collect();
    assert_eq!(rows.len(), 4);
    assert!(!rows[0].continuation && rows[1].continuation);
    let s: String = rows[1].cells().map(|c| c.ch).collect();
    assert_eq!(s, "abcdefghij");
    assert_eq!(rows[3].cells().count(), 0);
}

#[test]
fn soft_wrap_cursor_and_scroll() {
    let mut e = ed(&"x".repeat(95));
    e.set_soft_wrap(true);
    e.resize(3, 10);
    e.move_doc_end(false);
    // 95 chars at width 10: the cursor is on wrapped row 9.
    let (r, c) = e.cursor_screen().unwrap();
    assert_eq!((r, c), (2, 5));
    assert_eq!(e.top_line(), 0);
    assert_eq!(e.left_col(), 0);
    e.move_doc_start(false);
    assert_eq!(e.cursor_screen(), Some((0, 0)));
}

#[test]
fn soft_wrap_vertical_moves_by_visual_row() {
    let mut e = ed(&"y".repeat(25));
    e.set_soft_wrap(true);
    e.resize(5, 10);
    e.set_cursor(0, 3);
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (0, 13));
    e.move_vertical(1, false);
    assert_eq!(e.cursor(), (0, 23));
    e.move_vertical(-1, false);
    assert_eq!(e.cursor(), (0, 13));
}

#[test]
fn soft_wrap_over_multiple_lines() {
    let doc = format!("{}\nshort\n{}", "a".repeat(30), "b".repeat(30));
    let mut e = ed(&doc);
    e.set_soft_wrap(true);
    e.resize(4, 10);
    e.move_doc_end(false);
    assert!(e.cursor_screen().is_some());
    let lines: Vec<usize> = e.visible_rows().map(|r| r.line).collect();
    assert_eq!(lines.last(), Some(&2));
    assert_eq!(e.rows_of_line(0), 4);
    assert_eq!(e.rows_of_line(1), 1);
}

#[test]
fn pos_at_screen_maps_clicks() {
    let mut e = ed("abc\ndefgh\ni");
    assert_eq!(e.pos_at_screen(0, 1), 1);
    assert_eq!(e.pos_at_screen(1, 3), 7);
    assert_eq!(e.pos_at_screen(0, 50), 3);
    assert_eq!(e.pos_at_screen(20, 0), e.len_bytes());
    e.set_line_numbers(true);
    assert_eq!(e.pos_at_screen(1, 2 + 4), 8);
    assert_eq!(e.pos_at_screen(1, 0), 4);
}

#[test]
fn click_moves_cursor() {
    let mut e = ed("hello\nworld");
    e.mouse_down(1, 3, 1, false);
    assert_eq!(e.cursor(), (1, 3));
}

#[test]
fn scroll_by_does_not_move_cursor() {
    let mut e = ed(&numbered(100));
    e.resize(10, 30);
    e.scroll_by(20);
    assert_eq!(e.top_line(), 20);
    assert_eq!(e.cursor(), (0, 0));
    assert_eq!(e.cursor_screen(), None);
    e.scroll_by(-500);
    assert_eq!(e.top_line(), 0);
    e.scroll_by(10_000);
    assert_eq!(e.top_line(), 100);
}

#[test]
fn horizontal_scroll_by() {
    let mut e = ed(&"z".repeat(100));
    e.resize(3, 10);
    e.scroll_x_by(15);
    assert_eq!(e.left_col(), 15);
    e.scroll_x_by(-100);
    assert_eq!(e.left_col(), 0);
}

#[test]
fn ctrl_up_down_scroll_one_line() {
    let mut e = ed(&numbered(30));
    e.resize(5, 20);
    ckey(&mut e, KeyCode::Down);
    assert_eq!(e.top_line(), 1);
    ckey(&mut e, KeyCode::Up);
    assert_eq!(e.top_line(), 0);
}
