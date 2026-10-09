use super::*;

#[test]
fn dead_keys_compose_accents() {
    assert_eq!(compose_all("'a"), "\u{e1}");
    assert_eq!(compose_all("'c"), "\u{e7}");
    assert_eq!(compose_all("~a~o"), "\u{e3}\u{f5}");
    assert_eq!(compose_all("^e"), "\u{ea}");
    assert_eq!(compose_all("`a"), "\u{e0}");
    assert_eq!(compose_all("\"u"), "\u{fc}");
    assert_eq!(compose_all("'A"), "\u{c1}");
    assert_eq!(compose_all("~n"), "\u{f1}");
}

#[test]
fn dead_key_before_space_or_itself_is_literal() {
    assert_eq!(compose_all("' "), "'");
    assert_eq!(compose_all("''"), "'");
    assert_eq!(compose_all("~ "), "~");
}

#[test]
fn dead_key_before_a_non_composing_letter_gives_both() {
    assert_eq!(compose_all("'t"), "'t");
    assert_eq!(compose_all("don't"), "don't");
    assert_eq!(compose_all("~b"), "~b");
}

#[test]
fn trailing_dead_key_is_flushed() {
    assert_eq!(compose_all("ok'"), "ok'");
}

#[test]
fn two_dead_keys_in_a_row() {
    assert_eq!(compose_all("'~a"), "'\u{e3}");
}

#[test]
fn composition_through_the_field() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    for b in "a'c~ao".bytes() {
        st.on_key(&forms, Key::Char(b));
    }
    // a, c-cedilla ... wait: "'c" -> ç, "~a" -> ã, "o"
    assert_eq!(st.value(0, 0), "a\u{e7}\u{e3}o");
    assert_eq!(st.query(&forms, 0, None).unwrap(), "q=a%C3%A7%C3%A3o");
}

#[test]
fn pending_dead_key_comes_out_before_enter() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.on_key(&forms, Key::Char(b'x'));
    st.on_key(&forms, Key::Char(b'\''));
    assert_eq!(st.value(0, 0), "x");
    st.on_key(&forms, Key::Enter);
    assert_eq!(st.value(0, 0), "x'");
}

#[test]
fn accent_table_has_no_accidents() {
    assert_eq!(accent('\'', 'a'), Some('\u{e1}'));
    assert_eq!(accent('\'', 'b'), None);
    assert_eq!(accent('x', 'a'), None);
    assert_eq!(accent('^', 'y'), None);
}
