use super::*;

#[test]
fn typing_edits_the_focused_field() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    assert!(st.set_focus(&forms, 0, 1));
    typed(&forms, &mut st, "rust os");
    assert_eq!(st.value(0, 1), "rust os");
    st.on_key(&forms, Key::Backspace);
    assert_eq!(st.value(0, 1), "rust o");
    st.on_key(&forms, Key::Home);
    st.on_key(&forms, Key::Delete);
    assert_eq!(st.value(0, 1), "ust o");
    st.on_key(&forms, Key::End);
    typed(&forms, &mut st, "!");
    assert_eq!(st.value(0, 1), "ust o!");
}

#[test]
fn caret_moves_with_arrows() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 1);
    typed(&forms, &mut st, "abc");
    st.on_key(&forms, Key::Left);
    st.on_key(&forms, Key::Left);
    typed(&forms, &mut st, "X");
    assert_eq!(st.value(0, 1), "aXbc");
    st.on_key(&forms, Key::Right);
    st.on_key(&forms, Key::Right);
    st.on_key(&forms, Key::Right);
    st.on_key(&forms, Key::Right);
    typed(&forms, &mut st, "Y");
    assert_eq!(st.value(0, 1), "aXbcY");
}

#[test]
fn keys_without_focus_are_ignored() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    assert_eq!(st.on_key(&forms, Key::Char(b'a')), FormOutcome::Ignored);
}

#[test]
fn enter_submits_and_esc_blurs() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 1);
    assert_eq!(
        st.on_key(&forms, Key::Enter),
        FormOutcome::Submit {
            form: 0,
            submitter: None
        }
    );
    assert_eq!(st.on_key(&forms, Key::Esc), FormOutcome::Blur);
    assert_eq!(st.focus(), None);
}

#[test]
fn enter_on_a_button_submits_with_it() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 2);
    assert_eq!(
        st.on_key(&forms, Key::Enter),
        FormOutcome::Submit {
            form: 0,
            submitter: Some(2)
        }
    );
}

#[test]
fn tab_walks_visible_controls_and_leaves() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    assert!(st.tab(&forms, false));
    assert_eq!(st.focus(), Some((0, 1))); // the hidden control is skipped
    assert!(st.tab(&forms, false));
    assert_eq!(st.focus(), Some((0, 2)));
    assert!(!st.tab(&forms, false));
    assert_eq!(st.focus(), None);
    assert!(st.tab(&forms, true));
    assert_eq!(st.focus(), Some((0, 2)));
    assert!(st.tab(&forms, true));
    assert!(!st.tab(&forms, true));
}

#[test]
fn tab_without_controls_does_nothing() {
    let mut st = FormState::new(&[]);
    assert!(!st.tab(&[], false));
}

#[test]
fn cannot_focus_hidden_or_missing() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    assert!(!st.set_focus(&forms, 0, 0));
    assert!(!st.set_focus(&forms, 5, 0));
    assert!(!st.set_focus(&forms, 0, 9));
}

#[test]
fn query_encodes_utf8_and_spaces() {
    let forms = search_form();
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 1);
    st.insert_str(&forms, "a\u{e7}\u{e3}o & caf\u{e9}");
    assert_eq!(
        st.query(&forms, 0, None).unwrap(),
        "lang=pt+br&q=a%C3%A7%C3%A3o+%26+caf%C3%A9"
    );
}

#[test]
fn query_includes_only_the_pressed_button() {
    let forms = search_form();
    let st = FormState::new(&forms);
    assert_eq!(st.query(&forms, 0, None).unwrap(), "lang=pt+br&q=");
    assert_eq!(st.query(&forms, 0, Some(2)).unwrap(), "lang=pt+br&q=&ok=Go");
}

#[test]
fn query_skips_unnamed_controls() {
    let forms = form_page("<form><input value=semnome><input name=n value=v></form>").forms;
    let st = FormState::new(&forms);
    assert_eq!(st.query(&forms, 0, None).unwrap(), "n=v");
}

#[test]
fn urlencode_rules() {
    let mut s = String::new();
    urlencode("Az09*-._ ~!'\"<>\u{20ac}", &mut s);
    assert_eq!(s, "Az09*-._+%7E%21%27%22%3C%3E%E2%82%AC");
    let mut e = String::new();
    urlencode("", &mut e);
    assert_eq!(e, "");
}

#[test]
fn target_replaces_the_action_query_and_fragment() {
    let forms = search_form();
    let st = FormState::new(&forms);
    assert_eq!(st.target(&forms, 0, None).unwrap(), "/busca?lang=pt+br&q=");
}

#[test]
fn empty_action_stays_on_the_page() {
    let forms = form_page("<form><input name=q value=x></form>").forms;
    let st = FormState::new(&forms);
    assert_eq!(st.target(&forms, 0, None).unwrap(), "?q=x");
}

#[test]
fn post_forms_are_refused_with_a_message() {
    let forms = form_page("<form method=post action=/p><input name=q></form>").forms;
    let st = FormState::new(&forms);
    let e = st.target(&forms, 0, None).unwrap_err();
    assert_eq!(e, FormError::Post);
    assert_eq!(e.message(), "Formulários POST não são suportados.");
}

#[test]
fn bad_form_index_is_an_error() {
    let st = FormState::new(&[]);
    assert_eq!(st.target(&[], 3, None), Err(FormError::NoForm));
}

#[test]
fn oversized_queries_are_refused() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.insert_str(&forms, &"\u{e7}".repeat(120)); // 120 * 6 encoded bytes
    assert_eq!(st.query(&forms, 0, None), Err(FormError::TooLong));
}

#[test]
fn values_are_capped() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.insert_str(&forms, &"x".repeat(MAX_VALUE + 50));
    assert_eq!(st.value(0, 0).len(), MAX_VALUE);
    st.insert_str(&forms, "\u{e9}");
    assert_eq!(st.value(0, 0).len(), MAX_VALUE);
    let long = format!("<form><input name=q value='{}'></form>", "y".repeat(1000));
    let forms = form_page(&long).forms;
    assert_eq!(FormState::new(&forms).value(0, 0).len(), MAX_VALUE);
}

#[test]
fn pasting_drops_control_characters() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.insert_str(&forms, "a\nb\r\tc\u{7f}d");
    assert_eq!(st.value(0, 0), "abcd");
}

#[test]
fn editing_multibyte_text_stays_on_char_boundaries() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.insert_str(&forms, "\u{e7}\u{e3}o");
    st.on_key(&forms, Key::Left);
    st.on_key(&forms, Key::Left);
    st.on_key(&forms, Key::Backspace);
    assert_eq!(st.value(0, 0), "\u{e3}o");
    st.on_key(&forms, Key::Delete);
    assert_eq!(st.value(0, 0), "o");
}
