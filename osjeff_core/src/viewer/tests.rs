//! Tests of the image viewer logic.

use super::*;
use crate::image::Image;

fn file(name: &str) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: EntryKind::File,
        size: 1,
        mtime: 0,
    }
}

fn dir(name: &str) -> Entry {
    Entry {
        kind: EntryKind::Dir,
        ..file(name)
    }
}

// ---- zoom math ----

#[test]
fn scaled_dim_rounds_and_never_hits_zero() {
    assert_eq!(scaled_dim(1024, 1000), 1024);
    assert_eq!(scaled_dim(1024, 500), 512);
    assert_eq!(scaled_dim(3, 500), 2);
    assert_eq!(scaled_dim(1, 50), 1);
    assert_eq!(scaled_dim(100, 16_000), 1600);
}

#[test]
fn fit_zoom_shrinks_but_never_enlarges() {
    assert_eq!(fit_zoom(1024, 768, 512, 768), 500);
    assert_eq!(fit_zoom(1024, 768, 2048, 2048), 1000);
    assert_eq!(fit_zoom(100, 100, 1000, 1000), 1000);
    assert_eq!(fit_zoom(1000, 2000, 500, 500), 250);
}

#[test]
fn fit_zoom_result_fits_in_the_box() {
    for (iw, ih, vw, vh) in [
        (1024, 768, 700, 450),
        (4000, 3000, 640, 400),
        (333, 777, 200, 200),
    ] {
        let z = fit_zoom(iw, ih, vw, vh);
        assert!(
            scaled_dim(iw, z) as i32 <= vw + 1,
            "{iw}x{ih} in {vw}x{vh}: {z}"
        );
        assert!(scaled_dim(ih, z) as i32 <= vh + 1);
    }
}

#[test]
fn fit_zoom_degenerate_inputs() {
    assert_eq!(fit_zoom(0, 5, 10, 10), 1000);
    assert_eq!(fit_zoom(5, 5, 0, 10), 1000);
    assert_eq!(fit_zoom(100_000, 1, 10, 10), MIN_ZOOM);
}

#[test]
fn zoom_steps_walk_the_table() {
    assert_eq!(next_zoom(1000), 1250);
    assert_eq!(prev_zoom(1000), 750);
    assert_eq!(next_zoom(1200), 1250);
    assert_eq!(prev_zoom(1200), 1000);
    assert_eq!(next_zoom(MAX_ZOOM), MAX_ZOOM);
    assert_eq!(prev_zoom(MIN_ZOOM), MIN_ZOOM);
    assert_eq!(next_zoom(0), 50);
    // Strictly monotonic over the whole range.
    let mut z = MIN_ZOOM;
    let mut n = 0;
    while z < MAX_ZOOM {
        let nz = next_zoom(z);
        assert!(nz > z);
        z = nz;
        n += 1;
    }
    assert_eq!(n, ZOOM_STEPS.len() - 1);
}

#[test]
fn zoom_labels() {
    assert_eq!(zoom_label(1000), "100%");
    assert_eq!(zoom_label(667), "67%");
    assert_eq!(zoom_label(50), "5%");
    assert_eq!(zoom_label(16_000), "1600%");
}

// ---- pan ----

#[test]
fn clamp_pan_centres_small_images_and_limits_big_ones() {
    assert_eq!(clamp_pan(50, 100, 200), 0);
    assert_eq!(clamp_pan(50, 200, 200), 0);
    assert_eq!(clamp_pan(500, 400, 200), 100);
    assert_eq!(clamp_pan(-500, 400, 200), -100);
    assert_eq!(clamp_pan(30, 400, 200), 30);
}

#[test]
fn image_origin_centres_and_pans() {
    assert_eq!(image_origin(200, 100, 100, 50, 0, 0), (50, 25));
    assert_eq!(image_origin(200, 100, 400, 300, 0, 0), (-100, -100));
    assert_eq!(image_origin(200, 100, 400, 300, 30, -10), (-70, -110));
}

#[test]
fn view_fit_and_actual() {
    let mut v = View::default();
    v.fit_to(1024, 768, 512, 512);
    assert!(v.fit);
    assert_eq!(v.zoom, 500);
    v.actual();
    assert_eq!((v.zoom, v.fit), (1000, false));
}

#[test]
fn zoom_in_and_out_follow_the_steps_and_leave_fit_mode() {
    let mut v = View::default();
    v.fit_to(1024, 768, 400, 300);
    let z0 = v.zoom;
    v.zoom_in(1024, 768, 400, 300);
    assert!(v.zoom > z0 && !v.fit);
    v.zoom_out(1024, 768, 400, 300);
    v.zoom_out(1024, 768, 400, 300);
    assert!(v.zoom < z0);
}

#[test]
fn zoom_is_bounded() {
    let mut v = View::default();
    for _ in 0..50 {
        v.zoom_in(100, 100, 300, 300);
    }
    assert_eq!(v.zoom, MAX_ZOOM);
    for _ in 0..50 {
        v.zoom_out(100, 100, 300, 300);
    }
    assert_eq!(v.zoom, MIN_ZOOM);
}

#[test]
fn zoom_at_keeps_the_point_under_the_cursor() {
    // 1000x1000 image at 100% in a 400x400 viewport; zoom 2x around (100, 0).
    let mut v = View::default();
    v.actual();
    v.set_zoom_at(2000, (100, 0), 1000, 1000, 400, 400);
    // The image point that was at +100 from the centre is still there.
    assert_eq!(v.pan_x, -100);
    assert_eq!(v.pan_y, 0);
    assert_eq!(v.zoom, 2000);
}

#[test]
fn zoom_at_clamps_the_pan() {
    let mut v = View::default();
    v.actual();
    v.set_zoom_at(2000, (10_000, 0), 1000, 1000, 400, 400);
    let (sw, _) = v.scaled(1000, 1000);
    assert!(v.pan_x.abs() <= (sw - 400 + 1) / 2);
}

#[test]
fn pan_by_is_clamped_and_noop_when_the_image_fits() {
    let mut v = View::default();
    v.actual();
    v.pan_by(10, 10, 100, 100, 400, 400);
    assert_eq!((v.pan_x, v.pan_y), (0, 0));
    v.pan_by(10_000, -10_000, 1000, 1000, 400, 400);
    assert_eq!(v.pan_x, 300);
    assert_eq!(v.pan_y, -300);
    v.pan_by(i32::MAX, i32::MIN, 1000, 1000, 400, 400);
    assert_eq!((v.pan_x, v.pan_y), (300, -300));
}

#[test]
fn relayout_refits_in_fit_mode_and_reclamps_otherwise() {
    let mut v = View::default();
    v.relayout(1000, 1000, 500, 500);
    assert_eq!(v.zoom, 500);
    v.relayout(1000, 1000, 250, 250);
    assert_eq!(v.zoom, 250);
    v.actual();
    v.pan_by(100, 0, 1000, 1000, 500, 500);
    v.relayout(1000, 1000, 1500, 1500); // window grew past the image
    assert_eq!(v.pan_x, 0);
}

#[test]
fn origin_and_scaled_agree() {
    let mut v = View::default();
    v.fit_to(1024, 768, 512, 512);
    let (sw, sh) = v.scaled(1024, 768);
    assert_eq!((sw, sh), (512, 384));
    assert_eq!(v.origin(1024, 768, 512, 512), (0, 64));
}

// ---- column mapping ----

#[test]
fn column_map_at_100_percent_and_zoomed() {
    let m = column_map(0, 5, 2, 1000, 10);
    assert_eq!(m, vec![None, None, Some(0), Some(1), Some(2)]);
    let m = column_map(0, 6, 0, 2000, 10);
    assert_eq!(
        m,
        vec![Some(0), Some(0), Some(1), Some(1), Some(2), Some(2)]
    );
    let m = column_map(0, 4, 0, 500, 10);
    assert_eq!(m, vec![Some(0), Some(2), Some(4), Some(6)]);
}

#[test]
fn column_map_cuts_at_the_right_edge() {
    let m = column_map(8, 14, 0, 1000, 10);
    assert_eq!(m, vec![Some(8), Some(9), None, None, None, None]);
}

#[test]
fn checkerboard_alternates() {
    assert!(!checker_dark(0, 0));
    assert!(checker_dark(8, 0));
    assert!(checker_dark(0, 8));
    assert!(!checker_dark(8, 8));
    assert!(checker_dark(-1, 0));
    assert!(!checker_dark(-9, 0));
}

// ---- image list ----

#[test]
fn list_picks_images_and_sorts_naturally() {
    let es = [
        file("img10.png"),
        file("notes.txt"),
        file("img2.png"),
        dir("pics.png"),
        file("b.BMP"),
        file("c.ppm"),
    ];
    let l = ImageList::from_entries(b"/Fotos", &es, b"img2.png");
    assert_eq!(l.len(), 4);
    assert_eq!(l.current(), Some(b"/Fotos/img2.png".to_vec()));
    // b.BMP, c.ppm, img2.png, img10.png -> index 2.
    assert_eq!(l.index(), 2);
}

#[test]
fn next_and_prev_wrap() {
    let es = [file("a.png"), file("b.png"), file("c.png")];
    let mut l = ImageList::from_entries(b"/", &es, b"c.png");
    assert_eq!(l.go_next(), Some(b"/a.png".to_vec()));
    assert_eq!(l.go_prev(), Some(b"/c.png".to_vec()));
    assert_eq!(l.go_prev(), Some(b"/b.png".to_vec()));
    assert_eq!(l.index(), 1);
}

#[test]
fn path_at_and_go_to_address_images_by_index() {
    let es = [file("b.png"), file("a.png"), file("c.bmp"), dir("sub")];
    let mut l = ImageList::from_entries(b"/fotos", &es, b"a.png");
    assert_eq!(l.len(), 3);
    assert_eq!(l.path_at(0), Some(b"/fotos/a.png".to_vec()));
    assert_eq!(l.path_at(2), Some(b"/fotos/c.bmp".to_vec()));
    assert_eq!(l.path_at(3), None);
    assert_eq!(l.go_to(2), Some(b"/fotos/c.bmp".to_vec()));
    assert_eq!(l.index(), 2);
    assert_eq!(l.go_to(9), None);
    assert_eq!(l.index(), 2);
}

#[test]
fn list_includes_the_current_file_even_if_unlisted() {
    let l = ImageList::from_entries(b"/d", &[file("a.png")], b"zz.png");
    assert_eq!(l.len(), 2);
    assert_eq!(l.current(), Some(b"/d/zz.png".to_vec()));
}

#[test]
fn removing_the_current_image_moves_on() {
    let es = [file("a.png"), file("b.png")];
    let mut l = ImageList::from_entries(b"/", &es, b"b.png");
    l.remove_current();
    assert_eq!(l.len(), 1);
    assert_eq!(l.current(), Some(b"/a.png".to_vec()));
    l.remove_current();
    assert!(l.is_empty());
    assert_eq!(l.go_next(), None);
    assert_eq!(l.current(), None);
    l.remove_current();
}

#[test]
fn a_single_image_list_stays_put() {
    let mut l = ImageList::from_entries(b"/", &[file("only.png")], b"only.png");
    assert_eq!(l.go_next(), Some(b"/only.png".to_vec()));
    assert_eq!(l.go_prev(), Some(b"/only.png".to_vec()));
}

// ---- text ----

#[test]
fn info_rows_describe_the_image() {
    let l = info_rows(
        "foto.png".as_bytes(),
        1024,
        768,
        Some(Format::Png),
        3_000_000,
        500,
        (2, 12),
        false,
    );
    let get = |k: &str| l.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(get("Nome"), Some("foto.png"));
    assert_eq!(get("Dimensões"), Some("1024 × 768 px"));
    assert_eq!(get("Resolução"), Some("0,8 Mpx"));
    assert_eq!(get("Formato"), Some("PNG"));
    assert_eq!(get("Tamanho"), Some("2,8 MiB"));
    assert_eq!(get("Transparência"), Some("Não"));
    assert_eq!(get("Zoom"), Some("50%"));
    assert_eq!(get("Posição"), Some("3 de 12"));
}

#[test]
fn info_rows_keep_accents_and_skip_the_position_for_one_image() {
    let l = info_rows(
        "ação.bmp".as_bytes(),
        2,
        2,
        Some(Format::Bmp),
        70,
        1000,
        (0, 1),
        true,
    );
    assert_eq!(l[0].1, "ação.bmp");
    assert!(l.iter().all(|(k, _)| k != "Posição"));
    assert!(l.iter().any(|(k, v)| k == "Transparência" && v == "Sim"));
    let l = info_rows(b"x", 1, 1, None, 0, 1000, (0, 1), false);
    assert!(l.iter().any(|(k, v)| k == "Formato" && v == "—"));
}

#[test]
fn decode_errors_have_friendly_messages() {
    let e = crate::image::decode(b"not an image").unwrap_err();
    let m = decode_error_message(&e);
    assert_eq!(m[0], "Formato não reconhecido");
    assert!(m[1].contains("PNG, BMP e PPM"));
    let e = crate::image::decode(b"\x89PNG\r\n\x1a\ngarbage").unwrap_err();
    let m = decode_error_message(&e);
    assert!(m[1].contains("PNG") && !m[0].is_empty());
    let e = crate::image::decode(b"BM\0\0\0").unwrap_err();
    assert!(decode_error_message(&e)[1].contains("BMP"));
    let e = crate::image::decode(b"P6\n").unwrap_err();
    assert!(decode_error_message(&e)[1].contains("PPM"));
    // Plain words for the user: no codes, no implementation talk.
    for m in [
        decode_error_message(&e),
        decode_error_message(&crate::image::decode(b"x").unwrap_err()),
    ] {
        for line in &m {
            assert!(!line.to_lowercase().contains("rust") && !line.contains("0x"));
        }
    }
}

#[test]
fn image_errors_have_messages() {
    for e in [
        ImageError::TooLarge,
        ImageError::OutOfMemory,
        ImageError::ZeroSize,
        ImageError::BadBuffer,
        ImageError::OutOfBounds,
    ] {
        assert!(!image_error_message(e).is_empty());
    }
    assert_eq!(
        image_error_message(ImageError::OutOfMemory),
        "Memória insuficiente"
    );
}

#[test]
fn fill_covers_the_viewport_where_fit_stays_inside() {
    // A wide picture in a squarer window: fit leaves bars, fill crops the sides.
    let (iw, ih, vw, vh) = (800usize, 400usize, 500, 400);
    let fit = fit_zoom(iw, ih, vw, vh);
    let fill = fill_zoom(iw, ih, vw, vh);
    assert_eq!(fit, 625);
    assert_eq!(fill, 1000);
    assert!(fill >= fit);
    // Whatever the shape, the filled image covers both dimensions.
    for (iw, ih, vw, vh) in [
        (100, 300, 640, 480),
        (4000, 3000, 300, 300),
        (7, 5, 800, 600),
    ] {
        let z = fill_zoom(iw, ih, vw, vh).min(MAX_ZOOM);
        assert!(
            scaled_dim(iw, z) as i32 >= vw || z == MAX_ZOOM,
            "{iw}x{ih} in {vw}x{vh}"
        );
        assert!(scaled_dim(ih, z) as i32 >= vh || z == MAX_ZOOM);
    }
    assert_eq!(fill_zoom(0, 5, 100, 100), 1000);
    assert_eq!(fill_zoom(5, 5, 0, 100), 1000);
    assert_eq!(fill_zoom(1, 1, 1000, 1000), MAX_ZOOM);
}

#[test]
fn view_fill_mode_follows_the_viewport_and_leaves_on_zoom() {
    let mut v = View::default();
    v.fill_to(800, 400, 500, 400);
    assert!(v.fill && !v.fit);
    assert_eq!(v.zoom, 1000);
    // Dragging the filled picture sideways is allowed; a resize keeps the pan.
    v.pan_by(-60, 0, 800, 400, 500, 400);
    assert_eq!(v.pan_x, -60);
    v.relayout(800, 400, 560, 400);
    assert!(v.fill);
    assert_eq!(v.pan_x, -60);
    assert_eq!(v.zoom, fill_zoom(800, 400, 560, 400));
    // Zooming, fitting and 100% all leave fill mode.
    let mut z = v;
    z.zoom_in(800, 400, 560, 400);
    assert!(!z.fill);
    let mut f = v;
    f.fit_to(800, 400, 560, 400);
    assert!(f.fit && !f.fill);
    let mut a = v;
    a.actual();
    assert!(!a.fill && !a.fit);
}

#[test]
fn corrupted_files_never_panic_the_decoder() {
    // A real PNG, cut and bit-flipped every which way, must only ever return Err.
    let img = Image::new(16, 16, 0xFF30_60C0).unwrap();
    let png = crate::png::encode(&img).unwrap();
    for cut in 0..png.len() {
        let _ = crate::image::decode(&png[..cut]);
    }
    for i in 0..png.len() {
        let mut bad = png.clone();
        bad[i] ^= 0x5A;
        let _ = crate::image::decode(&bad);
    }
}

#[test]
fn save_formats_by_extension() {
    assert_eq!(save_format(b"a.png"), Some(Format::Png));
    assert_eq!(save_format(b"a.BMP"), Some(Format::Bmp));
    assert_eq!(save_format(b"a.ppm"), Some(Format::Ppm));
    assert_eq!(save_format(b"a.jpg"), None);
    assert_eq!(save_format(b"a"), None);
}

#[test]
fn suggested_save_name_is_a_png_copy() {
    assert_eq!(suggest_save_name(b"foto.bmp"), b"foto (copia).png");
    assert_eq!(suggest_save_name(b"semext"), b"semext (copia).png");
    assert_eq!(save_format(&suggest_save_name(b"x.ppm")), Some(Format::Png));
}

#[test]
fn encode_after_rotation_round_trips_through_each_format() {
    let mut img = Image::new(3, 2, 0xFF00_0000).unwrap();
    for (i, p) in img.pixels_mut().iter_mut().enumerate() {
        *p = 0xFF00_0000 | (i as u32 * 40) << 8 | i as u32;
    }
    let rot = img.rotate90().unwrap();
    assert_eq!((rot.width(), rot.height()), (2, 3));
    for f in [Format::Png, Format::Bmp, Format::Ppm] {
        let bytes = crate::image::encode(&rot, f).unwrap();
        assert_eq!(crate::image::detect(&bytes), Some(f));
        let back = crate::image::decode(&bytes).unwrap();
        assert_eq!(back.pixels(), rot.pixels(), "{f:?}");
    }
}

#[test]
fn flips_and_rotations_compose() {
    let mut img = Image::new(2, 2, 0).unwrap();
    img.pixels_mut().copy_from_slice(&[1, 2, 3, 4]);
    let mut a = img.clone();
    a.flip_horizontal();
    a.flip_horizontal();
    assert_eq!(a, img);
    let r4 = img
        .rotate90()
        .unwrap()
        .rotate90()
        .unwrap()
        .rotate90()
        .unwrap()
        .rotate90()
        .unwrap();
    assert_eq!(r4, img);
    let mut f = img.rotate90().unwrap();
    f.flip_vertical();
    assert_eq!(f.pixels(), &[4, 2, 3, 1]);
}
