//! OSJeff visual identity. Single source of truth for colors so the whole UI
//! can be ret, themed from one place. Still desktop-familiar, but its own look:
//! deep indigo base, teal + violet accents, dark window chrome on light bodies.

use crate::fb::Color;
use core::sync::atomic::{AtomicU32, Ordering};

/// The current accent colour (`0xRRGGBB`), the primary highlight of the whole UI.
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0x2D_D4_BF);

/// The accent colour in use (teal unless the settings changed it).
#[inline]
pub fn accent() -> Color {
    let v = ACCENT_RGB.load(Ordering::Relaxed);
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

// Wallpaper / backdrop.
pub const BG_TOP: Color = Color::rgb(0x0B, 0x0F, 0x1C);
pub const BG_BOTTOM: Color = Color::rgb(0x16, 0x1C, 0x30);
pub const GLOW_TEAL: Color = Color::rgb(0x2D, 0xD4, 0xBF);
pub const GLOW_VIOLET: Color = Color::rgb(0x7C, 0x6C, 0xFF);

// Brand accents.
/// The default accent (teal). The live accent is [`accent`], which the settings app can change.
pub const ACCENT_DEFAULT: Color = Color::rgb(0x2D, 0xD4, 0xBF);
pub const ACCENT_2: Color = Color::rgb(0x7C, 0x6C, 0xFF); // violet

// Surfaces.
pub const DOCK: Color = Color::rgb(0x12, 0x18, 0x2B);
pub const DOCK_EDGE: Color = Color::rgb(0x2A, 0x33, 0x52);
pub const WINDOW_BODY: Color = Color::rgb(0xF6, 0xF8, 0xFC);
pub const HEADER: Color = Color::rgb(0x18, 0x21, 0x39);
pub const HEADER_DIM: Color = Color::rgb(0x20, 0x26, 0x38);
pub const HEADER_TEXT: Color = Color::rgb(0xE8, 0xED, 0xF7);

// Foreground text on light surfaces.
pub const TEXT: Color = Color::rgb(0x12, 0x16, 0x26);
pub const TEXT_MUTED: Color = Color::rgb(0x5B, 0x64, 0x7A);

// Status / controls.
pub const CLOSE: Color = Color::rgb(0xFF, 0x5C, 0x5C);
pub const MINIMIZE: Color = Color::rgb(0xFF, 0xC1, 0x4D);
pub const MAXIMIZE: Color = ACCENT_DEFAULT;
pub const SHADOW: Color = Color::rgb(0, 0, 0);
pub const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);

// Terminal palette.
pub const TERM_PROMPT: Color = Color::rgb(0x18, 0xB8, 0x9A);
