use super::*;

#[test]
fn vertical_margins_collapse_between_siblings() {
    // Two paragraphs with 1em (16px) margins sit 16 px apart plus the line, not 32.
    let p = lay("<body style='margin:0'><p>aa</p><p>bb</p></body>", 600);
    let gap = y_of(&p, "bb") - y_of(&p, "aa");
    assert_eq!(gap, 24 + 16);
    // The first margin collapses through the body too: the line starts 16 px down, and the
    // text's top is 2 px below it (strut 18 above the baseline, ascent 16).
    assert_eq!(y_of(&p, "aa"), 18);
}

#[test]
fn width_max_width_and_auto_margins_centre_a_column() {
    let p = lay(
        "<body style='margin:0'><div style='max-width:200px;margin:0 auto;background:#eee'>x</div></body>",
        600,
    );
    let r = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { x, w, .. } => Some((*x, *w)),
        _ => None,
    });
    assert_eq!(r, Some((200, 200)));
    let p = lay(
        "<body style='margin:0'><div style='width:50%;margin:0 auto;background:#eee'>x</div></body>",
        600,
    );
    let r = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { x, w, .. } => Some((*x, *w)),
        _ => None,
    });
    assert_eq!(r, Some((150, 300)));
}

#[test]
fn width_never_exceeds_the_container() {
    let p = lay(
        "<body style='margin:0'><div style='width:5000px;background:#eee'>x</div></body>",
        300,
    );
    let w = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { w, .. } => Some(*w),
        _ => None,
    });
    assert_eq!(w, Some(300));
}

#[test]
fn box_sizing_border_box_includes_padding() {
    let p = lay(
        "<body style='margin:0'><div style='box-sizing:border-box;width:200px;padding:20px;background:#eee'>x</div></body>",
        600,
    );
    let w = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { w, .. } => Some(*w),
        _ => None,
    });
    assert_eq!(w, Some(200));
    let p = lay(
        "<body style='margin:0'><div style='width:200px;padding:20px;background:#eee'>x</div></body>",
        600,
    );
    let w = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { w, .. } => Some(*w),
        _ => None,
    });
    assert_eq!(w, Some(240));
}

#[test]
fn hr_draws_a_rule() {
    let p = lay("<body style='margin:0'><p>a</p><hr><p>b</p></body>", 300);
    assert!(p.cmds.iter().any(|c| matches!(
        c,
        Cmd::Border {
            widths: [1, 0, 0, 0],
            w: 300,
            ..
        }
    )));
    assert!(y_of(&p, "b") > y_of(&p, "a") + 30);
}

#[test]
fn blockquote_has_a_bar_and_an_indent() {
    let p = lay(
        "<body style='margin:0'><blockquote>quoted</blockquote></body>",
        400,
    );
    assert!(p.cmds.iter().any(|c| matches!(
        c,
        Cmd::Border {
            widths: [0, 0, 0, 3],
            ..
        }
    )));
    assert_eq!(x_of(&p, "quoted"), 3 + 16);
}

#[test]
fn unordered_lists_get_bullets_and_indent() {
    let p = lay(
        "<body style='margin:0'><ul><li>one</li><li>two</li></ul></body>",
        400,
    );
    let bullets: Vec<_> = p
        .cmds
        .iter()
        .filter(|c| matches!(c, Cmd::Rect { radius, .. } if *radius > 0))
        .collect();
    assert_eq!(bullets.len(), 2);
    assert_eq!(x_of(&p, "one"), 28);
    assert!(y_of(&p, "two") > y_of(&p, "one"));
}

#[test]
fn nested_lists_change_the_marker() {
    let p = lay(
        "<ul><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li></ul>",
        400,
    );
    assert!(
        p.cmds
            .iter()
            .any(|c| matches!(c, Cmd::Border { radius, .. } if *radius > 0)),
        "circle"
    );
    assert!(
        p.cmds
            .iter()
            .any(|c| matches!(c, Cmd::Rect { radius: 0, w, h, .. } if w == h && *w < 10)),
        "square"
    );
}

#[test]
fn ordered_lists_number_their_items() {
    let p = lay(
        "<ol start=3><li>a</li><li value=10>b</li><li>c</li></ol>",
        400,
    );
    let t = texts(&p);
    assert!(
        t.contains(&String::from("3."))
            && t.contains(&String::from("10."))
            && t.contains(&String::from("11.")),
        "{t:?}"
    );
    // The numbers are right-aligned just left of the text.
    let n3 = p.cmds.iter().find_map(|c| match c {
        Cmd::Text { text, x, w, .. } if text == "3." => Some(x + w),
        _ => None,
    });
    assert!(n3.unwrap() < x_of(&p, "a"));
    let p = lay(
        "<ol style='list-style-type:lower-roman'><li>a</li><li>b</li><li>c</li><li>d</li></ol>",
        400,
    );
    assert!(texts(&p).contains(&String::from("iv.")));
    let p = lay(
        "<ol type=a style='list-style-type:upper-alpha'><li>a</li><li>b</li></ol>",
        400,
    );
    assert!(texts(&p).contains(&String::from("B.")));
}

#[test]
fn list_items_with_block_content_keep_their_marker_on_the_first_line() {
    let p = lay("<ul><li><p>para</p></li></ul>", 400);
    assert_eq!(
        p.cmds
            .iter()
            .filter(|c| matches!(c, Cmd::Rect { radius, .. } if *radius > 0))
            .count(),
        1
    );
}

#[test]
fn definition_lists_indent_the_definition() {
    let p = lay("<dl><dt>term</dt><dd>meaning</dd></dl>", 400);
    assert!(x_of(&p, "meaning") > x_of(&p, "term"));
    assert!(font_of(&p, "term").bold);
}
