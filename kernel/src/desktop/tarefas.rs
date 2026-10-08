//! Tarefas: the process list window.

use super::*;

impl Desktop {
    /// Process list (scrolls to keep the selection visible), the real kernel
    /// threads and a footer pinned to the window's bottom edge.
    pub(crate) fn draw_taskmgr(&self, c: &mut Canvas, r: Rect) {
        let (x, y) = (r.x.max(0) as usize, r.y.max(0) as usize);
        let pad = 10usize;
        let line_h = 18usize;
        let tx = x + pad;
        let mut ty = y + TITLE_H as usize + 6;

        crate::text::legacy::draw_text(
            c,
            tx,
            ty,
            "PID NAME        ST   UP",
            theme::text_muted(),
            2,
        );
        ty += line_h + 2;

        // Rows that fit between the header and the thread block + footer.
        let threads = sched::thread_count();
        let apps = crate::wasm::statuses();
        let apps_h = if apps.is_empty() {
            0
        } else {
            6 + line_h + 2 + apps.len() * line_h
        };
        let reserved = 6 + line_h + 2 + threads * line_h + 30 + apps_h;
        let list_h = (r.h.max(0) as usize).saturating_sub(ty - y + reserved);
        let visible = (list_h / line_h).max(1);
        let n = self.procs.len();
        let sel = self.procs.selected();
        let first = (sel + 1)
            .saturating_sub(visible)
            .min(n.saturating_sub(visible));

        for i in first..(first + visible).min(n) {
            let p = match self.procs.at(i) {
                Some(p) => p,
                None => break,
            };
            if i == sel {
                c.fill_round_rect_alpha(
                    tx - 4,
                    ty - 2,
                    24 * crate::text::legacy::cell_w(2),
                    line_h,
                    4,
                    theme::accent(),
                    40,
                );
            }
            let mut line = [b' '; 27];
            write_uint(&mut line, 0, 3, p.pid as u32);
            let name = p.name();
            let n = name.len().min(12);
            line[4..4 + n].copy_from_slice(&name[..n]);
            let st: &[u8; 3] = match p.state {
                ProcState::Running => b"RUN",
                ProcState::Suspended => b"SUS",
                ProcState::Terminated => b"END",
            };
            line[17..20].copy_from_slice(st);
            write_uint(&mut line, 21, 6, p.ticks);
            crate::text::legacy::draw_bytes(c, tx, ty, &line, theme::text(), 2);
            ty += line_h;
        }

        // Real kernel threads from the scheduler, with live CPU time.
        let footer_y = (r.bottom() - 24).max(0) as usize;
        ty += 6;
        crate::text::legacy::draw_text(c, tx, ty, "KERNEL THREADS   CPU", theme::ACCENT_2, 2);
        ty += line_h + 2;
        for i in 0..threads {
            if ty + line_h > footer_y {
                break;
            }
            let name = sched::thread_name(i).as_bytes();
            let mut line = [b' '; 24];
            let n = name.len().min(14);
            line[..n].copy_from_slice(&name[..n]);
            if sched::thread_dead(i) {
                // A dead thread is never scheduled again: show that instead of a stale tick count.
                line[17..21].copy_from_slice(b"DEAD");
            } else {
                write_uint(&mut line, 17, 6, sched::thread_ticks(i) as u32);
            }
            crate::text::legacy::draw_bytes(c, tx, ty, &line, theme::text(), 2);
            ty += line_h;
        }

        // WASM apps: state, CPU over the last second (wall time of their slices) and memory.
        if !apps.is_empty() {
            ty += 6;
            crate::text::legacy::draw_text(
                c,
                tx,
                ty,
                "APPS         ST CPU%  MEM",
                theme::ACCENT_2,
                2,
            );
            ty += line_h + 2;
            for a in &apps {
                if ty + line_h > footer_y {
                    break;
                }
                let mut line = [b' '; 28];
                let name = alloc::format!("{}#{}", a.app_id, a.id);
                let nb = name.as_bytes();
                let n = nb.len().min(12);
                line[..n].copy_from_slice(&nb[..n]);
                line[13..16].copy_from_slice(a.state.label().as_bytes());
                write_uint(&mut line, 17, 3, a.cpu_pct as u32);
                write_uint(&mut line, 22, 5, a.mem_kib);
                line[27] = b'K';
                crate::text::legacy::draw_bytes(c, tx, ty, &line, theme::text(), 2);
                ty += line_h;
            }
        }

        let footer = "ENTER:open DEL:end R:restart";
        crate::text::legacy::draw_text(c, tx, footer_y, footer, theme::text_muted(), 2);
    }
}
