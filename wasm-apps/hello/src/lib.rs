//! `hello` — the minimal OSjeff app (ABI v2): no permissions, no files, no network.
//! It shows a greeting, counts key presses and clicks, and echoes what it saw.

#![no_std]

use core::fmt::Write;
use kitsune_sdk::*;

manifest!(
    "id=hello\nname=Hello\nname.pt=Olá\nname.en=Hello\nversion=1.0.0\nabi=2\nwin_w=380\nwin_h=240\nwin_min_w=260\nwin_min_h=160\nmem_mib=2\n"
);
icon!(include_bytes!("../icon.png"));

struct Hello {
    keys: u32,
    clicks: u32,
    last: StrBuf<48>,
    mouse: (i32, i32),
}

impl App for Hello {
    fn new() -> Self {
        log!("hello: started");
        Hello {
            keys: 0,
            clicks: 0,
            last: StrBuf::new(),
            mouse: (0, 0),
        }
    }

    fn on_key(&mut self, code: i32, mods: i32) {
        self.keys += 1;
        self.last.clear();
        let _ = match lang() {
            Lang::Pt => write!(self.last, "tecla {} modificadores {}", code, mods),
            Lang::En => write!(self.last, "key {} modifiers {}", code, mods),
        };
    }

    fn on_pointer(&mut self, x: i32, y: i32, buttons: i32) {
        self.mouse = (x, y);
        if buttons & BTN_LEFT != 0 {
            self.clicks += 1;
        }
    }

    fn render(&mut self, c: &mut Canvas) {
        let (w, h) = c.size();
        c.clear(0x10141F);
        c.fill_rect(0, 0, w, 36, 0x1FB5A6);
        c.text(14, 10, tr("Olá, OSjeff!", "Hello, OSjeff!"), 0xFFFFFF, 2);
        let mut b = StrBuf::<64>::new();
        let _ = match lang() {
            Lang::Pt => write!(b, "teclas: {}   cliques: {}", self.keys, self.clicks),
            Lang::En => write!(b, "keys: {}   clicks: {}", self.keys, self.clicks),
        };
        c.text(14, 84, b.as_str(), 0xFFFFFF, 2);
        b.clear();
        let _ = write!(
            b,
            "{}: {}, {}   {} {}\u{d7}{}",
            tr("mouse", "mouse"),
            self.mouse.0,
            self.mouse.1,
            tr("janela", "window"),
            w,
            h
        );
        c.text(14, 112, b.as_str(), 0x9AA6BD, 1);
        if !self.last.as_str().is_empty() {
            c.text(14, 134, self.last.as_str(), 0xFFD84D, 1);
        }
        c.fill_rect(14, h - 28, w - 28, 2, 0x2A3140);
        c.text(
            14,
            h - 20,
            tr(
                "Sem acesso a arquivos ou à rede",
                "No access to files or the network",
            ),
            0x6A7488,
            1,
        );
    }
}

export_app!(Hello);
