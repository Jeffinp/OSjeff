use super::*;

const PAGE: &str = "<form method=post action=/e>\
<select name=pais><option value=br>Brasil<option value=pt selected>Portugal<option>Argentina</select>\
<textarea name=msg rows=3 cols=30>\nOla\nmundo</textarea>\
<input type=submit name=ok value=Ok></form>";

#[test]
fn a_select_lists_its_options_and_starts_on_the_selected_one() {
    let forms = form_page(PAGE).forms;
    let f = &forms[0].fields[0];
    assert_eq!(f.kind, FieldKind::Select);
    let labels: Vec<_> = f.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Brasil", "Portugal", "Argentina"]);
    // A missing value attribute falls back to the text.
    assert_eq!(f.options[2].value, "Argentina");
    assert_eq!(f.selected, 1);
    let st = FormState::new(&forms);
    assert_eq!(st.select_label(&forms, 0, 0), "Portugal");
}

#[test]
fn a_long_unclosed_option_list_is_not_nested_past_the_depth_limit() {
    let mut html = String::from("<form><select name=n>");
    for i in 0..200 {
        html.push_str(&format!("<option value={i}>Item {i}\n"));
    }
    html.push_str("</select></form>");
    let forms = form_page(&html).forms;
    let f = &forms[0].fields[0];
    assert_eq!(f.options.len(), 200);
    assert_eq!(f.options[199].label, "Item 199");
}

#[test]
fn keys_and_letters_choose_options() {
    let forms = form_page(PAGE).forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.on_key(&forms, Key::Down);
    assert_eq!(st.select_label(&forms, 0, 0), "Argentina");
    st.on_key(&forms, Key::Down);
    assert_eq!(st.select_label(&forms, 0, 0), "Brasil", "wraps round");
    st.on_key(&forms, Key::Up);
    assert_eq!(st.select_label(&forms, 0, 0), "Argentina");
    st.on_key(&forms, Key::Char(b'p'));
    assert_eq!(st.select_label(&forms, 0, 0), "Portugal");
    assert!(st.step_select(&forms, 0, 0, 1));
    assert!(!st.step_select(&forms, 0, 2, 1), "not a select");
}

#[test]
fn a_select_sends_the_value_of_the_chosen_option() {
    let forms = form_page(PAGE).forms;
    let mut st = FormState::new(&forms);
    let body = |st: &FormState| st.submission(&forms, 0, Some(2)).unwrap().body.unwrap();
    assert_eq!(body(&st), "pais=pt&msg=Ola%0D%0Amundo&ok=Ok");
    st.step_select(&forms, 0, 0, -1);
    assert!(body(&st).starts_with("pais=br&"));
}

#[test]
fn a_textarea_edits_several_lines() {
    let forms = form_page(PAGE).forms;
    let mut st = FormState::new(&forms);
    assert_eq!(
        st.value(0, 1),
        "Ola\nmundo",
        "the first line break is not content"
    );
    st.set_focus(&forms, 0, 1);
    // Enter adds a line break instead of submitting.
    assert_eq!(st.on_key(&forms, Key::Enter), FormOutcome::Changed);
    typed(&forms, &mut st, "!");
    assert_eq!(st.value(0, 1), "Ola\nmundo\n!");
    st.on_key(&forms, Key::Up);
    st.on_key(&forms, Key::Home);
    typed(&forms, &mut st, ">");
    assert_eq!(st.value(0, 1), "Ola\n>mundo\n!");
    st.on_key(&forms, Key::Up);
    st.on_key(&forms, Key::End);
    typed(&forms, &mut st, "?");
    assert_eq!(st.value(0, 1), "Ola?\n>mundo\n!");
    // The text of a textarea may be longer than a one-line value.
    let long = "x".repeat(MAX_VALUE + 50);
    assert!(st.insert_str(&forms, &long));
    assert!(st.value(0, 1).len() > MAX_VALUE + 50);
}

#[test]
fn a_single_line_field_still_drops_line_breaks() {
    let forms = form_page("<form><input name=q></form>").forms;
    let mut st = FormState::new(&forms);
    st.set_focus(&forms, 0, 0);
    st.insert_str(&forms, "a\r\nb\tc");
    assert_eq!(st.value(0, 0), "abc");
}

#[test]
fn the_boxes_are_sized_by_their_content() {
    let p = form_page(PAGE);
    let by = |k: FieldKind| p.fields.iter().find(|f| f.kind == k).copied().unwrap();
    let (sel, area) = (by(FieldKind::Select), by(FieldKind::TextArea));
    assert!(sel.w > 60 && sel.h > 0);
    assert!(
        area.h > sel.h * 2,
        "three lines tall: {} vs {}",
        area.h,
        sel.h
    );
    assert!(area.line_h > 0 && area.pad_y > 0);
}

#[test]
fn rows_wrap_at_spaces_and_inside_long_words() {
    use crate::browsing::web::form::{caret_row, wrap_rows};
    let w = |_: char| 10; // every character 10 px wide
    let text = "aa bb cc\n\nxxxxxxxx yy";
    let rows = wrap_rows(text, 50, w);
    let shown: Vec<&str> = rows.iter().map(|&(a, b)| &text[a..b]).collect();
    // 5 characters fit a row (a space may hang past the edge).
    assert_eq!(shown, ["aa bb ", "cc", "", "xxxxx", "xxx ", "yy"]);
    // Every character is in exactly one row.
    let joined: usize = rows.iter().map(|&(a, b)| b - a).sum();
    assert_eq!(joined + 2, text.len(), "all but the two line breaks");
    assert_eq!(caret_row(&rows, 0), (0, 0));
    assert_eq!(caret_row(&rows, 3), (0, 3));
    assert_eq!(
        caret_row(&rows, 6),
        (1, 0),
        "after a soft break it is on the next row"
    );
    assert_eq!(caret_row(&rows, 8), (1, 2), "the end of a line stays on it");
    assert_eq!(caret_row(&rows, text.len()), (5, 2));
    assert_eq!(wrap_rows("", 50, w), [(0, 0)]);
}
