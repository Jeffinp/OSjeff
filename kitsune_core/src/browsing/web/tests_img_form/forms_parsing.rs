use super::*;

#[test]
fn form_fields_are_collected_in_order() {
    let p = form_page(
        "<form action='/s' method=get><input name=q value='oi'><input type=hidden name=h value=1><input type=submit value=Buscar></form>",
    );
    assert_eq!(p.forms.len(), 1);
    let f = &p.forms[0];
    assert_eq!(f.action, "/s");
    assert!(!f.post);
    let kinds: Vec<FieldKind> = f.fields.iter().map(|x| x.kind).collect();
    assert_eq!(
        kinds,
        [FieldKind::Text, FieldKind::Hidden, FieldKind::Submit]
    );
    assert_eq!(f.fields[0].value, "oi");
    assert_eq!(f.fields[2].label, "Buscar");
    // The hidden control has no box.
    assert_eq!(p.fields.len(), 2);
}

#[test]
fn method_post_is_flagged() {
    let p = form_page("<form method=POST action=/x><input name=a></form>");
    assert!(p.forms[0].post);
    let p = form_page("<form method=dialog><input name=a></form>");
    assert!(!p.forms[0].post);
}

#[test]
fn button_is_a_submit_with_its_text() {
    let p = form_page("<form><input name=q><button name=go value=1>Ir &amp; ver</button></form>");
    let f = &p.forms[0].fields[1];
    assert_eq!(f.kind, FieldKind::Submit);
    assert_eq!(f.label, "Ir & ver");
    assert_eq!(f.name, "go");
}

#[test]
fn button_type_button_is_a_button_that_submits_nothing() {
    let p =
        form_page("<form><input name=q value=a><button type=button name=b>Nada</button></form>");
    assert_eq!(p.forms[0].fields[1].kind, FieldKind::PushButton);
    assert_eq!(p.forms[0].fields[1].label, "Nada");
    let st = FormState::new(&p.forms);
    assert_eq!(st.query(&p.forms, 0, Some(1)).unwrap(), "q=a");
}

#[test]
fn unsupported_controls_are_skipped_and_the_others_are_not_text() {
    let p = form_page(
        "<form><input type=file name=f><input type=range name=g><select name=s><option>um</option></select><textarea name=t>txt</textarea></form>",
    );
    // Files and ranges are not drawn; a select and a textarea are controls now.
    let kinds: Vec<_> = p.forms[0].fields.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FieldKind::Select, FieldKind::TextArea]);
    // The option text and textarea content are not rendered as page text.
    let t = texts(&p);
    assert!(!t.iter().any(|s| s == "um" || s == "txt"), "{t:?}");
}

#[test]
fn controls_outside_a_form_are_ignored() {
    let p = form_page("<input name=q><p>x</p>");
    assert!(p.forms.is_empty() && p.fields.is_empty());
}

#[test]
fn input_types_that_are_text() {
    for ty in [
        "text", "search", "url", "email", "tel", "number", "TEXT", "",
    ] {
        let p = form_page(&format!("<form><input type='{ty}' name=a></form>"));
        assert_eq!(p.forms[0].fields[0].kind, FieldKind::Text, "{ty:?}");
    }
    let p = form_page("<form><input type=password name=a></form>");
    assert_eq!(p.forms[0].fields[0].kind, FieldKind::Password);
}

#[test]
fn size_attribute_sets_the_width() {
    let a = form_page("<form><input name=a size=10></form>");
    let b = form_page("<form><input name=a size=30></form>");
    assert!(b.fields[0].w > a.fields[0].w);
    assert_eq!(a.forms[0].fields[0].size, 10);
    let c = form_page("<form><input name=a size=9999></form>");
    assert_eq!(c.forms[0].fields[0].size, 80);
}

#[test]
fn two_forms_get_separate_indices() {
    let p = form_page(
        "<form action=a><input name=x></form><form action=b><input name=y><input name=z></form>",
    );
    assert_eq!(p.forms.len(), 2);
    assert_eq!(p.forms[1].fields.len(), 2);
    assert_eq!(p.fields[2].form, 1);
    assert_eq!(p.fields[2].field, 1);
}

#[test]
fn forms_and_fields_are_capped() {
    let mut html = String::new();
    for _ in 0..MAX_FORMS + 5 {
        html.push_str("<form><input name=a></form>");
    }
    assert!(form_page(&html).forms.len() <= MAX_FORMS);
    let mut one = String::from("<form>");
    for _ in 0..MAX_FIELDS + 10 {
        one.push_str("<input name=a>");
    }
    assert!(form_page(&one).forms[0].fields.len() <= MAX_FIELDS);
}

#[test]
fn field_boxes_hit_test() {
    let p = form_page("<form><input name=q size=10></form>");
    let b = p.fields[0];
    assert_eq!(p.field_at(b.x + 2, b.y + 2).map(|f| f.field), Some(0));
    assert!(p.field_at(b.x + b.w + 5, b.y).is_none());
}

#[test]
fn value_attribute_keeps_utf8() {
    let p = form_page("<form><input name=q value='a\u{e7}\u{e3}o &eacute;'></form>");
    assert_eq!(p.forms[0].fields[0].value, "a\u{e7}\u{e3}o \u{e9}");
}
