//! Pointer sprites: arrow, pointing hand and text I-beam, drawn as anti-aliased
//! vector shapes with a thin dark outline and a soft shadow.
//!
//! All sprites share one box ([`W`] x [`H`]) so the compositor can restore a fixed
//! rectangle around the pointer; [`hotspot`] says which pixel of the box is the
//! pointer position. The shapes are rendered once and cached by the kernel.

use crate::glyph::Path;
use crate::raster::{Paint, Surface};
use alloc::vec::Vec;

/// Sprite box size in pixels (includes the shadow margin).
pub const W: usize = 24;
pub const H: usize = 28;

/// Which pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Arrow,
    Hand,
    IBeam,
}

/// The box pixel that is the pointer position.
pub const fn hotspot(s: Shape) -> (i32, i32) {
    match s {
        Shape::Arrow => (4, 3),
        Shape::Hand => (11, 3),
        Shape::IBeam => (12, 14),
    }
}

/// Half-pixel units to 24.8.
const fn h(v: i32) -> i32 {
    v * 128
}

fn arrow() -> Path {
    let mut p = Path::new();
    p.polygon(&[
        (h(8), h(6)),
        (h(8), h(41)),
        (h(15), h(34)),
        (h(20), h(45)),
        (h(25), h(43)),
        (h(20), h(32)),
        (h(30), h(32)),
    ]);
    p
}

fn hand() -> Path {
    let mut p = Path::new();
    // Index finger, palm, two folded fingers and the thumb.
    p.rrect(h(18), h(6), h(8), h(28), h(4));
    p.rrect(h(12), h(26), h(26), h(20), h(8));
    p.rrect(h(24), h(24), h(8), h(12), h(4));
    p.rrect(h(30), h(26), h(8), h(12), h(4));
    p.rrect(h(8), h(30), h(10), h(12), h(5));
    p
}

fn ibeam() -> Path {
    let mut p = Path::new();
    p.rect(h(22), h(10), h(4), h(36));
    p.rect(h(16), h(8), h(16), h(4));
    p.rect(h(16), h(44), h(16), h(4));
    p
}

/// 1-pixel dilation of the alpha channel (the outline ring around a shape).
fn dilate(src: &Surface) -> Surface {
    let mut out = Surface::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            let mut a = 0u32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < src.w && (ny as usize) < src.h {
                        a = a.max(src.px[ny as usize * src.w + nx as usize] >> 24);
                    }
                }
            }
            out.px[y * src.w + x] = a << 24;
        }
    }
    out
}

/// Render pointer `s` into a [`W`] x [`H`] premultiplied surface.
pub fn render(s: Shape) -> Surface {
    let path = match s {
        Shape::Arrow => arrow(),
        Shape::Hand => hand(),
        Shape::IBeam => ibeam(),
    };
    // The shape in white, then an outline ring around it, then the soft shadow.
    let mut fill = Surface::new(W, H);
    fill.fill_path(&path, Paint::Solid(0xFFFF_FFFF));
    let ring = dilate(&fill);
    let mut body = Surface::new(W, H);
    let mut dark = ring.clone();
    for px in dark.px.iter_mut() {
        // A near-black ring with the ring's own coverage.
        let a = *px >> 24;
        *px = (a << 24) | ((0x14 * a / 255) << 16) | ((0x14 * a / 255) << 8) | (0x1A * a / 255);
    }
    body.blit(&dark, 0, 0, 256);
    body.blit(&fill, 0, 0, 256);
    // Shadow: blurred alpha of the ring, offset down by 1, under everything.
    let mut shadow = Surface::new(W, H);
    for y in 0..H {
        for x in 0..W {
            let a = ring.px[y * W + x] >> 24;
            if a != 0 && y + 1 < H {
                shadow.px[(y + 1) * W + x] = (a * 100 / 255) << 24;
            }
        }
    }
    shadow.blur(1, 2);
    shadow.blit(&body, 0, 0, 256);
    shadow
}

/// All sprites in [`Shape`] order.
pub fn render_all() -> Vec<Surface> {
    [Shape::Arrow, Shape::Hand, Shape::IBeam]
        .into_iter()
        .map(render)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(s: &Surface) -> usize {
        s.px.iter().filter(|p| (**p >> 24) > 200).count()
    }

    #[test]
    fn sprites_have_content_inside_the_box() {
        for shape in [Shape::Arrow, Shape::Hand, Shape::IBeam] {
            let s = render(shape);
            assert_eq!((s.w, s.h), (W, H));
            assert!(opaque(&s) > 30, "{shape:?} is blank");
            let (hx, hy) = hotspot(shape);
            assert!((0..W as i32).contains(&hx) && (0..H as i32).contains(&hy));
        }
    }

    #[test]
    fn hotspot_touches_the_shape() {
        // Within two pixels of the hotspot there is visible ink.
        for shape in [Shape::Arrow, Shape::Hand, Shape::IBeam] {
            let s = render(shape);
            let (hx, hy) = hotspot(shape);
            let mut best = 0;
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    let (x, y) = ((hx + dx) as usize, (hy + dy) as usize);
                    best = best.max(s.get(x, y) >> 24);
                }
            }
            assert!(best > 100, "{shape:?}: nothing at the hotspot");
        }
    }

    #[test]
    fn the_box_edge_is_clear_so_nothing_is_clipped() {
        for shape in [Shape::Arrow, Shape::Hand, Shape::IBeam] {
            let s = render(shape);
            for x in 0..W {
                assert!((s.get(x, 0) >> 24) < 12 && (s.get(x, H - 1) >> 24) < 12);
            }
            for y in 0..H {
                assert!((s.get(0, y) >> 24) < 12 && (s.get(W - 1, y) >> 24) < 12);
            }
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(render(Shape::Arrow).px, render(Shape::Arrow).px);
    }
}
