use super::*;

#[test]
fn links_get_the_ua_indigo() {
    let p = lay("<p>see <a href=x>this link</a> ok</p>", 600);
    assert!(p.cmds.iter().any(|c| matches!(c, Cmd::Text { text, color, .. } if text == "this link" && *color == Rgb(0x4f, 0x46, 0xe5))));
}

#[test]
fn links_are_recorded_with_hit_boxes() {
    let p = lay(
        "<p>go <a href='/x'>to x</a> or <a href=y>there</a></p>",
        600,
    );
    assert_eq!(p.links, ["/x", "y"]);
    assert_eq!(p.hits.len(), 2, "one box per run of link text");
    let first = p.hits[0];
    assert_eq!(p.link_at(first.x + 1, first.y + 1), Some("/x"));
    assert_eq!(p.link_index_at(first.x + 1, first.y + 1), Some(0));
    let last = p.hits[1];
    assert_eq!(
        p.link_at(last.x + last.w - 1, last.y + last.h - 1),
        Some("y")
    );
    assert_eq!(p.link_at(0, 0), None);
    assert_eq!(p.link_at(first.x, first.y + first.h), None);
}

#[test]
fn link_runs_carry_their_link_index() {
    let p = lay("<p><a href=a>one</a> <a href=b>two</a></p>", 600);
    let idx: Vec<Option<u32>> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { link, .. } => Some(*link),
            _ => None,
        })
        .collect();
    assert_eq!(idx, [Some(0), Some(1)]);
}

#[test]
fn nested_inline_inside_a_link_stays_clickable() {
    let p = lay("<a href='/n'>plain <b>bold</b></a> after", 600);
    assert_eq!(p.links, ["/n"]);
    assert_eq!(p.hits.len(), 2);
    assert!(p.hits.iter().all(|h| p.links[h.link] == "/n"));
}

#[test]
fn a_block_inside_a_link_keeps_the_link() {
    let p = lay("<a href='/card'><div>title</div><div>body</div></a>", 600);
    assert_eq!(p.links, ["/card"]);
    assert_eq!(p.hits.len(), 2);
}

#[test]
fn anchor_without_href_is_not_a_link_and_links_are_capped() {
    let p = lay("<a name=top>x</a><a>y</a>", 600);
    assert!(p.links.is_empty() && p.hits.is_empty());
    let mut html = String::new();
    for i in 0..(MAX_LINKS + 50) {
        html.push_str(&format!("<a href=/{i}>l</a> "));
    }
    let p = lay(&html, 600);
    assert!(p.links.len() <= MAX_LINKS);
}
