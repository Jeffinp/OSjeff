//! Visual identity: the design tokens of `docs/design/ui-design.md` as drawing
//! colours, the live accent and the current appearance (light / dark).
//!
//! New UI code reads [`pal`] (a palette that follows the appearance) and
//! [`solid`] / [`tint`] to turn an ARGB token into a colour and a blend alpha.
//! The constants further down are the legacy light-surface colours the existing
//! app interiors were drawn with; they keep working unchanged (wave 2 moves the
//! apps to the tokens).

use crate::fb::Color;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use kitsune_core::style::{self, Appearance, Palette};

/// The current accent colour (`0xRRGGBB`), the primary highlight of the whole UI.
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0x5B_5C_F6);
/// 0 = light, 1 = dark.
static APPEARANCE: AtomicU8 = AtomicU8::new(0);

/// The accent colour in use (indigo unless the settings changed it).
#[inline]
pub fn accent() -> Color {
    let v = ACCENT_RGB.load(Ordering::Relaxed);
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Change the accent (`0xRRGGBB`). Callers repaint the wallpaper and the windows.
pub fn set_accent(rgb: u32) {
    ACCENT_RGB.store(rgb & 0xFF_FFFF, Ordering::Relaxed);
}

/// The appearance in effect.
#[inline]
pub fn appearance() -> Appearance {
    if APPEARANCE.load(Ordering::Relaxed) == 0 {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

/// Switch appearance. Returns whether it changed (the caller then repaints the
/// cached background and everything else).
pub fn set_appearance(a: Appearance) -> bool {
    let v = (a == Appearance::Dark) as u8;
    APPEARANCE.swap(v, Ordering::Relaxed) != v
}

/// True in dark appearance.
#[inline]
pub fn dark() -> bool {
    appearance() == Appearance::Dark
}

/// The palette of the current appearance.
#[inline]
pub fn pal() -> &'static Palette {
    style::palette(appearance())
}

/// The colour of an ARGB token (alpha dropped).
#[inline]
pub fn solid(argb: u32) -> Color {
    Color::rgb((argb >> 16) as u8, (argb >> 8) as u8, argb as u8)
}

/// An ARGB token as `(colour, blend alpha 0..=256)`.
#[inline]
pub fn tint(argb: u32) -> (Color, u16) {
    let a = argb >> 24;
    (solid(argb), (a + (a >> 7)) as u16)
}

/// The accent with white text on it.
pub const ACCENT_TEXT: Color = Color::rgb(0xFF, 0xFF, 0xFF);

// ---- legacy app-interior colours (light surfaces) ----

#[allow(dead_code)]
pub const ACCENT_2: Color = Color::rgb(0x7C, 0x6C, 0xFF); // violet

// Surfaces.
/// App interiors are drawn on this: the unified window colour of the current appearance.
#[inline]
pub fn window_body() -> Color {
    solid(pal().window_bg)
}
#[allow(dead_code)]
pub const HEADER: Color = Color::rgb(0x18, 0x21, 0x39);
pub const HEADER_TEXT: Color = Color::rgb(0xE8, 0xED, 0xF7);

// Foreground text of the current appearance.
#[inline]
pub fn text() -> Color {
    solid(pal().text)
}
#[inline]
pub fn text_muted() -> Color {
    solid(pal().text_secondary)
}

// Status / controls.
pub const CLOSE: Color = Color::rgb(0xFF, 0x6B, 0x63);
pub const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);

// ---- semantic colours of the app interiors (they follow the appearance) ----

/// Toolbars and headers inside a window: the unified window colour.
#[inline]
pub fn toolbar() -> Color {
    solid(pal().window_bg)
}
/// Sidebars.
#[inline]
pub fn sidebar() -> Color {
    solid(pal().sidebar_bg)
}
/// Lists, text areas and fields.
#[inline]
pub fn surface() -> Color {
    solid(pal().content_bg)
}
/// Alternate list rows.
#[inline]
#[allow(dead_code)]
pub fn zebra() -> Color {
    if dark() {
        Color::rgb(0x25, 0x25, 0x28)
    } else {
        Color::rgb(0xF5, 0xF5, 0xF8)
    }
}
/// Hairlines and the outline of controls.
#[inline]
#[allow(dead_code)]
pub fn line() -> Color {
    if dark() {
        Color::rgb(0x3E, 0x3E, 0x42)
    } else {
        Color::rgb(0xD8, 0xD8, 0xDD)
    }
}
/// A button or field sitting on the window colour.
#[inline]
pub fn button_bg() -> Color {
    if dark() {
        Color::rgb(0x45, 0x45, 0x49)
    } else {
        Color::rgb(0xFF, 0xFF, 0xFF)
    }
}
/// A flat tool button on a toolbar.
#[inline]
pub fn tool_bg() -> Color {
    if dark() {
        Color::rgb(0x3A, 0x3A, 0x3E)
    } else {
        Color::rgb(0xEA, 0xEA, 0xEF)
    }
}
/// Disabled icon ink.
#[inline]
pub fn ink_dim() -> Color {
    if dark() {
        Color::rgb(0x6E, 0x6E, 0x73)
    } else {
        Color::rgb(0xB4, 0xB4, 0xBA)
    }
}
/// Errors.
#[inline]
pub fn danger() -> Color {
    solid(pal().danger)
}
/// Success.
#[inline]
pub fn ok() -> Color {
    if dark() {
        Color::rgb(0x4A, 0xDE, 0x80)
    } else {
        Color::rgb(0x1B, 0x7F, 0x5F)
    }
}
/// Selected row in a list (a tint of the accent).
#[inline]
#[allow(dead_code)]
pub fn selection() -> Color {
    accent().lerp(
        if dark() {
            Color::rgb(0x1E, 0x1E, 0x20)
        } else {
            Color::rgb(0xFF, 0xFF, 0xFF)
        },
        if dark() { 150 } else { 190 },
    )
}
