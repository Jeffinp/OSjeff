//! The Apps overlay: every app in a grid with a search field.

use super::helpers::draw_fit;
use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, Weight};
use kitsune_core::chrome::{self, LP_ICON, launcher_grid};
use kitsune_core::launcher;
use kitsune_core::style::PANEL_H;

impl Desktop {
    // ---- Apps ----

    /// Show the Apps overlay (or hide it when it is already up).
    pub(crate) fn open_apps(&mut self) {
        if let Some(a) = self.shell.apps.as_mut() {
            if !a.closing {
                a.closing = true;
                fade_out(&mut a.t, OVERLAY_FADE);
                self.force_full = true;
            }
            return;
        }
        self.close_transients();
        self.shell.search = None;
        let mut tiles = Vec::new();
        for k in Kind::ALL {
            if k == Kind::WasmApp {
                continue;
            }
            tiles.push(Tile {
                label: String::from(k.label()),
                target: Target::Kind(k),
                icon: icons::surface(k.icon(), LP_ICON).clone(),
                cat: launcher::system_category(k.proc_name()),
                key: k.recent_key(),
            });
        }
        for (i, a) in self.apps.iter().enumerate() {
            tiles.push(Tile {
                label: a.name.clone(),
                target: Target::Wasm(i),
                icon: icons::app_tile(a.icon.as_deref(), LP_ICON),
                cat: launcher::app_category(&a.id),
                key: alloc::format!("app:{}", a.id),
            });
        }
        let shown = (0..tiles.len()).collect();
        self.shell.apps = Some(AppsView {
            tiles,
            cat: 0,
            rail_hover: None,
            query: String::new(),
            shown,
            scroll: 0,
            hover: None,
            t: fade_in(OVERLAY_FADE),
            closing: false,
            backdrop: Default::default(),
        });
        self.apps_dirty_all();
        self.force_full = true;
    }

    pub(super) fn close_apps(&mut self) {
        if let Some(a) = self.shell.apps.as_mut()
            && !a.closing
        {
            a.closing = true;
            fade_out(&mut a.t, OVERLAY_FADE);
        }
        self.force_full = true;
    }

    /// Recompute what the grid shows for the category and the search text.
    fn apps_refilter(&mut self) {
        let Some(a) = self.shell.apps.as_mut() else {
            return;
        };
        let cat = launcher::CATEGORIES[a.cat.min(launcher::CATEGORIES.len() - 1)];
        a.shown = launcher::filter(
            a.tiles.iter().map(|t| (t.label.as_str(), t.cat)),
            cat,
            &a.query,
        );
        a.scroll = 0;
        a.hover = if a.query.is_empty() || a.shown.is_empty() {
            None
        } else {
            Some(0)
        };
        self.apps_dirty_all();
    }

    /// Select category `i` of the rail.
    pub(super) fn apps_set_category(&mut self, i: usize) {
        let Some(a) = self.shell.apps.as_mut() else {
            return;
        };
        let i = i.min(launcher::CATEGORIES.len() - 1);
        a.cat = i;
        a.query.clear();
        self.apps_refilter();
    }

    /// Launch an app from the overlay, a result of Busca or the taskbar, and remember it.
    pub(super) fn launch_target(&mut self, t: Target) {
        match t {
            Target::Kind(k) => {
                self.shell.recents.note(&k.recent_key());
                self.launch(k);
            }
            Target::Wasm(i) => {
                if let Some(id) = self.apps.get(i).map(|a| a.id.clone()) {
                    self.shell.recents.note(&alloc::format!("app:{id}"));
                    self.launch_wasm_app(&id);
                }
            }
        }
    }

    /// The tiles of the *Recentes* row (indices into the tiles, newest first): only on the
    /// first category with an empty search.
    pub(super) fn recent_tiles(&self, a: &AppsView) -> Vec<usize> {
        if a.cat != 0 || !a.query.is_empty() {
            return Vec::new();
        }
        self.shell
            .recents
            .list()
            .iter()
            .filter_map(|k| a.tiles.iter().position(|t| &t.key == k))
            .collect()
    }

    pub(super) fn apps_geom(&self, a: &AppsView) -> chrome::LaunchGrid {
        launcher_grid(
            self.sw,
            self.sh,
            a.shown.len(),
            a.scroll,
            self.recent_tiles(a).len(),
        )
    }

    pub(super) fn draw_apps(&self, c: &mut Canvas, a: &AppsView) {
        let p = theme::pal();
        let fade = level(&a.t);
        // Everything below the panel, which stays crisp.
        let full = Rect::new(0, PANEL_H, self.sw, self.sh - PANEL_H);
        a.backdrop.draw(c, full, 0, 64, fade);
        let (wash, wa) = if theme::dark() {
            (Color::rgb(0, 0, 0), 150u32)
        } else {
            (Color::rgb(0xF4, 0xF4, 0xF8), 140)
        };
        c.blend_rect(full, wash, (wa * fade / 256) as u16);
        let g = self.apps_geom(a);
        let clip = c.clip_rect();
        if fade > 100 {
            // The category rail: a translucent card, the selected row in the accent.
            ui::fill_token(c, g.rail, 14, p.menu_tint);
            ui::stroke_token(c, g.rail, 14, p.separator);
            for (i, (row, cat)) in g.rail_rows.iter().zip(launcher::CATEGORIES).enumerate() {
                let on = a.cat == i && a.query.is_empty();
                if on {
                    c.fill_rrect(*row, 8, Corner::Circle, theme::accent(), 256);
                } else if a.rail_hover == Some(i) {
                    ui::fill_token(c, *row, 8, p.hover);
                }
                let fg = if on {
                    theme::WHITE
                } else {
                    theme::solid(p.text)
                };
                text::draw_left(
                    c,
                    Rect::new(row.x + 14, row.y, row.w - 50, row.h),
                    cat.label(),
                    BODY,
                    if on { Weight::Semibold } else { Weight::Medium },
                    fg,
                );
                let n =
                    launcher::filter(a.tiles.iter().map(|t| (t.label.as_str(), t.cat)), cat, "")
                        .len();
                text::draw_right(
                    c,
                    Rect::new(row.x, row.y, row.w - 12, row.h),
                    &alloc::format!("{n}"),
                    FOOTNOTE,
                    Weight::Regular,
                    if on {
                        Color::rgb(0xE8, 0xE8, 0xFF)
                    } else {
                        theme::solid(p.text_tertiary)
                    },
                );
            }
            ui::text_field(
                c,
                g.field,
                &a.query,
                kitsune_core::t!("launcher.search_apps"),
                true,
                true,
            );
            let recents = self.recent_tiles(a);
            if !recents.is_empty() {
                text::draw_left(
                    c,
                    g.recents_label,
                    kitsune_core::t!("launcher.recents"),
                    FOOTNOTE,
                    Weight::Medium,
                    theme::solid(p.text_secondary),
                );
                for (r, &ti) in g.recents.iter().zip(&recents) {
                    let t = &a.tiles[ti];
                    ui::fill_token(c, *r, 10, p.menu_tint);
                    ui::stroke_token(c, *r, 10, p.separator);
                    c.blit_surface(&t.icon.resized(32, 32), r.x + (r.w - 32) / 2, r.y + 7, 256);
                    draw_fit(
                        c,
                        Rect::new(r.x + 6, r.y + 42, r.w - 12, 16),
                        &t.label,
                        FOOTNOTE,
                        Weight::Medium,
                        theme::solid(p.text),
                    );
                }
            }
        }
        let rise = ((256 - fade) as i32 * 14) / 256;
        let first_row = a.scroll;
        let last_row = a.scroll + g.visible_rows;
        for (pos, (&ti, cell)) in a.shown.iter().zip(&g.cells).enumerate() {
            let row = pos / g.cols;
            if row < first_row || row >= last_row {
                continue;
            }
            if cell.intersection(&clip).is_none() {
                continue;
            }
            let tile = &a.tiles[ti];
            let hov = a.hover == Some(pos);
            if hov {
                ui::fill_token(c, cell.inflated(-6), 14, p.hover);
            }
            let ix = cell.x + (cell.w - LP_ICON) / 2;
            let iy = cell.y + 12 + rise;
            c.blit_surface(&tile.icon, ix, iy, fade);
            let lr = Rect::new(cell.x + 6, iy + LP_ICON + 8, cell.w - 12, 20);
            text::draw_centered_a(
                c,
                lr,
                &tile.label,
                BODY,
                Weight::Medium,
                theme::solid(p.text),
                fade as u16,
            );
        }
        if g.total_rows > g.visible_rows {
            let track = Rect::new(
                self.sw - 14,
                g.top.max(120),
                10,
                g.visible_rows as i32 * chrome::LP_CELL_H,
            );
            ui::overlay_scrollbar(c, track, a.scroll, g.total_rows, g.visible_rows, fade);
        }
        if a.shown.is_empty() && fade > 120 {
            let r = Rect::new(g.left, self.sh / 2 - 20, self.sw - g.left, 40);
            text::draw_centered(
                c,
                r,
                kitsune_core::t!("launcher.empty"),
                CALLOUT,
                Weight::Regular,
                theme::solid(p.text_secondary),
            );
        }
    }

    pub(super) fn apps_hover_to(&mut self, new: Option<usize>) {
        let Some(a) = self.shell.apps.as_ref() else {
            return;
        };
        if a.hover == new {
            return;
        }
        let g = self.apps_geom(a);
        let mut dirty = self.shell.dirty.get();
        for i in [a.hover, new].into_iter().flatten() {
            if let Some(r) = g.cells.get(i) {
                dirty = if dirty.is_empty() { *r } else { dirty.union(r) };
            }
        }
        if let Some(a) = self.shell.apps.as_mut() {
            a.hover = new;
        }
        self.shell.dirty.set(dirty);
    }

    pub(super) fn apps_key(&mut self, key: Key) {
        let Some(a) = self.shell.apps.as_ref() else {
            return;
        };
        let g = self.apps_geom(a);
        let n = a.shown.len();
        let cur = a.hover;
        let ctrl = self.keymap.ctrl();
        match key {
            Key::Esc => self.close_apps(),
            // Ctrl+arrows walk the rail.
            Key::Up | Key::Down | Key::Left | Key::Right if ctrl => {
                let last = launcher::CATEGORIES.len() - 1;
                let c = a.cat;
                let next = if matches!(key, Key::Up | Key::Left) {
                    c.saturating_sub(1)
                } else {
                    (c + 1).min(last)
                };
                self.apps_set_category(next);
            }
            Key::Enter => {
                let pick = cur.or(if n > 0 { Some(0) } else { None });
                if let Some(pos) = pick
                    && let Some(t) = a
                        .shown
                        .get(pos)
                        .and_then(|&i| a.tiles.get(i))
                        .map(|t| t.target)
                {
                    self.close_apps();
                    self.launch_target(t);
                }
            }
            Key::Backspace => {
                if let Some(a) = self.shell.apps.as_mut() {
                    a.query.pop();
                }
                self.apps_refilter();
            }
            Key::Char(b) if (0x20..0x7F).contains(&b) || b >= 0xA0 => {
                if let Some(a) = self.shell.apps.as_mut()
                    && a.query.len() < 40
                {
                    a.query.push(char::from(b));
                }
                self.apps_refilter();
            }
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Tab if n > 0 => {
                let cols = g.cols;
                let cur = cur.unwrap_or(0);
                let next = match key {
                    Key::Left => cur.saturating_sub(1),
                    Key::Right | Key::Tab => (cur + 1).min(n - 1),
                    Key::Up => cur.saturating_sub(cols),
                    _ => (cur + cols).min(n - 1),
                };
                // Scroll to keep the selection on screen.
                if let Some(a) = self.shell.apps.as_mut() {
                    let row = next / cols;
                    if row < a.scroll {
                        a.scroll = row;
                    } else if row >= a.scroll + g.visible_rows {
                        a.scroll = row + 1 - g.visible_rows;
                    }
                }
                self.apps_hover_to(Some(next));
                self.apps_dirty_all();
            }
            Key::PageUp | Key::PageDown | Key::Home | Key::End => {
                if let Some(a) = self.shell.apps.as_mut() {
                    let max = g.total_rows.saturating_sub(g.visible_rows);
                    a.scroll = match key {
                        Key::PageUp => a.scroll.saturating_sub(g.visible_rows),
                        Key::PageDown => (a.scroll + g.visible_rows).min(max),
                        Key::Home => 0,
                        _ => max,
                    };
                }
                self.apps_dirty_all();
            }
            _ => {}
        }
    }
}
