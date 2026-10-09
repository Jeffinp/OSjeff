//! The full-screen and floating layers: the Apps overlay (every app in a grid with
//! a search field), Busca (apps, files and a calculator behind one field), the
//! confirmation sheet, and the pointer/keyboard handling of every shell layer
//! (menus, popovers, those overlays).

use crate::desktop::kit::glass::panel;
use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, TITLE2, TITLE3, Weight};
use kitsune_core::chrome::{self, LP_ICON, launcher_cell_at, launcher_grid, spotlight_geom};
use kitsune_core::iconart::Glyph;
use kitsune_core::launcher;
use kitsune_core::search;
use kitsune_core::style::PANEL_H;

/// Corner radius of the Busca panel and of the confirmation sheet.
const R_PANEL: i32 = 12;
/// Most files Busca indexes when it opens, and how deep it looks.
const INDEX_MAX: usize = 600;
const INDEX_DEPTH: usize = 5;

/// Confirmation sheet geometry: panel and the two buttons.
fn sheet_geom(sw: i32, sh: i32) -> (Rect, Rect, Rect) {
    let panel = Rect::new(sw / 2 - 190, sh / 2 - 110, 380, 220);
    let ok = Rect::new(panel.right() - 20 - 120, panel.bottom() - 20 - 32, 120, 32);
    let cancel = Rect::new(ok.x - 10 - 120, ok.y, 120, 32);
    (panel, cancel, ok)
}

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

    fn close_apps(&mut self) {
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
    fn apps_set_category(&mut self, i: usize) {
        let Some(a) = self.shell.apps.as_mut() else {
            return;
        };
        let i = i.min(launcher::CATEGORIES.len() - 1);
        a.cat = i;
        a.query.clear();
        self.apps_refilter();
    }

    /// Launch an app from the overlay, a result of Busca or the taskbar, and remember it.
    fn launch_target(&mut self, t: Target) {
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
    fn recent_tiles(&self, a: &AppsView) -> Vec<usize> {
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

    fn apps_geom(&self, a: &AppsView) -> chrome::LaunchGrid {
        launcher_grid(
            self.sw,
            self.sh,
            a.shown.len(),
            a.scroll,
            self.recent_tiles(a).len(),
        )
    }

    fn draw_apps(&self, c: &mut Canvas, a: &AppsView) {
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

    fn apps_hover_to(&mut self, new: Option<usize>) {
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

    fn apps_key(&mut self, key: Key) {
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

    // ---- Busca ----

    /// Show Busca (or hide it when it is up).
    pub(crate) fn open_search(&mut self) {
        if let Some(s) = self.shell.search.as_mut() {
            if !s.closing {
                s.closing = true;
                fade_out(&mut s.t, MENU_FADE);
                self.force_full = true;
            }
            return;
        }
        self.close_transients();
        self.shell.apps = None;
        let files = index_files();
        self.shell.search = Some(SearchView {
            query: String::new(),
            hits: Vec::new(),
            sel: 0,
            files,
            t: fade_in(MENU_FADE),
            closing: false,
            glass: Default::default(),
        });
        self.force_full = true;
    }

    fn close_search(&mut self) {
        if let Some(s) = self.shell.search.as_mut()
            && !s.closing
        {
            s.closing = true;
            fade_out(&mut s.t, MENU_FADE);
        }
        self.force_full = true;
    }

    /// Recompute the results for the current query.
    fn search_refresh(&mut self) {
        let Some(s) = self.shell.search.as_ref() else {
            return;
        };
        let q = s.query.clone();
        let mut hits: Vec<(u8, SearchHit)> = Vec::new();
        if !q.trim().is_empty() {
            if let Some(ans) = search::eval(&q) {
                hits.push((
                    0,
                    SearchHit {
                        title: alloc::format!("= {ans}"),
                        sub: String::from(kitsune_core::t!("search.calc_hint")),
                        kind: HitKind::Calc,
                    },
                ));
            }
            for k in Kind::ALL {
                if k == Kind::WasmApp {
                    continue;
                }
                if let Some(r) = search::rank(&q, k.label())
                    .into_iter()
                    .chain(search::rank(&q, k.label_in(kitsune_core::i18n::Lang::En)))
                    .min()
                {
                    hits.push((
                        r,
                        SearchHit {
                            title: String::from(k.label()),
                            sub: String::from(kitsune_core::t!("search.kind_app")),
                            kind: HitKind::App(Target::Kind(k)),
                        },
                    ));
                }
            }
            for (word_key, tab) in apps::tarefas::SEARCH_ALIASES {
                // The alias is known in both languages, like the app names above.
                if let Some(r) = search::rank(&q, kitsune_core::i18n::tr(word_key))
                    .into_iter()
                    .chain(search::rank(
                        &q,
                        kitsune_core::i18n::tr_in(kitsune_core::i18n::Lang::En, word_key),
                    ))
                    .min()
                    && search::rank(&q, Kind::TaskMgr.label()).is_none()
                {
                    hits.push((
                        r.saturating_add(1),
                        SearchHit {
                            title: kitsune_core::t!(
                                "search.tasks_tab",
                                app = Kind::TaskMgr.label(),
                                tab = apps::tarefas::tab_name(tab)
                            ),
                            sub: String::from(kitsune_core::t!("search.kind_app")),
                            kind: HitKind::Tab(tab),
                        },
                    ));
                }
            }
            for (i, a) in self.apps.iter().enumerate() {
                if let Some(r) = search::rank(&q, &a.name) {
                    hits.push((
                        r,
                        SearchHit {
                            title: a.name.clone(),
                            sub: String::from(kitsune_core::t!("search.kind_app")),
                            kind: HitKind::App(Target::Wasm(i)),
                        },
                    ));
                }
            }
            for (path, name) in &s.files {
                if let Some(r) = search::rank(&q, name) {
                    hits.push((
                        r + 1,
                        SearchHit {
                            title: name.clone(),
                            sub: String::from_utf8_lossy(&vfs::parent(path)).into_owned(),
                            kind: HitKind::File(path.clone()),
                        },
                    ));
                }
            }
        }
        // Stable: best rank first, apps before files at equal rank, calculator on top.
        hits.sort_by_key(|(r, _)| *r);
        hits.truncate(chrome::SPOT_MAX_ROWS);
        if let Some(s) = self.shell.search.as_mut() {
            s.hits = hits.into_iter().map(|(_, h)| h).collect();
            s.sel = 0;
        }
        self.force_full = true;
    }

    fn activate_hit(&mut self, hit: SearchHit) {
        match hit.kind {
            HitKind::App(t) => self.launch_target(t),
            HitKind::Tab(t) => self.open_tarefas_tab(t),
            HitKind::Calc => {
                let ans = hit.title.trim_start_matches("= ").as_bytes().to_vec();
                self.clipboard.set(&ans);
            }
            HitKind::File(path) => {
                if vfs::list(&path).is_ok() {
                    if let Some(id) = self.launch(Kind::Files) {
                        self.files_go(id, &path);
                    }
                } else {
                    let class = kitsune_core::fileman::classify(&path);
                    self.open_path(WindowId::from_raw(u32::MAX), &path, class);
                }
            }
        }
    }

    fn draw_search(&self, c: &mut Canvas, s: &SearchView) {
        let p = theme::pal();
        let fade = level(&s.t);
        let g = spotlight_geom(self.sw, self.sh, s.hits.len());
        // The backdrop covers the tallest the panel gets (a full list of results).
        let tallest = spotlight_geom(self.sw, self.sh, chrome::SPOT_MAX_ROWS).panel;
        s.glass.ensure(c, tallest.union(&g.panel).inflated(2), 14);
        let mut r = g.panel;
        r.y -= ((256 - fade) as i32 * 8) / 256;
        let dy = r.y - g.panel.y;
        panel(
            c,
            r,
            R_PANEL,
            &s.glass,
            14,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 24,
                dy: 14,
                alpha: 90,
            },
            fade,
        );
        if fade < 140 {
            return;
        }
        let field = Rect::new(g.field.x, g.field.y + dy, g.field.w, g.field.h);
        ui::draw_glyph(
            c,
            Glyph::Search,
            field.x + 18,
            field.y + (field.h - 22) / 2,
            22,
            0xFF00_0000 | pack_rgb(theme::solid(p.text_secondary)),
        );
        let tx = field.x + 54;
        let ty = text::center_y(field.y, field.h, TITLE2, Weight::Regular);
        if s.query.is_empty() {
            text::draw(
                c,
                tx,
                ty,
                kitsune_core::t!("search.placeholder"),
                TITLE2,
                Weight::Regular,
                theme::solid(p.text_tertiary),
            );
            c.blend_rect(
                Rect::new(tx - 2, field.y + 14, 2, field.h - 28),
                theme::accent(),
                256,
            );
        } else {
            let shown = text::ellipsize(&s.query, TITLE2, Weight::Regular, field.w - 74);
            let w = text::draw(
                c,
                tx,
                ty,
                &shown,
                TITLE2,
                Weight::Regular,
                theme::solid(p.text),
            );
            c.blend_rect(
                Rect::new(tx + w + 1, field.y + 14, 2, field.h - 28),
                theme::accent(),
                256,
            );
        }
        if s.hits.is_empty() {
            return;
        }
        let (sc, sa) = theme::tint(p.separator);
        c.blend_rect(Rect::new(r.x + 12, field.bottom(), r.w - 24, 1), sc, sa);
        for (i, (hit, row)) in s.hits.iter().zip(&g.rows).enumerate() {
            let row = Rect::new(row.x, row.y + dy, row.w, row.h);
            let sel = i == s.sel;
            if sel {
                c.fill_rrect(row, 8, Corner::Circle, theme::accent(), 256);
            }
            let (fg, fg2) = if sel {
                (theme::ACCENT_TEXT, Color::rgb(0xE6, 0xE6, 0xFF))
            } else {
                (theme::solid(p.text), theme::solid(p.text_secondary))
            };
            let ic = Rect::new(row.x + 10, row.y + (row.h - 28) / 2, 28, 28);
            match &hit.kind {
                HitKind::App(Target::Kind(k)) => {
                    c.blit_surface(icons::surface(k.icon(), 28), ic.x, ic.y, 256)
                }
                HitKind::App(Target::Wasm(_)) => {
                    c.blit_surface(icons::surface(Icon::WasmApp, 28), ic.x, ic.y, 256)
                }
                HitKind::Tab(_) => {
                    c.blit_surface(icons::surface(Kind::TaskMgr.icon(), 28), ic.x, ic.y, 256)
                }
                HitKind::File(_) => {
                    c.blit_surface(icons::surface(Icon::Files, 28), ic.x, ic.y, 256)
                }
                HitKind::Calc => {
                    c.fill_rrect(
                        ic,
                        7,
                        Corner::Squircle,
                        if sel {
                            Color::rgb(0xFF, 0xFF, 0xFF)
                        } else {
                            theme::accent()
                        },
                        256,
                    );
                    text::draw_centered(
                        c,
                        ic,
                        "=",
                        CALLOUT,
                        Weight::Semibold,
                        if sel {
                            theme::accent()
                        } else {
                            theme::ACCENT_TEXT
                        },
                    );
                }
            }
            let sub_w = text::measure(&hit.sub, FOOTNOTE, Weight::Regular).min(row.w / 3);
            let title_r = Rect::new(
                ic.right() + 12,
                row.y,
                row.w - 28 - 12 - 12 - sub_w - 8,
                row.h,
            );
            text::draw_left(c, title_r, &hit.title, CALLOUT, Weight::Medium, fg);
            text::draw_right(
                c,
                Rect::new(row.x, row.y, row.w - 12, row.h),
                &hit.sub,
                FOOTNOTE,
                Weight::Regular,
                fg2,
            );
        }
    }

    fn search_key(&mut self, key: Key) {
        match key {
            Key::Esc => self.close_search(),
            Key::Enter => {
                let hit = self
                    .shell
                    .search
                    .as_ref()
                    .and_then(|s| s.hits.get(s.sel).cloned());
                self.close_search();
                if let Some(h) = hit {
                    self.activate_hit(h);
                }
            }
            Key::Backspace => {
                if let Some(s) = self.shell.search.as_mut() {
                    s.query.pop();
                }
                self.search_refresh();
            }
            Key::Up | Key::Down | Key::Tab => {
                if let Some(s) = self.shell.search.as_mut()
                    && !s.hits.is_empty()
                {
                    let n = s.hits.len();
                    s.sel = if key == Key::Up {
                        (s.sel + n - 1) % n
                    } else {
                        (s.sel + 1) % n
                    };
                }
                self.force_full = true;
            }
            Key::Char(b) if (0x20..0x7F).contains(&b) || b >= 0xA0 => {
                if let Some(s) = self.shell.search.as_mut()
                    && s.query.len() < 60
                {
                    s.query.push(char::from(b));
                }
                self.search_refresh();
            }
            _ => {}
        }
    }

    // ---- confirmation sheet ----

    fn draw_dialog(&self, c: &mut Canvas, d: &Dialog) {
        let p = theme::pal();
        let fade = level(&d.t);
        let full = Rect::new(0, 0, self.sw, self.sh);
        c.blend_rect(full, Color::rgb(0, 0, 0), (110 * fade / 256) as u16);
        let (r, cancel, ok) = sheet_geom(self.sw, self.sh);
        let r = Rect::new(r.x, r.y - ((256 - fade) as i32 * 10) / 256, r.w, r.h);
        let dy = r.y - sheet_geom(self.sw, self.sh).0.y;
        let hole = Rect::new(r.x, r.y + R_PANEL, r.w, r.h - 2 * R_PANEL);
        c.draw_shadow(
            r,
            Shadow {
                blur: 28,
                dy: 16,
                alpha: (110 * fade / 256).min(255),
            },
            hole,
        );
        c.fill_rrect(
            r,
            R_PANEL,
            Corner::Circle,
            theme::solid(p.window_bg),
            fade as u16,
        );
        ui::stroke_token(c, r, R_PANEL, p.separator);
        if fade < 150 {
            return;
        }
        c.blit_surface(icons::surface(Icon::Brand, 56), r.x + 24, r.y + 22, 256);
        text::draw(
            c,
            r.x + 96,
            r.y + 26,
            &d.title,
            TITLE3,
            Weight::Semibold,
            theme::solid(p.text),
        );
        for (i, (a, b)) in text::wrap(&d.body, BODY, Weight::Regular, r.w - 96 - 24, 3)
            .into_iter()
            .enumerate()
        {
            text::draw(
                c,
                r.x + 96,
                r.y + 54 + i as i32 * 19,
                &d.body[a..b],
                BODY,
                Weight::Regular,
                theme::solid(p.text_secondary),
            );
        }
        let st = |i: usize| {
            if d.focus == i {
                ui::Control::Hover
            } else {
                ui::Control::Normal
            }
        };
        let off = |rc: Rect| Rect::new(rc.x, rc.y + dy, rc.w, rc.h);
        ui::push_button(
            c,
            off(cancel),
            kitsune_core::t!("common.cancel"),
            ui::ButtonKind::Secondary,
            st(0),
        );
        ui::push_button(
            c,
            off(ok),
            &d.ok,
            if d.cmd == Cmd::Shutdown {
                ui::ButtonKind::Destructive
            } else {
                ui::ButtonKind::Primary
            },
            st(1),
        );
    }

    fn dialog_key(&mut self, key: Key) {
        match key {
            Key::Esc => self.close_dialog(),
            Key::Left | Key::Right | Key::Tab => {
                if let Some(d) = self.shell.dialog.as_mut() {
                    d.focus = 1 - d.focus;
                }
            }
            Key::Enter => {
                let go = self.shell.dialog.as_ref().map(|d| (d.focus == 1, d.cmd));
                self.close_dialog();
                if let Some((true, cmd)) = go {
                    self.power_now(cmd);
                }
            }
            _ => {}
        }
        self.force_full = true;
    }

    // ---- drawing entry: one function per shell layer (see `compositor/layers.rs`) ----

    pub(crate) fn draw_apps_layer(&self, c: &mut Canvas) {
        if let Some(a) = &self.shell.apps {
            self.draw_apps(c, a);
        }
    }

    pub(crate) fn draw_search_layer(&self, c: &mut Canvas) {
        if let Some(s) = &self.shell.search {
            self.draw_search(c, s);
        }
    }

    pub(crate) fn draw_switcher_layer(&self, c: &mut Canvas) {
        if let Some(sw) = &self.switcher {
            self.draw_switcher(c, sw);
        }
    }

    pub(crate) fn draw_dialog_layer(&self, c: &mut Canvas) {
        if let Some(d) = &self.shell.dialog {
            self.draw_dialog(c, d);
        }
    }

    // ---- pointer ----

    /// The pointer moved to `(x, y)`: update hover states of the shell layers.
    pub(crate) fn shell_pointer(&mut self, x: i32, y: i32) -> bool {
        let mut changed = false;
        // Panel item under the pointer.
        let item = self.panel_item_at(x, y).map(|(i, _)| i);
        if item != self.shell.panel_hover {
            self.shell.panel_hover = item;
            self.mark_dirty(self.panel_rect());
            changed = true;
        }
        // (The hover washes inside a popover repaint with the overlay on every pointer move.)
        if let Some(m) = self.shell.menu.as_mut() {
            let hover = kitsune_core::chrome::menu_row_at(&m.geom, &m.rows, x, y);
            if hover != m.hover {
                m.hover = hover;
            }
        }
        if self.shell.apps.is_some() {
            let (hit, rail) = {
                let a = self.shell.apps.as_ref().expect("checked");
                let g = self.apps_geom(a);
                (
                    launcher_cell_at(&g, self.sh, x, y),
                    chrome::launcher_rail_at(&g, x, y),
                )
            };
            self.apps_hover_to(hit);
            if let Some(a) = self.shell.apps.as_mut()
                && a.rail_hover != rail
            {
                a.rail_hover = rail;
                self.apps_dirty_all();
            }
        }
        if let Some(s) = self.shell.search.as_mut() {
            let g = spotlight_geom(self.sw, self.sh, s.hits.len());
            if let Some(i) = g.rows.iter().position(|r| r.contains(x, y))
                && i != s.sel
            {
                s.sel = i;
            }
        }
        if let Some(d) = self.shell.dialog.as_mut() {
            let (_, cancel, ok) = sheet_geom(self.sw, self.sh);
            if cancel.contains(x, y) {
                d.focus = 0;
            } else if ok.contains(x, y) {
                d.focus = 1;
            }
        }
        changed
    }

    /// A left press while a shell layer is up. Returns `true` when the layers consumed it.
    pub(crate) fn shell_click(&mut self, x: i32, y: i32) -> bool {
        // Sheet: modal.
        if let Some(d) = self.shell.dialog.as_ref().filter(|d| !d.closing) {
            let (_, cancel, ok) = sheet_geom(self.sw, self.sh);
            let cmd = d.cmd;
            if ok.contains(x, y) {
                self.close_dialog();
                self.power_now(cmd);
            } else if cancel.contains(x, y) {
                self.close_dialog();
            }
            return true;
        }
        if self.shell.apps.as_ref().is_some_and(|a| !a.closing) {
            enum Pick {
                Launch(Target),
                Rail(usize),
                Nothing,
            }
            let pick = {
                let a = self.shell.apps.as_ref().expect("checked");
                let g = self.apps_geom(a);
                if g.field.contains(x, y)
                    || g.rail.contains(x, y) && chrome::launcher_rail_at(&g, x, y).is_none()
                {
                    Pick::Nothing
                } else if let Some(i) = chrome::launcher_rail_at(&g, x, y) {
                    Pick::Rail(i)
                } else if let Some(i) = chrome::launcher_recent_at(&g, x, y) {
                    self.recent_tiles(a)
                        .get(i)
                        .and_then(|&ti| a.tiles.get(ti))
                        .map_or(Pick::Nothing, |t| Pick::Launch(t.target))
                } else {
                    launcher_cell_at(&g, self.sh, x, y)
                        .and_then(|pos| a.shown.get(pos).and_then(|&i| a.tiles.get(i)))
                        .map_or(Pick::Nothing, |t| Pick::Launch(t.target))
                }
            };
            if y < PANEL_H && self.panel_item_at(x, y).is_some() {
                self.close_apps();
                return false;
            }
            match pick {
                Pick::Rail(i) => self.apps_set_category(i),
                Pick::Launch(t) => {
                    self.close_apps();
                    self.launch_target(t);
                }
                // The field, the rail's padding: stay; a click on empty space closes.
                Pick::Nothing => {
                    let inside = {
                        let a = self.shell.apps.as_ref().expect("checked");
                        let g = self.apps_geom(a);
                        g.field.contains(x, y) || g.rail.contains(x, y)
                    };
                    if !inside {
                        self.close_apps();
                    }
                }
            }
            return true;
        }
        if self.shell.search.as_ref().is_some_and(|s| !s.closing) {
            let (hit, inside) = {
                let s = self.shell.search.as_ref().expect("checked");
                let g = spotlight_geom(self.sw, self.sh, s.hits.len());
                (
                    g.rows
                        .iter()
                        .position(|r| r.contains(x, y))
                        .and_then(|i| s.hits.get(i).cloned()),
                    g.panel.contains(x, y),
                )
            };
            if !inside {
                self.close_search();
                return y >= PANEL_H;
            }
            if let Some(h) = hit {
                self.close_search();
                self.activate_hit(h);
            }
            return true;
        }
        // Menu.
        if self.shell.menu.as_ref().is_some_and(|m| !m.closing) {
            let (row, inside) = {
                let m = self.shell.menu.as_ref().expect("checked");
                (
                    kitsune_core::chrome::menu_row_at(&m.geom, &m.rows, x, y),
                    m.geom.rect.contains(x, y),
                )
            };
            if let Some(i) = row {
                self.menu_pick(i);
                return true;
            }
            if inside {
                return true;
            }
            // A click on a panel item closes the menu and acts on the item.
            if let Some((item, _)) = self.panel_item_at(x, y) {
                self.close_transients();
                self.panel_click(item, x, y);
                return true;
            }
            self.close_transients();
            return true;
        }
        // Popover.
        if self.shell.pop.as_ref().is_some_and(|p| !p.closing) {
            if self.popover_click(x, y) {
                return true;
            }
            let was = self.shell.pop.as_ref().map(|p| p.kind);
            self.close_transients();
            if let Some((item, _)) = self.panel_item_at(x, y) {
                let same = matches!(
                    (was, item),
                    (Some(PopKind::Quick), PanelItem::Tray)
                        | (Some(PopKind::Centre), PanelItem::Clock)
                );
                if !same {
                    self.panel_click(item, x, y);
                }
                return true;
            }
            return false;
        }
        // The panel.
        if let Some((item, _)) = self.panel_item_at(x, y) {
            self.panel_click(item, x, y);
            return true;
        }
        false
    }

    /// A left click on panel item `item`.
    pub(crate) fn panel_click(&mut self, item: PanelItem, x: i32, y: i32) {
        match item {
            PanelItem::Workspaces => {
                let n = self.wm.visible_workspaces();
                let cur = self.wm.workspace();
                if let Some((_, r)) = self.panel_items().into_iter().find(|(i, _)| *i == item)
                    && let Some(ws) = chrome::workspace_at(r, n, cur, x, y)
                {
                    self.go_workspace(ws);
                }
            }
            PanelItem::Apps => self.open_apps(),
            PanelItem::Search => self.open_search(),
            PanelItem::Tray => self.open_popover(PopKind::Quick),
            PanelItem::Clock => self.open_popover(PopKind::Centre),
        }
        self.force_full = true;
    }

    /// A right click on the Apps button: the system menu.
    pub(crate) fn panel_context(&mut self, item: PanelItem) {
        if item != PanelItem::Apps {
            return;
        }
        let Some((_, rect)) = self.panel_items().into_iter().find(|(i, _)| *i == item) else {
            return;
        };
        let entries = self.system_menu();
        self.open_menu(MenuOrigin::Context, entries, (rect.x, PANEL_H));
    }

    /// A wheel step while Apps is up: scroll its grid.
    pub(crate) fn shell_wheel(&mut self, dz: i32) -> bool {
        let Some(a) = self.shell.apps.as_ref() else {
            return false;
        };
        let g = self.apps_geom(a);
        let max = g.total_rows.saturating_sub(g.visible_rows) as i32;
        let next = (a.scroll as i32 + dz.signum()).clamp(0, max) as usize;
        let changed = next != a.scroll;
        if let Some(a) = self.shell.apps.as_mut() {
            a.scroll = next;
        }
        if changed {
            self.apps_dirty_all();
        }
        changed
    }

    // ---- keys ----

    /// A key while a shell layer is up. Returns `true` when consumed.
    pub(crate) fn shell_key(&mut self, key: Key) -> bool {
        if self.shell.dialog.as_ref().is_some_and(|d| !d.closing) {
            self.dialog_key(key);
            return true;
        }
        if self.shell.apps.as_ref().is_some_and(|a| !a.closing) {
            self.apps_key(key);
            return true;
        }
        if self.shell.search.as_ref().is_some_and(|s| !s.closing) {
            self.search_key(key);
            return true;
        }
        if let Some(m) = self.shell.menu.as_ref().filter(|m| !m.closing) {
            let n = m.entries.len();
            let cur = m.hover;
            match key {
                Key::Esc => self.close_transients(),
                Key::Up | Key::Down if n > 0 => {
                    let mut i = cur.unwrap_or(if key == Key::Down { n - 1 } else { 0 });
                    for _ in 0..n {
                        i = if key == Key::Down {
                            (i + 1) % n
                        } else {
                            (i + n - 1) % n
                        };
                        if m.entries[i].cmd != Cmd::Sep && m.entries[i].enabled {
                            break;
                        }
                    }
                    if let Some(m) = self.shell.menu.as_mut() {
                        m.hover = Some(i);
                    }
                    self.force_full = true;
                }
                Key::Enter => {
                    if let Some(i) = cur {
                        self.menu_pick(i);
                    }
                }
                _ => {}
            }
            return true;
        }
        if self.shell.pop.as_ref().is_some_and(|p| !p.closing) {
            if key == Key::Esc {
                self.close_transients();
            }
            return true;
        }
        false
    }

    /// Context menu on the empty desktop.
    pub(crate) fn desktop_context(&mut self, x: i32, y: i32) {
        let entries = alloc::vec![
            Entry::item(kitsune_core::t!("menu.view.show_apps"), "", Cmd::ShowApps),
            Entry::item(
                kitsune_core::t!("menu.view.search"),
                kitsune_core::t!("menu.shortcut.search"),
                Cmd::ShowSearch,
            ),
            Entry::item(
                kitsune_core::t!("menu.desktop.show_desktop"),
                "Ctrl+Alt+D",
                Cmd::ShowDesktop,
            ),
            Entry::sep(),
            Entry::item(
                &kitsune_core::t!("common.open_app", app = Kind::Files.label()),
                "",
                Cmd::Launch(Kind::Files),
            ),
            Entry::item(
                &kitsune_core::t!("common.open_app", app = Kind::Terminal.label()),
                "",
                Cmd::Launch(Kind::Terminal),
            ),
            Entry::sep(),
            Entry::item(kitsune_core::t!("menu.system.settings"), "", Cmd::Settings),
        ];
        self.open_menu(MenuOrigin::Context, entries, (x, y));
    }
}

/// Draw `label` left-aligned in `r`, cut with an ellipsis if it does not fit.
fn draw_fit(c: &mut Canvas, r: Rect, label: &str, px: u16, w: Weight, col: Color) {
    let t = text::ellipsize(label, px, w, r.w);
    text::draw_centered(c, r, &t, px, w, col);
}

fn pack_rgb(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// Walk the volume breadth-first and collect up to [`INDEX_MAX`] entries for Busca.
fn index_files() -> Vec<(Vec<u8>, String)> {
    let mut out: Vec<(Vec<u8>, String)> = Vec::new();
    let mut queue: Vec<(Vec<u8>, usize)> = alloc::vec![(alloc::vec![b'/'], 0)];
    let mut i = 0;
    while i < queue.len() && out.len() < INDEX_MAX {
        let (dir, depth) = queue[i].clone();
        i += 1;
        let Ok(list) = vfs::list(&dir) else {
            continue;
        };
        for e in list {
            let path = vfs::join(&dir, &e.name);
            let name = String::from_utf8_lossy(&e.name).into_owned();
            if e.name.first() == Some(&b'.') {
                continue;
            }
            if e.kind == vfs::EntryKind::Dir && depth + 1 < INDEX_DEPTH {
                queue.push((path.clone(), depth + 1));
            }
            out.push((path, name));
            if out.len() >= INDEX_MAX {
                break;
            }
        }
    }
    out
}
