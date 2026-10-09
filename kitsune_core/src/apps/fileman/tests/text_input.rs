use super::*;

#[test]
fn text_input_edits() {
    let mut t = TextInput::new(b"abc", 255);
    assert_eq!(t.text(), b"abc");
    t.insert(b'd');
    t.left();
    t.left();
    t.insert(b'X');
    assert_eq!(t.text(), b"abXcd");
    t.backspace();
    assert_eq!(t.text(), b"abcd");
    t.delete();
    assert_eq!(t.text(), b"abd");
    t.home();
    t.backspace();
    assert_eq!(t.text(), b"abd");
    t.end();
    t.delete();
    assert_eq!(t.text(), b"abd");
    assert_eq!(t.caret(), 3);
    t.clear();
    assert_eq!(t.text(), b"");
}

#[test]
fn text_input_refuses_slash_controls_and_overflow() {
    let mut t = TextInput::new(b"", 4);
    for b in *b"a/b\n\0cdef" {
        t.insert(b);
    }
    assert_eq!(t.text(), b"abcd");
    let t = TextInput::new(b"abcdef", 3);
    assert_eq!(t.text(), b"abc");
}

#[test]
fn text_input_moves_over_utf8_characters() {
    let mut t = TextInput::new("aé日".as_bytes(), 255);
    t.left();
    assert_eq!(t.caret(), 3); // before 日
    t.left();
    assert_eq!(t.caret(), 1); // before é
    t.right();
    assert_eq!(t.caret(), 3);
    t.backspace();
    assert_eq!(t.text(), "a日".as_bytes());
    t.delete();
    assert_eq!(t.text(), b"a");
    assert_eq!(t.caret_column(), 1);
}
