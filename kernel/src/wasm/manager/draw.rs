//! Drawing an app's surface into the compositor's canvas (and the messages shown instead when it
//! is starting, stopped or has failed).

use super::*;

const BG: crate::fb::Color = crate::fb::Color::rgb(0x10, 0x14, 0x20);
const FG: crate::fb::Color = crate::fb::Color::rgb(0x9a, 0xa6, 0xbd);
const ERR: crate::fb::Color = crate::fb::Color::rgb(0xe0, 0x70, 0x70);

enum Snap {
    Frame(*const u8, usize, i32, i32, usize),
    Message(String, bool),
}

/// Draw the app's latest frame into `c` at content origin `(cx, cy)`, clipped to a
/// `cw` x `ch` box; or a message panel while it loads or after it ended.
pub fn blit(id: AppId, c: &mut crate::fb::Canvas, cx: i32, cy: i32, cw: i32, ch: i32) {
    let snap = with(|slots| {
        let Some(s) = slot_of(slots, id) else {
            return Snap::Message(String::from(t!("apps.win.not_found")), true);
        };
        match s.state {
            State::Crashed => {
                return Snap::Message(t!("apps.win.ended", why = &s.reason.text()), true);
            }
            State::Exited => {
                return Snap::Message(t!("apps.win.ended", why = &s.reason.text()), false);
            }
            _ => {}
        }
        if !s.ready || s.surf[s.front].is_empty() {
            return Snap::Message(String::from(t!("apps.win.loading")), false);
        }
        s.reading = Some(s.front);
        let f = &s.surf[s.front];
        Snap::Frame(f.as_ptr(), f.len(), s.cur_w, s.cur_h, s.front)
    });
    match snap {
        Snap::Message(text, is_err) => message(c, cx, cy, cw, ch, &text, is_err),
        Snap::Frame(ptr, len, sw, sh, idx) => {
            // SAFETY: `reading = Some(idx)` tells `appd` not to render into or reallocate this surface and
            // `reap` not to free it until we clear it below; the Vec's buffer is `len` bytes and stays put.
            let src = unsafe { core::slice::from_raw_parts(ptr, len) };
            copy_frame(c, src, sw, sh, cx, cy, cw, ch);
            with(|slots| {
                if let Some(s) = slot_of(slots, id)
                    && s.reading == Some(idx)
                {
                    s.reading = None;
                }
            });
        }
    }
}

fn message(c: &mut crate::fb::Canvas, cx: i32, cy: i32, cw: i32, ch: i32, text: &str, err: bool) {
    let (x, y) = (cx.max(0) as usize, cy.max(0) as usize);
    c.fill_rect(x, y, cw.max(0) as usize, ch.max(0) as usize, BG);
    let col = if err { ERR } else { FG };
    // word-wrap at the box width, scale 2 (16 px cells)
    let per_line = ((cw - 32).max(16) as usize / crate::text::guest::cell_w(2)).max(8);
    let mut line = String::new();
    let mut ty = cy + 16;
    for word in text.split(' ') {
        if !line.is_empty() && line.len() + 1 + word.len() > per_line {
            crate::text::guest::draw_text(
                c,
                (cx + 16).max(0) as usize,
                ty.max(0) as usize,
                &line,
                col,
                2,
            );
            ty += 22;
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() && ty + 16 <= cy + ch {
        crate::text::guest::draw_text(
            c,
            (cx + 16).max(0) as usize,
            ty.max(0) as usize,
            &line,
            col,
            2,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_frame(
    c: &mut crate::fb::Canvas,
    src: &[u8],
    sw: i32,
    sh: i32,
    cx: i32,
    cy: i32,
    cw: i32,
    ch: i32,
) {
    let info = c.fb_info();
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let (scr_w, scr_h) = (info.width as i32, info.height as i32);
    let copy_w = sw.min(cw);
    let copy_h = sh.min(ch);
    let dst = c.buffer_mut();
    for y in 0..copy_h {
        let dyy = cy + y;
        if dyy < 0 || dyy >= scr_h {
            continue;
        }
        let x0 = cx.max(0);
        let x1 = (cx + copy_w).min(scr_w);
        if x1 <= x0 {
            continue;
        }
        let cols = (x1 - x0) as usize;
        let so = (y as usize * sw as usize + (x0 - cx) as usize) * bpp;
        let dofs = (dyy as usize * stride + x0 as usize) * bpp;
        if so + cols * bpp <= src.len() && dofs + cols * bpp <= dst.len() {
            dst[dofs..dofs + cols * bpp].copy_from_slice(&src[so..so + cols * bpp]);
        }
    }
    // The part of the box the (stale-size) frame does not cover.
    if sw < cw {
        c.fill_rect(
            (cx + sw).max(0) as usize,
            cy.max(0) as usize,
            (cw - sw) as usize,
            ch.max(0) as usize,
            BG,
        );
    }
    if sh < ch {
        c.fill_rect(
            cx.max(0) as usize,
            (cy + sh).max(0) as usize,
            cw.min(sw).max(0) as usize,
            (ch - sh) as usize,
            BG,
        );
    }
}
