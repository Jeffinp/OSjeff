use super::*;

#[test]
fn zoom_scales_text_and_lengths_from_50_to_300() {
    let html = "<body style='margin:0'><p style='margin:10px 0;padding:4px'>hello</p></body>";
    let base = lay_zoom(html, 800, 100);
    for z in [50u16, 75, 100, 125, 150, 200, 250, 300] {
        let p = lay_zoom(html, 800, z);
        assert_eq!(font_of(&p, "hello").size, (16 * z / 100), "zoom {z}");
        let want = (base.height as i64 * i64::from(z) / 100) as i32;
        assert!(
            (p.height - want).abs() <= 8,
            "zoom {z}: {} vs {want}",
            p.height
        );
        for c in &p.cmds {
            if let Cmd::Text { x, w, .. } = c {
                assert!(*x + *w <= 800);
            }
        }
    }
}

#[test]
fn zoomed_text_wraps_more() {
    let html = "<body style='margin:0'><p style='margin:0'>one two three four five six seven eight nine ten</p></body>";
    let a = distinct_ys(&lay_zoom(html, 400, 100)).len();
    let b = distinct_ys(&lay_zoom(html, 400, 300)).len();
    assert!(b > a);
}
