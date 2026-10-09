use super::*;

#[test]
fn gap_insert_and_read() {
    let mut g = GapBuffer::new();
    g.insert(0, b"hello");
    g.insert(5, b" world");
    g.insert(5, b",");
    assert_eq!(g.to_vec(), b"hello, world");
    assert_eq!(g.len(), 12);
}

#[test]
fn gap_delete_returns_removed() {
    let mut g = GapBuffer::from_bytes(b"abcdef");
    assert_eq!(g.delete(1, 3), b"bcd");
    assert_eq!(g.to_vec(), b"aef");
    assert_eq!(g.delete(10, 3), b"");
    assert_eq!(g.delete(1, 99), b"ef");
    assert_eq!(g.to_vec(), b"a");
}

#[test]
fn gap_get_across_gap() {
    let mut g = GapBuffer::from_bytes(b"abcdef");
    g.move_gap(3);
    assert_eq!(g.get(2), Some(b'c'));
    assert_eq!(g.get(3), Some(b'd'));
    assert_eq!(g.get(6), None);
}

#[test]
fn gap_copy_range_spanning_gap() {
    let mut g = GapBuffer::from_bytes(b"0123456789");
    g.move_gap(5);
    assert_eq!(g.copy_range(3, 8), b"34567");
    assert_eq!(g.copy_range(0, 100), b"0123456789");
    assert_eq!(g.copy_range(8, 2), b"");
}

#[test]
fn gap_grows_for_large_inserts() {
    let mut g = GapBuffer::new();
    let big = alloc::vec![b'x'; 10_000];
    g.insert(0, &big);
    g.insert(5000, &big);
    assert_eq!(g.len(), 20_000);
}

#[test]
fn gap_make_contiguous() {
    let mut g = GapBuffer::from_bytes(b"abc");
    g.insert(1, b"ZZ");
    assert_eq!(g.make_contiguous(), b"aZZbc");
}

#[test]
fn decode_ascii_and_multibyte() {
    assert_eq!(decode(b"a"), ('a', 1));
    assert_eq!(decode("é".as_bytes()), ('é', 2));
    assert_eq!(decode("€".as_bytes()), ('€', 3));
    assert_eq!(decode("😀".as_bytes()), ('😀', 4));
}

#[test]
fn decode_invalid_never_panics() {
    assert_eq!(decode(&[0xFF]), (REPLACEMENT, 1));
    assert_eq!(decode(&[0x80]), (REPLACEMENT, 1));
    assert_eq!(decode(&[0xE2, 0x82]), (REPLACEMENT, 1));
    assert_eq!(decode(&[0xC0, 0x80]), (REPLACEMENT, 1));
    assert_eq!(decode(&[0xED, 0xA0, 0x80]), (REPLACEMENT, 1));
    assert_eq!(decode(&[]), (REPLACEMENT, 1));
}

#[test]
fn line_index_basic() {
    let t = TextBuf::from_bytes(b"ab\ncd\n\nefg");
    assert_eq!(t.line_count(), 4);
    assert_eq!(t.line_start(1), 3);
    assert_eq!(t.line_end(0), 2);
    assert_eq!(t.line_end(2), 6);
    assert_eq!(t.line_end(3), 10);
    assert_eq!(t.line_of(4), 1);
    assert_eq!(t.line_of(10), 3);
}

#[test]
fn trailing_newline_makes_empty_last_line() {
    let t = TextBuf::from_bytes(b"a\n");
    assert_eq!(t.line_count(), 2);
    assert_eq!(t.line_end(1), 2);
}

#[test]
fn crlf_terminator_is_not_content() {
    let t = TextBuf::from_bytes(b"ab\r\ncd");
    assert_eq!(t.line_end(0), 2);
    assert_eq!(t.eol_len(0), 2);
    assert_eq!(t.eol_len(1), 0);
    assert_eq!(t.next_line_start(0), 4);
}

#[test]
fn lone_cr_stays_content() {
    let t = TextBuf::from_bytes(b"a\rb\n");
    assert_eq!(t.line_end(0), 3);
    assert_eq!(t.eol_len(0), 1);
}

#[test]
fn replace_keeps_index_consistent() {
    let mut t = TextBuf::from_bytes(b"one\ntwo\nthree");
    t.replace(4, 4, b"X\nY\nZ\n");
    let fresh = TextBuf::from_bytes(&t.to_vec());
    assert_eq!(t.lines, fresh.lines);
    assert_eq!(t.to_vec(), b"one\nX\nY\nZ\nthree");
}

#[test]
fn replace_deleting_newlines_merges_lines() {
    let mut t = TextBuf::from_bytes(b"a\nb\nc\nd");
    t.replace(1, 4, b"");
    assert_eq!(t.to_vec(), b"a\nd");
    assert_eq!(t.line_count(), 2);
}

#[test]
fn replace_at_end_and_start() {
    let mut t = TextBuf::new();
    t.replace(0, 0, b"x\n");
    t.replace(2, 0, b"y");
    t.replace(0, 0, b"\n");
    assert_eq!(t.to_vec(), b"\nx\ny");
    assert_eq!(t.line_count(), 3);
}

#[test]
fn boundaries_utf8() {
    let t = TextBuf::from_bytes("a€b😀".as_bytes());
    assert_eq!(t.next_boundary(0), 1);
    assert_eq!(t.next_boundary(1), 4);
    assert_eq!(t.prev_boundary(4), 1);
    assert_eq!(t.prev_boundary(5), 4);
    assert_eq!(t.prev_boundary(9), 5);
    assert_eq!(t.next_boundary(9), 9);
    assert_eq!(t.prev_boundary(0), 0);
}

#[test]
fn boundaries_invalid_bytes_are_single_chars() {
    let t = TextBuf::from_bytes(&[b'a', 0xFF, 0x80, 0x80, b'b']);
    assert_eq!(t.next_boundary(1), 2);
    assert_eq!(t.prev_boundary(4), 3);
    assert_eq!(t.prev_boundary(3), 2);
    assert_eq!(t.col_of(5), 5);
}

#[test]
fn boundaries_truncated_sequence() {
    let t = TextBuf::from_bytes(&[0xE2, 0x82]);
    assert_eq!(t.next_boundary(0), 1);
    assert_eq!(t.prev_boundary(2), 1);
    assert_eq!(t.prev_boundary(1), 0);
}

#[test]
fn forward_backward_boundaries_agree_on_garbage() {
    // Deterministic pseudo-random bytes: walking forward then backward over
    // every boundary must visit the same positions.
    let mut x: u32 = 12345;
    let mut data = Vec::new();
    for _ in 0..4000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        data.push((x >> 8) as u8);
    }
    let t = TextBuf::from_bytes(&data);
    let mut fwd = alloc::vec![0usize];
    let mut p = 0;
    while p < t.len() {
        p = t.next_boundary(p);
        fwd.push(p);
    }
    let mut p = t.len();
    let mut i = fwd.len() - 1;
    while p > 0 {
        p = t.prev_boundary(p);
        i -= 1;
        assert_eq!(fwd[i], p);
    }
}

#[test]
fn col_and_pos_roundtrip() {
    let t = TextBuf::from_bytes("é€x\nabc".as_bytes());
    assert_eq!(t.col_of(5), 2);
    assert_eq!(t.pos_of(0, 2), 5);
    assert_eq!(t.pos_of(0, 99), t.line_end(0));
    assert_eq!(t.pos_of(1, 2), t.line_start(1) + 2);
}

#[test]
fn display_columns_expand_tabs() {
    let t = TextBuf::from_bytes(b"a\tb\t\tc");
    assert_eq!(t.dc_of(2, 4), 4);
    assert_eq!(t.dc_of(4, 4), 8);
    assert_eq!(t.dc_of(5, 4), 12);
    assert_eq!(t.line_width(0, 4), 13);
    assert_eq!(t.pos_at_dc(0, 5, 4), 3);
    assert_eq!(t.pos_at_dc(0, 3, 4), 1);
    assert_eq!(t.pos_at_dc(0, 4, 4), 2);
}

#[test]
fn set_replaces_content() {
    let mut t = TextBuf::from_bytes(b"a\nb");
    t.set(b"x\ny\nz");
    assert_eq!(t.line_count(), 3);
    assert_eq!(t.to_vec(), b"x\ny\nz");
}
