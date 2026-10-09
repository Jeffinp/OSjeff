//! Framebuffer arithmetic that does not touch memory: colour mixing, integer
//! square root and rounded-corner insets.
//!
//! The kernel's `Canvas` owns the pixel buffer and the hot loops; it calls these
//! tiny `#[inline]` helpers so their results (and edge cases) are covered by
//! host tests.

/// An 8-bit-per-channel RGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Linear blend between `self` and `other`. `t` in 0..=255 (0 = self);
    /// larger values are treated as 255.
    #[inline]
    pub fn lerp(self, other: Color, t: u16) -> Color {
        let t = t.min(255);
        let mix = |a: u8, b: u8| -> u8 {
            let a = a as u16;
            let b = b as u16;
            ((a * (255 - t) + b * t) / 255) as u8
        };
        Color::rgb(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }
}

/// Integer square root (floor). Used to inset rounded-rectangle corner rows.
#[inline]
pub fn isqrt(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// How many pixels row `y` (0-based, of `h` rows) of a rounded rectangle with
/// corner radius `r` is inset from each side. `r` is clamped to `h / 2`.
#[inline]
pub fn corner_inset(r: usize, y: usize, h: usize) -> usize {
    let r = r.min(h / 2);
    if r == 0 {
        0
    } else if y < r {
        let dy = r - y; // 1..=r
        r - isqrt(r * r - dy * dy)
    } else if y >= h - r {
        let dy = y - (h - 1 - r); // 0..=r
        r - isqrt(r.saturating_mul(r).saturating_sub(dy * dy))
    } else {
        0
    }
}

/// `(dst * (256 - a) + src * a) / 256` with `a` clamped to `0..=256`
/// (0 = `dst` unchanged, 256 = `src`).
#[inline]
pub fn mix256(dst: u8, src: u8, a: u16) -> u8 {
    let a = a.min(256);
    ((dst as u16 * (256 - a) + src as u16 * a) / 256) as u8
}

/// Converts an 8-bit alpha (0..=255) to the 0..=256 range `mix256` expects, so
/// 255 maps to exactly 256 (opaque).
#[inline]
pub fn alpha255_to_256(a: u8) -> u16 {
    a as u16 + (a >> 7) as u16
}

/// Per-channel destination -> result tables for blending the constant colour
/// `c` at `alpha` (0..=256) over any destination byte. Bit-identical to calling
/// [`mix256`] per pixel.
#[inline]
pub fn blend_lut(c: [u8; 3], alpha: u16) -> [[u8; 256]; 3] {
    let a = alpha.min(256);
    let ia = 256 - a;
    let sa = [c[0] as u16 * a, c[1] as u16 * a, c[2] as u16 * a];
    let mut t = [[0u8; 256]; 3];
    for (table, &src) in t.iter_mut().zip(sa.iter()) {
        for (v, out) in table.iter_mut().enumerate() {
            *out = ((v as u16 * ia + src) / 256) as u8;
        }
    }
    t
}

/// Grayscale luminance of an RGB triple (for 8-bit framebuffers).
#[inline]
pub fn luma(c: Color) -> u8 {
    ((c.r as u16 * 54 + c.g as u16 * 183 + c.b as u16 * 19) >> 8) as u8
}

/// A half-open pixel span `[start, end)` within a row.
pub type Span = (usize, usize);

/// The parts of the span `[xs, xe)` left after removing the hole
/// `[hx, hx + hw)`: `(left, right)`, each `None` when empty.
pub fn split_span_around_hole(
    xs: usize,
    xe: usize,
    hx: usize,
    hw: usize,
) -> (Option<Span>, Option<Span>) {
    let left_end = xe.min(hx);
    let left = (xs < left_end).then_some((xs, left_end));
    let right_start = xs.max(hx + hw);
    let right = (right_start < xe).then_some((right_start, xe));
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt_matches_std_exhaustively_for_small_values() {
        for n in 0..200_000usize {
            assert_eq!(isqrt(n), n.isqrt(), "n={n}");
        }
    }

    #[test]
    fn isqrt_perfect_squares_and_neighbours() {
        for r in [1usize, 2, 3, 7, 255, 1000, 65_535, 1 << 20, (1 << 31) - 1] {
            let sq = r * r;
            assert_eq!(isqrt(sq), r);
            assert_eq!(isqrt(sq - 1), r - 1);
            assert_eq!(isqrt(sq + 1), r);
        }
    }

    #[test]
    fn isqrt_extremes_do_not_overflow() {
        assert_eq!(isqrt(usize::MAX), usize::MAX.isqrt());
        assert_eq!(isqrt(usize::MAX - 1), (usize::MAX - 1).isqrt());
    }

    #[test]
    fn inset_is_zero_without_a_radius() {
        for y in 0..10 {
            assert_eq!(corner_inset(0, y, 10), 0);
        }
    }

    #[test]
    fn inset_follows_the_circle() {
        // r = 4: rows 0..4 of the top corner. dy = 4,3,2,1.
        let insets: Vec<usize> = (0..4).map(|y| corner_inset(4, y, 20)).collect();
        assert_eq!(insets, vec![4, 2, 1, 1]);
        // Middle rows are not inset.
        for y in 4..16 {
            assert_eq!(corner_inset(4, y, 20), 0, "y={y}");
        }
    }

    #[test]
    fn inset_is_vertically_symmetric() {
        for r in 0..=12usize {
            for h in (2 * r).max(1)..(2 * r + 6) {
                for y in 0..h {
                    assert_eq!(
                        corner_inset(r, y, h),
                        corner_inset(r, h - 1 - y, h),
                        "r={r} h={h} y={y}"
                    );
                }
            }
        }
    }

    #[test]
    fn inset_never_exceeds_radius_and_never_eats_the_row() {
        for r in 0..=16usize {
            for y in 0..(2 * r + 3) {
                let h = 2 * r + 3;
                assert!(corner_inset(r, y, h) <= r);
            }
        }
    }

    #[test]
    fn inset_clamps_an_oversized_radius() {
        // Radius larger than half the height behaves like h / 2 and cannot
        // underflow `h - r`.
        for y in 0..6 {
            assert_eq!(corner_inset(100, y, 6), corner_inset(3, y, 6));
        }
        assert_eq!(corner_inset(5, 0, 0), 0);
        assert_eq!(corner_inset(5, 0, 1), 0);
    }

    #[test]
    fn mix256_endpoints_and_midpoint() {
        assert_eq!(mix256(10, 200, 0), 10);
        assert_eq!(mix256(10, 200, 256), 200);
        assert_eq!(mix256(0, 255, 128), 127);
        assert_eq!(mix256(255, 255, 77), 255);
    }

    #[test]
    fn mix256_clamps_alpha_above_256() {
        assert_eq!(mix256(10, 200, 9999), 200);
    }

    #[test]
    fn mix256_is_monotonic_in_alpha() {
        let mut prev = 0u8;
        for a in 0..=256u16 {
            let v = mix256(0, 255, a);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn alpha_scaling_makes_255_opaque() {
        assert_eq!(alpha255_to_256(0), 0);
        assert_eq!(alpha255_to_256(127), 127);
        assert_eq!(alpha255_to_256(128), 129);
        assert_eq!(alpha255_to_256(255), 256);
    }

    #[test]
    fn lerp_endpoints() {
        let a = Color::rgb(0, 100, 255);
        let b = Color::rgb(255, 100, 0);
        assert_eq!(a.lerp(b, 0), a);
        assert_eq!(a.lerp(b, 255), b);
        assert_eq!(a.lerp(b, 1000), b); // out-of-range t no longer underflows
        assert_eq!(a.lerp(b, 128), Color::rgb(128, 100, 127));
    }

    #[test]
    fn blend_lut_matches_the_per_pixel_formula() {
        let c = [200u8, 30, 99];
        for alpha in [0u16, 1, 60, 128, 255, 256, 400] {
            let t = blend_lut(c, alpha);
            for v in 0..=255u8 {
                for k in 0..3 {
                    assert_eq!(
                        t[k][v as usize],
                        mix256(v, c[k], alpha),
                        "a={alpha} v={v} k={k}"
                    );
                }
            }
        }
    }

    #[test]
    fn luma_of_primaries_and_gray() {
        assert_eq!(luma(Color::rgb(0, 0, 0)), 0);
        assert_eq!(luma(Color::rgb(255, 255, 255)), 255);
        assert!(luma(Color::rgb(0, 255, 0)) > luma(Color::rgb(255, 0, 0)));
        assert!(luma(Color::rgb(255, 0, 0)) > luma(Color::rgb(0, 0, 255)));
    }

    #[test]
    fn hole_splits_the_span() {
        // Hole strictly inside: two pieces.
        assert_eq!(
            split_span_around_hole(0, 100, 30, 20),
            (Some((0, 30)), Some((50, 100)))
        );
        // Hole covering the left edge: only the right piece.
        assert_eq!(
            split_span_around_hole(10, 100, 0, 40),
            (None, Some((40, 100)))
        );
        // Hole covering the right edge: only the left piece.
        assert_eq!(
            split_span_around_hole(0, 100, 60, 100),
            (Some((0, 60)), None)
        );
        // Hole covers everything.
        assert_eq!(split_span_around_hole(10, 20, 0, 100), (None, None));
    }

    #[test]
    fn hole_outside_the_span_leaves_it_whole_on_one_side() {
        // Hole entirely to the right: left piece is the whole span.
        assert_eq!(split_span_around_hole(0, 10, 50, 5), (Some((0, 10)), None));
        // Hole entirely to the left: right piece is the whole span.
        assert_eq!(split_span_around_hole(20, 30, 0, 5), (None, Some((20, 30))));
    }

    #[test]
    fn empty_span_stays_empty() {
        assert_eq!(split_span_around_hole(5, 5, 0, 0), (None, None));
        assert_eq!(split_span_around_hole(9, 5, 0, 0), (None, None));
    }
}
