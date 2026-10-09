use super::*;

#[test]
fn table_columns_follow_their_content() {
    // 8 px per character; cells pad 8 px each side; 2 px spacing.
    let p = lay(
        &body0("<table><tr><td>aa</td><td>bbbb</td></tr></table>"),
        600,
    );
    assert_eq!(x_of(&p, "aa"), 2 + 8);
    assert_eq!(x_of(&p, "bbbb"), 2 + (16 + 16) + 2 + 8);
    assert_eq!(y_of(&p, "aa"), y_of(&p, "bbbb"));
}

#[test]
fn an_auto_table_is_as_wide_as_its_content() {
    let p = lay(
        &body0("<table style='background:#abcdef'><tr><td>aa</td><td>bbbb</td></tr></table>"),
        600,
    );
    let (x, _, w, _) = rect_of(&p, Rgb(0xab, 0xcd, 0xef)).expect("table background");
    assert_eq!(x, 0);
    assert_eq!(w, 2 + 32 + 2 + 48 + 2);
}

#[test]
fn a_table_with_width_fills_it() {
    let p = lay(
        &body0(
            "<table style='width:100%;background:#abcdef'><tr><td>aa</td><td>bbbb</td></tr></table>",
        ),
        600,
    );
    let (_, _, w, _) = rect_of(&p, Rgb(0xab, 0xcd, 0xef)).unwrap();
    assert_eq!(w, 600);
    // The columns grew in proportion: the second starts far right of where its content needs.
    assert!(
        x_of(&p, "bbbb") > 2 + 32 + 2 + 8 + 100,
        "{}",
        x_of(&p, "bbbb")
    );
}

#[test]
fn cells_wrap_when_the_table_is_too_narrow() {
    let long = "palavra ".repeat(20);
    let p = lay(
        &body0(&format!(
            "<table style='width:200px'><tr><td>{long}</td><td>curto</td></tr></table>"
        )),
        600,
    );
    assert!(
        distinct_ys(&p).len() > 3,
        "the long cell wraps over several lines"
    );
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 200, "text ends inside the table: {}", x + w);
        }
    }
    // The short column keeps its content on one line.
    assert!(texts(&p).contains(&String::from("curto")));
}

#[test]
fn a_word_wider_than_the_table_squeezes_not_overflows() {
    let word = "w".repeat(60);
    let p = lay(
        &body0(&format!(
            "<table style='width:150px'><tr><td>{word}</td></tr></table>"
        )),
        600,
    );
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 150 + 8, "{} {}", x, w);
        }
    }
}

#[test]
fn colspan_spreads_over_columns() {
    let p = lay(
        &body0(
            "<table><tr><td>a</td><td>b</td></tr><tr><td colspan=2>una celda de dos columnas ancha</td></tr></table>",
        ),
        600,
    );
    // The wide cell forces the two columns to share its width.
    let a = x_of(&p, "a");
    let b = x_of(&p, "b");
    assert!(b - a > 40, "columns grew: {}", b - a);
    assert!(texts(&p).iter().any(|t| t.starts_with("una celda")));
}

#[test]
fn rows_stack_and_cells_share_the_row_height() {
    let p = lay(
        &body0(
            "<table><tr><td style='background:#112233'>x</td><td>un texto largo que da mas lineas aqui</td></tr><tr><td>y</td></tr></table>",
        ),
        200,
    );
    let (_, _, _, h) = rect_of(&p, Rgb(0x11, 0x22, 0x33)).unwrap();
    // The first cell's background is as tall as the taller neighbour.
    assert!(h > 24 + 8, "cell box stretched to the row: {h}");
    assert!(y_of(&p, "y") > y_of(&p, "x") + 24);
}

#[test]
fn cell_content_is_middle_aligned_by_default() {
    let p = lay(
        &body0("<table><tr><td>one<br>two<br>three</td><td>mid</td></tr></table>"),
        600,
    );
    assert!(y_of(&p, "mid") > y_of(&p, "one"));
    let p = lay(
        &body0("<table><tr><td>one<br>two<br>three</td><td valign=top>top</td></tr></table>"),
        600,
    );
    assert_eq!(y_of(&p, "top"), y_of(&p, "one"));
    let p = lay(
        &body0(
            "<table><tr><td>one<br>two<br>three</td><td style='vertical-align:bottom'>bot</td></tr></table>",
        ),
        600,
    );
    assert!(y_of(&p, "bot") > y_of(&p, "two"));
}

#[test]
fn table_border_attribute_draws_cell_borders() {
    let p = lay(
        &body0("<table border=1><tr><td>a</td><td>b</td></tr></table>"),
        600,
    );
    let borders = p
        .cmds
        .iter()
        .filter(|c| matches!(c, Cmd::Border { .. }))
        .count();
    assert_eq!(borders, 3, "the table and its two cells");
}

#[test]
fn collapsed_borders_share_an_edge() {
    let p = lay(
        &body0(
            "<style>table{border-collapse:collapse} td{border:1px solid #333}</style><table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>",
        ),
        600,
    );
    let widths: Vec<[i32; 4]> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Border { widths, .. } => Some(*widths),
            _ => None,
        })
        .collect();
    assert_eq!(widths.len(), 4);
    assert_eq!(widths[0], [1, 1, 1, 1]); // first row, first column
    assert_eq!(widths[1], [1, 1, 1, 0]); // first row, second column: no left edge
    assert_eq!(widths[2], [0, 1, 1, 1]); // second row: no top edge
    assert_eq!(widths[3], [0, 1, 1, 0]);
}

#[test]
fn caption_comes_first_and_centred() {
    let p = lay(
        &body0("<table><caption>Título</caption><tr><td>a</td></tr></table>"),
        600,
    );
    assert!(y_of(&p, "Título") < y_of(&p, "a"));
    assert!(font_of(&p, "Título").bold);
}

#[test]
fn nested_tables_lay_out() {
    let p = lay(
        &body0(
            "<table><tr><td>fora<table><tr><td>dentro</td><td>outro</td></tr></table></td></tr></table>",
        ),
        600,
    );
    assert!(y_of(&p, "dentro") > y_of(&p, "fora"));
    assert_eq!(y_of(&p, "dentro"), y_of(&p, "outro"));
    assert!(x_of(&p, "outro") > x_of(&p, "dentro"));
}

#[test]
fn table_links_and_forms_keep_their_boxes_after_alignment() {
    let p = lay(
        &body0("<table><tr><td>a<br>b<br>c</td><td><a href=x>link</a></td></tr></table>"),
        600,
    );
    let hit = p.hits[0];
    let ty = y_of(&p, "link");
    assert!(
        hit.y <= ty && ty < hit.y + hit.h,
        "the hit box moved with its text"
    );
    assert_eq!(p.link_at(hit.x + 1, hit.y + 1), Some("x"));
}

#[test]
fn vertical_alignment_moves_only_the_cell_it_belongs_to() {
    // A tall middle cell between short neighbours (one with a background): centring the
    // neighbours must not drag the cells laid out after them.
    let row = |n: &str| {
        alloc::format!(
            "<tr><td style=\"background:#66f;width:40px\">{n}</td><td>top{n}<br>bot{n}</td><td>end{n}</td></tr>"
        )
    };
    let p = lay(
        &alloc::format!("<table>{}{}{}</table>", row("1"), row("2"), row("3")),
        400,
    );
    for n in ["1", "2", "3"] {
        let (top, bot, end) = (
            y_of(&p, &alloc::format!("top{n}")),
            y_of(&p, &alloc::format!("bot{n}")),
            y_of(&p, &alloc::format!("end{n}")),
        );
        assert!(bot > top);
        // The short cells are centred on the two-line cell, not pushed below it.
        assert!(
            end >= top && end < bot,
            "end{n} at {end}, lines at {top}/{bot}"
        );
        let tile = y_of(&p, n);
        assert!(tile >= top && tile <= bot, "tile {n} at {tile}");
    }
    assert!(y_of(&p, "top2") > y_of(&p, "bot1"));
    assert!(y_of(&p, "top3") > y_of(&p, "bot2"));
}

#[test]
fn hostile_tables_are_bounded() {
    // Thousands of columns and rows, huge colspans.
    let mut html = String::from("<table>");
    for r in 0..3000 {
        html.push_str("<tr>");
        for c in 0..40 {
            html.push_str(&format!(
                "<td colspan='{}'>{}</td>",
                if c == 0 { 99999 } else { 1 },
                r
            ));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    let start = std::time::Instant::now();
    let p = lay(&html, 500);
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
    assert!(p.height > 0);
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x >= 0 && *x + *w <= 500 + 64, "{x} {w}");
        }
    }
}

#[test]
fn empty_and_odd_tables_do_not_panic() {
    for html in [
        "<table></table>",
        "<table><tr></tr></table>",
        "<table><tr><td></td></tr></table>",
        "<table><td>loose cell</td></table>",
        "<tr><td>row without table</td></tr>",
        "<table><caption>only</caption></table>",
        "<table><thead><tbody><tr><td>x</td></tr></tbody></thead></table>",
        "<table width=0><tr><td width=0>x</td></tr></table>",
        "<table style='width:5px'><tr><td style='width:99999px'>x</td></tr></table>",
        "<table cellpadding=99999 cellspacing=99999 border=99999><tr><td>x</td></tr></table>",
    ] {
        for z in [50, 300] {
            let p = lay_zoom(html, 300, z);
            assert!(p.height >= 0, "{html}");
        }
    }
}

#[test]
fn presentational_attributes_apply_and_css_overrides_them() {
    let p = lay(&body0("<p align=center>ab</p>"), 100);
    assert_eq!(x_of(&p, "ab"), (100 - 16) / 2);
    let p = lay(
        &body0("<p align=center style='text-align:left'>ab</p>"),
        100,
    );
    assert_eq!(x_of(&p, "ab"), 0);
    let p = lay("<font color='#ff0000' size=5>big</font>", 300);
    assert_eq!(font_of(&p, "big").size, 24);
    let p = lay(&body0("<div bgcolor='#112233'>x</div>"), 300);
    assert!(rect_of(&p, Rgb(0x11, 0x22, 0x33)).is_some());
    let p = lay("<ol type=a><li>x</li><li>y</li></ol>", 300);
    assert!(texts(&p).contains(&String::from("b.")));
}

#[test]
fn sup_and_sub_move_the_baseline() {
    let p = lay("<p>x<sup>2</sup> y<sub>i</sub> z</p>", 300);
    let base = y_of(&p, "x");
    let sup = y_of(&p, "2");
    let sub = y_of(&p, "i");
    assert!(sup < base + 3, "raised: {sup} vs {base}");
    assert!(sub > sup);
}

#[test]
fn inline_padding_shows_its_background_around_the_words() {
    let p = lay(
        &body0(
            "<p style='margin:0'>a <span style='background:#123456;padding:0 10px'>bb</span> c</p>",
        ),
        300,
    );
    let (x, _, w, _) = rect_of(&p, Rgb(0x12, 0x34, 0x56)).expect("badge background");
    let bb = x_of(&p, "bb");
    assert_eq!(x, bb - 10);
    assert_eq!(w, 16 + 20);
    // One box: the words' own background joins the padding's.
    let n = p
        .cmds
        .iter()
        .filter(|c| matches!(c, Cmd::Rect { color, .. } if *color == Rgb(0x12, 0x34, 0x56)))
        .count();
    assert_eq!(n, 1);
}

#[test]
fn controls_sit_on_the_text_baseline() {
    let p = lay(
        &body0("<form><p style='margin:0'>Nome: <input name=n></p></form>"),
        400,
    );
    let f = p.fields[0];
    // The label's baseline (16 below its top) is the field text's baseline (control font 14 px,
    // line 17, a 30 px box: 6 + 14 below the top).
    assert_eq!(y_of(&p, "Nome:") + 16, f.y + 20);
}

#[test]
fn a_check_box_keeps_a_gap_before_its_label() {
    let p = lay(
        &body0("<form><p style='margin:0'><input type=checkbox name=c> Aceito</p></form>"),
        400,
    );
    let f = p.fields[0];
    assert_eq!(f.w, 16);
    assert!(x_of(&p, "Aceito") >= f.x + f.w + 6);
}

#[test]
fn a_full_width_table_keeps_pixel_width_columns_and_widens_the_others() {
    let p = lay(
        "<table style=\"width:100%\"><tr><td style=\"width:40px\">a</td><td>bb</td></tr></table>",
        400,
    );
    // The first column stays 40 px wide: the second cell starts right after it.
    assert!(
        x_of(&p, "bb") - x_of(&p, "a") < 60,
        "{}",
        x_of(&p, "bb") - x_of(&p, "a")
    );
}

#[test]
fn a_long_paragraph_of_short_words_keeps_its_words_whole() {
    // More characters than one word may have: the count is per word, so ordinary words
    // late in a long text node are not cut in two.
    let text = "palavra ".repeat(1200);
    let p = lay(&alloc::format!("<p>{text}</p>"), 400);
    let all: Vec<String> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let joined = all.join(" ");
    assert!(joined.split(' ').all(|w| w == "palavra"), "{joined:.80}");
    assert_eq!(joined.split(' ').count(), 1200);
}

#[test]
fn markers_of_a_list_without_padding_stay_on_the_page() {
    let p = lay(
        "<style>ul,ol{padding:0;margin:0}</style><ul><li>a</li></ul><ol><li>b</li></ol>",
        300,
    );
    for c in &p.cmds {
        match c {
            Cmd::Rect { x, y, .. } | Cmd::Text { x, y, .. } => assert!(*x >= 0 && *y >= 0),
            _ => {}
        }
    }
}
