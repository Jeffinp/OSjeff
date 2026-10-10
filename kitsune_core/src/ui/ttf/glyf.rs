//! glyf (split out of `ttf.rs`).

use super::*;

impl<'a> Font<'a> {
    pub(super) fn simple(
        &self,
        a: usize,
        ncont: usize,
        end: usize,
        m: [i32; 4],
        off: (i32, i32),
        sink: &mut impl FnMut(Seg),
    ) {
        let d = self.d;
        if ncont == 0 {
            return;
        }
        let ends_at = a + 10;
        let Some(last) = d.u16(ends_at + (ncont - 1) * 2) else {
            return;
        };
        let npts = last as usize + 1;
        if npts > MAX_POINTS {
            return;
        }
        let Some(ilen) = d.u16(ends_at + ncont * 2) else {
            return;
        };
        let mut p = ends_at + ncont * 2 + 2 + ilen as usize;
        // Flags.
        let mut flags: Vec<u8> = Vec::with_capacity(npts);
        while flags.len() < npts {
            let Some(f) = d.u8(p) else { return };
            p += 1;
            flags.push(f);
            if f & 8 != 0 {
                let Some(rep) = d.u8(p) else { return };
                p += 1;
                for _ in 0..rep {
                    if flags.len() < npts {
                        flags.push(f);
                    }
                }
            }
        }
        let mut xs: Vec<i32> = Vec::with_capacity(npts);
        let mut v = 0i32;
        for &f in &flags {
            if f & 2 != 0 {
                let Some(dx) = d.u8(p) else { return };
                p += 1;
                v += if f & 16 != 0 { dx as i32 } else { -(dx as i32) };
            } else if f & 16 == 0 {
                let Some(dx) = d.i16(p) else { return };
                p += 2;
                v += dx as i32;
            }
            xs.push(v);
        }
        let mut ys: Vec<i32> = Vec::with_capacity(npts);
        v = 0;
        for &f in &flags {
            if f & 4 != 0 {
                let Some(dy) = d.u8(p) else { return };
                p += 1;
                v += if f & 32 != 0 { dy as i32 } else { -(dy as i32) };
            } else if f & 32 == 0 {
                let Some(dy) = d.i16(p) else { return };
                p += 2;
                v += dy as i32;
            }
            ys.push(v);
        }
        if p > end {
            return;
        }
        let tx = |x: i32, y: i32| -> (i32, i32) {
            (
                mul14(x, m[0], y, m[2]).saturating_add(off.0),
                mul14(x, m[1], y, m[3]).saturating_add(off.1),
            )
        };
        let mut start = 0usize;
        for c in 0..ncont {
            let Some(e) = d.u16(ends_at + c * 2) else {
                return;
            };
            let e = e as usize;
            if e < start || e >= npts {
                return;
            }
            contour(&flags[start..=e], &xs[start..=e], &ys[start..=e], &tx, sink);
            start = e + 1;
        }
    }

    pub(super) fn composite(
        &self,
        mut p: usize,
        end: usize,
        m_parent: [i32; 4],
        off_parent: (i32, i32),
        depth: u8,
        sink: &mut impl FnMut(Seg),
    ) {
        let d = self.d;
        for _ in 0..32 {
            let (Some(flags), Some(gid)) = (d.u16(p), d.u16(p + 2)) else {
                return;
            };
            p += 4;
            let (dx, dy);
            if flags & 1 != 0 {
                let (Some(a), Some(b)) = (d.i16(p), d.i16(p + 2)) else {
                    return;
                };
                p += 4;
                (dx, dy) = (a as i32, b as i32);
            } else {
                let (Some(a), Some(b)) = (d.u8(p), d.u8(p + 1)) else {
                    return;
                };
                p += 2;
                (dx, dy) = (a as i8 as i32, b as i8 as i32);
            }
            // Point-matching arguments (flag 2 clear) are not supported: no offset.
            let (dx, dy) = if flags & 2 != 0 { (dx, dy) } else { (0, 0) };
            let mut m = [1 << 14, 0, 0, 1 << 14];
            let f2 = |o: usize| d.i16(o).map(|v| v as i32);
            if flags & 8 != 0 {
                let Some(s) = f2(p) else { return };
                p += 2;
                m = [s, 0, 0, s];
            } else if flags & 0x40 != 0 {
                let (Some(sx), Some(sy)) = (f2(p), f2(p + 2)) else {
                    return;
                };
                p += 4;
                m = [sx, 0, 0, sy];
            } else if flags & 0x80 != 0 {
                let (Some(a), Some(b), Some(c), Some(e)) = (f2(p), f2(p + 2), f2(p + 4), f2(p + 6))
                else {
                    return;
                };
                p += 8;
                m = [a, b, c, e];
            }
            // Compose with the parent transform.
            let cm = [
                mul14(m[0], m_parent[0], m[1], m_parent[2]),
                mul14(m[0], m_parent[1], m[1], m_parent[3]),
                mul14(m[2], m_parent[0], m[3], m_parent[2]),
                mul14(m[2], m_parent[1], m[3], m_parent[3]),
            ];
            let o = (
                mul14(dx, m_parent[0], dy, m_parent[2]).saturating_add(off_parent.0),
                mul14(dx, m_parent[1], dy, m_parent[3]).saturating_add(off_parent.1),
            );
            self.outline_at(gid, cm, o, depth + 1, sink);
            if flags & 0x20 == 0 || p >= end {
                return;
            }
        }
    }

    // ---------------------------------------------------------------- kerning
}
