use super::*;

#[test]
fn two_megabyte_file_loads_and_round_trips() {
    let doc = big_doc(2 * 1024 * 1024);
    let mut e = Editor::from_bytes(&doc);
    assert!(e.line_count() > 30_000);
    assert_eq!(e.to_bytes(), doc);
    e.goto_line(e.line_count() / 2);
    type_str(&mut e, "MIDDLE");
    assert_eq!(e.len_bytes(), doc.len() + 6);
    e.undo();
    assert_eq!(e.to_bytes(), doc);
    check(&e);
}

#[test]
fn search_in_two_megabyte_file_is_fast() {
    let mut doc = big_doc(2 * 1024 * 1024);
    doc.extend_from_slice(b"NEEDLE_AT_THE_END");
    let mut e = Editor::from_bytes(&doc);
    let t = std::time::Instant::now();
    e.set_search("needle_at_the_end", false);
    assert!(e.find_next());
    assert_eq!(e.selected_bytes(), b"NEEDLE_AT_THE_END");
    assert!(e.find_prev());
    e.set_search("needle_at_the_end", true);
    assert!(!e.find_next());
    assert!(t.elapsed().as_secs_f64() < 5.0, "search too slow");
}

#[test]
fn replace_all_in_big_file() {
    let doc = big_doc(1024 * 1024);
    let mut e = Editor::from_bytes(&doc);
    e.set_search("fox", true);
    e.set_replacement("wolf");
    let n = e.replace_all();
    assert_eq!(n, e.line_count() - 1);
    assert_eq!(e.len_bytes(), doc.len() + n);
    e.undo();
    assert_eq!(e.to_bytes(), doc);
    check(&e);
}

#[test]
fn single_two_megabyte_line_works() {
    let doc = vec![b'q'; 2 * 1024 * 1024];
    let mut e = Editor::from_bytes(&doc);
    assert_eq!(e.line_count(), 1);
    e.move_doc_end(false);
    assert_eq!(e.cursor().1, 2 * 1024 * 1024);
    type_str(&mut e, "!");
    e.set_cursor(0, 1_000_000);
    e.backspace();
    assert_eq!(e.len_bytes(), 2 * 1024 * 1024);
    let row = e.visible_rows().next().unwrap();
    assert!(row.cells().count() <= 80);
    check(&e);
}

#[test]
fn typing_at_the_start_of_a_big_file_stays_correct() {
    let doc = big_doc(512 * 1024);
    let mut e = Editor::from_bytes(&doc);
    for _ in 0..200 {
        e.insert_char('x');
        e.newline();
    }
    assert_eq!(
        e.line_count(),
        doc.iter().filter(|&&b| b == b'\n').count() + 201
    );
    check(&e);
}

#[test]
fn many_small_edits_keep_index_consistent() {
    let mut e = Editor::new();
    for i in 0..500 {
        type_str(&mut e, "line");
        e.newline();
        if i % 3 == 0 {
            e.move_vertical(-1, false);
            e.move_end(false);
        }
    }
    check(&e);
}
