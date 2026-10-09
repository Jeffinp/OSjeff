//! Busca: apps, files and a calculator behind one field, and the file index it searches.

use super::dialog::R_PANEL;
use super::helpers::pack_rgb;
use crate::desktop::kit::glass::panel;
use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, CALLOUT, FOOTNOTE, TITLE2, Weight};
use kitsune_core::chrome::{self, spotlight_geom};
use kitsune_core::iconart::Glyph;
use kitsune_core::search;

/// Most files Busca indexes when it opens, and how deep it looks.
const INDEX_MAX: usize = 600;
const INDEX_DEPTH: usize = 5;

impl Desktop {
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

    pub(super) fn close_search(&mut self) {
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

    pub(super) fn activate_hit(&mut self, hit: SearchHit) {
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

    pub(super) fn draw_search(&self, c: &mut Canvas, s: &SearchView) {
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

    pub(super) fn search_key(&mut self, key: Key) {
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
