//! `paint` — draw with the mouse (ABI v2, `fs=own`).
//!
//! Click a swatch to pick a color; drag on the canvas to draw. Keys: `+` / `-`
//! brush size, `e` eraser, `c` clear, Ctrl+S saves `paint.bmp` (24-bit BMP) in the
//! app's folder (`/data/paint/`).

#![no_std]

use core::fmt::Write;
use kitsune_sdk::*;

manifest!(
    "id=paint\nname=Paint\nname.pt=Pintura\nname.en=Paint\nversion=1.0.0\nabi=2\nfs=own\ndisk_kib=1024\nmax_fds=2\nwin_w=500\nwin_h=372\nwin_min_w=500\nwin_min_h=372\nresizable=0\nmem_mib=4\n"
);
icon!(include_bytes!("../icon.png"));

const CW: usize = 480;
const CH: usize = 300;
const OX: i32 = 10;
const OY: i32 = 44;
const PALETTE: [u32; 8] = [
    0x000000, 0xFFFFFF, 0xE54B4B, 0xFFD84D, 0x39D353, 0x39A4FF, 0x654FF0, 0xFF8A3D,
];

/// The bitmap, RGBA. A static: it is too big for the guest's stack.
static mut CANVAS: [u8; CW * CH * 4] = [255; CW * CH * 4];

fn canvas() -> &'static mut [u8; CW * CH * 4] {
    // SAFETY: single-threaded guest; the host never re-enters an export while one runs.
    unsafe { &mut *core::ptr::addr_of_mut!(CANVAS) }
}

struct Paint {
    color: usize,
    size: i32,
    last: Option<(i32, i32)>,
    full: bool,
    status: StrBuf<64>,
}

impl Paint {
    fn rgb(&self) -> u32 {
        PALETTE[self.color]
    }

    /// One square dab at canvas coordinates, on the bitmap and on the surface.
    fn dab(&self, c: &mut Canvas, x: i32, y: i32) {
        let s = self.size;
        let (x0, y0) = (x - s / 2, y - s / 2);
        let (cx0, cy0) = (x0.max(0), y0.max(0));
        let (cx1, cy1) = ((x0 + s).min(CW as i32), (y0 + s).min(CH as i32));
        if cx1 <= cx0 || cy1 <= cy0 {
            return;
        }
        let rgb = self.rgb();
        let px = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255];
        let buf = canvas();
        for yy in cy0..cy1 {
            for xx in cx0..cx1 {
                let o = (yy as usize * CW + xx as usize) * 4;
                buf[o..o + 4].copy_from_slice(&px);
            }
        }
        c.fill_rect(OX + cx0, OY + cy0, cx1 - cx0, cy1 - cy0, rgb);
    }

    fn stroke(&mut self, c: &mut Canvas, x: i32, y: i32) {
        let (mut px, mut py) = self.last.unwrap_or((x, y));
        // step from the previous point to this one so fast drags leave no gaps
        let (dx, dy) = ((x - px).abs(), -(y - py).abs());
        let (sx, sy) = (if px < x { 1 } else { -1 }, if py < y { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            self.dab(c, px, py);
            if px == x && py == y {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                px += sx;
            }
            if e2 <= dx {
                err += dx;
                py += sy;
            }
        }
        self.last = Some((x, y));
    }

    fn clear(&mut self) {
        for b in canvas().iter_mut() {
            *b = 255;
        }
        self.full = true;
    }

    fn save_bmp(&mut self) {
        let r = (|| -> Result<(), Errno> {
            let mut f = File::open("paint.bmp", O_WRITE | O_CREATE | O_TRUNC)?;
            let row = CW * 3;
            let size = 54 + row * CH;
            let mut head = [0u8; 54];
            head[0] = b'B';
            head[1] = b'M';
            head[2..6].copy_from_slice(&(size as u32).to_le_bytes());
            head[10..14].copy_from_slice(&54u32.to_le_bytes());
            head[14..18].copy_from_slice(&40u32.to_le_bytes());
            head[18..22].copy_from_slice(&(CW as u32).to_le_bytes());
            head[22..26].copy_from_slice(&(CH as u32).to_le_bytes());
            head[26..28].copy_from_slice(&1u16.to_le_bytes());
            head[28..30].copy_from_slice(&24u16.to_le_bytes());
            head[34..38].copy_from_slice(&((row * CH) as u32).to_le_bytes());
            f.write_all(&head)?;
            let buf = canvas();
            let mut line = [0u8; CW * 3];
            for y in (0..CH).rev() {
                for x in 0..CW {
                    let o = (y * CW + x) * 4;
                    line[x * 3] = buf[o + 2];
                    line[x * 3 + 1] = buf[o + 1];
                    line[x * 3 + 2] = buf[o];
                }
                f.write_all(&line)?;
            }
            Ok(())
        })();
        self.status.clear();
        match r {
            Ok(()) => {
                let _ = match lang() {
                    Lang::Pt => write!(
                        self.status,
                        "salvo: paint.bmp ({}\u{d7}{}, BMP de 24 bits)",
                        CW, CH
                    ),
                    Lang::En => write!(
                        self.status,
                        "saved: paint.bmp ({}\u{d7}{}, 24-bit BMP)",
                        CW, CH
                    ),
                };
            }
            Err(e) => {
                let _ = match lang() {
                    Lang::Pt => write!(self.status, "erro ao salvar: {} (-6 = cota)", e.0),
                    Lang::En => write!(self.status, "could not save: {} (-6 = quota)", e.0),
                };
            }
        }
    }

    fn draw_ui(&self, c: &mut Canvas) {
        let (w, h) = c.size();
        c.fill_rect(0, 0, w, OY - 2, 0x181D27);
        for (i, &col) in PALETTE.iter().enumerate() {
            let x = 10 + i as i32 * 34;
            c.fill_rect(x, 8, 28, 26, 0x2A3140);
            c.fill_rect(x + 2, 10, 24, 22, col);
            if i == self.color {
                c.fill_rect(x, 36, 28, 3, 0xFFD84D);
            }
        }
        let mut t = StrBuf::<24>::new();
        let _ = write!(t, "{} {}", tr("pincel", "brush"), self.size);
        c.text(300, 14, t.as_str(), 0xE6EAF2, 2);
        c.fill_rect(0, OY + CH as i32 + 2, w, h - (OY + CH as i32 + 2), 0x10141F);
        c.text(
            10,
            OY + CH as i32 + 4,
            tr(
                "Ctrl+S salva BMP  +/- pincel  e borracha  c limpa",
                "Ctrl+S saves BMP  +/- brush  e eraser  c clear",
            ),
            0x6A7488,
            1,
        );
        c.text(10, OY + CH as i32 + 16, self.status.as_str(), 0x9AA6BD, 1);
    }
}

impl App for Paint {
    fn new() -> Self {
        Paint {
            color: 0,
            size: 4,
            last: None,
            full: true,
            status: StrBuf::new(),
        }
    }

    fn on_resize(&mut self, _w: i32, _h: i32) {
        self.full = true; // a resized surface starts blank: repaint it from the bitmap
    }

    fn on_text(&mut self, ch: u32) {
        match ch as u8 {
            b'+' | b'=' => self.size = (self.size + 1).min(24),
            b'-' => self.size = (self.size - 1).max(1),
            b'e' | b'E' => self.color = 1,
            b'c' | b'C' => self.clear(),
            _ => {}
        }
    }

    fn on_key(&mut self, code: i32, mods: i32) {
        if mods & MOD_CTRL != 0 && (code == b's' as i32 || code == b'S' as i32) {
            self.save_bmp();
        }
    }

    fn on_pointer(&mut self, x: i32, y: i32, buttons: i32) {
        if buttons & BTN_LEFT == 0 {
            self.last = None;
            return;
        }
        if y < OY - 4 {
            let i = (x - 10) / 34;
            if (0..8).contains(&i) {
                self.color = i as usize;
            }
            self.last = None;
            return;
        }
        let mut cv = Canvas::new();
        let (cx, cy) = (x - OX, y - OY);
        if (0..CW as i32).contains(&cx) && (0..CH as i32).contains(&cy) {
            self.stroke(&mut cv, cx, cy);
        } else {
            self.last = None;
        }
    }

    fn render(&mut self, c: &mut Canvas) {
        if self.full {
            self.full = false;
            c.clear(0x10141F);
            let _ = c.blit_rgba(&canvas()[..], CW as i32, CH as i32, OX, OY);
        }
        self.draw_ui(c);
    }
}

export_app!(Paint);
