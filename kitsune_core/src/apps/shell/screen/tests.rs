use super::*;
use alloc::string::ToString;
use alloc::vec;

fn rows(s: &Screen, cols: usize, h: usize, live: &str) -> Vec<String> {
    s.view(cols, h, live, live.chars().count()).rows
}

#[test]
fn lines_and_the_live_line() {
    let mut s = Screen::new();
    s.print(b"one\ntwo\n");
    assert_eq!(rows(&s, 20, 10, "$ ls"), ["one", "two", "$ ls"]);
    let v = s.view(20, 10, "$ ls", 4);
    assert_eq!(v.cursor, Some((2, 4)));
    assert_eq!((v.above, v.below), (0, 0));
}

#[test]
fn a_partial_line_shows_before_the_live_line() {
    let mut s = Screen::new();
    s.print(b"abc");
    assert_eq!(rows(&s, 20, 10, "$ "), ["abc", "$ "]);
    s.print(b"def\n");
    assert_eq!(rows(&s, 20, 10, "$ "), ["abcdef", "$ "]);
    s.print(b"x");
    s.ensure_newline();
    s.ensure_newline();
    assert_eq!(s.line_count(), 2);
}

#[test]
fn long_lines_wrap_and_rewrap_on_resize() {
    let mut s = Screen::new();
    s.print(b"abcdefghij\n");
    assert_eq!(rows(&s, 4, 10, "$"), ["abcd", "efgh", "ij", "$"]);
    assert_eq!(rows(&s, 5, 10, "$"), ["abcde", "fghij", "$"]);
    assert_eq!(rows(&s, 100, 10, "$"), ["abcdefghij", "$"]);
}

#[test]
fn the_window_shows_the_newest_rows_and_scrolls() {
    let mut s = Screen::new();
    for i in 0..20 {
        s.print(format!("line {i}\n").as_bytes());
    }
    let v = s.view(20, 5, "$", 1);
    assert_eq!(v.rows, ["line 16", "line 17", "line 18", "line 19", "$"]);
    assert_eq!((v.above, v.below), (16, 0));
}

#[test]
fn scrolling_moves_by_rows_and_clamps() {
    let mut s = Screen::new();
    for i in 0..20 {
        s.print(format!("l{i}\n").as_bytes());
    }
    // 20 lines + the live one = 21 rows; the window holds 5.
    assert_eq!(rows(&s, 20, 5, "$"), ["l16", "l17", "l18", "l19", "$"]);
    s.scroll(3, 20, 5, 1);
    assert!(s.is_scrolled());
    let v = s.view(20, 5, "$", 1);
    assert_eq!(v.rows, ["l13", "l14", "l15", "l16", "l17"]);
    assert_eq!((v.above, v.below), (13, 3));
    assert_eq!(v.cursor, None, "the live line is below the window");
    s.scroll(1000, 20, 5, 1);
    let v = s.view(20, 5, "$", 1);
    assert_eq!(v.rows[0], "l0");
    assert_eq!(v.above, 0);
    s.scroll(-2, 20, 5, 1);
    assert_eq!(s.view(20, 5, "$", 1).rows[0], "l2");
    s.scroll(-1000, 20, 5, 1);
    assert!(!s.is_scrolled());
    s.scroll(4, 20, 5, 1);
    s.to_bottom();
    assert_eq!(rows(&s, 20, 5, "$")[4], "$");
}

#[test]
fn a_short_history_is_not_padded() {
    let mut s = Screen::new();
    s.print(b"a\n");
    let v = s.view(10, 8, "$", 1);
    assert_eq!(v.rows, ["a", "$"]);
    s.scroll(5, 10, 8, 1);
    assert_eq!(
        s.view(10, 8, "$", 1).rows,
        ["a", "$"],
        "nothing to scroll to"
    );
}

#[test]
fn the_caret_wraps_with_the_live_line() {
    let s = Screen::new();
    let v = s.view(4, 5, "abcdefg", 7);
    assert_eq!(v.rows, ["abcd", "efg"]);
    assert_eq!(v.cursor, Some((1, 3)));
    // A caret at the very end of a full row sits on a new, empty row.
    let v = s.view(4, 5, "abcd", 4);
    assert_eq!(v.rows, ["abcd", ""]);
    assert_eq!(v.cursor, Some((1, 0)));
    // In the middle of the text.
    let v = s.view(4, 5, "abcdefg", 5);
    assert_eq!(v.cursor, Some((1, 1)));
}

#[test]
fn live_first_marks_where_the_live_line_starts() {
    let mut s = Screen::new();
    s.print(b"a\nb\n");
    assert_eq!(s.view(10, 8, "$ x", 3).live_first, Some(2));
    // A wrapped live line starts on its first row.
    assert_eq!(s.view(2, 8, "abcde", 5).live_first, Some(2));
    // Scrolled away, or hidden while a command runs: none.
    assert_eq!(s.view_history(10, 8).live_first, None);
    for i in 0..20 {
        s.print(format!("{i}\n").as_bytes());
    }
    s.scroll(3, 10, 4, 3);
    assert_eq!(s.view(10, 4, "$ x", 3).live_first, None);
}

#[test]
fn crlf_tabs_backspace_and_controls() {
    let mut s = Screen::new();
    s.print(b"a\r\nb\tc\x08d\x07\x00e\n");
    assert_eq!(rows(&s, 40, 5, ""), ["a", "b       de", ""]);
    // A lone CR rewrites the line (progress bars).
    let mut s = Screen::new();
    s.print(b"10%\r50%\r100%\n");
    assert_eq!(rows(&s, 40, 5, "")[0], "100%");
}

#[test]
fn escape_sequences_are_swallowed() {
    let mut s = Screen::new();
    s.print(b"\x1b[31mred\x1b[0m \x1b]0;title\x07ok\x1b[2");
    s.print(b"Jx\n");
    assert_eq!(rows(&s, 40, 5, "")[0], "red okx");
    // A lone ESC and an unknown two-byte sequence.
    let mut s = Screen::new();
    s.print(b"a\x1bcb\x1b");
    assert_eq!(rows(&s, 40, 5, "")[0], "ab");
}

#[test]
fn utf8_is_decoded_even_when_split_between_prints() {
    let mut s = Screen::new();
    let bytes = "ação €".as_bytes();
    for chunk in bytes.chunks(1) {
        s.print(chunk);
    }
    s.print(b"\n");
    assert_eq!(rows(&s, 40, 5, "")[0], "ação €");
    // Invalid bytes become U+FFFD and never stall the decoder.
    let mut s = Screen::new();
    s.print(b"a\xffb\xc3(c\xe2\x82");
    s.print(b"d\n");
    assert_eq!(rows(&s, 40, 5, "")[0], "a\u{FFFD}b\u{FFFD}(c\u{FFFD}d");
}

#[test]
fn memory_is_bounded_by_lines_chars_and_line_length() {
    let mut s = Screen::new();
    for i in 0..(MAX_LINES + 100) {
        s.print(format!("n{i}\n").as_bytes());
    }
    assert_eq!(s.line_count(), MAX_LINES);
    assert_eq!(rows(&s, 40, 3, "")[0], format!("n{}", MAX_LINES + 98));
    // One enormous line is cut into pieces of MAX_LINE_CHARS.
    let mut s = Screen::new();
    s.print(&vec![b'x'; MAX_LINE_CHARS * 3 + 5]);
    assert_eq!(s.line_count(), 4);
    assert_eq!(s.char_count(), MAX_LINE_CHARS * 3 + 5);
    // Many long lines hit the character cap.
    let mut s = Screen::new();
    for _ in 0..(MAX_CHARS / 1000 + 500) {
        s.print(&vec![b'y'; 1000]);
        s.print(b"\n");
    }
    assert!(s.char_count() <= MAX_CHARS + 1000);
}

#[test]
fn a_hostile_flood_stays_fast_and_bounded() {
    let mut s = Screen::new();
    let mut junk = Vec::new();
    let mut x: u32 = 12345;
    for _ in 0..200_000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        junk.push(x as u8);
    }
    s.print(&junk);
    assert!(s.char_count() <= MAX_CHARS + MAX_LINE_CHARS);
    let v = s.view(80, 24, "$ ", 2);
    assert!(v.rows.len() <= 24);
    s.scroll(10_000_000, 80, 24, 2);
    s.scroll(-3, 80, 24, 2);
    let _ = s.view(1, 1, "", 0);
}

#[test]
fn clear_forgets_everything() {
    let mut s = Screen::new();
    s.print(b"a\nb");
    s.scroll(1, 10, 1, 0);
    s.clear();
    assert_eq!(s.line_count(), 0);
    assert!(!s.is_scrolled());
    assert_eq!(rows(&s, 10, 3, "$"), ["$"]);
}

#[test]
fn degenerate_window_sizes_do_not_panic() {
    let mut s = Screen::new();
    s.print(b"hello world\n");
    for (c, r) in [(0, 0), (1, 1), (0, 5), (5, 0), (1, 100)] {
        let v = s.view(c, r, "$ x", 3);
        assert!(v.rows.len() <= r.max(1));
    }
}

#[test]
fn columns_fill_down_then_across() {
    let names: Vec<String> = ["a", "bb", "ccc", "d", "e"].map(String::from).to_vec();
    // cell = 3 + 2 = 5; 12 columns fit 2 per row -> 3 rows.
    assert_eq!(columnize(&names, 12), "a    d\nbb   e\nccc\n");
    assert_eq!(columnize(&names, 4), "a\nbb\nccc\nd\ne\n");
    assert_eq!(columnize(&[], 80), "");
    let one = vec!["only".to_string()];
    assert_eq!(columnize(&one, 80), "only\n");
    // Wide enough for everything: one row, no trailing spaces.
    assert_eq!(columnize(&names, 80), "a    bb   ccc  d    e\n");
}
