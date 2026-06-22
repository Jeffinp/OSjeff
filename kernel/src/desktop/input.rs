//! `Desktop` methods: input. Split out of the former monolithic desktop.rs.

use super::*;

impl Desktop {
    // ---- input ----

    pub fn handle_key(&mut self, scan: u8, extended: bool, pressed: bool, time: Time) -> bool {
        let Some(key) = self.keymap.process(scan, extended, pressed) else {
            return false;
        };
        let Some(top) = self.focused() else {
            return false;
        };
        // Ctrl+C / Ctrl+V / Ctrl+S are intercepted before the app sees the key.
        if self.keymap.ctrl() {
            match key {
                Key::Char(b'c') | Key::Char(b'C') => {
                    self.copy_from_focused();
                    return true;
                }
                Key::Char(b'v') | Key::Char(b'V') => {
                    self.paste_into_focused(time);
                    return true;
                }
                Key::Char(b's') | Key::Char(b'S') => {
                    self.save_editor_file();
                    return true;
                }
                _ => {}
            }
        }
        match self.windows[top].kind {
            Kind::Terminal => {
                let action = self.term.on_key(key, time);
                self.handle_terminal_action(action);
            }
            Kind::Editor => {
                if key == Key::Esc {
                    self.request_close(EDIT);
                } else {
                    self.editor.on_key(key);
                }
            }
            Kind::TaskMgr => self.task_key(key),
            Kind::Calculator => match key {
                Key::Char(b) => self.calc.input(b),
                Key::Enter => self.calc.input(b'='),
                Key::Backspace => self.calc.backspace(),
                Key::Esc => self.request_close(CALC),
                _ => {}
            },
            Kind::Browser => match key {
                Key::Esc => self.request_close(BROWSER),
                // Arrows scroll the rendered page (a pixel at a time feels slow,
                // so step by a few lines).
                Key::Up => self.scroll_page(-48),
                Key::Down => self.scroll_page(48),
                _ => {
                    self.browser.on_key(key);
                }
            },
            Kind::WasmApp => {
                if key == Key::Esc {
                    self.request_close(WASM);
                }
            }
        }
        true
    }

    /// Resolve a click inside the browser window: toolbar buttons (home /
    /// reload / search) or, on the start page, the shortcut tiles.
    pub(crate) fn browser_click(&mut self, rect: Rect, px: i32, py: i32) {
        let ch = BrowserChrome::of(rect);
        if ch.home.contains(px, py) {
            self.browser.go_home();
        } else if ch.reload.contains(px, py) {
            self.browser.reload();
        } else if ch.go.contains(px, py) {
            self.browser.submit();
        } else if self.browser.is_home() {
            let (_logo, tiles) = browser_home_layout(ch.content);
            for (i, t) in tiles.iter().enumerate() {
                if t.contains(px, py) {
                    self.browser
                        .open(osjeff_core::browser::QUICK_LINKS[i].1.as_bytes());
                    break;
                }
            }
        }
    }

    pub(crate) fn calc_input(&mut self, k: u8) {
        if k == 0x08 {
            self.calc.backspace();
        } else {
            self.calc.input(k);
        }
    }

    // ---- filesystem ----

    pub(crate) fn handle_terminal_action(&mut self, action: Action) {
        match action {
            Action::OpenEditor => self.open(EDIT),
            Action::OpenTasks => self.open(TASK),
            Action::OpenCalc => self.open(CALC),
            Action::Reboot => crate::power::reboot(),
            Action::Shutdown => crate::power::shutdown(),
            Action::List => self.fs_list(),
            Action::Save(f) => self.fs_save(f),
            Action::Load(f) => self.fs_load(f),
            Action::Cat(f) => self.fs_cat(f),
            Action::Remove(f) => self.fs_remove(f),
            Action::None => {}
        }
    }

    /// Copy the focused app's current text (terminal input line / editor current
    /// line / calculator display) into the shared clipboard.
    pub(crate) fn copy_from_focused(&mut self) {
        let Some(top) = self.focused() else {
            return;
        };
        // Snapshot to a local buffer so the immutable borrow of the app ends
        // before mutably borrowing the clipboard.
        let mut tmp = [0u8; clipboard::CAP];
        let n;
        {
            let text: &[u8] = match self.windows[top].kind {
                Kind::Terminal => self.term.input(),
                Kind::Editor => self.editor.line(self.editor.cursor().1),
                Kind::Calculator => self.calc.display(),
                Kind::Browser => self.browser.url(),
                Kind::TaskMgr | Kind::WasmApp => &[],
            };
            n = text.len().min(clipboard::CAP);
            tmp[..n].copy_from_slice(&text[..n]);
        }
        self.clipboard.set(&tmp[..n]);
    }

    /// Paste the clipboard into the focused app by replaying its bytes through
    /// the app's normal key handler (so editor line breaks, etc. just work).
    pub(crate) fn paste_into_focused(&mut self, time: Time) {
        let Some(top) = self.focused() else {
            return;
        };
        if self.clipboard.is_empty() {
            return;
        }
        let mut tmp = [0u8; clipboard::CAP];
        let n = self.clipboard.get().len();
        tmp[..n].copy_from_slice(self.clipboard.get());
        let data = &tmp[..n];

        match self.windows[top].kind {
            Kind::Terminal => {
                for &b in data {
                    if b != b'\n' && b != b'\r' {
                        let _ = self.term.on_key(Key::Char(b), time);
                    }
                }
            }
            Kind::Editor => {
                for &b in data {
                    if b == b'\n' {
                        self.editor.on_key(Key::Enter);
                    } else if b != b'\r' {
                        self.editor.on_key(Key::Char(b));
                    }
                }
            }
            Kind::Calculator => {
                for &b in data {
                    self.calc.input(b);
                }
            }
            Kind::Browser => {
                for &b in data {
                    if b != b'\n' && b != b'\r' {
                        self.browser.on_key(Key::Char(b));
                    }
                }
            }
            Kind::TaskMgr | Kind::WasmApp => {}
        }
    }

    pub(crate) fn task_key(&mut self, key: Key) {
        match key {
            Key::Up => self.procs.select_prev(),
            Key::Down => self.procs.select_next(),
            Key::Enter => {
                if let Some(pid) = self.procs.selected_pid()
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.open(w);
                }
            }
            Key::Delete => {
                if let Some(pid) = self.procs.selected_pid()
                    && let Some(p) = self.procs.get(pid)
                    && p.kind == ProcKind::App
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.request_close(w);
                }
            }
            Key::Esc => self.request_close(TASK),
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, dx: i32, dy: i32, left: bool, right: bool) -> MouseResult {
        self.cursor_x = (self.cursor_x + dx).clamp(0, self.sw - 1);
        self.cursor_y = (self.cursor_y - dy).clamp(0, self.sh - 1);
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        let cursor_moved = dx != 0 || dy != 0;
        let mut scene = false;

        let left_pressed = left && !self.prev_left;
        let right_pressed = right && !self.prev_right;
        let released = !left && self.prev_left;

        // Right click opens the context menu at the cursor.
        if right_pressed {
            self.menu = Some(self.clamp_menu(cx, cy));
            scene = true;
        }

        if left_pressed {
            if self.start_open {
                // Resolve a click on the open start panel (app / power / dismiss).
                match start_item_at(self.sw, self.sh, cx, cy) {
                    Some(StartItem::App(w)) => {
                        self.start_open = false;
                        self.open(w);
                    }
                    Some(StartItem::Reboot) => crate::power::reboot(),
                    Some(StartItem::Shutdown) => crate::power::shutdown(),
                    None => self.start_open = false,
                }
                scene = true;
            } else if let Some((mx, my)) = self.menu {
                // A click while the menu is open selects an item or dismisses it.
                if let Some(i) = menu_item_at(mx, my, cx, cy) {
                    self.open(MENU_ITEMS[i].1);
                }
                self.menu = None;
                scene = true;
            } else if let Some(w) = self.topmost_at(cx, cy) {
                let rect = self.windows[w].rect;
                if rect.on_close(cx, cy) {
                    self.request_close(w);
                    self.drag = None;
                } else {
                    self.bring_to_front(w);
                    if rect.on_title(cx, cy) {
                        self.drag = Some(Drag {
                            win: w,
                            grab_dx: cx - rect.x,
                            grab_dy: cy - rect.y,
                        });
                    } else if self.windows[w].kind == Kind::Calculator
                        && let Some(k) = calc_button_at(rect, cx, cy)
                    {
                        self.calc_input(k);
                    } else if self.windows[w].kind == Kind::Browser {
                        self.browser_click(rect, cx, cy);
                    }
                }
                scene = true;
            } else if let Some(action) = self.dock_hit(cx, cy) {
                match action {
                    DockAction::Start => self.start_open = !self.start_open,
                    DockAction::Open(app) => self.open(app),
                }
                scene = true;
            }
        }

        if released {
            self.drag = None;
        }

        if let Some(d) = &self.drag {
            if left {
                let w = d.win;
                let mut r = self.windows[w].rect;
                r.x = cx - d.grab_dx;
                r.y = cy - d.grab_dy;
                let (x, y) = r.clamped_pos(self.sw, self.sh);
                self.windows[w].rect.x = x;
                self.windows[w].rect.y = y;
                // NOT scene_dirty: a drag is driven by the per-frame damage path
                // (keyed on `cursor_moved`), which repaints only the window's
                // old+new rect. Marking the whole scene dirty would force a full
                // recompose + 8 MiB blit every mouse step — the very cost we're
                // removing. The dragged window is kept out of the static layer
                // (see `is_dynamic`), so moving it never touches the others.
            } else {
                self.drag = None;
            }
        }

        // Moving over an open menu / start panel updates the hover highlight,
        // but it is NOT a full-scene change: the compositor repaints only the
        // overlay's rectangle on `cursor_moved` (see the overlay path in the
        // main loop), so we deliberately do not set `scene` here.

        self.prev_left = left;
        self.prev_right = right;
        MouseResult {
            scene_dirty: scene,
            cursor_moved,
        }
    }
}
