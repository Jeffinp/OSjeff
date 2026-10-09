//! Wallpapers: the built-in presets and the "cover" fit used for a user image.
//!
//! The presets are data (two colours and a kind); the kernel paints them. The
//! user image goes through [`crate::image::decode`] and [`cover`], which scales
//! it until it covers the screen and crops the centre, so it never letterboxes.
//! Memory is bounded before anything is allocated ([`check_source`]).

use crate::image::{self, Filter, Format, Image, ImageError};
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
        Some(Format::Png) => crate::png::read_header(bytes)
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
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, p: u32) -> Image {
        Image::new(w, h, p).unwrap()
    }

    #[test]
    fn cover_dims_cases() {
        // Wider image on a 16:9 screen: height-limited.
        assert_eq!(cover_dims(400, 100, 1280, 720), Some((2880, 720)));
        // Taller image: width-limited.
        assert_eq!(cover_dims(100, 400, 1280, 720), Some((1280, 5120)));
        // Same aspect.
        assert_eq!(cover_dims(64, 36, 1280, 720), Some((1280, 720)));
        // Exact size stays.
        assert_eq!(cover_dims(1280, 720, 1280, 720), Some((1280, 720)));
        assert_eq!(cover_dims(0, 5, 10, 10), None);
        assert_eq!(cover_dims(5, 5, 0, 10), None);
    }

    #[test]
    fn cover_dims_always_covers() {
        for (iw, ih) in [(1, 1), (3, 7), (640, 480), (1920, 1080), (17, 3), (1, 1000)] {
            for (sw, sh) in [(1280, 720), (1024, 768), (800, 600), (1920, 1080)] {
                let (w, h) = cover_dims(iw, ih, sw, sh).unwrap();
                assert!(w >= sw && h >= sh, "{iw}x{ih} on {sw}x{sh} -> {w}x{h}");
                assert!(w == sw || h == sh);
            }
        }
    }

    #[test]
    fn cover_crops_the_centre() {
        // 8x2 image: left 4 columns red, right 4 blue; cover 4x4.
        let mut img = solid(8, 2, 0xFFFF0000);
        for y in 0..2 {
            for x in 4..8 {
                img.set(x, y, 0xFF0000FF);
            }
        }
        let out = cover(&img, 4, 4).unwrap();
        assert_eq!((out.width(), out.height()), (4, 4));
        // Centre crop of the scaled (16x4) image: columns 6..10 straddle the red/blue edge.
        assert_eq!(out.get(0, 0), Some(0xFFFF0000));
        assert_eq!(out.get(3, 3), Some(0xFF0000FF));
    }

    #[test]
    fn cover_exact_size_is_a_copy() {
        let img = solid(16, 9, 0xFF123456);
        let out = cover(&img, 16, 9).unwrap();
        assert_eq!(out.pixels(), img.pixels());
    }

    #[test]
    fn cover_a_constant_image_stays_constant() {
        let img = solid(5, 3, 0xFF336699);
        let out = cover(&img, 40, 30).unwrap();
        assert!(out.pixels().iter().all(|&p| p == 0xFF336699));
    }

    #[test]
    fn lerp_endpoints_and_middle() {
        assert_eq!(lerp_rgb(0x000000, 0xFFFFFF, 0), 0x000000);
        assert_eq!(lerp_rgb(0x000000, 0xFFFFFF, 255), 0xFFFFFF);
        assert_eq!(lerp_rgb(0x102030, 0x102030, 77), 0x102030);
        assert_eq!(lerp_rgb(0, 0xFF0000, 128) >> 16, 128);
        assert_eq!(lerp_rgb(0, 0xFF0000, 1000), 0xFF0000);
    }

    #[test]
    fn presets_are_sane() {
        // The default follows the appearance: pale in light, deep in dark.
        let luma = |c: u32| ((c >> 16) & 0xFF) + ((c >> 8) & 0xFF) + (c & 0xFF);
        let light = PRESETS[0].scheme(false);
        let dark = PRESETS[0].scheme(true);
        assert!(luma(light.top) > luma(dark.top) + 300);
        assert!(luma(light.bottom) > luma(dark.bottom) + 300);
        // A fixed preset shows the same scheme in both appearances.
        assert_eq!(PRESETS[2].scheme(true), PRESETS[2].scheme(false));
        assert_eq!(PRESETS.len(), 6);
        // Includes a light preset and a dark one.
        assert!(PRESETS.iter().any(|p| luma(p.top) + luma(p.bottom) > 900));
        assert!(PRESETS.iter().any(|p| luma(p.top) + luma(p.bottom) < 200));
        for p in PRESETS {
            assert!(!p.name_key.is_empty() && p.top <= 0xFFFFFF && p.bottom <= 0xFFFFFF);
            for b in p.scheme(false).blobs.iter().chain(&p.scheme(true).blobs) {
                assert!((0..=1000).contains(&b.x) && (0..=1000).contains(&b.y) && b.r <= 1000);
                assert!(b.color <= 0xFFFFFF);
            }
        }
    }

    #[test]
    fn shapes_stay_on_the_screen_and_only_shape_presets_have_them() {
        for p in PRESETS {
            assert_eq!(
                p.style == Style::Shapes,
                !p.shapes.is_empty(),
                "{}",
                p.name()
            );
            for sh in p.shapes {
                for (x, y) in sh.pts {
                    assert!(
                        (0..=1000).contains(&x) && (0..=1000).contains(&y),
                        "{}",
                        p.name()
                    );
                }
                for dark in [false, true] {
                    let (c, a) = sh.look(dark);
                    assert!(c <= 0xFFFFFF && a > 0, "{}", p.name());
                }
                // A polygon, not a line: the corners are not all collinear.
                let [a, b, c, _] = sh.pts;
                let cross = (b.0 - a.0) as i32 * (c.1 - a.1) as i32
                    - (b.1 - a.1) as i32 * (c.0 - a.0) as i32;
                assert_ne!(cross, 0, "{}", p.name());
            }
        }
        // The default and the sunset change nothing between appearances only where meant.
        assert_ne!(
            PRESETS[0].shapes[0].look(true),
            PRESETS[0].shapes[0].look(false)
        );
        assert_eq!(
            PRESETS[5].shapes[0].look(true),
            PRESETS[5].shapes[0].look(false)
        );
        // Names are short enough for the Settings thumbnails.
        for l in crate::i18n::Lang::ALL {
            assert!(PRESETS.iter().all(|p| {
                let n = p.name_in(l);
                !n.is_empty() && n != p.name_key && n.chars().count() <= 11
            }));
        }
    }

    #[test]
    fn check_source_limits() {
        assert_eq!(check_source(&[]), Err(WallpaperError::UnknownFormat));
        assert_eq!(
            check_source(b"hello world"),
            Err(WallpaperError::UnknownFormat)
        );
        let big = alloc::vec![0u8; MAX_FILE + 1];
        assert_eq!(check_source(&big), Err(WallpaperError::TooBigFile));
        // A BMP header claiming 100000 x 100000.
        let mut bmp = alloc::vec![0u8; 64];
        bmp[0] = b'B';
        bmp[1] = b'M';
        bmp[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&100_000i32.to_le_bytes());
        assert_eq!(check_source(&bmp), Err(WallpaperError::TooBigImage));
        // A truncated BMP header.
        assert_eq!(check_source(b"BM12"), Err(WallpaperError::TooBigImage));
    }

    #[test]
    fn load_roundtrip_png_and_bmp_and_garbage() {
        let img = solid(32, 18, 0xFF204060);
        for f in [Format::Png, Format::Bmp, Format::Ppm] {
            let bytes = image::encode(&img, f).unwrap();
            let out = load(&bytes, 128, 72).unwrap();
            assert_eq!((out.width(), out.height()), (128, 72));
            assert_eq!(out.get(5, 5), Some(0xFF204060), "{f:?}");
        }
        let mut bad = image::encode(&img, Format::Png).unwrap();
        let n = bad.len();
        bad.truncate(n - 20);
        assert!(load(&bad, 128, 72).is_err());
    }

    #[test]
    fn png_header_over_limit_is_refused_before_decoding() {
        // IHDR claiming 4000x4000 = 16 Mpx > 8 Mpx.
        let mut png = alloc::vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&4000u32.to_be_bytes());
        png.extend_from_slice(&4000u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        let crc = crate::inflate::crc32(&png[12..29]);
        png.extend_from_slice(&crc.to_be_bytes());
        assert_eq!(check_source(&png), Err(WallpaperError::TooBigImage));
    }

    #[test]
    fn every_refusal_has_a_reason_in_both_languages() {
        use crate::i18n::{Lang, tr_in};
        let errs = [
            WallpaperError::TooBigFile,
            WallpaperError::UnknownFormat,
            WallpaperError::TooBigImage,
            WallpaperError::Decode(image::DecodeError::UnknownFormat),
            WallpaperError::Image(ImageError::ZeroSize),
        ];
        for e in errs {
            for l in Lang::ALL {
                let t = tr_in(l, e.why_key());
                assert!(t != e.why_key() && !t.is_empty(), "{e:?}");
            }
        }
        assert_eq!(
            tr_in(Lang::En, WallpaperError::UnknownFormat.why_key()),
            "use PNG, BMP or PPM"
        );
        assert!(tr_in(Lang::Pt, WallpaperError::TooBigImage.why_key()).contains("é"));
    }
}
