use super::*;

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
