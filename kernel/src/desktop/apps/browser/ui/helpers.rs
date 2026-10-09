//! Small drawing helpers shared by the browser's chrome: badges and icon buttons.

use crate::desktop::*;
use crate::text::{self, BODY, CAPTION, TITLE3, Weight};
use kitsune_core::browser::tabs as tabs_model;
use kitsune_core::iconart::Glyph;

/// Amber of a favourite.
pub(super) const AMBER: Color = Color::rgb(0xF5, 0xA6, 0x23);

/// Straight ARGB of `c` with opacity `a` (0..=255).
pub(super) fn argb(c: Color, a: u32) -> u32 {
    (a.min(255) << 24) | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

/// The badge colour of a host or letter: a stable pick from the accent family.
pub(crate) fn badge_color(seed: &str) -> Color {
    const PALETTE: [(u8, u8, u8); 8] = [
        (0x5B, 0x5C, 0xF6),
        (0x14, 0xB8, 0xC4),
        (0xF5, 0x9E, 0x0B),
        (0xEC, 0x48, 0x99),
        (0x10, 0xB9, 0x81),
        (0xF9, 0x73, 0x16),
        (0x8B, 0x5C, 0xF6),
        (0x0E, 0xA5, 0xE9),
    ];
    let h = seed
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    let (r, g, b) = PALETTE[(h % 8) as usize];
    Color::rgb(r, g, b)
}

/// A rounded badge with a letter: the favicon-less site mark.
pub(super) fn letter_badge(c: &mut Canvas, r: Rect, letter: char, seed: &str, squircle: bool) {
    if letter == tabs_model::BRAND_BADGE {
        // The browser's own pages: the fox, like a favicon (its tile has the corners).
        c.blit_surface(icons::surface(Icon::Brand, r.w.min(r.h)), r.x, r.y, 256);
        return;
    }
    let style = if squircle {
        Corner::Squircle
    } else {
        Corner::Circle
    };
    let rad = if squircle { r.w * 28 / 100 } else { r.w / 2 };
    c.fill_rrect(r, rad, style, badge_color(seed), 256);
    let mut buf = [0u8; 4];
    let s = letter.encode_utf8(&mut buf);
    let px = if r.w >= 36 {
        TITLE3
    } else if r.w >= 22 {
        BODY
    } else {
        CAPTION
    };
    text::draw_centered(c, r, s, px, Weight::Semibold, theme::WHITE);
}

/// Flat icon button: a wash on hover.
pub(super) fn icon_button(c: &mut Canvas, r: Rect, g: Glyph, enabled: bool, hover: bool) {
    let p = theme::pal();
    if hover && enabled {
        ui::fill_token(c, r, 8, p.hover);
    }
    let ink = if enabled {
        theme::solid(p.text)
    } else {
        theme::ink_dim()
    };
    ui::draw_glyph(
        c,
        g,
        r.x + (r.w - 16) / 2,
        r.y + (r.h - 16) / 2,
        16,
        argb(ink, 255),
    );
}
