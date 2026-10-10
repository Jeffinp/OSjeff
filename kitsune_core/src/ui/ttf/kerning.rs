//! kerning (split out of `ttf.rs`).

use super::*;

impl<'a> Font<'a> {
    pub(super) fn load_kerning(&mut self) {
        let d = self.d;
        let Some((gpos, _)) = find_table(d, b"GPOS") else {
            return;
        };
        let Some(ll) = d.u16(gpos + 8) else { return };
        let ll = gpos + ll as usize;
        let Some(n) = d.u16(ll) else { return };
        for i in 0..(n as usize).min(64) {
            let Some(lo) = d.u16(ll + 2 + i * 2) else {
                return;
            };
            let l = ll + lo as usize;
            let (Some(ty), Some(sn)) = (d.u16(l), d.u16(l + 4)) else {
                return;
            };
            for s in 0..(sn as usize).min(64) {
                let Some(so) = d.u16(l + 6 + s * 2) else {
                    return;
                };
                let mut sub = l + so as usize;
                let mut kind = ty;
                if ty == 9 {
                    let (Some(et), Some(eo)) = (d.u16(sub + 2), d.u32(sub + 4)) else {
                        continue;
                    };
                    kind = et;
                    sub += eo as usize;
                }
                if kind == 2 && sub < d.0.len() {
                    self.pairs.push(sub);
                }
            }
        }
    }

    /// Horizontal kerning between two glyphs in font units (usually negative).
    pub fn kern(&self, left: u16, right: u16) -> i16 {
        if self.pairs.is_empty() || left == 0 || right == 0 {
            return 0;
        }
        let key = 1 << 31 | u32::from(left) << 16 | u32::from(right);
        let slot = &self.kern_cache[(usize::from(left) * 31 + usize::from(right)) % KERN_CACHE];
        let (k, v) = slot.get();
        if k == key {
            return v;
        }
        let mut found = 0;
        for &sub in &self.pairs {
            if let Some(Some(v)) = self.pair_value(sub, left, right) {
                found = v;
                break;
            }
        }
        slot.set((key, found));
        found
    }

    /// `Some(None)`: subtable does not apply (glyph not covered / pair absent),
    /// `Some(Some(v))`: the value.
    pub(super) fn pair_value(&self, sub: usize, left: u16, right: u16) -> Option<Option<i16>> {
        let d = self.d;
        let fmt = d.u16(sub)?;
        let cov = sub + d.u16(sub + 2)? as usize;
        let vf1 = d.u16(sub + 4)?;
        let vf2 = d.u16(sub + 6)?;
        let ci = coverage_index(d, cov, left);
        let Some(ci) = ci else {
            return Some(None);
        };
        let rec1 = (vf1.count_ones() * 2) as usize;
        let rec2 = (vf2.count_ones() * 2) as usize;
        // Offset of XAdvance inside value record 1 (bit 0x4).
        if vf1 & 4 == 0 {
            return Some(None);
        }
        let xadv_at = ((vf1 & 3).count_ones() * 2) as usize;
        match fmt {
            1 => {
                let n = d.u16(sub + 8)? as usize;
                if ci as usize >= n {
                    return Some(None);
                }
                let ps = sub + d.u16(sub + 10 + ci as usize * 2)? as usize;
                let cnt = d.u16(ps)? as usize;
                let step = 2 + rec1 + rec2;
                // Records are sorted by second glyph: binary search.
                let (mut lo, mut hi) = (0usize, cnt);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let g = d.u16(ps + 2 + mid * step)?;
                    if g < right {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo < cnt && d.u16(ps + 2 + lo * step)? == right {
                    return Some(Some(d.i16(ps + 2 + lo * step + 2 + xadv_at)?));
                }
                Some(None)
            }
            2 => {
                let cd1 = sub + d.u16(sub + 8)? as usize;
                let cd2 = sub + d.u16(sub + 10)? as usize;
                let n1 = d.u16(sub + 12)? as usize;
                let n2 = d.u16(sub + 14)? as usize;
                let c1 = class_of(d, cd1, left)? as usize;
                let c2 = class_of(d, cd2, right)? as usize;
                if c1 >= n1 || c2 >= n2 {
                    return Some(None);
                }
                let at = sub + 16 + (c1 * n2 + c2) * (rec1 + rec2);
                Some(Some(d.i16(at + xadv_at)?))
            }
            _ => Some(None),
        }
    }
}
