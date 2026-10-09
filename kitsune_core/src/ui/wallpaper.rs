//! Wallpapers: the built-in presets and the "cover" fit used for a user image.
//!
//! The presets are data (two colours and a kind); the kernel paints them. The
//! user image goes through [`crate::format::image::decode`] and [`cover`], which scales
//! it until it covers the screen and crops the centre, so it never letterboxes.
//! Memory is bounded before anything is allocated ([`check_source`]).

use crate::format::image::{self, Filter, Format, Image, ImageError};
use crate::tk;

/// Largest accepted image file.
pub const MAX_FILE: usize = 4 * 1024 * 1024;
/// Largest accepted source image, in pixels (8 Mpx = 32 MiB decoded).
pub const MAX_SRC_PIXELS: u64 = 8 * 1024 * 1024;

/// How a preset is painted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// Vertical gradient from `top` to `bottom` plus soft glows.
    Glow,
    /// Vertical gradient from `top` to `bottom`.
    Gradient,
    /// One flat colour (`top`).
    Solid,
    /// Vertical gradient, soft glows and the preset's [`Shape`]s (geometric facets, hills, bands).
    Shapes,
}

/// A flat translucent polygon of a wallpaper: up to four corners in per-mille of the screen
/// (a triangle repeats its last corner), and the colour and opacity it has in the light and in
/// the dark appearance (a preset that does not change with the appearance repeats them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub pts: [(i16, i16); 4],
    /// `(0xRRGGBB, opacity 0..=255)` in the light appearance and in the dark one.
    pub light: (u32, u8),
    pub dark: (u32, u8),
}

impl Shape {
    /// The colour and opacity to paint for the given appearance.
    pub fn look(&self, dark: bool) -> (u32, u8) {
        if dark { self.dark } else { self.light }
    }
}

const fn tri(
    a: (i16, i16),
    b: (i16, i16),
    c: (i16, i16),
    light: (u32, u8),
    dark: (u32, u8),
) -> Shape {
    Shape {
        pts: [a, b, c, c],
        light,
        dark,
    }
}

const fn quad(pts: [(i16, i16); 4], light: (u32, u8), dark: (u32, u8)) -> Shape {
    Shape { pts, light, dark }
}

/// A horizontal band between two heights (per-mille), the same in both appearances.
const fn band(y0: i16, y1: i16, c: (u32, u8)) -> Shape {
    Shape {
        pts: [(0, y0), (1000, y0), (1000, y1), (0, y1)],
        light: c,
        dark: c,
    }
}

/// A soft round glow on the wallpaper. Positions and sizes are per-mille of the
/// screen width / height (`r` of the width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blob {
    pub x: i16,
    pub y: i16,
    pub r: i16,
    /// `0xRRGGBB`.
    pub color: u32,
    /// Peak opacity (0..=255).
    pub alpha: u8,
}

const NO_BLOB: Blob = Blob {
    x: 0,
    y: 0,
    r: 0,
    color: 0,
    alpha: 0,
};

/// The colours of one appearance of a preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scheme {
    pub top: u32,
    pub bottom: u32,
    pub blobs: [Blob; 3],
}

/// A built-in wallpaper. Colours are `0xRRGGBB`. `top` / `bottom` / `blobs` are the
/// light (or only) scheme; `dark`, when present, is what the dark appearance shows.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    /// Catalog key of the name (see [`Preset::name`]).
    pub name_key: &'static str,
    pub style: Style,
    pub top: u32,
    pub bottom: u32,
    pub blobs: [Blob; 3],
    pub dark: Option<Scheme>,
    /// Painted over the gradient and the glows when `style` is [`Style::Shapes`].
    pub shapes: &'static [Shape],
}

impl Preset {
    /// The name in the language in effect.
    pub fn name(&self) -> &'static str {
        crate::i18n::tr(self.name_key)
    }

    /// The name in `lang`.
    pub fn name_in(&self, lang: crate::i18n::Lang) -> &'static str {
        crate::i18n::tr_in(lang, self.name_key)
    }

    /// The scheme to paint for the given appearance.
    pub fn scheme(&self, dark: bool) -> Scheme {
        match (dark, self.dark) {
            (true, Some(d)) => d,
            _ => Scheme {
                top: self.top,
                bottom: self.bottom,
                blobs: self.blobs,
            },
        }
    }
}

const fn blob(x: i16, y: i16, r: i16, color: u32, alpha: u8) -> Blob {
    Blob {
        x,
        y,
        r,
        color,
        alpha,
    }
}

/// Soft geometric facets of *Crepúsculo*, drawn bottom to top.
const DUSK_SHAPES: [Shape; 5] = [
    tri(
        (0, 1000),
        (520, 430),
        (1000, 1000),
        (0xFFFFFF, 46),
        (0x6D6EF8, 34),
    ),
    quad(
        [(300, 1000), (760, 520), (1000, 700), (1000, 1000)],
        (0xC4C9FF, 70),
        (0x8B3FD9, 40),
    ),
    tri((0, 0), (380, 0), (0, 330), (0xFFFFFF, 56), (0xFFFFFF, 12)),
    tri(
        (620, 0),
        (1000, 0),
        (1000, 260),
        (0x9BE8DE, 56),
        (0x14B8C4, 24),
    ),
    tri(
        (0, 700),
        (260, 1000),
        (0, 1000),
        (0xFFD3C2, 70),
        (0x4338CA, 46),
    ),
];

const MONO_SHAPES: [Shape; 3] = [
    quad(
        [(0, 0), (600, 0), (250, 1000), (0, 1000)],
        (0xFFFFFF, 7),
        (0xFFFFFF, 7),
    ),
    quad(
        [(600, 0), (1000, 0), (1000, 1000), (520, 1000)],
        (0xFFFFFF, 4),
        (0xFFFFFF, 4),
    ),
    tri(
        (1000, 0),
        (1000, 520),
        (560, 0),
        (0xFFFFFF, 6),
        (0xFFFFFF, 6),
    ),
];

const PAPER_SHAPES: [Shape; 3] = [
    quad(
        [(0, 0), (520, 0), (180, 1000), (0, 1000)],
        (0x8A6D3B, 9),
        (0x8A6D3B, 9),
    ),
    tri(
        (1000, 380),
        (1000, 1000),
        (420, 1000),
        (0xFFFFFF, 90),
        (0xFFFFFF, 90),
    ),
    tri(
        (0, 640),
        (300, 1000),
        (0, 1000),
        (0x8A6D3B, 12),
        (0x8A6D3B, 12),
    ),
];

/// Rolling hills of *Turquesa*, far to near.
const FIELD_SHAPES: [Shape; 3] = [
    quad(
        [(0, 700), (250, 610), (520, 690), (1000, 600)],
        (0x2DD4BF, 54),
        (0x2DD4BF, 54),
    ),
    quad(
        [(0, 840), (330, 760), (640, 830), (1000, 740)],
        (0x0F766E, 130),
        (0x0F766E, 130),
    ),
    quad(
        [(0, 940), (420, 880), (1000, 950), (1000, 1000)],
        (0x064E4F, 190),
        (0x064E4F, 190),
    ),
];

/// The bands (and the base of the sun) of *Pôr do sol*, thicker toward the horizon.
const SUNSET_SHAPES: [Shape; 5] = [
    band(430, 480, (0xFF5C8A, 150)),
    band(530, 600, (0xFF7A59, 170)),
    band(650, 740, (0xFFA05C, 190)),
    band(790, 900, (0xFFC36B, 205)),
    band(950, 1000, (0xFFE08A, 225)),
];

const NO_SHAPES: [Shape; 0] = [];

/// Preset 0 is the default and follows the appearance (a dusk gradient with soft geometric
/// facets, pale by day and deep indigo at night); the others keep one look.
pub const PRESETS: [Preset; 6] = [
    Preset {
        name_key: tk!("settings.wp.name.dusk"),
        style: Style::Shapes,
        top: 0xE3E6FF,
        bottom: 0xFCE4D8,
        blobs: [
            blob(250, 200, 600, 0x8C93FF, 70),
            blob(850, 150, 420, 0x7FE3D8, 60),
            blob(700, 900, 700, 0xFFB9A0, 80),
        ],
        dark: Some(Scheme {
            top: 0x10122E,
            bottom: 0x2B1650,
            blobs: [
                blob(200, 200, 600, 0x5B5CF6, 90),
                blob(880, 120, 400, 0x14B8C4, 60),
                blob(650, 950, 700, 0xA23FB5, 80),
            ],
        }),
        shapes: &DUSK_SHAPES,
    },
    Preset {
        name_key: tk!("settings.wp.name.aurora"),
        style: Style::Glow,
        top: 0x04161F,
        bottom: 0x0B3A44,
        blobs: [
            blob(200, 200, 600, 0x14B8C4, 110),
            blob(800, 800, 600, 0x22C55E, 80),
            blob(600, 100, 400, 0x5B5CF6, 80),
        ],
        dark: None,
        shapes: &NO_SHAPES,
    },
    Preset {
        name_key: tk!("settings.wp.name.mono"),
        style: Style::Shapes,
        top: 0x1E1F24,
        bottom: 0x0C0D10,
        blobs: [blob(300, 200, 500, 0xFFFFFF, 10), NO_BLOB, NO_BLOB],
        dark: None,
        shapes: &MONO_SHAPES,
    },
    Preset {
        name_key: tk!("settings.wp.name.paper"),
        style: Style::Shapes,
        top: 0xF6F2EA,
        bottom: 0xE8E1D3,
        blobs: [NO_BLOB, NO_BLOB, NO_BLOB],
        dark: None,
        shapes: &PAPER_SHAPES,
    },
    Preset {
        name_key: tk!("settings.wp.name.turquoise"),
        style: Style::Shapes,
        top: 0x0C5F66,
        bottom: 0x083742,
        blobs: [blob(800, 150, 500, 0x5FE0D0, 60), NO_BLOB, NO_BLOB],
        dark: None,
        shapes: &FIELD_SHAPES,
    },
    Preset {
        name_key: tk!("settings.wp.name.sunset"),
        style: Style::Shapes,
        top: 0x24184F,
        bottom: 0xFF8A5C,
        blobs: [blob(500, 640, 500, 0xFFD27A, 150), NO_BLOB, NO_BLOB],
        dark: None,
        shapes: &SUNSET_SHAPES,
    },
];

/// Linear blend of two `0xRRGGBB` colours, `t` in `0..=255` (0 = `a`).
pub fn lerp_rgb(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(255);
    let mix = |sh: u32| {
        let (x, y) = ((a >> sh) & 0xFF, (b >> sh) & 0xFF);
        // Same rounding the framebuffer gradient uses: (x*(255-t) + y*t) / 255.
        (x * (255 - t) + y * t) / 255
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/// Size an `iw x ih` image is scaled to so that it covers `sw x sh`
/// (aspect ratio kept, one side exactly the screen's, the other at least).
pub fn cover_dims(iw: usize, ih: usize, sw: usize, sh: usize) -> Option<(usize, usize)> {
    if iw == 0 || ih == 0 || sw == 0 || sh == 0 {
        return None;
    }
    let (iw, ih, sw, sh) = (iw as u128, ih as u128, sw as u128, sh as u128);
    // Height-limited when the image is relatively wider than the screen.
    let (w, h) = if iw * sh >= ih * sw {
        ((iw * sh).div_ceil(ih), sh)
    } else {
        (sw, (ih * sw).div_ceil(iw))
    };
    Some((w.max(sw) as usize, h.max(sh) as usize))
}

/// Why a wallpaper image was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperError {
    TooBigFile,
    UnknownFormat,
    /// Dimensions beyond [`MAX_SRC_PIXELS`] (or unreadable header).
    TooBigImage,
    Decode(image::DecodeError),
    Image(ImageError),
}

impl WallpaperError {
    /// Catalog key of the reason, for the person (the [`Display`](core::fmt::Display) text is
    /// for logs).
    pub fn why_key(&self) -> &'static str {
        match self {
            WallpaperError::TooBigFile => tk!("settings.wp.why_big_file"),
            WallpaperError::UnknownFormat => tk!("settings.wp.why_format"),
            WallpaperError::TooBigImage => tk!("settings.wp.why_big_image"),
            WallpaperError::Decode(_) | WallpaperError::Image(_) => tk!("settings.wp.why_bad"),
        }
    }
}

impl core::fmt::Display for WallpaperError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WallpaperError::TooBigFile => f.write_str("file too big"),
            WallpaperError::UnknownFormat => f.write_str("not PNG/BMP/PPM"),
            WallpaperError::TooBigImage => f.write_str("image too big"),
            WallpaperError::Decode(e) => write!(f, "{e}"),
            WallpaperError::Image(e) => write!(f, "{e}"),
        }
    }
}

/// Reject a file that would need too much memory, looking only at its size and
/// header (nothing is allocated). PNG and BMP carry their size at fixed
/// offsets; a PPM is bounded by the file size.
pub fn check_source(bytes: &[u8]) -> Result<(), WallpaperError> {
    if bytes.len() > MAX_FILE {
        return Err(WallpaperError::TooBigFile);
    }
    let dims = match image::detect(bytes) {
        None => return Err(WallpaperError::UnknownFormat),
        Some(Format::Png) => crate::format::png::read_header(bytes)
            .map(|h| (h.width as u64, h.height as u64))
            .map_err(|_| WallpaperError::TooBigImage)?,
        Some(Format::Bmp) => {
            let w = bytes
                .get(18..22)
                .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            let h = bytes
                .get(22..26)
                .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            match (w, h) {
                (Some(w), Some(h)) => (w.unsigned_abs() as u64, h.unsigned_abs() as u64),
                _ => return Err(WallpaperError::TooBigImage),
            }
        }
        Some(Format::Ppm) => return Ok(()),
    };
    if dims.0.saturating_mul(dims.1) > MAX_SRC_PIXELS {
        return Err(WallpaperError::TooBigImage);
    }
    Ok(())
}

/// Scale `img` until it covers `sw x sh` and crop the centre to exactly that.
pub fn cover(img: &Image, sw: usize, sh: usize) -> Result<Image, ImageError> {
    let (w, h) = cover_dims(img.width(), img.height(), sw, sh).ok_or(ImageError::ZeroSize)?;
    let scaled;
    let src = if (w, h) == (img.width(), img.height()) {
        img
    } else {
        scaled = img.resize(w, h, Filter::Auto)?;
        &scaled
    };
    if (w, h) == (sw, sh) {
        return Ok(src.clone());
    }
    src.crop((w - sw) / 2, (h - sh) / 2, sw, sh)
}

/// Decode `bytes` and fit them to the screen: the whole "user wallpaper" path.
pub fn load(bytes: &[u8], sw: usize, sh: usize) -> Result<Image, WallpaperError> {
    check_source(bytes)?;
    let img = image::decode(bytes).map_err(WallpaperError::Decode)?;
    cover(&img, sw, sh).map_err(WallpaperError::Image)
}

#[cfg(test)]
mod tests;
