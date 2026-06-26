//! `Desktop::draw_files`: the file manager window's rendering — a macOS
//! Finder-style layout: a dark sidebar (Favoritos + Discos) on the left and a
//! main pane that shows the file list, the trash, or a disk's details. The
//! manager's logic (navigation, trash/restore/purge) lives in `mod.rs`.

use super::*;

const SBW: i32 = 176; // sidebar width
const ROW: i32 = 30; // list / sidebar row height
const BLUE: Color = Color::rgb(0x0A, 0x84, 0xFF); // macOS selection blue
const SIDEBAR: Color = Color::rgb(0x18, 0x1D, 0x27);
const FG: Color = Color::rgb(0xE6, 0xEA, 0xF2);
const MUTED: Color = Color::rgb(0x86, 0x90, 0xA4);
const SEP: Color = Color::rgb(0x2A, 0x31, 0x40);

/// Append `src` to `buf` at `off`, clamped to the buffer; returns the new offset.
fn push(buf: &mut [u8], off: usize, src: &[u8]) -> usize {
    let n = src.len().min(buf.len().saturating_sub(off));
    buf[off..off + n].copy_from_slice(&src[..n]);
    off + n
}

/// Append the decimal digits of `v` to `buf` at `off`.
fn push_num(buf: &mut [u8], off: usize, mut v: u32) -> usize {
    if v == 0 {
        return push(buf, off, b"0");
    }
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    push(buf, off, &tmp[i..])
}

/// Small sidebar/list glyph by id: 0 folder, 1 trash, 2 disk, 3 document.
fn glyph(c: &mut Canvas, id: u8, x: i32, y: i32, s: i32, col: Color) {
    let (x, y, s) = (x as usize, y as usize, s as usize);
    match id {
        0 => {
            // folder: tab + body
            c.fill_round_rect(x, y + 1, s / 2, s / 4, 2, col);
            c.fill_round_rect(x, y + s / 5, s, s - s / 5, 2, col);
        }
        1 => {
            // trash: lid + can
            c.fill_rect(x, y + s / 6, s, s / 12 + 1, col);
            c.fill_round_rect(x + s / 8, y + s / 4, s - s / 4, s - s / 3, 2, col);
        }
        2 => {
            // disk: rounded square + spindle hole
            c.fill_round_rect(x, y, s, s, s / 4, col);
            c.fill_round_rect(x + s / 2 - s / 8, y + s / 2 - s / 8, s / 4, s / 4, s / 8, SIDEBAR);
        }
        _ => {
            // document: sheet + a couple of lines
            c.fill_round_rect(x + s / 6, y, s - s / 3, s, 2, col);
            let lc = Color::rgb(0x6A, 0x74, 0x88);
            c.fill_rect(x + s / 4, y + s / 3, s / 2, 1, lc);
            c.fill_rect(x + s / 4, y + s / 2, s / 2, 1, lc);
        }
    }
}

impl Desktop {
    pub(crate) fn draw_files(&self, c: &mut Canvas, r: Rect) {
        let cy0 = r.y + TITLE_H;

        // ---- sidebar ----
        c.fill_rect(
            r.x.max(0) as usize,
            cy0.max(0) as usize,
            SBW as usize,
            (r.bottom() - cy0).max(0) as usize,
            SIDEBAR,
        );
        let sx = r.x + 14;
        let mut yy = cy0 + 12;
        let items: [(&[u8], u8, u8); 4] = [
            (b"Arquivos", 0, 0),
            (b"Lixeira", 1, 1),
            (b"boot", 2, 2),
            (b"FS", 3, 2),
        ];
        font::draw_bytes(c, sx as usize, yy as usize, b"Favoritos", MUTED, 2);
        yy += 22;
        for it in &items[..2] {
            yy = self.sb_item(c, sx, yy, *it);
        }
        yy += 12;
        font::draw_bytes(c, sx as usize, yy as usize, b"Discos", MUTED, 2);
        yy += 22;
        for it in &items[2..] {
            yy = self.sb_item(c, sx, yy, *it);
        }

        // ---- main pane ----
        let mx = r.x + SBW + 16;
        let mw = r.right() - 16 - mx;
        match self.files_view {
            0 | 1 => self.draw_file_list(c, r, mx, mw),
            v => self.draw_disk_panel(c, r, mx, (v - 2) as usize),
        }
    }

    /// Draw one sidebar row; returns the next y.
    fn sb_item(&self, c: &mut Canvas, sx: i32, y: i32, item: (&[u8], u8, u8)) -> i32 {
        let (label, view, gid) = item;
        let selected = self.files_view == view;
        if selected {
            c.fill_round_rect(
                (sx - 6) as usize,
                y as usize,
                (SBW - 16) as usize,
                ROW as usize,
                7,
                BLUE,
            );
        }
        let tc = if selected { theme::WHITE } else { FG };
        let gc = if selected { theme::WHITE } else { BLUE };
        glyph(c, gid, sx + 2, y + (ROW - 16) / 2, 16, gc);
        font::draw_bytes(c, (sx + 26) as usize, (y + (ROW - 14) / 2) as usize, label, tc, 2);
        y + ROW + 2
    }

    /// Main pane: the Files or Trash list.
    fn draw_file_list(&self, c: &mut Canvas, r: Rect, mx: i32, mw: i32) {
        let right = mx + mw;
        let mut my = r.y + TITLE_H + 14;
        if self.files_view == 1 {
            font::draw_text(c, mx as usize, my as usize, "Lixeira", FG, 3);
        } else {
            // Breadcrumb: the current folder name (or "Arquivos" at the root).
            let cwd = self.files_cwd();
            if cwd == fs::ROOT {
                font::draw_text(c, mx as usize, my as usize, "Arquivos", FG, 3);
            } else {
                let name = fs::name_at(disk(), cwd as usize);
                let nw = name.len() as i32 * font::cell_w(3) as i32;
                font::draw_text(c, mx as usize, my as usize, "Arquivos / ", MUTED, 2);
                let off = font::text_width("Arquivos / ", 2) as i32;
                font::draw_bytes(c, (mx + off) as usize, my as usize, name, FG, 2);
                let _ = nw;
            }
        }
        my += 36;
        font::draw_text(c, mx as usize, my as usize, "Nome", MUTED, 2);
        let th = font::text_width("Tamanho", 2) as i32;
        font::draw_text(c, (right - th) as usize, my as usize, "Tamanho", MUTED, 2);
        my += 20;
        c.fill_rect(mx as usize, my as usize, mw as usize, 1, SEP);
        my += 8;

        let rows = self.files_rows();
        if rows == 0 {
            font::draw_text(c, mx as usize, (my + 6) as usize, "(vazio)", MUTED, 2);
        }
        for n in 0..rows {
            let ry = my + n as i32 * ROW;
            if ry + ROW > r.bottom() - 34 {
                break;
            }
            let sel = self.files_sel == n;
            if sel {
                c.fill_round_rect((mx - 6) as usize, (ry - 1) as usize, (mw + 12) as usize, (ROW - 2) as usize, 6, BLUE);
            }
            let Some(slot) = self.files_slot(n) else { break };
            let img = disk();
            let tc = if sel { theme::WHITE } else { FG };
            let is_dir = fs::is_dir(img, slot);
            let gcol = if sel {
                theme::WHITE
            } else if is_dir {
                BLUE
            } else {
                MUTED
            };
            glyph(c, if is_dir { 0 } else { 3 }, mx, ry + (ROW - 18) / 2, 18, gcol);
            font::draw_bytes(c, (mx + 26) as usize, (ry + (ROW - 14) / 2) as usize, fs::name_at(img, slot), tc, 2);
            let mut sz = [0u8; 12];
            let q = push_num(&mut sz, 0, fs::size_at(img, slot) as u32);
            let q2 = push(&mut sz, q, b" B");
            let sw = font::text_width(core::str::from_utf8(&sz[..q2]).unwrap_or(""), 2) as i32;
            let szc = if sel { theme::WHITE } else { MUTED };
            font::draw_bytes(c, (right - sw) as usize, (ry + (ROW - 14) / 2) as usize, &sz[..q2], szc, 2);
        }

        self.files_footer(c, r, mx, rows);
    }

    /// Main pane: a single disk's details (Finder's "Get Info" feel).
    fn draw_disk_panel(&self, c: &mut Canvas, r: Rect, mx: i32, idx: usize) {
        let mut my = r.y + TITLE_H + 14;
        glyph(c, 2, mx, my, 56, BLUE);
        let tx = mx + 72;
        let d = self.disks.get(idx).copied().flatten();
        match d {
            Some(d) => font::draw_bytes(c, tx as usize, (my + 6) as usize, &d.model[..d.model_len], FG, 3),
            None => font::draw_text(c, tx as usize, (my + 6) as usize, "Disco ausente", FG, 3),
        }
        let role: &str = if idx == 0 {
            "Disco de inicializacao - onde o OSjeff esta instalado"
        } else {
            "Disco do sistema de arquivos"
        };
        font::draw_text(c, tx as usize, (my + 34) as usize, role, MUTED, 2);
        my += 80;
        c.fill_rect(mx as usize, my as usize, (r.right() - 16 - mx) as usize, 1, SEP);
        my += 16;

        if let Some(d) = d {
            let kind: &str = if d.ssd {
                "SSD (memoria nao-rotacional)"
            } else if d.rpm > 0 {
                "HD (disco rotacional)"
            } else {
                "HD / nao reportado"
            };
            self.disk_row(c, mx, &mut my, "Tipo", kind.as_bytes());
            let mut cap = [0u8; 16];
            let p = push_num(&mut cap, 0, d.mib() as u32);
            let p = push(&mut cap, p, b" MiB");
            self.disk_row(c, mx, &mut my, "Capacidade", &cap[..p]);
            let mut sec = [0u8; 16];
            let p = push_num(&mut sec, 0, d.sectors as u32);
            self.disk_row(c, mx, &mut my, "Setores", &sec[..p]);
            if d.rpm > 0 {
                let mut rp = [0u8; 16];
                let p = push_num(&mut rp, 0, d.rpm as u32);
                let p = push(&mut rp, p, b" RPM");
                self.disk_row(c, mx, &mut my, "Rotacao", &rp[..p]);
            }
        }
        self.files_footer(c, r, mx, 0);
    }

    fn disk_row(&self, c: &mut Canvas, mx: i32, my: &mut i32, key: &str, val: &[u8]) {
        font::draw_text(c, mx as usize, *my as usize, key, MUTED, 2);
        font::draw_bytes(c, (mx + 140) as usize, *my as usize, val, FG, 2);
        *my += 26;
    }

    /// Bottom status bar: item count + key hints.
    fn files_footer(&self, c: &mut Canvas, r: Rect, mx: i32, rows: usize) {
        let by = r.bottom() - 26;
        c.fill_rect((r.x + 1) as usize, (by - 8) as usize, (r.w - 2) as usize, 1, SEP);
        if self.files_view <= 1 {
            let mut s = [0u8; 16];
            let p = push_num(&mut s, 0, rows as u32);
            let p = push(&mut s, p, b" itens");
            font::draw_bytes(c, mx as usize, by as usize, &s[..p], MUTED, 2);
        }
        let hint = "N pasta  Bksp volta  Del apaga";
        let hw = font::text_width(hint, 2) as i32;
        font::draw_text(c, (r.right() - 16 - hw) as usize, by as usize, hint, MUTED, 2);
    }
}
