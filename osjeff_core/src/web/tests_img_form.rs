//! Tests for images, forms, zoom, find and selection in the `web` engine.

use super::form::*;
use super::imgcache::{ImageLookup, ImgState, NoImages};
use super::layout::img_box;
use super::textops::CharPos;
use super::*;
use crate::Key;

struct Fixed(ImgState);

impl ImageLookup for Fixed {
    fn lookup(&self, _src: &str) -> ImgState {
        self.0
    }
}

fn lay(html: &str, w: i32, zoom: u16, lookup: &dyn ImageLookup) -> Page {
    Doc::parse(html.as_bytes()).layout(&Layout {
        width: w,
        zoom,
        images: lookup,
        metrics: &FixedAdvance,
    })
}

fn image_cmds(p: &Page) -> Vec<(i32, i32, i32, i32, usize)> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Image { x, y, w, h, idx } => Some((*x, *y, *w, *h, *idx)),
            _ => None,
        })
        .collect()
}

fn texts(p: &Page) -> Vec<String> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

// ---- sizing ----

#[test]
fn declared_size_wins() {
    assert_eq!(img_box(100, 50, ImgState::Pending, 600, 100), (100, 50));
    assert_eq!(
        img_box(100, 50, ImgState::Ready { w: 999, h: 999 }, 600, 100),
        (100, 50)
    );
}

#[test]
fn width_only_uses_the_aspect_ratio_when_known() {
    assert_eq!(
        img_box(100, 0, ImgState::Ready { w: 400, h: 200 }, 600, 100),
        (100, 50)
    );
}

#[test]
fn height_only_uses_the_aspect_ratio_when_known() {
    assert_eq!(
        img_box(0, 50, ImgState::Ready { w: 400, h: 200 }, 600, 100),
        (100, 50)
    );
}

#[test]
fn pending_with_one_side_guesses_4_to_3() {
    assert_eq!(img_box(200, 0, ImgState::Pending, 600, 100), (200, 150));
    assert_eq!(img_box(0, 90, ImgState::Pending, 600, 100), (120, 90));
}

#[test]
fn no_attributes_uses_the_natural_size() {
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 320, h: 240 }, 600, 100),
        (320, 240)
    );
}

#[test]
fn no_attributes_pending_is_a_default_box() {
    assert_eq!(img_box(0, 0, ImgState::Pending, 600, 100), (160, 120));
}

#[test]
fn a_box_wider_than_the_line_shrinks_keeping_the_ratio() {
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 1000, h: 500 }, 250, 100),
        (250, 125)
    );
    assert_eq!(img_box(800, 400, ImgState::Pending, 200, 100), (200, 100));
}

#[test]
fn zoom_scales_the_natural_size() {
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 100, h: 60 }, 900, 200),
        (200, 120)
    );
    assert_eq!(
        img_box(0, 0, ImgState::Ready { w: 100, h: 60 }, 900, 50),
        (50, 30)
    );
}

#[test]
fn failed_states_get_a_message_box() {
    for s in [
        ImgState::Unsupported,
        ImgState::Failed,
        ImgState::TooBig,
        ImgState::TooMany,
    ] {
        assert_eq!(img_box(0, 0, s, 600, 100), (240, 48));
    }
}

#[test]
fn sizes_are_never_zero_or_huge() {
    for (dw, dh) in [(0, 0), (1, 0), (0, 1), (4096, 4096)] {
        for s in [
            ImgState::Pending,
            ImgState::Ready { w: 1, h: 100000 },
            ImgState::Ready { w: 100000, h: 1 },
        ] {
            let (w, h) = img_box(dw, dh, s, 700, 300);
            assert!(
                (1..=20_000).contains(&w) && (1..=20_000).contains(&h),
                "{dw} {dh} {s:?} -> {w}x{h}"
            );
        }
    }
}

// ---- images in the layout ----

#[test]
fn pending_image_reserves_its_declared_box() {
    let p = lay(
        "<p><img src=a.png width=120 height=80></p>",
        600,
        100,
        &NoImages,
    );
    assert_eq!(p.images.len(), 1);
    assert_eq!(p.images[0].src, "a.png");
    // A grey placeholder rect of 120x80.
    assert!(p.cmds.iter().any(
        |c| matches!(c, Cmd::Rect { w: 120, h: 80, color, .. } if *color == Rgb(0xDD, 0xE1, 0xE8))
    ));
    assert!(image_cmds(&p).is_empty());
}

#[test]
fn ready_image_emits_an_image_command_of_the_natural_size() {
    let p = lay(
        "<img src=a.png>",
        600,
        100,
        &Fixed(ImgState::Ready { w: 200, h: 100 }),
    );
    let c = image_cmds(&p);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].2, c[0].3, c[0].4), (200, 100, 0));
}

#[test]
fn the_page_height_includes_the_picture() {
    let small = lay("<p>x</p>", 600, 100, &NoImages);
    let big = lay(
        "<p><img src=a.png width=100 height=300></p>",
        600,
        100,
        &NoImages,
    );
    assert!(big.height >= small.height + 250);
}

#[test]
fn layout_is_stable_when_the_image_matches_the_declared_size() {
    let html = "<p>before</p><p><img src=a.png width=100 height=60></p><p>after</p>";
    let a = lay(html, 600, 100, &NoImages);
    let b = lay(html, 600, 100, &Fixed(ImgState::Ready { w: 400, h: 240 }));
    assert_eq!(a.height, b.height);
}

#[test]
fn layout_reflows_when_the_natural_size_differs() {
    let html = "<p>before</p><p><img src=a.png></p><p>after</p>";
    let a = lay(html, 600, 100, &NoImages); // 160x120 placeholder
    let b = lay(html, 600, 100, &Fixed(ImgState::Ready { w: 300, h: 400 }));
    assert!(b.height > a.height);
}

#[test]
fn unsupported_image_shows_alt_and_the_message() {
    let p = lay(
        "<img src=a.jpg alt='Foto da praia'>",
        600,
        100,
        &Fixed(ImgState::Unsupported),
    );
    let t = texts(&p);
    assert!(t.iter().any(|s| s.contains("Foto da praia")), "{t:?}");
    assert!(t.iter().any(|s| s == "formato não suportado"), "{t:?}");
}

#[test]
fn alt_text_keeps_its_accents() {
    let p = lay(
        "<img src=a.jpg alt='A\u{e7}\u{e3}o &eacute; boa'>",
        600,
        100,
        &Fixed(ImgState::Unsupported),
    );
    assert_eq!(p.images[0].alt, "A\u{e7}\u{e3}o \u{e9} boa");
}

#[test]
fn alt_is_clipped_to_the_box() {
    let long = "x".repeat(300);
    let p = lay(
        &format!("<img src=a.jpg alt='{long}' width=100 height=50>"),
        600,
        100,
        &Fixed(ImgState::Failed),
    );
    for c in &p.cmds {
        if let Cmd::Text { x, w, text, .. } = c {
            assert!(*x + *w <= 100, "{} wide at {x}", text.len());
        }
    }
}

#[test]
fn image_inside_a_link_is_clickable() {
    let p = lay(
        "<a href='/go'><img src=a.png width=50 height=40></a>",
        600,
        100,
        &NoImages,
    );
    assert_eq!(p.links, ["/go"]);
    let h = p.hits[0];
    assert_eq!((h.w, h.h), (50, 40));
    assert_eq!(p.link_at(h.x + 5, h.y + 5), Some("/go"));
}

#[test]
fn image_without_a_link_adds_no_hit() {
    let p = lay("<img src=a.png width=50 height=40>", 600, 100, &NoImages);
    assert!(p.hits.is_empty());
}

#[test]
fn image_without_src_is_a_failed_box() {
    let p = lay("<img alt=nada>", 600, 100, &NoImages);
    assert!(texts(&p).iter().any(|s| s == "falha ao carregar"));
}

#[test]
fn src_entities_are_decoded() {
    let p = lay("<img src='a.png?x=1&amp;y=2'>", 600, 100, &NoImages);
    assert_eq!(p.images[0].src, "a.png?x=1&y=2");
}

#[test]
fn href_entities_are_decoded_too() {
    let p = lay("<a href='/s?a=1&amp;b=2'>x</a>", 600, 100, &NoImages);
    assert_eq!(p.links, ["/s?a=1&b=2"]);
}

#[test]
fn text_and_image_share_a_line_bottom_aligned() {
    let p = lay(
        "<p>ab <img src=a.png width=40 height=60></p>",
        600,
        100,
        &NoImages,
    );
    let (img_y, img_h) = p
        .cmds
        .iter()
        .find_map(|c| match c {
            Cmd::Rect { y, h: 60, .. } => Some((*y, 60)),
            _ => None,
        })
        .unwrap();
    let text_y = p
        .cmds
        .iter()
        .find_map(|c| match c {
            Cmd::Text { text, y, .. } if text == "ab" => Some(*y),
            _ => None,
        })
        .unwrap();
    assert!(text_y > img_y, "text sits at the bottom of the tall line");
    // The baseline (16 px below the text's top with the test metrics) is the picture's bottom.
    assert_eq!(text_y + 16, img_y + img_h);
}

#[test]
fn many_images_are_capped() {
    let html = "<img src=a.png>".repeat(MAX_IMAGES + 50);
    let p = lay(&html, 600, 100, &NoImages);
    assert!(p.images.len() <= MAX_IMAGES);
}

#[test]
fn wide_images_wrap_to_their_own_lines() {
    let p = lay(
        "<img src=a.png width=300 height=20><img src=b.png width=300 height=20>",
        400,
        100,
        &NoImages,
    );
    let ys: Vec<i32> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Rect { w: 300, y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(ys.len(), 2);
    assert!(ys[1] > ys[0]);
}

#[test]
fn zoom_scales_declared_image_sizes() {
    let p = lay("<img src=a.png width=100 height=40>", 900, 200, &NoImages);
    assert!(
        p.cmds
            .iter()
            .any(|c| matches!(c, Cmd::Rect { w: 200, h: 80, .. }))
    );
}

// ---- zoom ----

#[test]
fn zoom_steps_walk_up_and_down() {
    assert_eq!(zoom_in(100), 125);
    assert_eq!(zoom_in(300), 300);
    assert_eq!(zoom_out(100), 75);
    assert_eq!(zoom_out(50), 50);
    assert_eq!(zoom_in(110), 125);
    assert_eq!(zoom_out(110), 100);
    let mut z = 100;
    for _ in 0..20 {
        z = zoom_in(z);
    }
    assert_eq!(z, MAX_ZOOM);
    for _ in 0..20 {
        z = zoom_out(z);
    }
    assert_eq!(z, MIN_ZOOM);
}

#[test]
fn zoom_changes_the_font_size_and_height() {
    let html = "<p>hello world</p>";
    let size = |zoom| {
        let p = lay(html, 600, zoom, &NoImages);
        p.cmds
            .iter()
            .find_map(|c| match c {
                Cmd::Text { font, .. } => Some(font.size),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(size(100), 16);
    assert_eq!(size(50), 8);
    assert_eq!(size(150), 24);
    assert_eq!(size(200), 32);
    assert_eq!(size(300), 48);
    let h = |zoom| lay(html, 600, zoom, &NoImages).height;
    assert!(h(200) > h(100) && h(100) > h(50));
}

#[test]
fn zoom_100_matches_render() {
    let html = "<h1>T</h1><p>some <b>text</b> <a href=x>link</a></p><ul><li>a</li></ul>";
    let a = render(html.as_bytes(), 500);
    let b = lay(html, 500, 100, &NoImages);
    assert_eq!(a.height, b.height);
    assert_eq!(a.cmds.len(), b.cmds.len());
}

#[test]
fn zoom_out_of_range_is_clamped() {
    let a = lay("<p>x</p>", 600, 5000, &NoImages);
    let b = lay("<p>x</p>", 600, MAX_ZOOM, &NoImages);
    assert_eq!(a.height, b.height);
    let c = lay("<p>x</p>", 600, 1, &NoImages);
    let d = lay("<p>x</p>", 600, MIN_ZOOM, &NoImages);
    assert_eq!(c.height, d.height);
}

#[test]
fn hostile_lengths_at_max_zoom_do_not_overflow() {
    let html = "<div style='margin:4096px;padding:4096px'><p style='margin:4096px;padding:4096px'>x</p></div>";
    let p = lay(html, 600, 300, &NoImages);
    assert!(p.height > 0);
    for c in &p.cmds {
        if let Cmd::Text { x, y, .. } = c {
            assert!(*x >= 0 && *y >= 0);
        }
    }
}

// ---- title ----

#[test]
fn title_is_extracted_and_collapsed() {
    let d = Doc::parse(
        "<html><head><title> Ol\u{e1}  mundo </title></head><body>x</body></html>".as_bytes(),
    );
    assert_eq!(d.title(), "Ol\u{e1} mundo");
}

#[test]
fn missing_or_empty_title() {
    assert_eq!(Doc::parse(b"<p>x</p>").title(), "");
    assert_eq!(Doc::parse(b"<title>  </title>").title(), "");
}

#[test]
fn title_is_not_painted() {
    let p = render(b"<title>Segredo</title><p>visivel</p>", 600);
    assert!(!texts(&p).iter().any(|t| t == "Segredo"));
}

// ---- forms: parsing ----

fn form_page(html: &str) -> Page {
    lay(html, 700, 100, &NoImages)
}

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
fn unsupported_controls_are_skipped() {
    let p = form_page(
        "<form><input type=file name=f><input type=range name=g><select name=s><option>um</option></select><textarea name=t>txt</textarea></form>",
    );
    assert!(p.forms[0].fields.is_empty());
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

// ---- forms: state and query ----

fn search_form() -> Vec<FormInfo> {
    form_page(
        "<form action='/busca?velho=1#x' method=get><input type=hidden name=lang value='pt br'><input name=q><input type=submit name=ok value=Go></form>",
    )
    .forms
}

fn typed(forms: &[FormInfo], st: &mut FormState, s: &str) {
    for b in s.bytes() {
        st.on_key(forms, Key::Char(b));
    }
}

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

// ---- dead keys ----

fn compose_all(s: &str) -> String {
    let mut c = Compose::new();
    let mut out = String::new();
    for ch in s.chars() {
        out.extend(c.feed(ch).iter());
    }
    out.extend(c.flush().iter());
    out
}

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

// ---- find in page ----

fn text_page() -> Page {
    render(
        b"<p>The quick brown fox</p><p>jumps over the lazy dog. The end.</p>",
        600,
    )
}

#[test]
fn find_is_case_insensitive_and_counts_matches() {
    let p = text_page();
    assert_eq!(p.find("the", &FixedAdvance).len(), 3);
    assert_eq!(p.find("THE", &FixedAdvance).len(), 3);
    assert_eq!(p.find("fox", &FixedAdvance).len(), 1);
    assert_eq!(p.find("zebra", &FixedAdvance).len(), 0);
    assert!(p.find("", &FixedAdvance).is_empty());
}

#[test]
fn find_matches_inside_a_word_with_a_precise_box() {
    let p = render(b"<p>abcdef</p>", 600);
    let m = p.find("cd", &FixedAdvance);
    assert_eq!(m.len(), 1);
    let s = m[0][0];
    assert_eq!(s.w, 2 * 8); // two characters of 8 px
    let word_x = p
        .cmds
        .iter()
        .find_map(|c| match c {
            Cmd::Text { x, .. } => Some(*x),
            _ => None,
        })
        .unwrap();
    assert_eq!(s.x, word_x + 2 * 8);
}

#[test]
fn find_inside_one_run_is_one_box() {
    let p = text_page();
    let m = p.find("quick brown", &FixedAdvance);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].len(), 1);
    assert_eq!(m[0][0].w, "quick brown".len() as i32 * 8);
}

#[test]
fn find_spans_lines() {
    let p = text_page();
    let m = p.find("fox jumps", &FixedAdvance);
    assert_eq!(m.len(), 1);
    assert!(m[0][1].y > m[0][0].y);
}

#[test]
fn find_matches_are_in_reading_order() {
    let p = text_page();
    let ys: Vec<i32> = p
        .find("the", &FixedAdvance)
        .iter()
        .map(|m| m[0].y)
        .collect();
    assert!(ys.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn find_does_not_overlap_matches() {
    let p = render(b"<p>aaaa</p>", 600);
    assert_eq!(p.find("aa", &FixedAdvance).len(), 2);
}

#[test]
fn find_with_a_huge_needle_or_page_is_bounded() {
    let p = text_page();
    assert!(p.find(&"x".repeat(500), &FixedAdvance).is_empty());
    let html = format!("<p>{}</p>", "a ".repeat(5000));
    let big = render(html.as_bytes(), 600);
    assert!(big.find("a", &FixedAdvance).len() <= super::textops::MAX_MATCHES);
}

// ---- selection ----

fn first_run(p: &Page) -> (i32, i32) {
    p.cmds
        .iter()
        .find_map(|c| match c {
            Cmd::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn drag_selects_a_range_of_characters() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    // "The quick brown fox": 8 px per character; from inside "quick" to inside "fox".
    let a = (x0 + 4 * 8 + 1, y0 + 2);
    let b = (x0 + 19 * 8 - 1, y0 + 2);
    let r = p.select(a, b, &FixedAdvance).unwrap();
    assert_eq!(p.selection_text(&r), "quick brown fox");
    let spans = p.selection_spans(&r, &FixedAdvance);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].x, x0 + 4 * 8);
    assert_eq!(spans[0].w, 15 * 8);
}

#[test]
fn a_drag_snaps_to_the_nearer_character_boundary() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    // 3 px into the 'T' (8 px wide) is nearer its left edge; 5 px into the fourth character
    // is nearer its right edge.
    let r = p
        .select((x0 + 3, y0 + 2), (x0 + 3 * 8 + 5, y0 + 2), &FixedAdvance)
        .unwrap();
    assert_eq!(p.selection_text(&r), "The ");
}

#[test]
fn dragging_backwards_is_the_same_selection() {
    let p = text_page();
    let (_, y0) = first_run(&p);
    let a = (400, y0 + 2);
    let b = (30, y0 + 2);
    let r = p.select(a, b, &FixedAdvance).unwrap();
    assert_eq!(p.select(b, a, &FixedAdvance), Some(r));
}

#[test]
fn selection_across_lines_has_a_newline() {
    let p = text_page();
    let r = p.select((0, 0), (5000, 5000), &FixedAdvance).unwrap();
    let t = p.selection_text(&r);
    assert!(t.starts_with("The quick brown fox\njumps"), "{t}");
    assert!(t.ends_with("The end."));
}

#[test]
fn a_click_without_movement_selects_nothing() {
    let p = text_page();
    assert_eq!(p.select((50, 40), (50, 40), &FixedAdvance), None);
}

#[test]
fn selection_on_a_page_without_text_is_none() {
    let p = render(b"<div></div>", 600);
    assert_eq!(p.select((1, 1), (50, 50), &FixedAdvance), None);
    assert_eq!(p.word_count(), 0);
}

#[test]
fn dragging_below_the_page_selects_to_the_end() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    let r = p.select((x0, y0 + 2), (20, 5000), &FixedAdvance).unwrap();
    assert!(p.selection_text(&r).ends_with("The end."));
}

#[test]
fn dragging_above_the_page_selects_from_the_start() {
    let p = text_page();
    let r = p.select((20, 0), (400, 5000), &FixedAdvance).unwrap();
    assert_eq!(r.start, CharPos { run: 0, off: 0 });
}

#[test]
fn double_click_picks_a_word() {
    let p = text_page();
    let (x0, y0) = first_run(&p);
    let r = p.select_word(x0 + 6 * 8, y0 + 2, &FixedAdvance).unwrap();
    assert_eq!(p.selection_text(&r), "quick");
    assert!(p.select_word(2, 2, &FixedAdvance).is_none());
}

// ---- hostile input ----

#[test]
fn hostile_img_and_form_markup_does_not_panic() {
    let junk = [
        "<img>",
        "<img src>",
        "<img src= width=-5 height=99999999999999999999>",
        "<img src='data:image/png;base64,' alt='\u{0}\u{1}\u{ffff}'>",
        "<form><form><input><button></form></form>",
        "<form action=<input name=x>>",
        "<input name=q size=-1 value='\u{ffff}'>",
        "<form><input type=hidden><input type=submit value=''><button></button></form>",
        "<img src=a width=1e9 height=0x10>",
        "<a href=><img src=x></a>",
    ];
    for j in junk {
        for zoom in [50, 100, 300] {
            let p = lay(j, 300, zoom, &Fixed(ImgState::Ready { w: 1, h: 1 }));
            assert!(p.height >= 0);
        }
        let p = lay(j, 300, 100, &NoImages);
        let _ = p.find("x", &FixedAdvance);
        let _ = p.select((0, 0), (100, 100), &FixedAdvance);
    }
}
