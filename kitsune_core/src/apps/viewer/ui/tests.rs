//! Tests of the Imagens geometry, filmstrip, inertia, slideshow and rotation maths.

use super::*;

fn win() -> Rect {
    Rect::new(100, 80, 820, 540)
}

// ---- regions ----

#[test]
fn regions_fit_the_window_and_do_not_overlap() {
    for multi in [false, true] {
        for info in [None, Some(7)] {
            let l = Layout::of(win(), multi, info);
            let w = l.window;
            for r in [
                l.toolbar,
                l.rot_left,
                l.rot_right,
                l.flip,
                l.modes,
                l.slideshow,
                l.info_btn,
                l.save,
                l.canvas,
                l.caption,
            ] {
                assert!(r.x >= w.x && r.right() <= w.right(), "{r:?}");
                assert!(r.y >= w.y + TITLE_H && r.bottom() <= w.bottom(), "{r:?}");
            }
            let row = [
                l.rot_left,
                l.rot_right,
                l.flip,
                l.modes,
                l.slideshow,
                l.info_btn,
                l.save,
            ];
            for pair in row.windows(2) {
                assert!(pair[0].right() <= pair[1].x, "{:?} {:?}", pair[0], pair[1]);
            }
            assert!(l.toolbar.bottom() <= l.canvas.y);
            assert!(l.canvas.bottom() <= l.caption.y);
            assert_eq!(l.strip.is_some(), multi);
            if let Some(s) = l.strip {
                assert!(l.canvas.bottom() <= s.y && s.bottom() <= l.caption.y);
                assert!(s.h >= THUMB);
            }
            if let Some(i) = l.info {
                assert!(
                    l.canvas.contains(i.x, i.y) && l.canvas.contains(i.right() - 1, i.bottom() - 1)
                );
            }
        }
    }
}

#[test]
fn the_minimum_window_keeps_every_control_apart() {
    let l = Layout::of(Rect::new(0, 0, 520, 340), true, Some(8));
    let row = [
        l.rot_left,
        l.rot_right,
        l.flip,
        l.modes,
        l.slideshow,
        l.info_btn,
        l.save,
    ];
    for pair in row.windows(2) {
        assert!(pair[0].right() <= pair[1].x, "{:?} {:?}", pair[0], pair[1]);
    }
    assert!(l.modes.w >= 90);
    assert!(l.canvas.h > 80 && l.canvas.w > 300, "{:?}", l.canvas);
}

#[test]
fn layout_survives_tiny_windows() {
    let l = Layout::of(Rect::new(0, 0, 60, 40), true, Some(9));
    assert!(l.canvas.w >= 0 && l.canvas.h >= 0 && l.caption.h >= 0);
    let l = Layout::of(Rect::new(0, 0, 0, 0), false, None);
    assert!(l.canvas.h >= 0);
}

#[test]
fn hit_testing_finds_each_region() {
    let l = Layout::of(win(), true, Some(5));
    let c = |r: Rect| (r.x + r.w / 2, r.y + r.h / 2);
    let h = |p: (i32, i32)| l.hit(p.0, p.1, 0, 20);
    assert_eq!(h(c(l.rot_left)), Some(Hit::RotateLeft));
    assert_eq!(h(c(l.rot_right)), Some(Hit::RotateRight));
    assert_eq!(h(c(l.flip)), Some(Hit::Flip));
    assert_eq!(h(c(l.slideshow)), Some(Hit::Slideshow));
    assert_eq!(h(c(l.info_btn)), Some(Hit::Info));
    assert_eq!(h(c(l.save)), Some(Hit::Save));
    let m = l.modes;
    assert_eq!(h((m.x + 4, m.y + 4)), Some(Hit::Mode(FitMode::Fit)));
    assert_eq!(h((m.x + m.w / 2, m.y + 4)), Some(Hit::Mode(FitMode::Fill)));
    assert_eq!(
        h((m.right() - 4, m.y + 4)),
        Some(Hit::Mode(FitMode::Actual))
    );
    assert_eq!(h((l.canvas.x + 20, l.canvas.y + 20)), Some(Hit::Canvas));
    assert_eq!(h(c(l.info.unwrap())), Some(Hit::InfoPanel));
    assert_eq!(h((l.caption.x + 5, l.caption.y + 5)), Some(Hit::Dead));
    assert_eq!(h((l.toolbar.x + 2, l.toolbar.y + 2)), Some(Hit::Dead));
    let s = l.strip.unwrap();
    let t3 = thumb_rect(s, 0, 3);
    assert_eq!(h(c(t3)), Some(Hit::Thumb(3)));
    // The gap between thumbnails is dead space, not the picture.
    assert_eq!(h((t3.x - 3, t3.y + 4)), Some(Hit::Dead));
    assert_eq!(l.hit(0, 0, 0, 20), None);
}

// ---- fit mode ----

#[test]
fn the_mode_of_a_view_names_its_segment() {
    let mut v = View::default();
    assert_eq!(mode_of(&v), Some(FitMode::Fit));
    v.fill_to(800, 400, 500, 400);
    assert_eq!(mode_of(&v), Some(FitMode::Fill));
    v.actual();
    assert_eq!(mode_of(&v), Some(FitMode::Actual));
    v.zoom_in(800, 400, 500, 400);
    assert_eq!(mode_of(&v), None);
    for (i, m) in FitMode::ALL.iter().enumerate() {
        assert_eq!(m.index(), i);
    }
}

// ---- filmstrip ----

#[test]
fn strip_content_and_scroll_centre_the_current_thumbnail() {
    assert_eq!(strip_content_w(0), 0);
    assert_eq!(strip_content_w(1), 32 + THUMB);
    assert_eq!(strip_content_w(3), 32 + 3 * THUMB + 2 * THUMB_GAP);
    // A strip that fits is not scrolled.
    assert_eq!(strip_scroll_for(800, 5, 4), 0);
    // A long strip centres the current one, but never past its ends.
    let n = 40;
    let w = 600;
    assert_eq!(strip_scroll_for(w, n, 0), 0);
    assert_eq!(strip_scroll_for(w, n, n - 1), strip_content_w(n) - w);
    let mid = strip_scroll_for(w, n, 20);
    let r = thumb_rect(Rect::new(0, 0, w, 80), mid, 20);
    assert!((r.x + r.w / 2 - w / 2).abs() <= 1, "{r:?}");
    // Monotonic in the current index.
    let mut last = 0;
    for i in 0..n {
        let s = strip_scroll_for(w, n, i);
        assert!(s >= last);
        last = s;
    }
}

#[test]
fn strip_visible_covers_exactly_what_shows() {
    let n = 50;
    let strip = Rect::new(0, 0, 500, 80);
    for scroll in [0, 1, 63, 64, 200, 777, strip_content_w(n) - 500] {
        let (a, b) = strip_visible(strip.w, n, scroll);
        assert!(a <= b && b <= n);
        for i in 0..n {
            let r = thumb_rect(strip, scroll, i);
            let shows = r.x < strip.right() && r.right() > 0;
            if shows {
                assert!(i >= a && i < b, "scroll {scroll} thumb {i} not in {a}..{b}");
            }
        }
        // And not much more than shows.
        assert!(b - a <= (500 / (THUMB + THUMB_GAP)) as usize + 3);
    }
    assert_eq!(strip_visible(500, 0, 0), (0, 0));
    assert_eq!(strip_visible(0, 5, 0), (0, 0));
}

#[test]
fn thumbnails_are_found_by_position_and_scroll() {
    let strip = Rect::new(10, 400, 500, 70);
    for i in 0..30 {
        let scroll = 150;
        let r = thumb_rect(strip, scroll, i);
        if r.right() > strip.x && r.x < strip.right() {
            let px = (r.x + r.w / 2).clamp(strip.x, strip.right() - 1);
            let py = r.y + r.h / 2;
            if r.contains(px, py) {
                assert_eq!(strip_item_at(strip, 30, scroll, px, py), Some(i));
            }
        }
    }
    assert_eq!(strip_item_at(strip, 30, 0, strip.x + 2, strip.y + 2), None);
}

#[test]
fn cover_dims_cover_the_square_and_keep_the_aspect() {
    assert_eq!(cover_dims(800, 400, 56), (112, 56));
    assert_eq!(cover_dims(400, 800, 56), (56, 112));
    assert_eq!(cover_dims(100, 100, 56), (56, 56));
    for (w, h) in [(1, 1), (3, 100), (100, 3), (4000, 3000), (7, 5)] {
        let (cw, ch) = cover_dims(w, h, 56);
        assert!(cw >= 56 && ch >= 56, "{w}x{h} -> {cw}x{ch}");
        // The aspect ratio holds to within a pixel of rounding.
        let (a, b) = (cw as u64 * h as u64, ch as u64 * w as u64);
        assert!(
            a.abs_diff(b) <= (w.max(h) as u64) * 2,
            "{w}x{h} -> {cw}x{ch}"
        );
    }
    assert_eq!(cover_dims(0, 5, 56), (56, 56));
}

#[test]
fn thumbnails_are_made_nearest_first() {
    assert_eq!(thumb_order(5, 10, 4), vec![5, 4, 6, 3]);
    assert_eq!(thumb_order(0, 4, 10), vec![0, 1, 2, 3]);
    assert_eq!(thumb_order(9, 10, 3), vec![9, 8, 7]);
    assert!(thumb_order(0, 0, 5).is_empty());
}

// ---- inertia ----

#[test]
fn a_flick_glides_and_stops() {
    let mut i = Inertia::new();
    assert!(!i.active());
    assert_eq!(i.step(0.016), (0, 0));
    // Drag right at 600 px/s for a few events, then let go while still moving.
    for _ in 0..6 {
        i.push(10, 0, 0.0166);
    }
    i.release(0.01);
    assert!(i.active());
    let (mut total, mut frames) = (0, 0);
    while i.active() {
        let (dx, dy) = i.step(0.016);
        assert!(dx >= 0 && dy == 0);
        total += dx;
        frames += 1;
        assert!(frames < 200, "never stops");
    }
    assert!(total > 60 && total < 400, "{total}");
}

#[test]
fn no_glide_after_a_pause_or_a_slow_drag() {
    let mut i = Inertia::new();
    for _ in 0..6 {
        i.push(10, 0, 0.0166);
    }
    i.release(0.3); // the pointer had stopped
    assert!(!i.active());
    let mut i = Inertia::new();
    i.push(1, 0, 0.1);
    i.release(0.0);
    assert!(!i.active());
    // A new press stops a glide in flight.
    let mut i = Inertia::new();
    for _ in 0..6 {
        i.push(0, 12, 0.0166);
    }
    i.release(0.0);
    assert!(i.active());
    i.grab();
    assert!(!i.active());
    assert_eq!(i.step(0.016), (0, 0));
}

#[test]
fn glide_distance_does_not_depend_on_the_frame_rate() {
    let run = |dt: f32| {
        let mut i = Inertia::new();
        for _ in 0..6 {
            i.push(0, 14, 0.0166);
        }
        i.release(0.0);
        let mut total = 0i32;
        while i.active() {
            total += i.step(dt).1;
        }
        total
    };
    let (a, b) = (run(0.004), run(0.033));
    assert!((a - b).abs() * 6 < a.max(b), "{a} vs {b}");
}

#[test]
fn speeds_are_bounded_and_vertical_works() {
    let mut i = Inertia::new();
    i.push(100_000, -100_000, 0.001);
    i.release(0.0);
    let (dx, dy) = i.step(1.0 / 60.0);
    // At most 3200 px/s: about 53 px in a 60 Hz frame.
    assert!(
        (1..=54).contains(&dx) && (-54..0).contains(&dy),
        "{dx} {dy}"
    );
    i.stop();
    assert!(!i.active());
}

// ---- slideshow ----

#[test]
fn the_slideshow_waits_three_seconds_per_slide() {
    let mut s = Slideshow::new();
    assert!(!s.due(10_000));
    s.toggle(1000);
    assert!(s.running());
    assert!(!s.due(1000 + SLIDE_SECS * 250 - 1));
    assert!(s.due(1000 + SLIDE_SECS * 250));
    s.restart(5000);
    assert!(!s.due(5000 + 100));
    assert!(s.due(5000 + SLIDE_SECS * 250 + 1));
    s.toggle(9000);
    assert!(!s.running());
    assert!(!s.due(90_000));
    s.toggle(10);
    s.stop();
    assert!(!s.running());
}

// ---- rotation ----

#[test]
fn a_zero_angle_map_is_the_plain_zoom_map() {
    let (vw, vh, iw, ih) = (400, 300, 200, 100);
    for zoom in [500u32, 1000, 2000, 3000] {
        let m = RotMap::new(vw, vh, (0, 0), iw, ih, zoom, 0);
        // The viewport centre shows the image centre.
        let (u, v) = m.at(vw / 2, vh / 2);
        assert!(
            ((u >> 16) - iw as i64 / 2).abs() <= 1,
            "{zoom}: {}",
            u >> 16
        );
        assert!(
            ((v >> 16) - ih as i64 / 2).abs() <= 1,
            "{zoom}: {}",
            v >> 16
        );
        // One screen pixel is 1000/zoom image pixels.
        let (u2, _) = m.at(vw / 2 + 100, vh / 2);
        let moved = (u2 - u) as f64 / 65536.0;
        assert!(
            (moved - 100.0 * 1000.0 / zoom as f64).abs() < 0.5,
            "{zoom}: {moved}"
        );
    }
}

#[test]
fn the_map_follows_the_pan() {
    let m0 = RotMap::new(400, 300, (0, 0), 200, 100, 1000, 0);
    let m1 = RotMap::new(400, 300, (30, -20), 200, 100, 1000, 0);
    // Panning the picture right by 30 shows what was 30 pixels to the left.
    let (u0, v0) = m0.at(200, 150);
    let (u1, v1) = m1.at(230, 130);
    assert!((u0 - u1).abs() < 70_000 && (v0 - v1).abs() < 70_000);
}

#[test]
fn quarter_turns_swap_the_axes() {
    let (vw, vh, iw, ih) = (200, 200, 100, 50);
    let m = RotMap::new(vw, vh, (0, 0), iw, ih, 1000, 90);
    // Turned clockwise by 90 degrees, the image's top edge is on the right: screen pixel
    // 20 to the right of the centre samples the image 20 above its centre.
    let (u, v) = m.at(vw / 2 + 20, vh / 2);
    let (ix, iy) = (u as f64 / 65536.0, v as f64 / 65536.0);
    assert!((ix - 50.0).abs() < 0.6, "{ix}");
    assert!((iy - (25.0 - 20.0)).abs() < 0.6, "{iy}");
    // 180 degrees: the corners swap.
    let m = RotMap::new(vw, vh, (0, 0), iw, ih, 1000, 180);
    let (u, v) = m.at(vw / 2 + 10, vh / 2 + 5);
    assert!((u as f64 / 65536.0 - 40.0).abs() < 0.6);
    assert!((v as f64 / 65536.0 - 20.0).abs() < 0.6);
}

#[test]
fn walking_a_row_adds_a_constant_step() {
    let m = RotMap::new(300, 200, (5, 7), 123, 77, 1700, 37);
    let (u0, v0) = m.at(10, 40);
    let (su, sv) = m.step_x();
    for k in 0..50 {
        let (u, v) = m.at(10 + k, 40);
        assert_eq!((u, v), (u0 + su * k as i64, v0 + sv * k as i64));
    }
}

#[test]
fn pixels_outside_the_image_are_none() {
    assert_eq!(RotMap::pixel(0, 0, 10, 10), Some((0, 0)));
    assert_eq!(
        RotMap::pixel(9 << 16 | 0xFFFF, 9 << 16, 10, 10),
        Some((9, 9))
    );
    assert_eq!(RotMap::pixel(10 << 16, 0, 10, 10), None);
    assert_eq!(RotMap::pixel(-1, 5, 10, 10), None);
    assert_eq!(RotMap::pixel(5, -65536, 10, 10), None);
}

#[test]
fn rotated_bounds_contain_the_picture_and_stay_in_the_viewport() {
    let (vw, vh) = (400, 300);
    // No rotation: the picture's own rectangle (plus a pixel of slack), clipped.
    let b = rotated_bounds(vw, vh, (0, 0), 200, 100, 1000, 0);
    assert!(
        b.w >= 200 && b.w <= 204 && b.h >= 100 && b.h <= 104,
        "{b:?}"
    );
    // A quarter turn swaps the extents.
    let b = rotated_bounds(vw, vh, (0, 0), 200, 100, 1000, 90);
    assert!(
        b.w >= 100 && b.w <= 104 && b.h >= 200 && b.h <= 204,
        "{b:?}"
    );
    // At 45 degrees it is the diagonal box.
    let b = rotated_bounds(vw, vh, (0, 0), 200, 200, 1000, 45);
    assert!(b.w > 270 && b.w < 290, "{b:?}");
    // Huge and off-screen cases are clipped or empty.
    let b = rotated_bounds(vw, vh, (0, 0), 5000, 5000, 1000, 30);
    assert_eq!((b.x, b.y, b.w, b.h), (0, 0, vw, vh));
    let b = rotated_bounds(vw, vh, (900, 0), 100, 100, 1000, 0);
    assert!(b.is_empty());
    // Every pixel the map would sample inside the image is inside the bounds.
    for angle in [0, 15, 45, 90, 133, 270] {
        let (iw, ih, zoom) = (120usize, 80usize, 1500u32);
        let bounds = rotated_bounds(vw, vh, (10, -5), iw, ih, zoom, angle);
        let m = RotMap::new(vw, vh, (10, -5), iw, ih, zoom, angle);
        for y in (0..vh).step_by(7) {
            for x in (0..vw).step_by(7) {
                let (u, v) = m.at(x, y);
                if RotMap::pixel(u, v, iw, ih).is_some() {
                    assert!(
                        bounds.contains(x, y),
                        "angle {angle}: ({x},{y}) outside {bounds:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_turned_picture_stays_a_rectangle() {
    // Find, for each corner of the image, the viewport pixel that samples it: the four must form
    // a rectangle with the image's proportions, for any angle (the picture is only turned).
    let (vw, vh, iw, ih) = (600i32, 500i32, 120usize, 200usize);
    for angle in [-80, -45, -30, -10, 10, 45, 60, 89] {
        let m = RotMap::new(vw, vh, (0, 0), iw, ih, 1000, angle);
        let mut hit = [(i32::MAX, i32::MAX); 4]; // nearest to each corner (u, v)
        let mut best = [i64::MAX; 4];
        let corners = [
            (0i64, 0i64),
            (iw as i64, 0),
            (iw as i64, ih as i64),
            (0, ih as i64),
        ];
        for y in 0..vh {
            for x in 0..vw {
                let (u, v) = m.at(x, y);
                for (k, (cu, cv)) in corners.iter().enumerate() {
                    let d = ((u >> 16) - cu).pow(2) + ((v >> 16) - cv).pow(2);
                    if d < best[k] {
                        best[k] = d;
                        hit[k] = (x, y);
                    }
                }
            }
        }
        let e = |a: (i32, i32), b: (i32, i32)| {
            (((b.0 - a.0).pow(2) + (b.1 - a.1).pow(2)) as f64).sqrt()
        };
        let (top, right, bottom, left) = (
            e(hit[0], hit[1]),
            e(hit[1], hit[2]),
            e(hit[2], hit[3]),
            e(hit[3], hit[0]),
        );
        assert!(
            (top - iw as f64).abs() < 3.0 && (bottom - iw as f64).abs() < 3.0,
            "angle {angle}: {top} {bottom}"
        );
        assert!(
            (right - ih as f64).abs() < 3.0 && (left - ih as f64).abs() < 3.0,
            "angle {angle}: {right} {left}"
        );
    }
}
