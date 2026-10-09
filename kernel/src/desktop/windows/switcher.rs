//! `Desktop` methods: apps. Split out of the former monolithic desktop.rs.

use crate::desktop::*;

impl Desktop {
    /// Render the resident WASM application into window `r`'s content area. The
    /// guest paints through the host drawing ABI; the engine translates and
    /// clips it to this box (see [`crate::wasm::draw_app`]).
    pub(crate) fn draw_wasm(&self, c: &mut Canvas, r: Rect, w: &WasmWin) {
        // Each app renders on the `appd` thread into its own offscreen surface; the
        // compositor just copies the latest finished frame into the window (or shows
        // why the app is not running).
        let cr = wasm_content(r);
        crate::wasm::blit(w.id, c, cr.x, cr.y, cr.w, cr.h);
    }

    /// Alt+Tab panel geometry for a list of `n` windows: centered, `SWITCH_ROWS`
    /// rows at most.
    pub(crate) fn switcher_rect(&self, n: usize) -> Rect {
        let rows = n.clamp(1, SWITCH_ROWS) as i32;
        let h = SWITCH_PAD * 2 + rows * SWITCH_ROW_H;
        Rect::new(
            (self.sw - SWITCH_W) / 2,
            (self.sh - h) / 2 - 40,
            SWITCH_W,
            h,
        )
    }

    /// The Alt+Tab overlay: every window in most-recently-used order on a glass
    /// panel, the selection highlighted; minimized windows are tagged.
    pub(crate) fn draw_switcher(&self, c: &mut Canvas, sw: &Switcher) {
        use crate::text::{self, BODY, FOOTNOTE, Weight};
        let p = theme::pal();
        let list = sw.list();
        let r = self.switcher_rect(list.len());
        let hole = Rect::new(r.x, r.y + 16, r.w, r.h - 32);
        c.draw_shadow(
            r,
            Shadow {
                blur: 24,
                dy: 14,
                alpha: 90,
            },
            hole,
        );
        self.shell.switcher_glass.draw(c, r, 16, 14, 256);
        ui::fill_token(c, r, 16, p.menu_tint);
        ui::stroke_token(c, r, 16, p.separator);
        let sel = sw.selected_index();
        let first = (sel + 1).saturating_sub(SWITCH_ROWS);
        for (row, idx) in (first..list.len().min(first + SWITCH_ROWS)).enumerate() {
            let ry = r.y + SWITCH_PAD + row as i32 * SWITCH_ROW_H;
            let rr = Rect::new(r.x + 8, ry, r.w - 16, SWITCH_ROW_H);
            let Some(w) = self.wm.get(list[idx]) else {
                continue;
            };
            let fg = if idx == sel {
                c.fill_rrect(rr, 8, Corner::Circle, theme::accent(), 256);
                theme::ACCENT_TEXT
            } else {
                theme::solid(p.text)
            };
            icons::blit(
                c,
                w.app.kind().icon(),
                rr.x + 8,
                ry + (SWITCH_ROW_H - 28) / 2,
                28,
                256,
            );
            let tag_w = if w.minimized { 56 } else { 0 };
            text::draw_left(
                c,
                Rect::new(rr.x + 48, ry, rr.w - 48 - 12 - tag_w, SWITCH_ROW_H),
                &w.app.title,
                BODY,
                Weight::Medium,
                fg,
            );
            if w.minimized {
                let tag = if idx == sel {
                    Color::rgb(0xE6, 0xE6, 0xFF)
                } else {
                    theme::solid(p.text_secondary)
                };
                text::draw_right(
                    c,
                    Rect::new(rr.x, ry, rr.w - 12, SWITCH_ROW_H),
                    "oculta",
                    FOOTNOTE,
                    Weight::Regular,
                    tag,
                );
            }
        }
    }
}
