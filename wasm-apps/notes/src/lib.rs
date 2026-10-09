//! `notes` — a notepad whose files live in the app's own folder (`fs=own`:
//! `/data/notes/` on the system, `/` for the app).
//!
//! Keys: type to edit; arrows/Home/End/Backspace/Delete/Enter edit; Tab switches
//! between the note list and the editor; Ctrl+S saves; Ctrl+N starts a new note;
//! Ctrl+X deletes the open note; Ctrl+T runs the sandbox self-test (tries to open
//! `../../etc/passwd` and a sibling app's folder: both must be refused).

#![no_std]

use core::fmt::Write;
use osjeff_sdk::*;

manifest!(
    "id=notes\nname=Notes\nname.pt=Notas\nname.en=Notes\nversion=1.0.0\nabi=2\nfs=own\ndisk_kib=64\nmax_fds=4\nwin_w=520\nwin_h=300\nwin_min_w=360\nwin_min_h=220\nmem_mib=2\n"
);
icon!(include_bytes!("../icon.png"));

const CAP: usize = 4096;
const MAX_FILES: usize = 16;
const NAME: usize = 32;

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    List,
    Editor,
}

struct Notes {
    buf: [u8; CAP],
    len: usize,
    cur: usize,
    name: StrBuf<NAME>,
    files: [[u8; NAME]; MAX_FILES],
    file_len: [usize; MAX_FILES],
    n_files: usize,
    sel: usize,
    focus: Focus,
    dirty: bool,
    status: StrBuf<72>,
}

impl Notes {
    fn say(&mut self, args: core::fmt::Arguments) {
        self.status.clear();
        let _ = self.status.write_fmt(args);
    }

    fn refresh_list(&mut self) {
        self.n_files = 0;
        let mut name = [0u8; 49];
        let mut i = 0;
        while self.n_files < MAX_FILES {
            match read_dir("/", i, &mut name) {
                Ok(Some((Kind::File, n))) if n <= NAME => {
                    self.files[self.n_files][..n].copy_from_slice(&name[..n]);
                    self.file_len[self.n_files] = n;
                    self.n_files += 1;
                }
                Ok(Some(_)) => {}
                _ => break,
            }
            i += 1;
        }
        self.sel = self.sel.min(self.n_files.saturating_sub(1));
    }

    fn file_name(&self, i: usize) -> &str {
        core::str::from_utf8(&self.files[i][..self.file_len[i]]).unwrap_or("?")
    }

    fn insert(&mut self, b: u8) {
        if self.len < CAP {
            self.buf.copy_within(self.cur..self.len, self.cur + 1);
            self.buf[self.cur] = b;
            self.cur += 1;
            self.len += 1;
            self.dirty = true;
        }
    }

    fn backspace(&mut self) {
        if self.cur > 0 {
            self.buf.copy_within(self.cur..self.len, self.cur - 1);
            self.cur -= 1;
            self.len -= 1;
            self.dirty = true;
        }
    }

    fn delete(&mut self) {
        if self.cur < self.len {
            self.buf.copy_within(self.cur + 1..self.len, self.cur);
            self.len -= 1;
            self.dirty = true;
        }
    }

    /// Start of the line containing `pos`.
    fn line_start(&self, pos: usize) -> usize {
        self.buf[..pos]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self, pos: usize) -> usize {
        self.buf[pos..self.len]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(self.len, |i| pos + i)
    }

    fn up(&mut self) {
        let ls = self.line_start(self.cur);
        if ls == 0 {
            return;
        }
        let col = self.cur - ls;
        let ps = self.line_start(ls - 1);
        self.cur = (ps + col).min(ls - 1);
    }

    fn down(&mut self) {
        let le = self.line_end(self.cur);
        if le >= self.len {
            return;
        }
        let col = self.cur - self.line_start(self.cur);
        let ns = le + 1;
        self.cur = (ns + col).min(self.line_end(ns));
    }

    fn save(&mut self) {
        if self.name.as_str().is_empty() {
            let mut n = 1;
            loop {
                let mut s = StrBuf::<NAME>::new();
                let _ = write!(s, "nota-{}.txt", n);
                if stat(s.as_str()).is_err() {
                    self.name = s;
                    break;
                }
                n += 1;
                if n > 99 {
                    self.say(format_args!("muitas notas"));
                    return;
                }
            }
        }
        let result = File::open(self.name.as_str(), O_WRITE | O_CREATE | O_TRUNC)
            .and_then(|mut f| f.write_all(&self.buf[..self.len]));
        match result {
            Ok(()) => {
                self.dirty = false;
                let mut s = StrBuf::<NAME>::new();
                let _ = s.write_str(self.name.as_str());
                let n = self.len;
                self.say(format_args!("salvo: {} ({} bytes)", s.as_str(), n));
                self.refresh_list();
            }
            Err(Errno::NOSPC) => self.say(format_args!("disco cheio (cota do app)")),
            Err(e) => self.say(format_args!("erro ao salvar: {}", e.0)),
        }
    }

    fn open_selected(&mut self) {
        if self.n_files == 0 {
            return;
        }
        let mut s = StrBuf::<NAME>::new();
        let _ = s.write_str(self.file_name(self.sel));
        match File::open(s.as_str(), O_READ) {
            Ok(mut f) => match f.read_full(&mut self.buf) {
                Ok(n) => {
                    self.len = n;
                    self.cur = 0;
                    self.name = StrBuf::new();
                    let _ = self.name.write_str(s.as_str());
                    self.dirty = false;
                    self.focus = Focus::Editor;
                    self.say(format_args!("aberto: {}", s.as_str()));
                }
                Err(e) => self.say(format_args!("erro de leitura: {}", e.0)),
            },
            Err(e) => self.say(format_args!("erro ao abrir: {}", e.0)),
        }
    }

    fn new_note(&mut self) {
        self.len = 0;
        self.cur = 0;
        self.name.clear();
        self.dirty = false;
        self.focus = Focus::Editor;
        self.say(format_args!("nova nota"));
    }

    fn delete_note(&mut self) {
        if self.name.as_str().is_empty() {
            return;
        }
        let mut s = StrBuf::<NAME>::new();
        let _ = s.write_str(self.name.as_str());
        match unlink(s.as_str()) {
            Ok(()) => {
                self.say(format_args!("apagado: {}", s.as_str()));
                self.new_note();
                self.refresh_list();
            }
            Err(e) => self.say(format_args!("erro ao apagar: {}", e.0)),
        }
    }

    /// The sandbox self-test: paths that try to leave the app's folder.
    fn sandbox_test(&mut self) {
        let a = File::open("../../etc/passwd", O_READ)
            .err()
            .map_or(0, |e| e.0);
        let b = File::open("/../notes2/x", O_READ).err().map_or(0, |e| e.0);
        let c = stat("..").err().map_or(0, |e| e.0);
        log!(
            "notes sandbox test: ../../etc/passwd={} /../notes2/x={} ..={}",
            a,
            b,
            c
        );
        self.say(format_args!(
            "sandbox: ../../etc={} /../x={} ..={} (-1 = recusado)",
            a, b, c
        ));
    }
}

impl App for Notes {
    fn new() -> Self {
        let mut n = Notes {
            buf: [0; CAP],
            len: 0,
            cur: 0,
            name: StrBuf::new(),
            files: [[0; NAME]; MAX_FILES],
            file_len: [0; MAX_FILES],
            n_files: 0,
            sel: 0,
            focus: Focus::Editor,
            dirty: false,
            status: StrBuf::new(),
        };
        n.refresh_list();
        n.say(format_args!(
            "Ctrl+S salva  Ctrl+N nova  Tab lista"
        ));
        n
    }

    fn on_text(&mut self, ch: u32) {
        if self.focus == Focus::Editor && (0x20..0x7F).contains(&ch) {
            self.insert(ch as u8);
        }
    }

    fn on_key(&mut self, code: i32, mods: i32) {
        if mods & MOD_CTRL != 0 {
            match code as u8 {
                b's' | b'S' => self.save(),
                b'n' | b'N' => self.new_note(),
                b'x' | b'X' => self.delete_note(),
                b't' | b'T' => self.sandbox_test(),
                _ => {}
            }
            return;
        }
        if code == 9 {
            self.focus = if self.focus == Focus::List {
                Focus::Editor
            } else {
                Focus::List
            };
            return;
        }
        match self.focus {
            Focus::List => match code {
                KEY_UP => self.sel = self.sel.saturating_sub(1),
                KEY_DOWN => self.sel = (self.sel + 1).min(self.n_files.saturating_sub(1)),
                10 => self.open_selected(),
                _ => {}
            },
            Focus::Editor => match code {
                8 => self.backspace(),
                127 => self.delete(),
                10 => self.insert(b'\n'),
                KEY_LEFT => self.cur = self.cur.saturating_sub(1),
                KEY_RIGHT => self.cur = (self.cur + 1).min(self.len),
                KEY_UP => self.up(),
                KEY_DOWN => self.down(),
                KEY_HOME => self.cur = self.line_start(self.cur),
                KEY_END => self.cur = self.line_end(self.cur),
                _ => {}
            },
        }
    }

    fn on_pointer(&mut self, x: i32, y: i32, buttons: i32) {
        if buttons & BTN_LEFT == 0 {
            return;
        }
        if x < LIST_W {
            self.focus = Focus::List;
            let row = (y - 40) / 20;
            if row >= 0 && (row as usize) < self.n_files {
                self.sel = row as usize;
            }
        } else {
            self.focus = Focus::Editor;
        }
    }

    fn on_close(&mut self) {
        if self.dirty && !self.name.as_str().is_empty() {
            self.save();
        }
    }

    fn render(&mut self, c: &mut Canvas) {
        let (w, h) = c.size();
        c.clear(0x10141F);
        // list panel
        c.fill_rect(0, 0, LIST_W, h, 0x181D27);
        c.text(10, 12, "NOTAS", 0x9AA6BD, 2);
        for i in 0..self.n_files {
            let y = 40 + i as i32 * 20;
            let selected = i == self.sel;
            if selected {
                let col = if self.focus == Focus::List {
                    0x0A84FF
                } else {
                    0x2A3140
                };
                c.fill_rect(4, y - 2, LIST_W - 8, 18, col);
            }
            c.text(10, y, self.file_name(i), 0xE6EAF2, 1);
        }
        // editor
        let ex = LIST_W + 12;
        let mut title = StrBuf::<48>::new();
        let nm = if self.name.as_str().is_empty() {
            "(nova nota)"
        } else {
            self.name.as_str()
        };
        let _ = write!(title, "{}{}", nm, if self.dirty { " *" } else { "" });
        c.text(ex, 12, title.as_str(), 0xFFFFFF, 2);
        c.fill_rect(ex, 34, w - ex - 10, 1, 0x2A3140);
        let cols = ((w - ex - 14) / 6).max(8);
        let max_y = h - 50;
        let (mut col, mut row, mut from) = (0i32, 0i32, 0usize);
        let mut cursor_at = (ex, 42);
        for i in 0..=self.len {
            if i == self.cur {
                cursor_at = (ex + col * 6, 42 + row * 12);
            }
            if i == self.len {
                draw_line(c, &self.buf, from, i, ex, 42 + row * 12, max_y);
                break;
            }
            if self.buf[i] == b'\n' {
                draw_line(c, &self.buf, from, i, ex, 42 + row * 12, max_y);
                row += 1;
                col = 0;
                from = i + 1;
                continue;
            }
            col += 1;
            if col >= cols {
                draw_line(c, &self.buf, from, i + 1, ex, 42 + row * 12, max_y);
                row += 1;
                col = 0;
                from = i + 1;
            }
        }
        if self.focus == Focus::Editor {
            c.fill_rect(cursor_at.0, cursor_at.1, 2, 10, 0xFFD84D);
        }
        // status bar
        c.fill_rect(0, h - 22, w, 22, 0x181D27);
        c.text(10, h - 15, self.status.as_str(), 0x9AA6BD, 1);
    }
}

const LIST_W: i32 = 150;

fn draw_line(c: &mut Canvas, buf: &[u8], from: usize, to: usize, x: i32, y: i32, max_y: i32) {
    if y <= max_y && to > from {
        if let Ok(s) = core::str::from_utf8(&buf[from..to]) {
            c.text(x, y, s, 0xE6EAF2, 1);
        }
    }
}

export_app!(Notes);
