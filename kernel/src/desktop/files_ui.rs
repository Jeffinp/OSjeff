//! `Desktop::draw_files`: the file manager window's rendering — a disk panel
//! (boot + filesystem, with HD/SSD), the Files/Trash tabs, and the file list.
//! The manager's logic (navigation, trash/restore/purge) lives in `mod.rs`.

use super::*;

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

impl Desktop {
    /// File manager: a disk panel (boot + FS, with HD/SSD), a Files/Trash tab,
    /// and the file list with the current selection highlighted.
    pub(crate) fn draw_files(&self, c: &mut Canvas, r: Rect) {
        let pad = 14i32;
        let cx = r.x + pad;
        let cw = r.w - pad * 2;
        let right = r.right() - pad;
        let mut cy = r.y + TITLE_H + 12;

        // ---- disk panel: where the OS lives + HD/SSD per disk ----
        c.fill_round_rect(cx as usize, cy as usize, cw as usize, 66, 8, theme::HEADER_DIM);
        font::draw_text(c, (cx + 12) as usize, (cy + 8) as usize, "Discos", theme::TEXT_MUTED, 2);
        let labels: [&[u8]; 2] = [b"boot", b"FS"];
        for (i, d) in self.disks.iter().enumerate() {
            let ly = cy + 30 + i as i32 * 18;
            let mut line = [0u8; 96];
            let mut p = push(&mut line, 0, labels[i]);
            p = push(&mut line, p, b": ");
            match d {
                Some(d) => {
                    p = push(&mut line, p, &d.model[..d.model_len]);
                    p = push(&mut line, p, b"  ");
                    p = push_num(&mut line, p, d.mib() as u32);
                    p = push(&mut line, p, b" MiB  ");
                    p = push(
                        &mut line,
                        p,
                        if d.ssd {
                            b"SSD" as &[u8]
                        } else if d.rpm > 0 {
                            b"HD"
                        } else {
                            b"HD?"
                        },
                    );
                }
                None => p = push(&mut line, p, b"ausente"),
            }
            font::draw_bytes(c, (cx + 12) as usize, ly as usize, &line[..p], theme::TEXT, 2);
        }
        cy += 78;

        // ---- Files / Trash tabs ----
        let img = disk();
        let counts = [fs::count_active(img) as u32, fs::count_trashed(img) as u32];
        let tab_labels: [&[u8]; 2] = [b"Arquivos", b"Lixeira"];
        for v in 0..2usize {
            let tx = cx + v as i32 * 170;
            let mut line = [0u8; 24];
            let mut p = push(&mut line, 0, tab_labels[v]);
            p = push(&mut line, p, b" (");
            p = push_num(&mut line, p, counts[v]);
            p = push(&mut line, p, b")");
            let active = self.files_view as usize == v;
            let col = if active { theme::WHITE } else { theme::TEXT_MUTED };
            font::draw_bytes(c, tx as usize, cy as usize, &line[..p], col, 2);
            if active {
                let w = font::text_width(core::str::from_utf8(&line[..p]).unwrap_or(""), 2);
                c.fill_round_rect(tx as usize, (cy + 22) as usize, w, 3, 1, theme::ACCENT);
            }
        }
        cy += 36;

        // ---- file list ----
        let rows = self.files_rows();
        if rows == 0 {
            font::draw_text(c, (cx + 6) as usize, (cy + 6) as usize, "(vazio)", theme::TEXT_MUTED, 2);
        }
        for n in 0..rows {
            let ry = cy + n as i32 * 22;
            if ry + 20 > r.bottom() - 26 {
                break;
            }
            if self.files_sel == n {
                c.fill_round_rect_alpha(cx as usize, (ry - 2) as usize, cw as usize, 22, 6, theme::ACCENT, 40);
            }
            let Some(slot) = self.files_slot(n) else { break };
            font::draw_bytes(c, (cx + 8) as usize, (ry + 2) as usize, fs::name_at(img, slot), theme::TEXT, 2);
            let mut sz = [0u8; 12];
            let q = push_num(&mut sz, 0, fs::size_at(img, slot) as u32);
            let q2 = push(&mut sz, q, b" B");
            let sw = font::text_width(core::str::from_utf8(&sz[..q2]).unwrap_or(""), 2) as i32;
            font::draw_bytes(c, (right - 6 - sw) as usize, (ry + 2) as usize, &sz[..q2], theme::TEXT_MUTED, 2);
        }

        // ---- help ----
        let help = if self.files_view == 0 {
            "Setas mover  Tab lixeira  Del apagar  Enter abrir"
        } else {
            "Setas mover  Tab arquivos  Del apagar def.  Enter restaurar"
        };
        font::draw_text(c, cx as usize, (r.bottom() - 20) as usize, help, theme::TEXT_MUTED, 2);
    }
}
