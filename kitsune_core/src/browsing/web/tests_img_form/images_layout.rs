use super::*;

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
