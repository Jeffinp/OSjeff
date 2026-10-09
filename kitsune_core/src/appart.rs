//! Art for the app interiors: the coloured file-type icons of Arquivos (folder, text, image, app,
//! generic file, drive) and the small monochrome tool glyphs of the toolbars (arrows, view
//! switches, sort, eye, house, bin, rotate, play...).
//!
//! Everything is drawn from anti-aliased paths in 24.8 fixed point at the size asked, on a
//! 64-unit (file icons) or 16-unit (glyphs) design grid, so nothing is a scaled bitmap and the
//! same code gives the 16 px sidebar glyph and the 64 px preview. The kernel caches the
//! surfaces per (kind, size, colour). Pure and host tested.

use crate::glyph::Path;
use crate::iconart::stroke_path;
use crate::raster::{Corner, CornerMasks, Paint, Surface, rgb, rgba};
use alloc::vec::Vec;

/// What a file icon shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Folder,
    Text,
    Image,
    App,
    Generic,
    Drive,
}

/// Every file kind, in a stable order.
pub const FILE_KINDS: [FileKind; 6] = [
    FileKind::Folder,
    FileKind::Text,
    FileKind::Image,
    FileKind::App,
    FileKind::Generic,
    FileKind::Drive,
];

fn mix(a: u32, b: u32, t: u32) -> u32 {
    crate::raster::lerp(a, b, t)
}

/// A scaler from the 64-unit design grid to 24.8 pixels for a `px`-wide icon.
struct Grid {
    px: i32,
}

impl Grid {
    /// 24.8 coordinate of design unit `v` (64 units span the whole icon).
    fn u(&self, v: i32) -> i32 {
        v * self.px * 256 / 64
    }

    /// Same with a half-unit fraction (`v` in half units).
    fn h(&self, v2: i32) -> i32 {
        v2 * self.px * 256 / 128
    }
}

#[allow(clippy::too_many_arguments)]
fn rrect(s: &mut Surface, g: &Grid, x: i32, y: i32, w: i32, h: i32, r: i32, paint: Paint) {
    let mut p = Path::new();
    p.rrect(g.u(x), g.u(y), g.u(w), g.u(h), g.u(r));
    s.fill_path(&p, paint);
}

fn line(s: &mut Surface, g: &Grid, a: (i32, i32), b: (i32, i32), w2: i32, c: u32) {
    s.fill_path(
        &stroke_path(&[(g.u(a.0), g.u(a.1)), (g.u(b.0), g.u(b.1))], g.h(w2), true),
        Paint::Solid(c),
    );
}

/// A page with the top-right corner folded: the shared body of the document icons.
fn page(s: &mut Surface, g: &Grid, x: i32, y: i32, w: i32, h: i32, fold: i32) {
    let (x2, y2) = (x + w, y + h);
    let r = 5;
    let mut outline = Path::new();
    outline.move_to(g.u(x + r), g.u(y));
    outline.line_to(g.u(x2 - fold), g.u(y));
    outline.line_to(g.u(x2), g.u(y + fold));
    outline.line_to(g.u(x2), g.u(y2 - r));
    outline.quad_to(g.u(x2), g.u(y2), g.u(x2 - r), g.u(y2));
    outline.line_to(g.u(x + r), g.u(y2));
    outline.quad_to(g.u(x), g.u(y2), g.u(x), g.u(y2 - r));
    outline.line_to(g.u(x), g.u(y + r));
    outline.quad_to(g.u(x), g.u(y), g.u(x + r), g.u(y));
    outline.close();
    // A hairline of shade around the page, then the paper on top.
    s.fill_path(&outline, Paint::Solid(rgba(0x000000, 70)));
    let mut inner = Path::new();
    let (ix, iy, ix2, iy2) = (x, y, x2, y2);
    let d = g.h(1).max(64); // inset: about half a design unit, at least a quarter pixel
    let f = fold;
    inner.move_to(g.u(ix + r) + d, g.u(iy) + d);
    inner.line_to(g.u(ix2 - f) - d / 2, g.u(iy) + d);
    inner.line_to(g.u(ix2) - d, g.u(iy + f) + d / 2);
    inner.line_to(g.u(ix2) - d, g.u(iy2 - r));
    inner.quad_to(g.u(ix2) - d, g.u(iy2) - d, g.u(ix2 - r), g.u(iy2) - d);
    inner.line_to(g.u(ix + r), g.u(iy2) - d);
    inner.quad_to(g.u(ix) + d, g.u(iy2) - d, g.u(ix) + d, g.u(iy2 - r));
    inner.line_to(g.u(ix) + d, g.u(iy + r));
    inner.quad_to(g.u(ix) + d, g.u(iy) + d, g.u(ix + r), g.u(iy) + d);
    inner.close();
    // The outline above already put the shade down; paint the paper through a fresh surface so
    // the hairline stays only at the edge.
    let mut paper = Surface::new(s.w, s.h);
    paper.fill_path(&inner, Paint::Vertical(rgb(0xFFFFFF), rgb(0xEEEEF3)));
    s.blit(&paper, 0, 0, 256);
    // The folded corner.
    let mut tri = Path::new();
    tri.polygon(&[
        (g.u(x2 - fold), g.u(y)),
        (g.u(x2), g.u(y + fold)),
        (g.u(x2 - fold), g.u(y + fold)),
    ]);
    s.fill_path(&tri, Paint::Solid(rgb(0xD9D9E2)));
}

/// Render the icon of `kind` at `px` x `px`. `accent` (`0xRRGGBB`) tints folders, apps and
/// the drive. `masks` provides the squircle corners of the app tile.
pub fn file_icon(kind: FileKind, px: usize, accent: u32, masks: &CornerMasks) -> Surface {
    let px = px.clamp(8, 256);
    let mut s = Surface::new(px, px);
    let g = Grid { px: px as i32 };
    let acc = 0xFF00_0000 | (accent & 0xFF_FFFF);
    match kind {
        FileKind::Folder => {
            let dark = mix(acc, 0xFF00_0000, 70);
            let light = mix(acc, 0xFFFF_FFFF, 90);
            // Back plate with its tab.
            rrect(&mut s, &g, 5, 10, 54, 44, 6, Paint::Solid(dark));
            rrect(&mut s, &g, 5, 8, 24, 12, 4, Paint::Solid(dark));
            // The paper peeking out.
            rrect(&mut s, &g, 10, 16, 44, 20, 3, Paint::Solid(rgb(0xF4F4F8)));
            // Front plate.
            rrect(&mut s, &g, 5, 20, 54, 36, 6, Paint::Vertical(light, acc));
            line(&mut s, &g, (10, 22), (54, 22), 3, rgba(0xFFFFFF, 90));
        }
        FileKind::Text => {
            page(&mut s, &g, 12, 4, 40, 56, 12);
            let ink = rgb(0x9A9AAE);
            for (i, w) in [24, 24, 24, 15].iter().enumerate() {
                let y = 26 + i as i32 * 8;
                line(&mut s, &g, (19, y), (19 + w, y), 5, ink);
            }
            line(&mut s, &g, (19, 17), (30, 17), 5, mix(acc, 0xFFFF_FFFF, 40));
        }
        FileKind::Image => {
            rrect(
                &mut s,
                &g,
                5,
                11,
                54,
                42,
                7,
                Paint::Solid(rgba(0x000000, 60)),
            );
            rrect(&mut s, &g, 6, 12, 52, 40, 6, Paint::Solid(rgb(0xFFFFFF)));
            // The picture: sky, sun, two hills.
            let mut pic = Surface::new(px, px);
            rrect(
                &mut pic,
                &g,
                10,
                16,
                44,
                32,
                3,
                Paint::Vertical(rgb(0x8FD3FF), rgb(0x4C8DF6)),
            );
            let mut sun = Path::new();
            sun.ellipse(g.u(42), g.u(25), g.u(5), g.u(5));
            pic.fill_path(&sun, Paint::Solid(rgb(0xFFE08A)));
            let mut hills = Path::new();
            hills.polygon(&[
                (g.u(10), g.u(48)),
                (g.u(10), g.u(40)),
                (g.u(22), g.u(28)),
                (g.u(34), g.u(42)),
                (g.u(40), g.u(36)),
                (g.u(54), g.u(46)),
                (g.u(54), g.u(48)),
            ]);
            pic.fill_path(&hills, Paint::Vertical(rgb(0x4FD08B), rgb(0x1C9D63)));
            // Keep the picture inside its rounded frame.
            let mut clip = Surface::new(px, px);
            rrect(
                &mut clip,
                &g,
                10,
                16,
                44,
                32,
                3,
                Paint::Solid(rgb(0xFFFFFF)),
            );
            for (d, c) in pic.px.iter_mut().zip(&clip.px) {
                *d = crate::raster::scale_premul(*d, (*c >> 24) + (*c >> 31));
            }
            s.blit(&pic, 0, 0, 256);
        }
        FileKind::App => {
            let top = mix(acc, 0xFFFF_FFFF, 60);
            let bottom = mix(acc, 0xFF00_0000, 60);
            let side = (px * 52 / 64).max(4);
            let off = ((px - side) / 2) as i32;
            s.fill_rrect(
                off,
                off,
                side,
                side,
                side * 23 / 100,
                Corner::Squircle,
                Paint::Vertical(top, bottom),
                masks,
            );
            let chev = [(g.u(23), g.u(22)), (g.u(37), g.u(32)), (g.u(23), g.u(42))];
            s.fill_path(
                &stroke_path(&chev, g.h(10), true),
                Paint::Solid(rgb(0xFFFFFF)),
            );
            line(&mut s, &g, (40, 44), (49, 44), 8, rgba(0xFFFFFF, 220));
        }
        FileKind::Generic => {
            page(&mut s, &g, 12, 4, 40, 56, 12);
            // A neutral badge where a document would show text.
            rrect(&mut s, &g, 22, 28, 20, 20, 5, Paint::Solid(rgb(0xC9CAD8)));
            line(&mut s, &g, (27, 38), (37, 38), 4, rgb(0xFFFFFF));
        }
        FileKind::Drive => {
            rrect(
                &mut s,
                &g,
                4,
                18,
                56,
                30,
                8,
                Paint::Solid(rgba(0x000000, 70)),
            );
            rrect(
                &mut s,
                &g,
                5,
                18,
                54,
                28,
                7,
                Paint::Vertical(rgb(0xD7D8E2), rgb(0x9FA2B5)),
            );
            line(&mut s, &g, (11, 27), (53, 27), 3, rgba(0xFFFFFF, 130));
            let mut led = Path::new();
            led.ellipse(g.u(48), g.u(37), g.u(3), g.u(3));
            s.fill_path(&led, Paint::Solid(mix(acc, 0xFFFF_FFFF, 80)));
            line(&mut s, &g, (11, 37), (28, 37), 4, rgba(0x000000, 70));
        }
    }
    s
}

// ------------------------------------------------------------------ tool glyphs

/// Small monochrome glyphs for the toolbars of the apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    ChevronDown,
    ViewList,
    ViewGrid,
    Sort,
    Eye,
    Home,
    Folder,
    Photo,
    Apps,
    Trash,
    Disk,
    Search,
    Cancel,
    Plus,
    Minus,
    RotateLeft,
    RotateRight,
    Fit,
    Actual,
    Info,
    Play,
    Pause,
    Save,
    FlipH,
    Replace,
    Doc,
    Check,
    ArrowUp,
    ArrowDown,
    Warning,
}

/// Every tool glyph, in a stable order.
pub const TOOLS: [Tool; 33] = [
    Tool::ChevronLeft,
    Tool::ChevronRight,
    Tool::ChevronUp,
    Tool::ChevronDown,
    Tool::ViewList,
    Tool::ViewGrid,
    Tool::Sort,
    Tool::Eye,
    Tool::Home,
    Tool::Folder,
    Tool::Photo,
    Tool::Apps,
    Tool::Trash,
    Tool::Disk,
    Tool::Search,
    Tool::Cancel,
    Tool::Plus,
    Tool::Minus,
    Tool::RotateLeft,
    Tool::RotateRight,
    Tool::Fit,
    Tool::Actual,
    Tool::Info,
    Tool::Play,
    Tool::Pause,
    Tool::Save,
    Tool::FlipH,
    Tool::Replace,
    Tool::Doc,
    Tool::Check,
    Tool::ArrowUp,
    Tool::ArrowDown,
    Tool::Warning,
];

/// Draw `t` at `px` x `px` in straight ARGB colour `c`. Design grid: 16 units.
pub fn tool_glyph(t: Tool, px: usize, c: u32) -> Surface {
    let px = px.clamp(8, 128);
    let mut s = Surface::new(px, px);
    let u = |v: i32| v * px as i32 * 256 / 16;
    // Half-unit helper for finer details.
    let hu = |v2: i32| v2 * px as i32 * 256 / 32;
    let pt = |x: i32, y: i32| (u(x), u(y));
    let width = |w2: i32| (w2 * px as i32 * 256 / 32).max(256 * 3 / 4);
    let stroke = |s: &mut Surface, pts: &[(i32, i32)], w2: i32| {
        let q8: Vec<(i32, i32)> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        s.fill_path(&stroke_path(&q8, width(w2), true), Paint::Solid(c));
    };
    let fill_rr = |s: &mut Surface, x: i32, y: i32, w: i32, h: i32, r: i32| {
        let mut p = Path::new();
        p.rrect(u(x), u(y), u(w), u(h), u(r).max(1));
        s.fill_path(&p, Paint::Solid(c));
    };
    let ring = |s: &mut Surface, cx: i32, cy: i32, r: i32, t2: i32| {
        let mut p = Path::new();
        p.ellipse(hu(cx), hu(cy), hu(r), hu(r));
        p.ellipse_hole(hu(cx), hu(cy), hu(r - t2).max(1), hu(r - t2).max(1));
        s.fill_path(&p, Paint::Solid(c));
    };
    match t {
        Tool::ChevronLeft => stroke(&mut s, &[(10, 3), (5, 8), (10, 13)], 4),
        Tool::ChevronRight => stroke(&mut s, &[(6, 3), (11, 8), (6, 13)], 4),
        Tool::ChevronUp => stroke(&mut s, &[(3, 10), (8, 5), (13, 10)], 4),
        Tool::ChevronDown => stroke(&mut s, &[(3, 6), (8, 11), (13, 6)], 4),
        Tool::ViewList => {
            for y in [4, 8, 12] {
                fill_rr(&mut s, 2, y - 1, 2, 2, 1);
                stroke(&mut s, &[(6, y), (14, y)], 3);
            }
        }
        Tool::ViewGrid => {
            for (x, y) in [(2, 2), (9, 2), (2, 9), (9, 9)] {
                fill_rr(&mut s, x, y, 5, 5, 1);
            }
        }
        Tool::Sort => {
            stroke(&mut s, &[(2, 4), (14, 4)], 3);
            stroke(&mut s, &[(2, 8), (11, 8)], 3);
            stroke(&mut s, &[(2, 12), (8, 12)], 3);
        }
        Tool::Eye => {
            let mut p = Path::new();
            p.ellipse(u(8), u(8), u(7), u(4) + hu(1));
            p.ellipse_hole(u(8), u(8), u(7) - hu(3), u(4) - hu(2));
            s.fill_path(&p, Paint::Solid(c));
            let mut d = Path::new();
            d.ellipse(u(8), u(8), hu(4), hu(4));
            s.fill_path(&d, Paint::Solid(c));
        }
        Tool::Home => {
            stroke(&mut s, &[(2, 8), (8, 2), (14, 8)], 3);
            let mut body = Path::new();
            body.polygon(&[
                (u(4), u(8)),
                (u(8), u(4) + hu(1)),
                (u(12), u(8)),
                (u(12), u(14)),
                (u(4), u(14)),
            ]);
            // The door is a hole in the same path.
            body.rrect_hole(u(7), u(10), u(2), u(4), 0);
            s.fill_path(&body, Paint::Solid(c));
        }
        Tool::Folder => {
            fill_rr(&mut s, 1, 3, 6, 4, 1);
            fill_rr(&mut s, 1, 5, 14, 9, 2);
        }
        Tool::Photo => {
            let mut p = Path::new();
            p.rrect(u(1), u(3), u(14), u(10), u(2));
            p.rrect_hole(
                u(1) + hu(3),
                u(3) + hu(3),
                u(14) - hu(6),
                u(10) - hu(6),
                u(1),
            );
            s.fill_path(&p, Paint::Solid(c));
            let mut hills = Path::new();
            hills.polygon(&[
                (u(3), u(11)),
                (u(6), u(7)),
                (u(8), u(9) + hu(1)),
                (u(10), u(7)),
                (u(13), u(11)),
            ]);
            s.fill_path(&hills, Paint::Solid(c));
            let mut sun = Path::new();
            sun.ellipse(u(11), u(6), hu(2), hu(2));
            s.fill_path(&sun, Paint::Solid(c));
        }
        Tool::Apps => {
            for (x, y) in [(2, 2), (9, 2), (2, 9), (9, 9)] {
                fill_rr(&mut s, x, y, 5, 5, 2);
            }
        }
        Tool::Trash => {
            fill_rr(&mut s, 2, 3, 12, 2, 1);
            fill_rr(&mut s, 6, 1, 4, 2, 1);
            let mut bin = Path::new();
            bin.polygon(&[(u(3), u(6)), (u(13), u(6)), (u(12), u(15)), (u(4), u(15))]);
            s.fill_path(&bin, Paint::Solid(c));
        }
        Tool::Disk => {
            let mut p = Path::new();
            p.rrect(u(1), u(4), u(14), u(8), u(2));
            p.ellipse_hole(u(12), u(8), hu(1) + 64, hu(1) + 64);
            p.rrect_hole(u(3), u(7), u(5), hu(3), 0);
            s.fill_path(&p, Paint::Solid(c));
        }
        Tool::Search => {
            ring(&mut s, 14, 14, 11, 4);
            stroke(&mut s, &[(11, 11), (14, 14)], 4);
        }
        Tool::Cancel => {
            // A filled disc with a cross cut out reads at 12 px.
            let mut d = Path::new();
            d.ellipse(u(8), u(8), u(7), u(7));
            s.fill_path(&d, Paint::Solid(c));
            let cross = |a: (i32, i32), b: (i32, i32)| {
                stroke_path(&[pt(a.0, a.1), pt(b.0, b.1)], width(3), true)
            };
            let mut cut = Surface::new(px, px);
            cut.fill_path(&cross((5, 5), (11, 11)), Paint::Solid(rgb(0xFFFFFF)));
            cut.fill_path(&cross((11, 5), (5, 11)), Paint::Solid(rgb(0xFFFFFF)));
            for (d, k) in s.px.iter_mut().zip(&cut.px) {
                let a = k >> 24;
                if a != 0 {
                    *d = crate::raster::scale_premul(*d, 256 - (a + (a >> 7)));
                }
            }
        }
        Tool::Plus => {
            stroke(&mut s, &[(3, 8), (13, 8)], 4);
            stroke(&mut s, &[(8, 3), (8, 13)], 4);
        }
        Tool::Minus => stroke(&mut s, &[(3, 8), (13, 8)], 4),
        Tool::RotateLeft | Tool::RotateRight => {
            // Three quarters of a ring (clockwise from 30 to 270 degrees) with an arrow head
            // at the end; the left variant is the mirror image.
            let pts: Vec<(i32, i32)> = (0..=8)
                .map(|k| crate::iconart::rot(u(8), u(8), 0, -u(5), 30 + k * 30))
                .collect();
            s.fill_path(&stroke_path(&pts, width(3), true), Paint::Solid(c));
            let e = *pts.last().unwrap_or(&(0, 0));
            let mut head = Path::new();
            head.polygon(&[
                (e.0 - u(3), e.1 + u(1)),
                (e.0 + u(3), e.1 + u(1)),
                (e.0, e.1 - u(3)),
            ]);
            s.fill_path(&head, Paint::Solid(c));
            if t == Tool::RotateLeft {
                for row in s.px.chunks_mut(px) {
                    row.reverse();
                }
            }
        }
        Tool::Fit => {
            for (a, b, d) in [
                ((2, 6), (2, 2), (6, 2)),
                ((10, 2), (14, 2), (14, 6)),
                ((14, 10), (14, 14), (10, 14)),
                ((6, 14), (2, 14), (2, 10)),
            ] {
                stroke(&mut s, &[a, b, d], 3);
            }
        }
        Tool::Actual => {
            // A "1" in a frame.
            let mut f = Path::new();
            f.rrect(u(2), u(2), u(12), u(12), u(3));
            f.rrect_hole(
                u(2) + hu(3),
                u(2) + hu(3),
                u(12) - hu(6),
                u(12) - hu(6),
                u(2),
            );
            s.fill_path(&f, Paint::Solid(c));
            stroke(&mut s, &[(6, 6), (8, 5), (8, 11)], 3);
        }
        Tool::Info => {
            ring(&mut s, 16, 16, 14, 3);
            stroke(&mut s, &[(8, 7), (8, 11)], 3);
            let mut dot = Path::new();
            dot.ellipse(u(8), u(5), hu(1) + 96, hu(1) + 96);
            s.fill_path(&dot, Paint::Solid(c));
        }
        Tool::Play => {
            let mut p = Path::new();
            p.polygon(&[(u(5), u(2)), (u(13), u(8)), (u(5), u(14))]);
            s.fill_path(&p, Paint::Solid(c));
        }
        Tool::Pause => {
            fill_rr(&mut s, 3, 2, 3, 12, 1);
            fill_rr(&mut s, 10, 2, 3, 12, 1);
        }
        Tool::Save => {
            stroke(&mut s, &[(8, 2), (8, 10)], 3);
            stroke(&mut s, &[(4, 7), (8, 11), (12, 7)], 3);
            stroke(&mut s, &[(2, 13), (14, 13)], 3);
        }
        Tool::FlipH => {
            stroke(&mut s, &[(8, 1), (8, 15)], 2);
            let mut l = Path::new();
            l.polygon(&[(u(6), u(3)), (u(1), u(13)), (u(6), u(13))]);
            s.fill_path(&l, Paint::Solid(c));
            let mut r = Path::new();
            r.polygon(&[(u(10), u(3)), (u(15), u(13)), (u(10), u(13))]);
            s.fill_path(
                &r,
                Paint::Solid(rgba(c & 0xFF_FFFF, ((c >> 24) * 90 / 255) as u8)),
            );
        }
        Tool::Replace => {
            stroke(&mut s, &[(2, 6), (13, 6)], 3);
            stroke(&mut s, &[(10, 3), (13, 6), (10, 9)], 3);
            stroke(&mut s, &[(14, 11), (3, 11)], 3);
            stroke(&mut s, &[(6, 8), (3, 11), (6, 14)], 3);
        }
        Tool::Doc => {
            let mut p = Path::new();
            p.rrect(u(3), u(1), u(10), u(14), u(2));
            p.rrect_hole(
                u(3) + hu(3),
                u(1) + hu(3),
                u(10) - hu(6),
                u(14) - hu(6),
                u(1),
            );
            s.fill_path(&p, Paint::Solid(c));
            stroke(&mut s, &[(6, 6), (10, 6)], 2);
            stroke(&mut s, &[(6, 9), (10, 9)], 2);
        }
        Tool::Check => stroke(&mut s, &[(3, 8), (6, 11), (13, 4)], 4),
        Tool::ArrowUp => {
            stroke(&mut s, &[(8, 13), (8, 3)], 3);
            stroke(&mut s, &[(4, 7), (8, 3), (12, 7)], 3);
        }
        Tool::ArrowDown => {
            stroke(&mut s, &[(8, 3), (8, 13)], 3);
            stroke(&mut s, &[(4, 9), (8, 13), (12, 9)], 3);
        }
        Tool::Warning => {
            let mut tri = Path::new();
            tri.polygon(&[(u(8), u(1) + hu(1)), (u(15), u(14)), (u(1), u(14))]);
            s.fill_path(&tri, Paint::Solid(c));
            let mut cut = Surface::new(px, px);
            cut.fill_path(
                &stroke_path(&[pt(8, 6), pt(8, 10)], width(3), true),
                Paint::Solid(rgb(0xFFFFFF)),
            );
            let mut dot = Path::new();
            dot.ellipse(u(8), u(12), hu(1) + 96, hu(1) + 96);
            cut.fill_path(&dot, Paint::Solid(rgb(0xFFFFFF)));
            for (d, k) in s.px.iter_mut().zip(&cut.px) {
                let a = k >> 24;
                if a != 0 {
                    *d = crate::raster::scale_premul(*d, 256 - (a + (a >> 7)));
                }
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masks() -> CornerMasks {
        CornerMasks::with_max_radius(64)
    }

    fn opaque(s: &Surface) -> usize {
        s.px.iter().filter(|&&p| p >> 24 > 16).count()
    }

    #[test]
    fn every_file_icon_has_content_at_every_size() {
        let m = masks();
        for k in FILE_KINDS {
            for side in [16usize, 20, 32, 48, 64, 96] {
                let s = file_icon(k, side, 0x5B5CF6, &m);
                assert_eq!((s.w, s.h), (side, side));
                let n = opaque(&s);
                assert!(n > side * side / 8, "{k:?} at {side}: {n} px");
                assert!(n < side * side, "{k:?} at {side} fills the whole box");
            }
        }
    }

    #[test]
    fn file_icons_are_distinct_and_deterministic() {
        let m = masks();
        let mut seen: Vec<Vec<u32>> = Vec::new();
        for k in FILE_KINDS {
            let a = file_icon(k, 32, 0x5B5CF6, &m);
            let b = file_icon(k, 32, 0x5B5CF6, &m);
            assert_eq!(a.px, b.px, "{k:?} is not deterministic");
            assert!(!seen.contains(&a.px), "{k:?} duplicates another icon");
            seen.push(a.px);
        }
    }

    #[test]
    fn the_accent_tints_folders_and_apps_but_not_documents() {
        let m = masks();
        for k in [FileKind::Folder, FileKind::App] {
            let a = file_icon(k, 48, 0x5B5CF6, &m);
            let b = file_icon(k, 48, 0xF59E0B, &m);
            assert_ne!(a.px, b.px, "{k:?} ignores the accent");
        }
        let a = file_icon(FileKind::Text, 48, 0x5B5CF6, &m);
        let b = file_icon(FileKind::Text, 48, 0xF59E0B, &m);
        // Only the heading line of the text icon follows the accent.
        let diff = a.px.iter().zip(&b.px).filter(|(x, y)| x != y).count();
        assert!(diff < 48 * 48 / 10, "{diff}");
    }

    #[test]
    fn corners_of_a_file_icon_stay_transparent() {
        let m = masks();
        for k in FILE_KINDS {
            let s = file_icon(k, 64, 0x5B5CF6, &m);
            assert_eq!(s.get(0, 0) >> 24, 0, "{k:?} top-left");
            assert_eq!(s.get(63, 63) >> 24, 0, "{k:?} bottom-right");
        }
    }

    #[test]
    fn every_tool_glyph_draws_in_the_colour_asked() {
        for t in TOOLS {
            for side in [12usize, 14, 16, 20, 24] {
                let s = tool_glyph(t, side, 0xFF11_80F0);
                let n = opaque(&s);
                assert!(n >= side * side / 20, "{t:?} at {side}: {n}");
                assert!(n < side * side * 9 / 10, "{t:?} at {side} is a blob: {n}");
                // Premultiplied pixels never carry more colour than alpha.
                for &p in &s.px {
                    let a = p >> 24;
                    assert!((p >> 16) & 0xFF <= a && (p >> 8) & 0xFF <= a && p & 0xFF <= a);
                }
            }
        }
    }

    #[test]
    fn tool_glyphs_are_distinct() {
        let mut seen: Vec<Vec<u32>> = Vec::new();
        for t in TOOLS {
            let s = tool_glyph(t, 16, 0xFF00_0000);
            assert!(!seen.contains(&s.px), "{t:?} duplicates another glyph");
            seen.push(s.px);
        }
    }

    #[test]
    fn mirrored_pairs_are_mirrors() {
        let l = tool_glyph(Tool::ChevronLeft, 16, 0xFF00_0000);
        let r = tool_glyph(Tool::ChevronRight, 16, 0xFF00_0000);
        // The two chevrons have the same ink (within rounding of the anti-aliasing).
        let (a, b) = (opaque(&l) as i32, opaque(&r) as i32);
        assert!((a - b).abs() <= 6, "{a} vs {b}");
        let up = tool_glyph(Tool::ChevronUp, 16, 0xFF00_0000);
        let down = tool_glyph(Tool::ChevronDown, 16, 0xFF00_0000);
        assert!((opaque(&up) as i32 - opaque(&down) as i32).abs() <= 6);
    }

    #[test]
    fn sizes_are_clamped_not_rejected() {
        let m = masks();
        assert_eq!(file_icon(FileKind::Folder, 0, 0, &m).w, 8);
        assert_eq!(file_icon(FileKind::Folder, 10_000, 0, &m).w, 256);
        assert_eq!(tool_glyph(Tool::Eye, 1, 0xFF00_0000).w, 8);
    }

    #[test]
    #[ignore = "writes a contact sheet: APPART_SHEET=/tmp/appart.ppm"]
    fn dump_sheet() {
        let m = masks();
        let Ok(path) = std::env::var("APPART_SHEET") else {
            return;
        };
        let (w, h) = (6 * 72 + 8, 100 + 6 * 40 + 8);
        let mut px = alloc::vec![0xFFF6F6F8u32; w * h];
        let paste = |px: &mut Vec<u32>, s: &Surface, x: usize, y: usize| {
            for sy in 0..s.h {
                for sx in 0..s.w {
                    let p = s.px[sy * s.w + sx];
                    let a = p >> 24;
                    let d = &mut px[(y + sy) * w + x + sx];
                    let ch = |sh: u32| {
                        let sc = (p >> sh) & 0xFF;
                        let dc = (*d >> sh) & 0xFF;
                        sc + dc * (255 - a) / 255
                    };
                    *d = 0xFF00_0000
                        | (ch(16).min(255) << 16)
                        | (ch(8).min(255) << 8)
                        | ch(0).min(255);
                }
            }
        };
        for (i, k) in FILE_KINDS.iter().enumerate() {
            paste(&mut px, &file_icon(*k, 64, 0x5B5CF6, &m), 4 + i * 72, 4);
            paste(&mut px, &file_icon(*k, 20, 0x5B5CF6, &m), 4 + i * 72, 72);
        }
        for (i, t) in TOOLS.iter().enumerate() {
            paste(
                &mut px,
                &tool_glyph(*t, 32, 0xFF1D1D1F),
                4 + (i % 6) * 72,
                100 + (i / 6) * 40,
            );
        }
        let mut out = alloc::format!("P6\n{w} {h}\n255\n").into_bytes();
        for p in px {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(path, out).ok();
    }
}
