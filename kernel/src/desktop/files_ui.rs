//! `Desktop::draw_files`: the file manager window. Pure drawing: the state is
//! `FilesState` (rows already loaded: no disk access happens while painting) and
//! the geometry is `osjeff_core::fileman::Layout`, shared with the mouse handler.
//!
//! Light theme with dark text everywhere on the light surfaces (the old dark-on-
//! light mix had unreadable rows); selected rows are white on blue.

use super::files::{MENU_ROW_H, MENU_W_FILES, menu_height};
use super::*;
use osjeff_core::fileman::{self, CELL, Layout, Place, ROW_H, SortKey};

const BLUE: Color = Color::rgb(0x25, 0x63, 0xEB);
const TOOLBAR: Color = Color::rgb(0xE9, 0xED, 0xF5);
const SIDEBAR: Color = Color::rgb(0xE2, 0xE7, 0xF1);
const HEADER_BG: Color = Color::rgb(0xEE, 0xF1, 0xF8);
const ZEBRA: Color = Color::rgb(0xEF, 0xF2, 0xF9);
const SEP: Color = Color::rgb(0xCB, 0xD2, 0xE0);
const BTN: Color = Color::rgb(0xFF, 0xFF, 0xFF);
const ERR: Color = Color::rgb(0xC0, 0x2B, 0x2B);
const OKC: Color = Color::rgb(0x1B, 0x7F, 0x5F);

fn text(c: &mut Canvas, x: i32, y: i32, s: &[u8], col: Color) {
    if x >= 0 && y >= 0 {
        font::draw_bytes(c, x as usize, y as usize, s, col, 2);
    }
}

fn rect(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, col: Color) {
    if w > 0 && h > 0 {
        c.fill_rect(
            x.max(0) as usize,
            y.max(0) as usize,
            w as usize,
            h as usize,
            col,
        );
    }
}

fn round(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, rad: i32, col: Color) {
    if w > 0 && h > 0 {
        c.fill_round_rect(
            x.max(0) as usize,
            y.max(0) as usize,
            w as usize,
            h as usize,
            rad.max(0) as usize,
            col,
        );
    }
}

/// Small glyph by id: 0 folder, 1 trash, 2 disk, 3 document, 4 apps (a 2x2 grid).
fn glyph(c: &mut Canvas, id: u8, x: i32, y: i32, s: i32, col: Color, bg: Color) {
    match id {
        0 => {
            round(c, x, y + 1, s / 2, s / 4 + 1, 2, col);
            round(c, x, y + s / 5, s, s - s / 5, 2, col);
        }
        1 => {
            rect(c, x, y + s / 6, s, s / 12 + 1, col);
            round(c, x + s / 8, y + s / 4, s - s / 4, s - s / 3, 2, col);
        }
        2 => {
            round(c, x, y, s, s, s / 4, col);
            round(
                c,
                x + s / 2 - s / 8,
                y + s / 2 - s / 8,
                s / 4,
                s / 4,
                s / 8,
                bg,
            );
        }
        4 => {
            let q = s / 2 - 1;
            for (dx, dy) in [(0, 0), (s - q, 0), (0, s - q), (s - q, s - q)] {
                round(c, x + dx, y + dy, q, q, 2, col);
            }
        }
        _ => {
            round(c, x + s / 6, y, s - s / 3, s, 2, col);
            rect(c, x + s / 4, y + s / 3, s / 2, 1, bg);
            rect(c, x + s / 4, y + s / 2, s / 2, 1, bg);
        }
    }
}

/// Fit `name` (UTF-8) in `cols` columns for the ASCII font.
fn shown(name: &[u8], cols: usize) -> Vec<u8> {
    fileman::ellipsize(&fileman::display_ascii(name), cols)
}

impl Desktop {
    pub(crate) fn draw_files(&self, c: &mut Canvas, r: Rect, st: &FilesState) {
        let lay = Layout::of(r);
        let v = &st.view;
        let vis = lay.visible_rows();

        // ---- toolbar ----
        rect(
            c,
            r.x + 1,
            r.y + TITLE_H,
            r.w - 2,
            fileman::TOOLBAR_H,
            TOOLBAR,
        );
        let nav = [
            (lay.back, b"<", v.history.can_back()),
            (lay.forward, b">", v.history.can_forward()),
            (lay.up, b"^", !vfs::is_root(&v.cwd)),
        ];
        for (b, label, on) in nav {
            round(c, b.x, b.y, b.w, b.h, 6, BTN);
            let col = if on { theme::TEXT } else { SEP };
            text(c, b.x + (b.w - CELL) / 2, b.y + (b.h - 14) / 2, label, col);
        }
        let ab = lay.address;
        round(c, ab.x, ab.y, ab.w, ab.h, 8, SEP);
        round(c, ab.x + 1, ab.y + 1, ab.w - 2, ab.h - 2, 7, BTN);
        let crumbs = fileman::breadcrumbs(&v.cwd);
        let labels: Vec<usize> = crumbs
            .iter()
            .map(|k| fileman::display_ascii(&k.label).len())
            .collect();
        let (spans, folded) = lay.crumb_spans(&labels);
        let ty = ab.y + (ab.h - 14) / 2;
        if folded {
            text(c, ab.x + 10, ty, b"...", theme::TEXT_MUTED);
        }
        let last = crumbs.len() - 1;
        for (n, &(i, x, _)) in spans.iter().enumerate() {
            let col = if i == last { theme::TEXT } else { BLUE };
            text(c, x, ty, &fileman::display_ascii(&crumbs[i].label), col);
            if n + 1 < spans.len() {
                let w = labels[i] as i32 * CELL;
                text(c, x + w + CELL, ty, b">", theme::TEXT_MUTED);
            }
        }

        // ---- sidebar ----
        rect(
            c,
            lay.sidebar.x,
            lay.sidebar.y,
            lay.sidebar.w,
            lay.sidebar.h,
            SIDEBAR,
        );
        text(
            c,
            lay.sidebar.x + 12,
            lay.sidebar.y + 6,
            b"Locais",
            theme::TEXT_MUTED,
        );
        let docs = v.cwd.starts_with(b"/Documentos");
        for (p, pr) in lay.places() {
            let (label, gid, active): (&[u8], u8, bool) = match p {
                Place::Root => (b"Raiz", 2, v.cwd == b"/"),
                Place::Documents => (b"Documentos", 0, docs),
                Place::Apps => (b"Apps", 4, v.in_apps()),
                Place::Trash => (b"Lixeira", 1, v.in_trash()),
                Place::Disk => (b"", 2, false),
            };
            if p == Place::Disk {
                self.draw_sidebar_disk(c, lay.sidebar.x, pr, st);
                continue;
            }
            if active {
                round(c, pr.x, pr.y, pr.w, pr.h, 7, BLUE);
            }
            let (tc, gc) = if active {
                (theme::WHITE, theme::WHITE)
            } else {
                (theme::TEXT, BLUE)
            };
            glyph(
                c,
                gid,
                pr.x + 8,
                pr.y + (pr.h - 16) / 2,
                16,
                gc,
                if active { BLUE } else { SIDEBAR },
            );
            text(c, pr.x + 32, pr.y + (pr.h - 14) / 2, label, tc);
        }

        // ---- column header ----
        let h = lay.header;
        rect(c, h.x, h.y, h.w, h.h, HEADER_BG);
        rect(c, h.x, h.bottom() - 1, h.w, 1, SEP);
        let date_title: &[u8] = if v.in_trash() {
            b"Apagado em"
        } else if v.in_apps() {
            b"Estado"
        } else {
            b"Modificado"
        };
        let cols: [(&[u8], i32, SortKey); 3] = [
            (b"Nome", lay.name_x, SortKey::Name),
            (b"Tamanho", lay.size_x + 8, SortKey::Size),
            (date_title, lay.date_x + 8, SortKey::Modified),
        ];
        for (title, x, key) in cols {
            text(c, x, h.y + (h.h - 14) / 2, title, theme::TEXT);
            if v.sort.key == key {
                let ax = x + title.len() as i32 * CELL + 4;
                text(
                    c,
                    ax,
                    h.y + (h.h - 14) / 2,
                    if v.sort.asc { b"^" } else { b"v" },
                    BLUE,
                );
            }
        }

        // ---- rows ----
        let l = lay.list;
        rect(c, l.x, l.y, l.w, l.h, theme::WINDOW_BODY);
        if v.rows.is_empty() {
            let msg: &[u8] = if v.in_trash() {
                b"(lixeira vazia)"
            } else if v.in_apps() {
                b"(nenhum app)"
            } else {
                b"(pasta vazia)"
            };
            text(c, lay.name_x, l.y + 10, msg, theme::TEXT_MUTED);
        }
        let name_cols = ((lay.size_x - lay.name_x - 8) / CELL).max(4) as usize;
        for i in v.scroll..v.rows.len().min(v.scroll + vis + 1) {
            let y = lay.row_y(i, v.scroll);
            if y + ROW_H > l.bottom() {
                break;
            }
            let row = &v.rows[i];
            let sel = v.sel.is_selected(i);
            let cut = !v.in_trash()
                && !v.in_apps()
                && self.pathclip.is_cut_path(&vfs::join(&v.cwd, &row.name));
            if sel {
                round(c, l.x + 4, y, l.w - 8, ROW_H - 2, 6, BLUE);
            } else if i % 2 == 1 {
                rect(c, l.x, y, l.w, ROW_H - 2, ZEBRA);
            }
            if !sel && i == v.sel.cursor() && st.input.is_none() && !v.rows.is_empty() {
                rect(c, l.x + 4, y, l.w - 8, 1, BLUE);
                rect(c, l.x + 4, y + ROW_H - 3, l.w - 8, 1, BLUE);
            }
            let (tc, mc) = if sel {
                (theme::WHITE, theme::WHITE)
            } else if cut {
                (SEP, SEP)
            } else {
                (theme::TEXT, theme::TEXT_MUTED)
            };
            let gcol = if sel {
                theme::WHITE
            } else if row.is_dir() {
                BLUE
            } else {
                theme::TEXT_MUTED
            };
            let bg = if sel { BLUE } else { theme::WINDOW_BODY };
            // Installed apps show their launcher icon, the rest the document glyph.
            let icon = if v.in_apps() && row.installed {
                self.apps
                    .iter()
                    .find(|a| a.id.as_bytes() == &row.id[..])
                    .and_then(|a| a.icon.as_deref())
            } else {
                None
            };
            match icon {
                Some(rgba) => c.draw_rgba(
                    rgba,
                    24,
                    24,
                    (lay.name_x - 30).max(0) as usize,
                    y.max(0) as usize,
                ),
                None => glyph(
                    c,
                    if v.in_apps() {
                        4
                    } else if row.is_dir() {
                        0
                    } else {
                        3
                    },
                    lay.name_x - 26,
                    y + (ROW_H - 18) / 2,
                    16,
                    gcol,
                    bg,
                ),
            }
            let ty = y + (ROW_H - 2 - 14) / 2;
            text(c, lay.name_x, ty, &shown(&row.name, name_cols), tc);
            let size: String = if row.is_dir() {
                String::from("--")
            } else {
                fileman::format_size(row.size)
            };
            let sx = lay.date_x - 8 - size.len() as i32 * CELL;
            text(c, sx, ty, size.as_bytes(), mc);
            if v.in_apps() {
                let status = fileman::apps::status_label(row.installed);
                let sc = if sel {
                    theme::WHITE
                } else if row.installed {
                    OKC
                } else {
                    theme::TEXT_MUTED
                };
                text(c, lay.date_x + 8, ty, status.as_bytes(), sc);
            } else {
                text(c, lay.date_x + 8, ty, files_local(row.mtime).as_bytes(), mc);
            }
        }

        // ---- scrollbar ----
        let sb = lay.scrollbar;
        rect(c, sb.x, sb.y, sb.w, sb.h, HEADER_BG);
        if v.rows.len() > vis {
            let (ty, th) = lay.thumb(v.scroll, v.rows.len());
            round(
                c,
                sb.x + 2,
                ty,
                sb.w - 4,
                th,
                4,
                Color::rgb(0x9A, 0xA5, 0xBD),
            );
        }

        // ---- status bar ----
        self.draw_files_status(c, &lay, st);

        // ---- overlays ----
        if let Some(edit) = &st.input {
            self.draw_name_box(c, r, edit);
        }
        if let Some(q) = &st.confirm {
            self.draw_confirm(c, r, q);
        }
        if let Some(lines) = &st.props {
            self.draw_props(c, r, lines);
        }
        if let Some(m) = &st.menu {
            self.draw_files_menu(c, m);
        }
    }

    fn draw_sidebar_disk(&self, c: &mut Canvas, sx: i32, pr: Rect, st: &FilesState) {
        let usage = st.usage;
        let label: &[u8] = if vfs::volume() == vfs::Volume::Memory {
            b"Memoria"
        } else {
            b"Disco"
        };
        text(c, sx + 12, pr.y - 22, b"Discos", theme::TEXT_MUTED);
        glyph(c, 2, pr.x + 8, pr.y + 2, 16, BLUE, SIDEBAR);
        text(c, pr.x + 32, pr.y + 2, label, theme::TEXT);
        let bar_x = pr.x + 8;
        let bar_w = pr.w - 16;
        round(c, bar_x, pr.y + 22, bar_w, 8, 4, SEP);
        let fill = bar_w * usage.used_permille() as i32 / 1000;
        if fill > 0 {
            let col = if usage.used_permille() > 900 {
                ERR
            } else {
                BLUE
            };
            round(c, bar_x, pr.y + 22, fill.max(8), 8, 4, col);
        }
        let s = alloc::format!("{} livres", fileman::format_size(usage.free));
        font::draw_bytes(
            c,
            bar_x.max(0) as usize,
            (pr.y + 36).max(0) as usize,
            s.as_bytes(),
            theme::TEXT_MUTED,
            1,
        );
    }

    fn draw_files_status(&self, c: &mut Canvas, lay: &Layout, st: &FilesState) {
        let s = lay.status;
        rect(c, s.x + 1, s.y, s.w - 2, s.h - 1, TOOLBAR);
        rect(c, s.x + 1, s.y, s.w - 2, 1, SEP);
        let ty = s.y + (s.h - 14) / 2;
        let cols = ((s.w - 24) / CELL).max(8) as usize;
        if let Some(job) = &st.job {
            // Progress: label, bar, percent. Esc cancels.
            let pm = job.copy.permille();
            let name = shown(job.copy.current_name(), 22);
            let left = alloc::format!("{} {}", job.label, String::from_utf8_lossy(&name));
            text(c, s.x + 12, ty, left.as_bytes(), theme::TEXT);
            let bw = (s.w / 3).max(80);
            let bx = s.right() - 12 - bw - 5 * CELL;
            round(c, bx, ty + 2, bw, 10, 5, SEP);
            round(c, bx, ty + 2, (bw * pm as i32 / 1000).max(10), 10, 5, BLUE);
            let pct = alloc::format!("{:>3}%", pm / 10);
            text(c, bx + bw + 8, ty, pct.as_bytes(), theme::TEXT);
            return;
        }
        let summary = st.view.summary();
        text(c, s.x + 12, ty, summary.as_bytes(), theme::TEXT_MUTED);
        if let Some((m, err)) = &st.msg {
            let x = s.x + 12 + (summary.len() as i32 + 3) * CELL;
            let room = ((s.right() - 12 - x) / CELL).max(0) as usize;
            let col = if *err { ERR } else { OKC };
            let mb = m.as_bytes();
            text(c, x, ty, &fileman::ellipsize(mb, room.min(cols)), col);
        } else if st.view.in_apps() {
            let x = s.x + 12 + (summary.len() as i32 + 3) * CELL;
            let room = ((s.right() - 12 - x) / CELL).max(0) as usize;
            let hint: &[u8] = b"Enter abre   I instala   Del remove";
            text(c, x, ty, &fileman::ellipsize(hint, room), theme::TEXT_MUTED);
        }
    }

    /// The name field (new file / new folder / rename), centered in the window.
    fn draw_name_box(&self, c: &mut Canvas, r: Rect, edit: &NameEdit) {
        let title: &[u8] = match edit.purpose {
            EditPurpose::NewFile => b"Novo arquivo",
            EditPurpose::NewFolder => b"Nova pasta",
            EditPurpose::Rename(_) => b"Renomear",
        };
        let w = (r.w - 80).clamp(260, 460);
        let h = 96;
        let x = r.x + (r.w - w) / 2;
        let y = r.y + (r.h - h) / 2;
        round(c, x - 3, y - 3, w + 6, h + 6, 12, SEP);
        round(c, x, y, w, h, 10, theme::WINDOW_BODY);
        text(c, x + 14, y + 10, title, theme::TEXT);
        let (bx, by, bw) = (x + 14, y + 34, w - 28);
        round(c, bx, by, bw, 26, 6, BLUE);
        round(c, bx + 2, by + 2, bw - 4, 22, 5, BTN);
        let cols = ((bw - 16) / CELL).max(4) as usize;
        let disp = fileman::display_ascii(edit.input.text());
        let caret = edit.input.caret_column();
        // Scroll the text so the caret stays inside the box.
        let start = (caret + 1).saturating_sub(cols);
        let end = disp.len().min(start + cols);
        text(
            c,
            bx + 8,
            by + 6,
            &disp[start.min(disp.len())..end],
            theme::TEXT,
        );
        rect(
            c,
            bx + 8 + ((caret - start) as i32) * CELL,
            by + 5,
            2,
            16,
            BLUE,
        );
        text(
            c,
            x + 14,
            y + 70,
            b"Enter confirma   Esc cancela",
            theme::TEXT_MUTED,
        );
    }

    fn draw_confirm(&self, c: &mut Canvas, r: Rect, q: &Confirm) {
        let (l1, n): (&str, usize) = match q {
            Confirm::Purge(p) => ("Excluir permanentemente?", p.len()),
            Confirm::PurgeTrash(p) => ("Excluir da lixeira para sempre?", p.len()),
            Confirm::EmptyTrash => ("Esvaziar a lixeira?", 0),
        };
        let w = (r.w - 80).clamp(280, 440);
        let h = 104;
        let x = r.x + (r.w - w) / 2;
        let y = r.y + (r.h - h) / 2;
        round(c, x - 3, y - 3, w + 6, h + 6, 12, ERR);
        round(c, x, y, w, h, 10, theme::WINDOW_BODY);
        text(c, x + 14, y + 12, l1.as_bytes(), theme::TEXT);
        let l2 = if n > 0 {
            alloc::format!("{n} item(ns). Nao ha como desfazer.")
        } else {
            String::from("Tudo sera apagado. Nao ha como desfazer.")
        };
        text(
            c,
            x + 14,
            y + 38,
            &fileman::ellipsize(l2.as_bytes(), ((w - 28) / CELL) as usize),
            theme::TEXT_MUTED,
        );
        text(
            c,
            x + 14,
            y + 74,
            b"Enter confirma   Esc cancela",
            theme::TEXT,
        );
    }

    fn draw_props(&self, c: &mut Canvas, r: Rect, lines: &[String]) {
        let w = (r.w - 60).clamp(280, 520);
        let h = 44 + lines.len() as i32 * 20;
        let x = r.x + (r.w - w) / 2;
        let y = r.y + (r.h - h).max(0) / 2;
        round(c, x - 3, y - 3, w + 6, h + 6, 12, SEP);
        round(c, x, y, w, h, 10, theme::WINDOW_BODY);
        text(c, x + 14, y + 10, b"Propriedades", BLUE);
        let cols = ((w - 28) / CELL) as usize;
        for (i, l) in lines.iter().enumerate() {
            text(
                c,
                x + 14,
                y + 34 + i as i32 * 20,
                &fileman::ellipsize(l.as_bytes(), cols),
                theme::TEXT,
            );
        }
    }

    fn draw_files_menu(&self, c: &mut Canvas, m: &CtxMenu) {
        let h = menu_height(m.items.len());
        round(
            c,
            m.x + 3,
            m.y + 5,
            MENU_W_FILES,
            h,
            10,
            Color::rgb(0xB5, 0xBC, 0xCB),
        );
        round(c, m.x, m.y, MENU_W_FILES, h, 9, theme::WINDOW_BODY);
        round(c, m.x, m.y, MENU_W_FILES, 1, 0, SEP);
        for (i, (cmd, label)) in m.items.iter().enumerate() {
            let y = m.y + 4 + i as i32 * MENU_ROW_H;
            let hover = self.cursor_x >= m.x
                && self.cursor_x < m.x + MENU_W_FILES
                && self.cursor_y >= y
                && self.cursor_y < y + MENU_ROW_H;
            if hover {
                round(c, m.x + 4, y + 1, MENU_W_FILES - 8, MENU_ROW_H - 2, 6, BLUE);
            }
            let col = if hover {
                theme::WHITE
            } else if matches!(
                cmd,
                fileman::Cmd::DeletePermanent | fileman::Cmd::EmptyTrash | fileman::Cmd::RemoveApp
            ) {
                ERR
            } else {
                theme::TEXT
            };
            text(
                c,
                m.x + 14,
                y + (MENU_ROW_H - 14) / 2,
                label.as_bytes(),
                col,
            );
        }
    }
}

fn files_local(t: u64) -> String {
    super::files::local_time(t)
}
