//! Procedural app icons and UI glyphs.
//!
//! Every built-in app icon is drawn here, at 128 px on a flat rounded-square tile (22 % radius,
//! one flat colour and a single highlight facet, a 1 px bevel) with a bold glyph, from
//! anti-aliased paths: no bitmap assets. The kernel caches the 128 px sources and the scaled
//! copies it needs (taskbar, Apps, menus). Small monochrome UI glyphs (search, network,
//! chevrons, ...) are drawn at the size asked, in the colour asked.
//!
//! Pure and host tested; `cargo test -p osjeff_core -- --ignored dump_icons` with
//! `ICON_SHEET=/tmp/icons.ppm` writes a contact sheet to look at.

use crate::glyph::Path;
use crate::raster::{Corner, CornerMasks, Paint, Surface, argb, rgb, rgba};
use alloc::vec::Vec;

/// Side of the icon sources.
pub const SRC: usize = 128;
/// Corner radius of a tile of side [`SRC`]: 22 %.
pub const TILE_R: usize = 28;

/// The built-in icons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconId {
    Launchpad,
    Terminal,
    Notes,
    Files,
    Browser,
    Calculator,
    Monitor,
    Settings,
    Console,
    Photos,
    Tasks,
    Apps,
    Brand,
}

/// Every icon, in a stable order.
pub const ALL: [IconId; 13] = [
    IconId::Launchpad,
    IconId::Terminal,
    IconId::Notes,
    IconId::Files,
    IconId::Browser,
    IconId::Calculator,
    IconId::Monitor,
    IconId::Settings,
    IconId::Console,
    IconId::Photos,
    IconId::Tasks,
    IconId::Apps,
    IconId::Brand,
];

/// `sin` of `deg` degrees in Q14 (16384 = 1.0).
pub fn sin_q14(deg: i32) -> i32 {
    const T: [i16; 91] = [
        0, 286, 572, 857, 1143, 1428, 1713, 1997, 2280, 2563, 2845, 3126, 3406, 3686, 3964, 4240,
        4516, 4790, 5063, 5334, 5604, 5872, 6138, 6402, 6664, 6924, 7182, 7438, 7692, 7943, 8192,
        8438, 8682, 8923, 9162, 9397, 9630, 9860, 10087, 10311, 10531, 10749, 10963, 11174, 11381,
        11585, 11786, 11982, 12176, 12365, 12551, 12733, 12911, 13085, 13255, 13421, 13583, 13741,
        13894, 14044, 14189, 14330, 14466, 14598, 14726, 14849, 14968, 15082, 15191, 15296, 15396,
        15491, 15582, 15668, 15749, 15826, 15897, 15964, 16026, 16083, 16135, 16182, 16225, 16262,
        16294, 16322, 16344, 16362, 16374, 16382, 16384,
    ];
    let d = deg.rem_euclid(360);
    let (idx, neg) = match d {
        0..=90 => (d, false),
        91..=180 => (180 - d, false),
        181..=270 => (d - 180, true),
        _ => (360 - d, true),
    };
    let v = T[idx as usize] as i32;
    if neg { -v } else { v }
}

/// `cos` of `deg` degrees in Q14.
pub fn cos_q14(deg: i32) -> i32 {
    sin_q14(90 - deg)
}

/// Rotate the local point `(x, y)` (Q8) by `deg` degrees clockwise about `(cx, cy)` (Q8).
pub fn rot(cx: i32, cy: i32, x: i32, y: i32, deg: i32) -> (i32, i32) {
    let (s, c) = (sin_q14(deg) as i64, cos_q14(deg) as i64);
    let (x, y) = (x as i64, y as i64);
    (
        cx + ((x * c - y * s) >> 14) as i32,
        cy + ((x * s + y * c) >> 14) as i32,
    )
}

fn q(v: i32) -> i32 {
    v * 256
}

/// A stroked line (or a chain) as one clockwise path: quads plus round caps.
pub fn stroke_path(pts: &[(i32, i32)], width: i32, round: bool) -> Path {
    let mut p = Path::new();
    let half = width / 2;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
        let len = crate::gfx::isqrt((dx * dx + dy * dy) as usize) as i64;
        if len == 0 {
            continue;
        }
        let (nx, ny) = (
            (-dy * half as i64 / len) as i32,
            (dx * half as i64 / len) as i32,
        );
        p.polygon(&[
            (a.0 - nx, a.1 - ny),
            (b.0 - nx, b.1 - ny),
            (b.0 + nx, b.1 + ny),
            (a.0 + nx, a.1 + ny),
        ]);
    }
    if round {
        for &(x, y) in pts {
            p.ellipse(x, y, half, half);
        }
    }
    p
}

/// A quadratic curve sampled into points (for stroking).
pub fn quad_points(p0: (i32, i32), p1: (i32, i32), p2: (i32, i32), n: i32) -> Vec<(i32, i32)> {
    (0..=n)
        .map(|i| {
            let (a, b, c) = ((n - i) * (n - i), 2 * i * (n - i), i * i);
            let nn = n * n;
            (
                (a * p0.0 + b * p1.0 + c * p2.0 + nn / 2) / nn,
                (a * p0.1 + b * p1.1 + c * p2.1 + nn / 2) / nn,
            )
        })
        .collect()
}

/// An ellipse rotated by `deg` degrees about its own centre.
pub fn rotated_ellipse(cx: i32, cy: i32, rx: i32, ry: i32, deg: i32) -> Path {
    let k = |r: i32| (r as i64 * 5523 / 10000) as i32;
    let (kx, ky) = (k(rx), k(ry));
    let r = |x: i32, y: i32| rot(cx, cy, x, y, deg);
    let mut p = Path::new();
    let e = r(rx, 0);
    p.move_to(e.0, e.1);
    let (c1, c2, t) = (r(rx, ky), r(kx, ry), r(0, ry));
    p.cubic_to(c1, c2, t);
    let (c1, c2, t) = (r(-kx, ry), r(-rx, ky), r(-rx, 0));
    p.cubic_to(c1, c2, t);
    let (c1, c2, t) = (r(-rx, -ky), r(-kx, -ry), r(0, -ry));
    p.cubic_to(c1, c2, t);
    let (c1, c2, t) = (r(kx, -ry), r(rx, -ky), r(rx, 0));
    p.cubic_to(c1, c2, t);
    p.close();
    p
}

fn fill(s: &mut Surface, p: &Path, c: u32) {
    s.fill_path(p, Paint::Solid(c));
}

fn circle(s: &mut Surface, cx: i32, cy: i32, r: i32, paint: Paint) {
    let mut p = Path::new();
    p.ellipse(q(cx), q(cy), q(r), q(r));
    s.fill_path(&p, paint);
}

#[allow(clippy::too_many_arguments)]
fn rrect(s: &mut Surface, x: i32, y: i32, w: i32, h: i32, r: i32, paint: Paint, m: &CornerMasks) {
    s.fill_rrect(
        x,
        y,
        w as usize,
        h as usize,
        r as usize,
        Corner::Circle,
        paint,
        m,
    );
}

fn line(s: &mut Surface, a: (i32, i32), b: (i32, i32), w: i32, c: u32) {
    fill(
        s,
        &stroke_path(&[(q(a.0), q(a.1)), (q(b.0), q(b.1))], q(w), true),
        c,
    );
}

/// The tile: one flat colour (the middle of the old gradient pair `top` / `bottom`) and one
/// highlight facet, a lighter triangle across the top-left corner. Everything drawn afterwards
/// is clipped to the rounded square by [`finish`].
fn tile(s: &mut Surface, top: u32, bottom: u32, m: &CornerMasks) {
    s.fill_rrect(
        0,
        0,
        SRC,
        SRC,
        TILE_R,
        Corner::Circle,
        Paint::Solid(rgb(crate::wallpaper::lerp_rgb(top, bottom, 100))),
        m,
    );
    let mut facet = Path::new();
    facet.polygon(&[(q(0), q(0)), (q(86), q(0)), (q(0), q(86))]);
    fill(s, &facet, rgba(0xFFFFFF, 34));
}

/// Clip the finished icon to the tile's rounded square and add a crisp 1 px bevel: light along
/// the top edge, a shade along the bottom one.
fn finish(s: &mut Surface, m: &CornerMasks) {
    let mut cov = Surface::new(SRC, SRC);
    cov.fill_rrect(
        0,
        0,
        SRC,
        SRC,
        TILE_R,
        Corner::Circle,
        Paint::Solid(0xFFFF_FFFF),
        m,
    );
    for (px, c) in s.px.iter_mut().zip(&cov.px) {
        let a = c >> 24;
        if a < 255 {
            let sc = |v: u32| v * a / 255;
            *px = (sc(*px >> 24) << 24)
                | (sc((*px >> 16) & 0xFF) << 16)
                | (sc((*px >> 8) & 0xFF) << 8)
                | sc(*px & 0xFF);
        }
    }
    s.stroke_rrect(
        0,
        0,
        SRC,
        SRC,
        TILE_R,
        1,
        Corner::Circle,
        Paint::Vertical(rgba(0xFFFFFF, 80), rgba(0x000000, 70)),
        m,
    );
}

/// Render icon `id` at 128 x 128.
pub fn render(id: IconId, m: &CornerMasks) -> Surface {
    let mut s = Surface::new(SRC, SRC);
    match id {
        IconId::Launchpad => launcher(&mut s, m),
        IconId::Terminal => terminal(&mut s, m),
        IconId::Notes => editor(&mut s, m),
        IconId::Files => files(&mut s, m),
        IconId::Browser => browser(&mut s, m),
        IconId::Calculator => calculator(&mut s, m),
        IconId::Monitor => monitor(&mut s, m),
        IconId::Settings => settings(&mut s, m),
        IconId::Console => console(&mut s, m),
        IconId::Photos => viewer(&mut s, m),
        IconId::Tasks => tasks(&mut s, m),
        IconId::Apps => generic(&mut s, m),
        IconId::Brand => brand(&mut s, m),
    }
    finish(&mut s, m);
    s
}

/// The icon language: one bold white glyph on a flat saturated tile, with a single accent
/// detail where it helps.
const WHITE: u32 = 0xFFFF_FFFF;

fn ring(s: &mut Surface, cx: i32, cy: i32, r: i32, t: i32, c: u32) {
    let mut p = Path::new();
    p.ellipse(q(cx), q(cy), q(r), q(r));
    p.ellipse_hole(q(cx), q(cy), q(r - t), q(r - t));
    fill(s, &p, c);
}

fn launcher(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x7C83FF, 0x4B3FE0, m);
    for r in 0..3 {
        for c in 0..3 {
            let accent = r == 1 && c == 1;
            circle(
                s,
                36 + c * 28,
                36 + r * 28,
                if accent { 11 } else { 9 },
                Paint::Solid(if accent { rgb(0x5EEAD4) } else { WHITE }),
            );
        }
    }
}

fn terminal(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x3B4160, 0x14172A, m);
    let chev = [(q(36), q(42)), (q(62), q(64)), (q(36), q(86))];
    fill(s, &stroke_path(&chev, q(12), true), rgb(0x2DD4E8));
    line(s, (72, 88), (98, 88), 11, WHITE);
}

fn editor(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0xFFC25C, 0xFF7E3D, m);
    let mut page = Path::new();
    page.rrect(q(30), q(22), q(62), q(84), q(9));
    fill(s, &page, WHITE);
    for (i, w) in [40, 40, 26].iter().enumerate() {
        line(
            s,
            (41, 44 + i as i32 * 18),
            (41 + w, 44 + i as i32 * 18),
            6,
            rgb(0xFFB070),
        );
    }
    // A pen laid across the lower right corner of the page.
    fill(
        s,
        &stroke_path(&[(q(104), q(52)), (q(70), q(96))], q(12), true),
        rgb(0x3A2418),
    );
    fill(
        s,
        &stroke_path(&[(q(70), q(96)), (q(66), q(102))], q(5), true),
        WHITE,
    );
}

fn files(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x4CB5FF, 0x2255F0, m);
    let mut tab = Path::new();
    tab.rrect(q(22), q(32), q(42), q(26), q(8));
    fill(s, &tab, rgba(0xFFFFFF, 215));
    let mut body = Path::new();
    body.rrect(q(22), q(46), q(84), q(54), q(10));
    fill(s, &body, WHITE);
    line(s, (34, 62), (94, 62), 4, rgba(0x2255F0, 60));
}

fn browser(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x2FD5EE, 0x2F5BEA, m);
    ring(s, 64, 64, 40, 7, WHITE);
    let mut mer = Path::new();
    mer.ellipse(q(64), q(64), q(18), q(40));
    mer.ellipse_hole(q(64), q(64), q(18) - q(5), q(40) - q(5));
    fill(s, &mer, WHITE);
    line(s, (24, 64), (104, 64), 6, WHITE);
    line(s, (32, 44), (96, 44), 5, rgba(0xFFFFFF, 220));
    line(s, (32, 84), (96, 84), 5, rgba(0xFFFFFF, 220));
}

fn calculator(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x3EE0A6, 0x08966B, m);
    let ink = rgb(0x06543F);
    for (i, (x, y)) in [(22, 22), (68, 22), (22, 68), (68, 68)].iter().enumerate() {
        let mut k = Path::new();
        k.rrect(q(*x), q(*y), q(38), q(38), q(10));
        fill(s, &k, WHITE);
        let (cx, cy) = (x + 19, y + 19);
        match i {
            0 => {
                line(s, (cx - 9, cy), (cx + 9, cy), 6, ink);
                line(s, (cx, cy - 9), (cx, cy + 9), 6, ink);
            }
            1 => line(s, (cx - 9, cy), (cx + 9, cy), 6, ink),
            2 => {
                line(s, (cx - 8, cy - 8), (cx + 8, cy + 8), 6, ink);
                line(s, (cx + 8, cy - 8), (cx - 8, cy + 8), 6, ink);
            }
            _ => {
                line(s, (cx - 9, cy - 5), (cx + 9, cy - 5), 6, ink);
                line(s, (cx - 9, cy + 5), (cx + 9, cy + 5), 6, ink);
            }
        }
    }
}

fn monitor(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0xFF7A8C, 0xE0245E, m);
    rrect(s, 16, 92, 96, 4, 2, Paint::Solid(rgba(0xFFFFFF, 70)), m);
    let pulse = [(18, 68), (42, 68), (54, 36), (70, 94), (82, 68), (110, 68)];
    let q8: Vec<(i32, i32)> = pulse.iter().map(|&(x, y)| (q(x), q(y))).collect();
    fill(s, &stroke_path(&q8, q(9), true), WHITE);
}

fn settings(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0xA3B0C8, 0x4E5C78, m);
    for (i, (kx, y)) in [(44, 36), (84, 64), (56, 92)].iter().enumerate() {
        let _ = i;
        line(s, (22, *y), (106, *y), 8, rgba(0xFFFFFF, 120));
        circle(s, *kx, *y, 13, Paint::Solid(WHITE));
        circle(s, *kx, *y, 5, Paint::Solid(rgb(0x4E5C78)));
    }
}

fn console(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0xB59BFF, 0x6A2BD8, m);
    let lens = [70, 52, 76, 40];
    for (i, len) in lens.iter().enumerate() {
        let y = 30 + i as i32 * 22;
        circle(
            s,
            26,
            y,
            5,
            Paint::Solid(if i == 2 { rgb(0xFDE68A) } else { WHITE }),
        );
        line(s, (40, y), (40 + len, y), 7, rgba(0xFFFFFF, 235));
    }
}

fn viewer(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x56E389, 0x0E9F55, m);
    let mut frame = Path::new();
    frame.rrect(q(22), q(28), q(84), q(72), q(12));
    frame.rrect_hole(
        q(22) + q(7),
        q(28) + q(7),
        q(84) - q(14),
        q(72) - q(14),
        q(6),
    );
    fill(s, &frame, WHITE);
    circle(s, 78, 52, 8, Paint::Solid(WHITE));
    let mut hills = Path::new();
    hills.polygon(&[
        (q(31), q(91)),
        (q(52), q(62)),
        (q(66), q(80)),
        (q(74), q(72)),
        (q(97), q(91)),
    ]);
    fill(s, &hills, WHITE);
}

fn tasks(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x7B7EF5, 0x30297E, m);
    let bars = [(54, false), (86, true), (40, false), (70, false)];
    for (i, (h, accent)) in bars.iter().enumerate() {
        let x = 20 + i as i32 * 24;
        let mut b = Path::new();
        b.rrect(q(x), q(108 - h), q(16), q(*h), q(6));
        fill(s, &b, if *accent { rgb(0x5EEAD4) } else { WHITE });
    }
}

fn generic(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x8EA0C8, 0x44527A, m);
    for (x, y) in [(24, 24), (24, 68), (68, 68)] {
        let mut k = Path::new();
        k.rrect(q(x), q(y), q(36), q(36), q(10));
        fill(s, &k, WHITE);
    }
    let mut d = Path::new();
    d.polygon(&[
        (q(86), q(20)),
        (q(108), q(42)),
        (q(86), q(64)),
        (q(64), q(42)),
    ]);
    fill(s, &d, rgb(0x5EEAD4));
}

fn brand(s: &mut Surface, m: &CornerMasks) {
    tile(s, 0x6366F8, 0x4F46E5, m);
    let chev = [(q(46), q(34)), (q(80), q(64)), (q(46), q(94))];
    fill(s, &stroke_path(&chev, q(15), true), rgb(0xFFFFFF));
}

/// An installed app's own icon (`w x h` RGBA bytes, straight alpha) centred on a
/// light rounded tile. Falls back to the generic apps icon for bad input.
pub fn wrap_app_icon(rgba_px: &[u8], w: usize, h: usize, m: &CornerMasks) -> Surface {
    if w == 0 || h == 0 || rgba_px.len() < w * h * 4 {
        return render(IconId::Apps, m);
    }
    let mut s = Surface::new(SRC, SRC);
    tile(&mut s, 0xF4F4F8, 0xE6E6EC, m);
    let mut icon = Surface::new(w, h);
    for i in 0..w * h {
        let p = &rgba_px[i * 4..i * 4 + 4];
        icon.px[i] = crate::raster::premul(argb(p[3], p[0], p[1], p[2]));
    }
    let side = SRC * 66 / 100;
    let scaled = icon.resized(side, side);
    s.blit(
        &scaled,
        ((SRC - side) / 2) as i32,
        ((SRC - side) / 2) as i32,
        256,
    );
    finish(&mut s, m);
    s
}

// ------------------------------------------------------------------ UI glyphs

/// Monochrome glyphs for the menu bar, menus and controls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Search,
    Network,
    NetworkOff,
    Control,
    Check,
    ChevronRight,
    ChevronDown,
    Close,
    Plus,
    Minus,
    Brand,
    Sun,
    Moon,
    Info,
    ChevronUp,
    Trash,
    Save,
    Copy,
    Keyboard,
    Clock,
    Disk,
    Power,
    Image,
    Dock,
    Bell,
    Wave,
    ChevronLeft,
    Reload,
    Lock,
    Warning,
    Star,
    StarFill,
    Globe,
    Home,
}

/// Draw `g` at `px` x `px` in straight ARGB colour `c`.
pub fn glyph(g: Glyph, px: usize, c: u32) -> Surface {
    let mut s = Surface::new(px, px);
    // Work in a 16 x 16 design grid scaled to the size asked.
    let u = |v: i32| (v * px as i32 * 256) / 16;
    let w = |v: i32| (v * px as i32 * 256) / 16; // stroke widths scale the same way
    let pt = |x: i32, y: i32| (u(x), u(y));
    let stroke = |s: &mut Surface, pts: &[(i32, i32)], width: i32| {
        let q8: Vec<(i32, i32)> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        s.fill_path(
            &stroke_path(&q8, w(width).max(256 * 3 / 4), true),
            Paint::Solid(c),
        );
    };
    match g {
        Glyph::Search => {
            let mut ring = Path::new();
            ring.ellipse(u(7), u(7), u(4) + u(1) / 4, u(4) + u(1) / 4);
            ring.ellipse_hole(u(7), u(7), u(3) - u(1) / 4, u(3) - u(1) / 4);
            s.fill_path(&ring, Paint::Solid(c));
            stroke(&mut s, &[(10, 10), (14, 14)], 2);
        }
        Glyph::Network | Glyph::NetworkOff => {
            // Three arcs and a dot (wi-fi), or a dashed variant.
            let col = if g == Glyph::NetworkOff {
                rgba(c & 0xFFFFFF, ((c >> 24) * 90 / 255) as u8)
            } else {
                c
            };
            let arc = |s: &mut Surface, r: i32| {
                let pts = quad_points(
                    pt(8 - r, 8 - r / 3 + 1),
                    pt(8, 8 - r * 2 + 1),
                    pt(8 + r, 8 - r / 3 + 1),
                    10,
                );
                s.fill_path(&stroke_path(&pts, w(1) + 64, true), Paint::Solid(col));
            };
            arc(&mut s, 6);
            arc(&mut s, 4);
            arc(&mut s, 2);
            let mut dot = Path::new();
            dot.ellipse(u(8), u(12), u(1) + 32, u(1) + 32);
            s.fill_path(&dot, Paint::Solid(col));
        }
        Glyph::Control => {
            // Two toggles: pill outlines with knobs.
            for (y, kx) in [(5, 11), (11, 5)] {
                let mut p = Path::new();
                p.ellipse(u(8), u(y), u(6) + 32, u(2) + 64);
                p.ellipse_hole(u(8), u(y), u(6) - 80, u(2) - 80);
                s.fill_path(&p, Paint::Solid(c));
                let mut k = Path::new();
                k.ellipse(u(kx), u(y), u(1) + 128, u(1) + 128);
                s.fill_path(&k, Paint::Solid(c));
            }
        }
        Glyph::Check => stroke(&mut s, &[(3, 8), (6, 11), (13, 4)], 2),
        Glyph::ChevronRight => stroke(&mut s, &[(6, 3), (11, 8), (6, 13)], 2),
        Glyph::ChevronDown => stroke(&mut s, &[(3, 6), (8, 11), (13, 6)], 2),
        Glyph::ChevronUp => stroke(&mut s, &[(3, 10), (8, 5), (13, 10)], 2),
        Glyph::Trash => {
            stroke(&mut s, &[(2, 4), (14, 4)], 2);
            stroke(&mut s, &[(6, 4), (6, 2), (10, 2), (10, 4)], 1);
            stroke(&mut s, &[(4, 6), (5, 14), (11, 14), (12, 6)], 2);
        }
        Glyph::Save => {
            stroke(&mut s, &[(8, 2), (8, 10)], 2);
            stroke(&mut s, &[(4, 7), (8, 11), (12, 7)], 2);
            stroke(&mut s, &[(3, 11), (3, 14), (13, 14), (13, 11)], 2);
        }
        Glyph::Keyboard => {
            stroke(&mut s, &[(2, 4), (14, 4), (14, 12), (2, 12), (2, 4)], 1);
            for x in [5, 8, 11] {
                let mut d = Path::new();
                d.ellipse(u(x), u(7), u(1) - 32, u(1) - 32);
                s.fill_path(&d, Paint::Solid(c));
            }
            stroke(&mut s, &[(5, 10), (11, 10)], 1);
        }
        Glyph::Clock => {
            let mut p = Path::new();
            p.ellipse(u(8), u(8), u(7), u(7));
            p.ellipse_hole(u(8), u(8), u(7) - 96, u(7) - 96);
            s.fill_path(&p, Paint::Solid(c));
            stroke(&mut s, &[(8, 4), (8, 8), (11, 10)], 1);
        }
        Glyph::Disk => {
            stroke(&mut s, &[(2, 5), (14, 5), (14, 11), (2, 11), (2, 5)], 1);
            stroke(&mut s, &[(4, 8), (7, 8)], 1);
            let mut d = Path::new();
            d.ellipse(u(11), u(8), u(1) - 32, u(1) - 32);
            s.fill_path(&d, Paint::Solid(c));
        }
        Glyph::Power => {
            // An open ring (the gap at the top) and the bar through it.
            let pts: Vec<(i32, i32)> = (0..=9)
                .map(|k| rot(u(8), u(9), 0, -u(5), 40 + k * 31))
                .collect();
            s.fill_path(&stroke_path(&pts, w(1) + 96, true), Paint::Solid(c));
            stroke(&mut s, &[(8, 2), (8, 8)], 2);
        }
        Glyph::Image => {
            stroke(&mut s, &[(2, 3), (14, 3), (14, 13), (2, 13), (2, 3)], 1);
            stroke(&mut s, &[(2, 12), (6, 8), (9, 11), (11, 9), (14, 12)], 1);
            let mut d = Path::new();
            d.ellipse(u(11), u(6), u(1) - 32, u(1) - 32);
            s.fill_path(&d, Paint::Solid(c));
        }
        Glyph::Dock => {
            stroke(&mut s, &[(1, 9), (15, 9), (15, 14), (1, 14), (1, 9)], 1);
            for x in [4, 8, 12] {
                let mut d = Path::new();
                d.ellipse(u(x), u(11) + 128, u(1) - 48, u(1) - 48);
                s.fill_path(&d, Paint::Solid(c));
            }
            stroke(&mut s, &[(4, 4), (12, 4)], 1);
        }
        Glyph::Copy => {
            stroke(&mut s, &[(3, 6), (10, 6), (10, 13), (3, 13), (3, 6)], 2);
            stroke(&mut s, &[(6, 3), (13, 3), (13, 10), (11, 10)], 2);
        }
        Glyph::Close => {
            stroke(&mut s, &[(4, 4), (12, 12)], 2);
            stroke(&mut s, &[(12, 4), (4, 12)], 2);
        }
        Glyph::Plus => {
            stroke(&mut s, &[(3, 8), (13, 8)], 2);
            stroke(&mut s, &[(8, 3), (8, 13)], 2);
        }
        Glyph::Minus => stroke(&mut s, &[(3, 8), (13, 8)], 2),
        Glyph::Brand => {
            let pts = [pt(5, 2), pt(11, 8), pt(5, 14)];
            s.fill_path(&stroke_path(&pts, w(2) + 96, true), Paint::Solid(c));
        }
        Glyph::Sun => {
            let mut d = Path::new();
            d.ellipse(u(8), u(8), u(3) + 64, u(3) + 64);
            s.fill_path(&d, Paint::Solid(c));
            for k in 0..8 {
                let a = rot(u(8), u(8), 0, -u(5) - 64, k * 45);
                let b = rot(u(8), u(8), 0, -u(7) - 32, k * 45);
                s.fill_path(&stroke_path(&[a, b], w(1) + 64, true), Paint::Solid(c));
            }
        }
        Glyph::Moon => {
            let mut p = Path::new();
            p.ellipse(u(8), u(8), u(5) + 128, u(5) + 128);
            p.ellipse_hole(u(11), u(6), u(4) + 128, u(4) + 128);
            s.fill_path(&p, Paint::Solid(c));
        }
        Glyph::Bell => {
            let mut pts = quad_points(pt(4, 11), pt(4, 3), pt(8, 3), 8);
            pts.extend(quad_points(pt(8, 3), pt(12, 3), pt(12, 11), 8));
            pts.push(pt(14, 12));
            pts.push(pt(2, 12));
            pts.push(pt(4, 11));
            s.fill_path(&stroke_path(&pts, w(1) + 96, true), Paint::Solid(c));
            let mut clap = Path::new();
            clap.ellipse(u(8), u(14) + 64, u(1) + 64, u(1));
            s.fill_path(&clap, Paint::Solid(c));
        }
        Glyph::Wave => {
            let mut pts = quad_points(pt(1, 8), pt(4, 2), pt(8, 8), 8);
            pts.extend(quad_points(pt(8, 8), pt(12, 14), pt(15, 8), 8));
            s.fill_path(&stroke_path(&pts, w(2) - 32, true), Paint::Solid(c));
        }
        Glyph::Info => {
            let mut p = Path::new();
            p.ellipse(u(8), u(8), u(7), u(7));
            p.ellipse_hole(u(8), u(8), u(7) - 80, u(7) - 80);
            s.fill_path(&p, Paint::Solid(c));
            stroke(&mut s, &[(8, 7), (8, 11)], 2);
            let mut dot = Path::new();
            dot.ellipse(u(8), u(5), u(1) - 32, u(1) - 32);
            s.fill_path(&dot, Paint::Solid(c));
        }
        Glyph::ChevronLeft => stroke(&mut s, &[(10, 3), (5, 8), (10, 13)], 2),
        Glyph::Reload => {
            // An open ring (from 40 to 330 degrees, clockwise from the top) with an arrow
            // head at its end.
            let centre = (u(8), u(8));
            let radius = u(5) + 32;
            let at = |deg: i32| rot(centre.0, centre.1, 0, -radius, deg);
            let pts: Vec<(i32, i32)> = (0..=15).map(|k| at(40 + k * 20)).collect();
            s.fill_path(&stroke_path(&pts, w(2) - 16, true), Paint::Solid(c));
            // The tip leads along the direction of travel at the end of the arc.
            let end = at(330);
            let t = rot(0, 0, 0, -256, 330 + 90);
            let n = rot(0, 0, 0, -256, 330);
            let k = u(3) + 64;
            let tip = (end.0 + t.0 * k / 256 * 3 / 2, end.1 + t.1 * k / 256 * 3 / 2);
            let a = (end.0 + n.0 * k / 256, end.1 + n.1 * k / 256);
            let b = (end.0 - n.0 * k / 256, end.1 - n.1 * k / 256);
            let mut head = Path::new();
            head.polygon(&[tip, a, b]);
            s.fill_path(&head, Paint::Solid(c));
        }
        Glyph::Lock => {
            let mut body = Path::new();
            body.rrect(u(3), u(7), u(10), u(7) + 128, u(2));
            s.fill_path(&body, Paint::Solid(c));
            let sh = [
                pt(5, 7),
                pt(5, 5),
                (u(6) - 40, u(3) + 80),
                pt(8, 2),
                (u(10) + 40, u(3) + 80),
                pt(11, 5),
                pt(11, 7),
            ];
            s.fill_path(&stroke_path(&sh, w(2) - 32, true), Paint::Solid(c));
        }
        Glyph::Warning => {
            let tri = [pt(8, 2), pt(14, 13), pt(2, 13), pt(8, 2)];
            s.fill_path(&stroke_path(&tri, w(2) - 24, true), Paint::Solid(c));
            stroke(&mut s, &[(8, 6), (8, 9)], 2);
            let mut dot = Path::new();
            dot.ellipse(u(8), u(11) + 40, u(1) - 24, u(1) - 24);
            s.fill_path(&dot, Paint::Solid(c));
        }
        Glyph::Star | Glyph::StarFill => {
            let centre = (u(8), u(8) + 40);
            let outer = u(6) + 128;
            let inner = u(3);
            let pts: Vec<(i32, i32)> = (0..10)
                .map(|k| {
                    let r = if k % 2 == 0 { outer } else { inner };
                    rot(centre.0, centre.1, 0, -r, k * 36)
                })
                .collect();
            if g == Glyph::StarFill {
                let mut p = Path::new();
                p.polygon(&pts);
                s.fill_path(&p, Paint::Solid(c));
            } else {
                let mut closed = pts.clone();
                closed.push(pts[0]);
                s.fill_path(&stroke_path(&closed, w(1) + 96, true), Paint::Solid(c));
            }
        }
        Glyph::Globe => {
            let mut ring = Path::new();
            ring.ellipse(u(8), u(8), u(7), u(7));
            ring.ellipse_hole(u(8), u(8), u(7) - 72, u(7) - 72);
            s.fill_path(&ring, Paint::Solid(c));
            let mut mer = Path::new();
            mer.ellipse(u(8), u(8), u(3) + 40, u(7) - 20);
            mer.ellipse_hole(u(8), u(8), u(3) - 32, u(7) - 90);
            s.fill_path(&mer, Paint::Solid(c));
            stroke(&mut s, &[(1, 8), (15, 8)], 1);
        }
        Glyph::Home => {
            stroke(&mut s, &[(2, 8), (8, 2), (14, 8)], 2);
            let body = [pt(4, 7), pt(4, 14), pt(12, 14), pt(12, 7)];
            s.fill_path(&stroke_path(&body, w(2) - 32, true), Paint::Solid(c));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masks() -> CornerMasks {
        CornerMasks::with_max_radius(48)
    }

    #[test]
    fn sine_table_is_accurate_and_wraps() {
        assert_eq!(sin_q14(0), 0);
        assert_eq!(sin_q14(90), 16384);
        assert_eq!(sin_q14(30), 8192);
        assert_eq!(sin_q14(180), 0);
        assert_eq!(sin_q14(270), -16384);
        assert_eq!(sin_q14(-90), -16384);
        assert_eq!(sin_q14(450), 16384);
        assert_eq!(cos_q14(0), 16384);
        assert_eq!(cos_q14(60), 8192);
        for d in 0..360 {
            let (s, c) = (sin_q14(d) as i64, cos_q14(d) as i64);
            let n = s * s + c * c;
            assert!((n - 16384 * 16384).abs() < 16384 * 8, "deg {d}");
        }
    }

    #[test]
    fn every_glyph_draws_ink_inside_its_box_at_every_size() {
        let all = [
            Glyph::Search,
            Glyph::Network,
            Glyph::NetworkOff,
            Glyph::Control,
            Glyph::Check,
            Glyph::ChevronRight,
            Glyph::ChevronDown,
            Glyph::Close,
            Glyph::Plus,
            Glyph::Minus,
            Glyph::Brand,
            Glyph::Sun,
            Glyph::Moon,
            Glyph::Info,
            Glyph::Power,
            Glyph::Bell,
            Glyph::Wave,
            Glyph::Clock,
        ];
        for g in all {
            for px in [12usize, 16, 20, 24] {
                let s = glyph(g, px, 0xFFFF_FFFF);
                assert_eq!((s.w, s.h), (px, px));
                let ink = s.px.iter().filter(|&&p| p >> 24 > 24).count();
                assert!(ink >= px / 2, "{g:?} at {px}: {ink} inked pixels");
                // Not a filled box: the glyph leaves room around itself.
                assert!(ink < px * px * 3 / 4, "{g:?} at {px}: {ink}");
            }
        }
    }

    #[test]
    fn rotation_moves_points_on_a_circle() {
        assert_eq!(rot(0, 0, 0, -256, 90), (256, 0));
        let (x, y) = rot(100, 100, 0, -2560, 45);
        assert!(
            (x - 100 - 1810).abs() < 4 && (y - 100 + 1810).abs() < 4,
            "{x} {y}"
        );
    }

    #[test]
    fn every_icon_is_a_squircle_with_content() {
        let m = masks();
        for id in ALL {
            let s = render(id, &m);
            assert_eq!((s.w, s.h), (SRC, SRC));
            // Transparent outside the rounded corner, opaque in the middle area.
            assert!(s.get(0, 0) >> 24 < 30, "{id:?} corner");
            assert!(s.get(127, 127) >> 24 < 30, "{id:?} corner br");
            let opaque = s.px.iter().filter(|&&p| p >> 24 == 255).count();
            assert!(opaque > SRC * SRC * 7 / 10, "{id:?} opaque {opaque}");
            // Not a flat colour: it has a glyph.
            let first = s.get(64, 20);
            let distinct = s.px.iter().filter(|&&p| p != first).count();
            assert!(distinct > 2000, "{id:?}");
        }
    }

    #[test]
    fn icons_are_deterministic_and_distinct() {
        let m = masks();
        let a = render(IconId::Files, &m);
        let b = render(IconId::Files, &m);
        assert_eq!(a.px, b.px);
        let mut seen: Vec<Vec<u32>> = Vec::new();
        for id in ALL {
            let s = render(id, &m).resized(16, 16);
            assert!(!seen.contains(&s.px), "{id:?} duplicates another icon");
            seen.push(s.px);
        }
    }

    #[test]
    fn downscaled_icons_keep_their_shape() {
        let m = masks();
        for id in ALL {
            for side in [24usize, 48, 72] {
                let s = render(id, &m).resized(side, side);
                assert!(s.get(0, 0) >> 24 < 40);
                assert!(s.get(side / 2, side / 2) >> 24 > 200);
            }
        }
    }

    #[test]
    fn app_icons_wrap_on_a_tile() {
        let m = masks();
        let mut px = alloc::vec![0u8; 24 * 24 * 4];
        for i in 0..24 * 24 {
            px[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        let s = wrap_app_icon(&px, 24, 24, &m);
        // Red in the middle, light tile around it, rounded corner.
        let c = s.get(64, 64);
        assert!(((c >> 16) & 0xFF) > 200 && (c & 0xFF) < 60);
        let edge = s.get(12, 64);
        assert!((edge & 0xFF) > 200);
        assert!(s.get(0, 0) >> 24 < 30);
        // Garbage in: the generic icon out.
        assert_eq!(
            wrap_app_icon(&[1, 2, 3], 24, 24, &m).px,
            render(IconId::Apps, &m).px
        );
    }

    #[test]
    fn glyphs_draw_ink_in_the_colour_asked() {
        for g in [
            Glyph::Search,
            Glyph::Network,
            Glyph::NetworkOff,
            Glyph::Control,
            Glyph::Check,
            Glyph::ChevronRight,
            Glyph::ChevronDown,
            Glyph::Close,
            Glyph::Plus,
            Glyph::Minus,
            Glyph::Brand,
            Glyph::Sun,
            Glyph::Moon,
            Glyph::Info,
            Glyph::ChevronUp,
            Glyph::Trash,
            Glyph::Save,
            Glyph::Copy,
            Glyph::Keyboard,
            Glyph::Clock,
            Glyph::Disk,
            Glyph::Power,
            Glyph::Image,
            Glyph::Dock,
            Glyph::ChevronLeft,
            Glyph::Reload,
            Glyph::Lock,
            Glyph::Warning,
            Glyph::Star,
            Glyph::StarFill,
            Glyph::Globe,
            Glyph::Home,
        ] {
            for px in [12usize, 16, 20] {
                let s = glyph(g, px, rgb(0xFF0000));
                let ink: u32 = s.px.iter().map(|&p| p >> 24).sum();
                assert!(ink > 255 * (px as u32) / 2, "{g:?} {px}px has no ink");
                // Only red ink (premultiplied: g and b stay zero).
                assert!(s.px.iter().all(|&p| p & 0xFFFF == 0), "{g:?}");
                // It stays inside its box with a margin to spare on all sides.
                assert!(
                    s.px[..px].iter().all(|&p| p >> 24 < 200)
                        || matches!(g, Glyph::Info | Glyph::Globe),
                    "{g:?} touches the top edge"
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn dump_icons() {
        // Writes a contact sheet (PPM) of all icons and glyphs: ICON_SHEET=/tmp/x.ppm
        let Ok(path) = std::env::var("ICON_SHEET") else {
            return;
        };
        let m = masks();
        let sizes = [128usize, 64, 32];
        let (w, h) = (13 * 136 + 8, 128 + 8 + 64 + 8 + 32 + 8 + 48);
        let mut img = std::vec![0xE8u8; w * h * 3];
        let mut put = |s: &Surface, x0: usize, y0: usize, dark: bool| {
            for y in 0..s.h {
                for x in 0..s.w {
                    let p = s.px[y * s.w + x];
                    let a = p >> 24;
                    let o = ((y0 + y) * w + x0 + x) * 3;
                    if o + 2 >= img.len() {
                        continue;
                    }
                    let bg = if dark { 0x20u32 } else { 0xE8 };
                    for (k, sh) in [16, 8, 0].iter().enumerate() {
                        let c = (p >> sh) & 0xFF;
                        img[o + k] = (c + bg * (255 - a) / 255).min(255) as u8;
                    }
                }
            }
        };
        for (i, id) in ALL.iter().enumerate() {
            let src = render(*id, &m);
            let mut y = 4;
            for &sz in &sizes {
                let s = if sz == SRC {
                    src.clone()
                } else {
                    src.resized(sz, sz)
                };
                put(&s, 4 + i * 136, y, false);
                y += sz + 8;
            }
        }
        let gl = [
            Glyph::Search,
            Glyph::Network,
            Glyph::NetworkOff,
            Glyph::Control,
            Glyph::Check,
            Glyph::ChevronRight,
            Glyph::ChevronDown,
            Glyph::Close,
            Glyph::Plus,
            Glyph::Minus,
            Glyph::Brand,
            Glyph::Sun,
            Glyph::Moon,
            Glyph::Info,
            Glyph::ChevronUp,
            Glyph::Trash,
            Glyph::Save,
            Glyph::Copy,
            Glyph::Keyboard,
            Glyph::Clock,
            Glyph::Disk,
            Glyph::Power,
            Glyph::Image,
            Glyph::Dock,
            Glyph::ChevronLeft,
            Glyph::Reload,
            Glyph::Lock,
            Glyph::Warning,
            Glyph::Star,
            Glyph::StarFill,
            Glyph::Globe,
            Glyph::Home,
        ];
        for (i, g) in gl.iter().enumerate() {
            put(
                &glyph(*g, 16, rgb(0x1D1D1F)),
                4 + i * 40,
                128 + 8 + 64 + 8 + 32 + 16,
                false,
            );
            put(
                &glyph(*g, 32, rgb(0x1D1D1F)),
                4 + i * 40,
                128 + 8 + 64 + 8 + 32 + 40,
                false,
            );
        }
        let mut out = std::format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend_from_slice(&img);
        std::fs::write(path, out).unwrap();
    }
}
