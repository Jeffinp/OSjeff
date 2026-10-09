use super::*;

#[test]
fn auto_follows_the_clock() {
    let a = AppearanceSetting::Auto;
    for h in 0..24 {
        let want = if (7..19).contains(&h) {
            Appearance::Light
        } else {
            Appearance::Dark
        };
        assert_eq!(a.resolve(h), want, "hour {h}");
    }
    for h in 0..24 {
        assert_eq!(AppearanceSetting::Light.resolve(h), Appearance::Light);
        assert_eq!(AppearanceSetting::Dark.resolve(h), Appearance::Dark);
    }
}

#[test]
fn names_roundtrip_and_cycle() {
    for s in [
        AppearanceSetting::Auto,
        AppearanceSetting::Light,
        AppearanceSetting::Dark,
    ] {
        assert_eq!(AppearanceSetting::from_name(s.name().as_bytes()), Some(s));
    }
    assert_eq!(AppearanceSetting::from_name(b"sepia"), None);
    let mut s = AppearanceSetting::Auto;
    for _ in 0..3 {
        s = s.next();
    }
    assert_eq!(s, AppearanceSetting::Auto);
}

fn luma(c: u32) -> i32 {
    (((c >> 16) & 0xFF) * 54 + ((c >> 8) & 0xFF) * 183 + (c & 0xFF) * 19) as i32 >> 8
}

#[test]
fn text_is_readable_on_its_surfaces() {
    for a in [Appearance::Light, Appearance::Dark] {
        let p = palette(a);
        for bg in [p.window_bg, p.content_bg, p.sidebar_bg] {
            assert!(
                (luma(p.text) - luma(bg)).abs() > 150,
                "{a:?} text on {bg:08x}"
            );
            assert!(
                (luma(p.text_secondary) - luma(bg)).abs() > 60,
                "{a:?} secondary on {bg:08x}"
            );
        }
        assert_eq!(alpha_of(p.window_bg), 255);
        assert!(alpha_of(p.menubar_tint) < 255 && alpha_of(p.dock_tint) < 255);
        // Tooltips invert the surface.
        assert!((luma(p.tooltip_text) - luma(p.tooltip_bg)).abs() > 120);
    }
    // The two appearances are really different.
    assert!(luma(LIGHT.window_bg) > luma(DARK.window_bg) + 100);
    assert!(luma(LIGHT.text) < luma(DARK.text) - 100);
}

#[test]
fn tile_radius_is_22_percent() {
    assert_eq!(tile_radius(128), 28);
    assert_eq!(tile_radius(48), 10);
    assert_eq!(rgb_of(0xFF12_3456), 0x12_3456);
}
