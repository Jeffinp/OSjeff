//! The Kitsune brand mark: a geometric fox head, as pure polygon data.
//!
//! The mark is **vector first**. [`fox_head`] returns a dozen flat polygons on a 128 x 128
//! design grid (coordinates in quarter units, so `512` is the full width and the symmetry axis
//! is `x = 256`), each tagged with a colour [`Role`]. Everything else is derived from that data:
//!
//! * [`tile`]: the head on a rounded-square indigo tile (22 % radius), the app icon;
//! * [`mono`]: one single-colour silhouette with the eyes and the nose knocked out, for
//!   favicons, symbolic glyphs and anything that is tinted by the UI;
//! * [`halo`]: the head, smaller, in front of a half-circle crown of curved tails (nine in the
//!   source art; see [`HALO_TAILS`] for how many are drawn and why), for the boot splash and
//!   the About page;
//! * [`render`]: the anti-aliased raster at any size (16 to 128 px are the tuned ones), made by
//!   supersampling the polygons and averaging down, with per-size hints ([`hint`]) that merge
//!   facets and thicken the eyes and the nose so that 16 and 24 px stay readable;
//! * [`to_svg`]: the same polygons as hand-readable SVG (`docs/brand/` is generated with
//!   `cargo run -p kitsune_core --example brand_svg`).
//!
//! No floating point, no `unsafe`; the module is pure and host tested. The polygons are
//! painted back to front and overlap where facets meet, so no seam of the background shows
//! through between two flat facets.

use crate::glyph::Path;
use crate::iconart::{cos_q14, sin_q14};
use crate::raster::{Paint, Surface, argb};
use alloc::string::String;
use alloc::vec::Vec;

/// Side of the design grid, in grid units.
pub const GRID: i32 = 128;
/// Coordinates are in `1 / UNIT` of a grid unit.
pub const UNIT: i32 = 4;
/// Side of the design grid in coordinate units (the valid range is `0..=EXTENT`).
pub const EXTENT: i32 = GRID * UNIT;
/// Corner radius of the tile: 22 % of the side, in coordinate units.
pub const TILE_RADIUS: i32 = 28 * UNIT;
/// How many of the nine tails of the story are drawn in [`halo`].
///
/// Nine tails over a half circle are 20 degrees apart: at the 96-112 px of the splash the
/// gaps are 3-4 px wide and the crown turns into a noisy comb, and at 48 px it is a smudge.
/// Seven tails (24 degrees apart, the middle one peeking out between the ears) keep every
/// tail, every gap and the cream tips legible down to about 64 px. [`halo_n`] builds any
/// count from 1 to 9 (the nine-tail art is a test and a docs example, not a UI asset).
pub const HALO_TAILS: usize = 7;

/// What a polygon is for; a [`Scheme`] maps it to a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The rounded-square background of the app icon.
    Tile,
    /// Left half of the face.
    LightFace,
    /// Right half of the face and the two cheek facets.
    ShadeFace,
    /// Outer facet of an ear.
    EarOuter,
    /// Darker inner facet of an ear.
    EarInner,
    /// The cream V-shaped muzzle and cheeks.
    Muzzle,
    Eye,
    Nose,
    /// The single colour of the [`mono`] silhouette.
    Mark,
    /// A hole cut out of the [`mono`] silhouette (eye, nose).
    Knockout,
    /// A tail (even ones).
    Tail,
    /// A tail (odd ones, a shade darker so neighbours separate).
    TailDark,
    /// The cream tip of a tail.
    TailTip,
}

/// Straight `0xRRGGBB` colours for every role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scheme {
    pub tile: u32,
    pub light_face: u32,
    pub shade_face: u32,
    pub ear_outer: u32,
    pub ear_inner: u32,
    pub muzzle: u32,
    pub eye: u32,
    pub nose: u32,
    pub tail: u32,
    pub tail_dark: u32,
    pub tail_tip: u32,
}

impl Scheme {
    /// The brand palette (see `docs/brand/README.md`).
    pub const KITSUNE: Scheme = Scheme {
        tile: 0x2A2A5C,
        light_face: 0xFF7A33,
        shade_face: 0xC94F1C,
        ear_outer: 0xF76B2A,
        ear_inner: 0xB7371A,
        muzzle: 0xFFF3E6,
        eye: 0x1A1A3A,
        nose: 0x1A1A3A,
        tail: 0xC94F1C,
        tail_dark: 0xB7371A,
        tail_tip: 0xFFF3E6,
    };

    /// The colour of `role` (`Mark`/`Knockout` are not part of a scheme: the caller
    /// chooses them, see [`mono`]).
    pub fn color(&self, role: Role) -> u32 {
        match role {
            Role::Tile => self.tile,
            Role::LightFace => self.light_face,
            Role::ShadeFace => self.shade_face,
            Role::EarOuter => self.ear_outer,
            Role::EarInner => self.ear_inner,
            Role::Muzzle => self.muzzle,
            Role::Eye => self.eye,
            Role::Nose => self.nose,
            Role::Tail => self.tail,
            Role::TailDark => self.tail_dark,
            Role::TailTip => self.tail_tip,
            Role::Mark | Role::Knockout => 0xFFFFFF,
        }
    }
}

/// The corners of a polygon.
type Pts = Vec<(i32, i32)>;

/// One flat polygon: a role, the colour it was given and its corners (coordinate units,
/// y down, implicitly closed, clockwise unless it is a [`Role::Knockout`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Poly {
    pub role: Role,
    pub color: u32,
    pub pts: Vec<(i32, i32)>,
}

/// A drawable mark: an optional tile behind some polygons, painted in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// Colour of the rounded-square tile, if the mark has one.
    pub tile: Option<u32>,
    pub polys: Vec<Poly>,
}

impl Mark {
    /// Every polygon corner of the mark.
    pub fn points(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.polys.iter().flat_map(|p| p.pts.iter().copied())
    }
}

// ----------------------------------------------------------------------- the head

/// Mirror a point about the symmetry axis.
fn mir((x, y): (i32, i32)) -> (i32, i32) {
    (EXTENT - x, y)
}

fn mirrored(pts: &[(i32, i32)]) -> Vec<(i32, i32)> {
    // Mirroring flips the winding; reverse to stay clockwise.
    pts.iter().rev().map(|&p| mir(p)).collect()
}

const TIP: (i32, i32) = (93, 44);
const FORE_L: (i32, i32) = (206, 153);
const FORE_C: (i32, i32) = (256, 153);
const EAR_BASE: (i32, i32) = (101, 246);
const INNER_MID: (i32, i32) = (170, 185);
const CHEEK_TIP: (i32, i32) = (65, 324);
const CHEEK_IN: (i32, i32) = (137, 294);
const NOSE_L: (i32, i32) = (229, 400);
const NOSE_T: (i32, i32) = (256, 391);
const NOSE_B: (i32, i32) = (256, 434);
const CHIN: (i32, i32) = (256, 468);

/// The left eye (an almond: flat on top, pointing down towards the nose).
const EYE_L: [(i32, i32); 7] = [
    (165, 280),
    (190, 277),
    (206, 287),
    (216, 311),
    (194, 312),
    (176, 301),
    (167, 289),
];

/// The head as 12 flat polygons, back to front.
pub fn fox_head(s: &Scheme) -> Vec<Poly> {
    let mut out: Vec<Poly> = Vec::with_capacity(12);
    let mut add = |role: Role, pts: Vec<(i32, i32)>| {
        out.push(Poly {
            role,
            color: s.color(role),
            pts,
        });
    };
    // Ears: the outer facet reaches under the face, so the edge they share has no seam.
    let ear_outer = [TIP, FORE_L, (200, 190), (125, 255), EAR_BASE];
    add(Role::EarOuter, ear_outer.to_vec());
    add(Role::EarOuter, mirrored(&ear_outer));
    let ear_inner = [TIP, INNER_MID, (150, 235), EAR_BASE];
    add(Role::EarInner, ear_inner.to_vec());
    add(Role::EarInner, mirrored(&ear_inner));
    // The cream V reaches up under the cheeks and the face.
    let half = [
        CHEEK_TIP,
        (CHEEK_IN.0, CHEEK_IN.1 - 16),
        (NOSE_L.0, NOSE_L.1 - 15),
    ];
    let mut muzzle: Vec<(i32, i32)> = half.to_vec();
    muzzle.push((256, 410));
    muzzle.extend(half.iter().rev().map(|&p| mir(p)));
    muzzle.push(CHIN);
    add(Role::Muzzle, muzzle);
    // Cheek facets (rust), reaching under the face.
    let cheek = [EAR_BASE, (137, 262), CHEEK_IN, CHEEK_TIP];
    add(Role::ShadeFace, cheek.to_vec());
    add(Role::ShadeFace, mirrored(&cheek));
    // The face, split down the middle (x = 256 is a pixel edge at every size).
    let face = [FORE_L, FORE_C, (256, 420), NOSE_L, CHEEK_IN, EAR_BASE];
    add(Role::LightFace, face.to_vec());
    add(Role::ShadeFace, mirrored(&face));
    add(Role::Eye, EYE_L.to_vec());
    add(Role::Eye, mirrored(&EYE_L));
    add(Role::Nose, alloc::vec![NOSE_L, NOSE_T, mir(NOSE_L), NOSE_B]);
    out
}

/// The head on its indigo tile: the app icon.
pub fn tile(s: &Scheme) -> Mark {
    Mark {
        tile: Some(s.tile),
        polys: fox_head(s),
    }
}

/// The single-colour silhouette of the head (ears, face, chin), with the eyes and the nose
/// knocked out. `color` is straight `0xRRGGBB`.
pub fn mono(color: u32) -> Mark {
    let right = |p: (i32, i32)| mir(p);
    let outline = alloc::vec![
        TIP,
        FORE_L,
        right(FORE_L),
        right(TIP),
        right(EAR_BASE),
        right(CHEEK_TIP),
        CHIN,
        CHEEK_TIP,
        EAR_BASE,
    ];
    let mut polys = alloc::vec![Poly {
        role: Role::Mark,
        color,
        pts: outline,
    }];
    // Knockouts wind the other way (counter-clockwise).
    let mut hole = |pts: Vec<(i32, i32)>| {
        polys.push(Poly {
            role: Role::Knockout,
            color,
            pts: pts.into_iter().rev().collect(),
        });
    };
    hole(EYE_L.to_vec());
    hole(mirrored(&EYE_L));
    hole(alloc::vec![NOSE_L, NOSE_T, mir(NOSE_L), NOSE_B]);
    Mark { tile: None, polys }
}

// ----------------------------------------------------------------------- the halo

/// The spine of a tail pointing up from the pivot at the origin, in coordinate units:
/// `(x, y, half width)` from the base to the tip. The spine bends to the right (the tails
/// on the left are mirrored, so every tail curls outwards) and the tail swells in the
/// middle like a brush before it tapers to the tip.
const TAIL_SPINE: [(i32, i32, i32); 9] = [
    (0, 0, 14),
    (2, -30, 22),
    (5, -62, 27),
    (10, -94, 30),
    (17, -126, 30),
    (26, -156, 27),
    (36, -186, 21),
    (48, -212, 13),
    (62, -236, 0),
];
/// Index of the spine point where the cream tip starts.
const TAIL_TIP_FROM: usize = 6;

/// A tail's outline (left edge up, right edge down) and its cream tip.
fn tail_shapes() -> (Pts, Pts) {
    let left = |i: usize| (TAIL_SPINE[i].0 - TAIL_SPINE[i].2, TAIL_SPINE[i].1);
    let right = |i: usize| (TAIL_SPINE[i].0 + TAIL_SPINE[i].2, TAIL_SPINE[i].1);
    let last = TAIL_SPINE.len() - 1;
    let mut outline: Vec<(i32, i32)> = (0..last).map(left).collect();
    outline.push((TAIL_SPINE[last].0, TAIL_SPINE[last].1));
    outline.extend((0..last).rev().map(right));
    let mut tip: Vec<(i32, i32)> = (TAIL_TIP_FROM..last).map(left).collect();
    tip.push((TAIL_SPINE[last].0, TAIL_SPINE[last].1));
    tip.extend((TAIL_TIP_FROM..last).rev().map(right));
    (outline, tip)
}

/// Where the tails fan out from (behind the head).
const HALO_PIVOT: (i32, i32) = (256, 372);
/// Scale of the head in front of the crown, in 1/256.
const HALO_HEAD_K: i32 = 134;
/// Where the chin of the small head sits.
const HALO_CHIN_Y: i32 = 478;

fn rotate_about(pivot: (i32, i32), p: (i32, i32), deg: i32) -> (i32, i32) {
    let (s, c) = (sin_q14(deg) as i64, cos_q14(deg) as i64);
    let (x, y) = (p.0 as i64, p.1 as i64);
    (
        pivot.0 + ((x * c - y * s + (1 << 13)) >> 14) as i32,
        pivot.1 + ((x * s + y * c + (1 << 13)) >> 14) as i32,
    )
}

/// The head and a crown of [`HALO_TAILS`] tails (the mark of the boot splash and the About
/// page); no tile, a transparent background.
pub fn halo(s: &Scheme) -> Mark {
    halo_n(s, HALO_TAILS)
}

/// Like [`halo`] with `n` tails (clamped to 1..=9), spread over a half circle.
pub fn halo_n(s: &Scheme, n: usize) -> Mark {
    let n = n.clamp(1, 9);
    let mut polys: Vec<Poly> = Vec::new();
    let span = 136; // degrees between the outermost tails
    // Draw from the sides inwards so the middle tails lie over their neighbours.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| core::cmp::Reverse((2 * i as i32 - (n as i32 - 1)).abs()));
    for &i in &order {
        let deg = if n == 1 {
            0
        } else {
            -span / 2 + span * i as i32 / (n as i32 - 1)
        };
        let (role, tip_role) = if i % 2 == 0 {
            (Role::Tail, Role::TailTip)
        } else {
            (Role::TailDark, Role::TailTip)
        };
        // The tails left of the axis curl left, the others right: the crown is symmetric.
        let flip = deg < 0;
        let place = |pts: &[(i32, i32)]| -> Vec<(i32, i32)> {
            let it = pts
                .iter()
                .map(|&(x, y)| rotate_about(HALO_PIVOT, (if flip { -x } else { x }, y), deg));
            if flip {
                it.rev().collect()
            } else {
                it.collect()
            }
        };
        let (outline, tip) = tail_shapes();
        polys.push(Poly {
            role,
            color: s.color(role),
            pts: place(&outline),
        });
        polys.push(Poly {
            role: tip_role,
            color: s.color(tip_role),
            pts: place(&tip),
        });
    }
    for mut p in fox_head(s) {
        for q in p.pts.iter_mut() {
            q.0 = EXTENT / 2 + ((q.0 - EXTENT / 2) * HALO_HEAD_K + 128) / 256;
            q.1 = HALO_CHIN_Y + ((q.1 - 468) * HALO_HEAD_K - 128) / 256;
        }
        polys.push(p);
    }
    Mark { tile: None, polys }
}

// -------------------------------------------------------------------------- hints

/// Per-size tuning of a mark (see [`hint`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    /// Eyes scaled about their centres, in percent.
    pub eye_pct: i32,
    /// The nose, in percent.
    pub nose_pct: i32,
    /// Keep the dark inner facets of the ears (below 24 px they only add noise).
    pub ear_inner: bool,
    /// Supersampling factor of the rasteriser.
    pub supersample: usize,
}

/// The tuning for a mark rendered at `px` pixels square. Eyes and nose grow as the size
/// shrinks so that they keep a footprint of about two pixels; the ear facets are merged
/// below 24 px.
pub fn hint(px: usize) -> Hint {
    let (eye_pct, nose_pct, ear_inner) = match px {
        0..=16 => (250, 190, false),
        17..=24 => (165, 140, true),
        25..=32 => (135, 120, true),
        33..=48 => (112, 108, true),
        _ => (100, 100, true),
    };
    Hint {
        eye_pct,
        nose_pct,
        ear_inner,
        supersample: if px <= 32 { 8 } else { 4 },
    }
}

fn centroid(pts: &[(i32, i32)]) -> (i32, i32) {
    let n = pts.len().max(1) as i64;
    (
        (pts.iter().map(|p| p.0 as i64).sum::<i64>() / n) as i32,
        (pts.iter().map(|p| p.1 as i64).sum::<i64>() / n) as i32,
    )
}

/// Scale `pts` by `pct` percent about their centroid.
fn scaled(pts: &[(i32, i32)], pct: i32) -> Vec<(i32, i32)> {
    let (cx, cy) = centroid(pts);
    let f = |v: i32, c: i32| c + ((v - c) * pct + 50 * (v - c).signum()) / 100;
    pts.iter().map(|&(x, y)| (f(x, cx), f(y, cy))).collect()
}

/// `mark` with the tuning for `px` applied.
pub fn hinted(mark: &Mark, px: usize) -> Mark {
    let h = hint(px);
    let mut out = mark.clone();
    out.polys
        .retain(|p| h.ear_inner || p.role != Role::EarInner);
    for p in out.polys.iter_mut() {
        let pct = match p.role {
            Role::Eye => h.eye_pct,
            Role::Nose => h.nose_pct,
            // A knockout is an eye or the nose: the nose is the one with four corners.
            Role::Knockout if p.pts.len() == 4 => h.nose_pct,
            Role::Knockout => h.eye_pct,
            _ => continue,
        };
        if pct == 100 {
            continue;
        }
        // Scale the right-hand half through its mirror image so the two sides stay
        // pixel-for-pixel mirrored whatever the rounding does.
        p.pts = if centroid(&p.pts).0 > EXTENT / 2 {
            let m: Vec<_> = p.pts.iter().map(|&q| mir(q)).collect();
            scaled(&m, pct).into_iter().map(mir).collect()
        } else {
            scaled(&p.pts, pct)
        };
    }
    out
}

// ------------------------------------------------------------------------- raster

/// Add the polygon to `out` keeping its winding (a knockout winds the other way and cuts a
/// hole; `Path::polygon` would straighten it out).
fn path_of(pts: &[(i32, i32)], scale: i64, out: &mut Path) {
    // Coordinate units -> 24.8 pixels: x * scale * 256 / EXTENT.
    let conv = |v: i32| ((v as i64 * scale * 256 + (EXTENT as i64) / 2) / EXTENT as i64) as i32;
    for (i, &(x, y)) in pts.iter().enumerate() {
        if i == 0 {
            out.move_to(conv(x), conv(y));
        } else {
            out.line_to(conv(x), conv(y));
        }
    }
    out.close();
}

/// Fill the rounded-square tile path.
fn tile_path(side: usize) -> Path {
    let mut p = Path::new();
    let s = (side as i32) * 256;
    let r = (TILE_RADIUS as i64 * side as i64 * 256 / EXTENT as i64) as i32;
    p.rrect(0, 0, s, s, r);
    p
}

/// The mark at `px` x `px`, anti-aliased (straight colours, premultiplied surface), with the
/// per-size [`hint`] applied.
pub fn render(mark: &Mark, px: usize) -> Surface {
    let px = px.clamp(8, 512);
    let h = hint(px);
    let m = hinted(mark, px);
    let ss = h.supersample;
    let side = px * ss;
    let mut big = Surface::new(side, side);
    if let Some(c) = m.tile {
        big.fill_path(
            &tile_path(side),
            Paint::Solid(argb(255, (c >> 16) as u8, (c >> 8) as u8, c as u8)),
        );
    }
    // A mono mark is one path (outline + holes) so the holes really cut.
    let mono_color = m
        .polys
        .iter()
        .find(|p| p.role == Role::Mark)
        .map(|p| p.color);
    if let Some(c) = mono_color {
        let mut path = Path::new();
        for p in m
            .polys
            .iter()
            .filter(|p| matches!(p.role, Role::Mark | Role::Knockout))
        {
            path_of(&p.pts, side as i64, &mut path);
        }
        big.fill_path(
            &path,
            Paint::Solid(argb(255, (c >> 16) as u8, (c >> 8) as u8, c as u8)),
        );
    } else {
        for p in &m.polys {
            let mut path = Path::new();
            path_of(&p.pts, side as i64, &mut path);
            big.fill_path(
                &path,
                Paint::Solid(argb(
                    255,
                    (p.color >> 16) as u8,
                    (p.color >> 8) as u8,
                    p.color as u8,
                )),
            );
        }
    }
    if ss == 1 { big } else { big.resized(px, px) }
}

// ---------------------------------------------------------------------------- SVG

fn coord(out: &mut String, v: i32) {
    use core::fmt::Write;
    let neg = v < 0;
    let a = v.abs();
    let (i, f) = (a / UNIT, a % UNIT * 100 / UNIT);
    let _ = match (neg, f) {
        (false, 0) => write!(out, "{i}"),
        (true, 0) => write!(out, "-{i}"),
        (false, _) if f % 10 == 0 => write!(out, "{}.{}", i, f / 10),
        (true, _) if f % 10 == 0 => write!(out, "-{}.{}", i, f / 10),
        (false, _) => write!(out, "{i}.{f:02}"),
        (true, _) => write!(out, "-{i}.{f:02}"),
    };
}

fn points_attr(pts: &[(i32, i32)]) -> String {
    let mut s = String::new();
    for (i, &(x, y)) in pts.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        coord(&mut s, x);
        s.push(',');
        coord(&mut s, y);
    }
    s
}

fn hex(c: u32) -> String {
    use core::fmt::Write;
    let mut s = String::new();
    let _ = write!(s, "#{:06X}", c & 0xFF_FFFF);
    s
}

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Tile => "tile",
        Role::LightFace => "light face",
        Role::ShadeFace => "shade face",
        Role::EarOuter => "ear outer",
        Role::EarInner => "ear inner",
        Role::Muzzle => "muzzle",
        Role::Eye => "eye",
        Role::Nose => "nose",
        Role::Mark => "mark",
        Role::Knockout => "knockout",
        Role::Tail => "tail",
        Role::TailDark => "tail (dark)",
        Role::TailTip => "tail tip",
    }
}

/// The mark as an SVG document on the 128 x 128 design grid (one element per polygon, in
/// paint order, each with the role as a comment; the mono mark is a single even-odd path).
pub fn to_svg(mark: &Mark, title: &str) -> String {
    use core::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {GRID} {GRID}\" width=\"{GRID}\" height=\"{GRID}\" role=\"img\" aria-label=\"{title}\">"
    );
    let _ = writeln!(s, "  <title>{title}</title>");
    if let Some(c) = mark.tile {
        let _ = writeln!(
            s,
            "  <rect width=\"{GRID}\" height=\"{GRID}\" rx=\"{}\" fill=\"{}\"/> <!-- tile -->",
            TILE_RADIUS / UNIT,
            hex(c)
        );
    }
    let is_mono = mark.polys.iter().any(|p| p.role == Role::Mark);
    if is_mono {
        let color = mark.polys[0].color;
        let mut d = String::new();
        for p in &mark.polys {
            d.push_str(if d.is_empty() { "M" } else { " M" });
            d.push_str(&points_attr(&p.pts).replace(' ', " L"));
            d.push_str(" Z");
        }
        let _ = writeln!(
            s,
            "  <path fill=\"{}\" fill-rule=\"evenodd\" d=\"{d}\"/> <!-- silhouette; eyes and nose are holes -->",
            hex(color)
        );
    } else {
        for p in &mark.polys {
            let _ = writeln!(
                s,
                "  <polygon fill=\"{}\" points=\"{}\"/> <!-- {} -->",
                hex(p.color),
                points_attr(&p.pts),
                role_name(p.role)
            );
        }
    }
    s.push_str("</svg>\n");
    s
}

#[cfg(test)]
mod tests;
