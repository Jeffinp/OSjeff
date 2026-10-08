//! Wallpapers: the built-in presets and the "cover" fit used for a user image.
//!
//! The presets are data (two colours and a kind); the kernel paints them. The
//! user image goes through [`crate::image::decode`] and [`cover`], which scales
//! it until it covers the screen and crops the centre, so it never letterboxes.
//! Memory is bounded before anything is allocated ([`check_source`]).

use crate::image::{self, Filter, Format, Image, ImageError};

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
    pub name: &'static str,
    pub style: Style,
    pub top: u32,
    pub bottom: u32,
    pub blobs: [Blob; 3],
    pub dark: Option<Scheme>,
}

impl Preset {
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

/// Preset 0 is the default and follows the appearance (pale by day, deep indigo at
/// night); the others keep one look.
pub const PRESETS: [Preset; 5] = [
    Preset {
        name: "Dinamico",
        style: Style::Glow,
        top: 0xDCE6FF,
        bottom: 0xF3E8FF,
        blobs: [
            blob(200, 250, 600, 0x7C83FF, 120),
            blob(820, 200, 420, 0x5EEAD4, 100),
            blob(650, 900, 700, 0xF5A3D7, 90),
        ],
        dark: Some(Scheme {
            top: 0x0F1230,
            bottom: 0x1B1448,
            blobs: [
                blob(180, 250, 650, 0x5B5CF6, 120),
                blob(850, 150, 420, 0x14B8C4, 80),
                blob(700, 950, 700, 0x8B3FD9, 100),
            ],
        }),
    },
    Preset {
        name: "Ceu",
        style: Style::Glow,
        top: 0x8EC9FF,
        bottom: 0xE6F4FF,
        blobs: [
            blob(300, 200, 600, 0xFFFFFF, 120),
            blob(800, 700, 600, 0xFFFFFF, 90),
            blob(500, 1000, 500, 0xBFE3FF, 100),
        ],
        dark: None,
    },
    Preset {
        name: "Ocaso",
        style: Style::Glow,
        top: 0x2B1B5A,
        bottom: 0xFF7A59,
        blobs: [
            blob(800, 900, 700, 0xFFB36B, 130),
            blob(200, 100, 500, 0x8B5CF6, 100),
            blob(500, 550, 400, 0xFF5C8A, 70),
        ],
        dark: None,
    },
    Preset {
        name: "Aurora",
        style: Style::Glow,
        top: 0x04161F,
        bottom: 0x0B3A44,
        blobs: [
            blob(200, 200, 600, 0x14B8C4, 110),
            blob(800, 800, 600, 0x22C55E, 80),
            blob(600, 100, 400, 0x5B5CF6, 80),
        ],
        dark: None,
    },
    Preset {
        name: "Grafite",
        style: Style::Glow,
        top: 0x2A2C35,
        bottom: 0x14151A,
        blobs: [
            blob(300, 200, 500, 0xFFFFFF, 22),
            blob(800, 850, 600, 0x8B90A0, 26),
            NO_BLOB,
        ],
        dark: None,
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
        // Includes a light preset and a dark one.
        assert!(PRESETS.iter().any(|p| luma(p.top) + luma(p.bottom) > 900));
        assert!(PRESETS.iter().any(|p| luma(p.top) + luma(p.bottom) < 200));
        for p in PRESETS {
            assert!(!p.name.is_empty() && p.top <= 0xFFFFFF && p.bottom <= 0xFFFFFF);
            for b in p.scheme(false).blobs.iter().chain(&p.scheme(true).blobs) {
                assert!((0..=1000).contains(&b.x) && (0..=1000).contains(&b.y) && b.r <= 1000);
                assert!(b.color <= 0xFFFFFF);
            }
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
}
