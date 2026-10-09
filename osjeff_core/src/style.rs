//! Design tokens: the colour palettes of the light and dark appearance, resolved
//! from the user's setting, and the size/radius/spacing constants of the design
//! system (`docs/design/ui-macos.md`).
//!
//! Colours are straight ARGB (`0xAARRGGBB`); opaque ones have alpha `FF`. The
//! kernel's `theme` module wraps these in `Color` values; nothing here touches
//! pixels, so the tables and the Auto rule are host tested.

/// The appearance in effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

/// What the user picked in Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AppearanceSetting {
    /// Dark in the evening and at night, light by day.
    #[default]
    Auto,
    Light,
    Dark,
}

/// First hour (local, 24 h) of the dark period of [`AppearanceSetting::Auto`].
pub const AUTO_DARK_FROM: u8 = 19;
/// First hour of the light period.
pub const AUTO_LIGHT_FROM: u8 = 7;

impl AppearanceSetting {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_name(s: &[u8]) -> Option<Self> {
        match s {
            b"auto" => Some(Self::Auto),
            b"light" => Some(Self::Light),
            b"dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// The appearance this setting gives at local hour `hour` (0..=23).
    pub fn resolve(self, hour: u8) -> Appearance {
        match self {
            Self::Light => Appearance::Light,
            Self::Dark => Appearance::Dark,
            Self::Auto => {
                if !(AUTO_LIGHT_FROM..AUTO_DARK_FROM).contains(&hour) {
                    Appearance::Dark
                } else {
                    Appearance::Light
                }
            }
        }
    }

    /// The next setting when cycling Auto -> Light -> Dark.
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::Auto,
        }
    }
}

/// One appearance's colours.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Unified title bar, toolbars and window body.
    pub window_bg: u32,
    /// Text areas, lists and fields.
    pub content_bg: u32,
    pub sidebar_bg: u32,
    /// 1 px hairlines.
    pub separator: u32,
    pub text: u32,
    pub text_secondary: u32,
    pub text_tertiary: u32,
    /// Over the blurred wallpaper.
    pub menubar_tint: u32,
    pub dock_tint: u32,
    /// Hairline around the dock and the menu bar's bottom edge.
    pub glass_edge: u32,
    pub menu_tint: u32,
    pub field_bg: u32,
    pub control_bg: u32,
    pub control_border: u32,
    /// Text on the menu bar and the dock labels.
    pub bar_text: u32,
    /// Grey traffic lights of an unfocused window.
    pub light_inactive: u32,
    /// Title text of an unfocused window.
    pub title_inactive: u32,
    /// Hover wash on rows and bar items.
    pub hover: u32,
    pub danger: u32,
    /// Tooltips.
    pub tooltip_bg: u32,
    pub tooltip_text: u32,
}

pub const LIGHT: Palette = Palette {
    window_bg: 0xFFF6_F6F8,
    content_bg: 0xFFFF_FFFF,
    sidebar_bg: 0xFFED_EDF1,
    separator: 0x1A00_0000,
    text: 0xFF1D_1D1F,
    text_secondary: 0xFF6E_6E73,
    text_tertiary: 0xFFA1_A1A6,
    menubar_tint: 0xB2F6_F6F8,
    dock_tint: 0x4DFF_FFFF,
    glass_edge: 0x66FF_FFFF,
    menu_tint: 0xCCF2_F2F5,
    field_bg: 0xFFFF_FFFF,
    control_bg: 0xFFFF_FFFF,
    control_border: 0x2400_0000,
    bar_text: 0xFF1D_1D1F,
    light_inactive: 0xFFD1_D1D6,
    title_inactive: 0xFFA1_A1A6,
    hover: 0x1400_0000,
    danger: 0xFFFF_453A,
    tooltip_bg: 0xE62B_2B2E,
    tooltip_text: 0xFFF5_F5F7,
};

pub const DARK: Palette = Palette {
    window_bg: 0xFF2C_2C2F,
    content_bg: 0xFF1E_1E20,
    sidebar_bg: 0xFF26_2629,
    separator: 0x1AFF_FFFF,
    text: 0xFFF5_F5F7,
    text_secondary: 0xFFA1_A1A6,
    text_tertiary: 0xFF6E_6E73,
    menubar_tint: 0x9E1E_1E20,
    dock_tint: 0x751E_1E22,
    glass_edge: 0x1FFF_FFFF,
    menu_tint: 0xCC2A_2A2E,
    field_bg: 0x14FF_FFFF,
    control_bg: 0x1FFF_FFFF,
    control_border: 0x1FFF_FFFF,
    bar_text: 0xFFF5_F5F7,
    light_inactive: 0xFF4A_4A4E,
    title_inactive: 0xFF6E_6E73,
    hover: 0x1FFF_FFFF,
    danger: 0xFFFF_453A,
    tooltip_bg: 0xE6F2_F2F5,
    tooltip_text: 0xFF1D_1D1F,
};

/// The palette of `a`.
pub fn palette(a: Appearance) -> &'static Palette {
    match a {
        Appearance::Light => &LIGHT,
        Appearance::Dark => &DARK,
    }
}

/// Traffic-light colours of an active window.
pub const LIGHT_CLOSE: u32 = 0xFFFF_6B63;
pub const LIGHT_MINIMIZE: u32 = 0xFFFF_C24A;
pub const LIGHT_ZOOM: u32 = 0xFF3F_D07C;

// ----------------------------------------------------------------- dimensions

/// Height of the top panel: no window may cover it. (`MENUBAR_H` is the old name, kept for the
/// code that predates the panel.)
pub const PANEL_H: i32 = 30;
pub const MENUBAR_H: i32 = PANEL_H;
/// Corner radii.
pub const R_WINDOW: i32 = 12;
pub const R_POPOVER: i32 = 12;
pub const R_MENU: i32 = 8;
pub const R_CONTROL: i32 = 6;
pub const R_DOCK: i32 = 22;
pub const R_TOOLTIP: i32 = 6;
/// Tile radius of an app icon of side `s`: 22.5 %.
pub const fn tile_radius(s: i32) -> i32 {
    s * 225 / 1000
}

/// `0xRRGGBB` of an ARGB colour.
pub const fn rgb_of(argb: u32) -> u32 {
    argb & 0x00FF_FFFF
}

/// Alpha 0..=255 of an ARGB colour.
pub const fn alpha_of(argb: u32) -> u32 {
    argb >> 24
}

#[cfg(test)]
mod tests {
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
    fn tile_radius_is_22_5_percent() {
        assert_eq!(tile_radius(128), 28);
        assert_eq!(tile_radius(48), 10);
        assert_eq!(rgb_of(0xFF12_3456), 0x12_3456);
    }
}
