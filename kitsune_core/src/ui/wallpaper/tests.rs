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
            let cross =
                (b.0 - a.0) as i32 * (c.1 - a.1) as i32 - (b.1 - a.1) as i32 * (c.0 - a.0) as i32;
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
    let crc = crate::format::inflate::crc32(&png[12..29]);
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
