//! `Desktop` methods: input. Keyboard routing to the focused window's app,
//! global shortcuts (Ctrl+C/V/S/N, Alt+Tab), mouse hit-testing, window drag /
//! resize / maximize / minimize and the dock and context menus.

use super::*;

impl Desktop {
    // ---- keyboard ----

    pub fn handle_key(&mut self, scan: u8, extended: bool, pressed: bool, time: Time) -> bool {
        let alt_before = self.keymap.alt();
        let key = self.keymap.process(scan, extended, pressed);
        let alt_now = self.keymap.alt();

        // Alt released: commit the Alt+Tab selection (restoring a minimized
        // window), or just drop a stale switcher.
        if alt_before
            && !alt_now
            && let Some(sw) = self.switcher.take()
        {
            self.wm.activate(sw.selected());
            return true;
        }
        let Some(key) = key else {
            return false;
        };
        if !alt_now {
            self.switcher = None;
        }

        if alt_now {
            // Alt+Tab / Alt+Shift+Tab opens the switcher, repeats advance it.
            match key {
                Key::Tab => {
                    let back = self.keymap.shift();
                    match self.switcher.as_mut() {
                        Some(s) => s.advance(back),
                        None => self.switcher = Switcher::start(self.wm.switch_list(), back),
                    }
                    return true;
                }
                Key::Esc if self.switcher.take().is_some() => return true,
                // Other Alt+key chords belong to no app: do not type them.
                _ => return self.switcher.is_some(),
            }
        }

        let (Some(top), Some(kind)) =
            (self.focused(), self.focused().and_then(|f| self.kind_of(f)))
        else {
            return false;
        };
        // Ctrl+C / Ctrl+V / Ctrl+S / Ctrl+N are intercepted before the app sees the key.
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
                // Ctrl+N: another window of the focused app (single-instance
                // apps only re-focus; the WASM guest and the task manager keep
                // the key).
                Key::Char(b'n') | Key::Char(b'N')
                    if !matches!(kind, Kind::WasmApp | Kind::TaskMgr) =>
                {
                    self.new_window(kind);
                    return true;
                }
                _ => {}
            }
        }
        match kind {
            Kind::Terminal => {
                let action = self
                    .term_mut(top)
                    .map_or(Action::None, |t| t.on_key(key, time));
                self.handle_terminal_action(top, action);
            }
            Kind::Editor => {
                if key == Key::Esc {
                    self.request_close(top);
                } else if let Some(e) = self.editor_mut(top) {
                    e.editor.on_key(key);
                }
            }
            Kind::TaskMgr => self.task_key(top, key),
            Kind::Calculator => match key {
                Key::Char(b) => self.calc_input(top, b),
                Key::Enter => self.calc_input(top, b'='),
                Key::Backspace => self.calc_input(top, 0x08),
                Key::Esc => self.request_close(top),
                _ => {}
            },
            Kind::Browser => match key {
                Key::Esc => self.request_close(top),
                // Arrows scroll the rendered page (a pixel at a time feels slow,
                // so step by a few lines).
                Key::Up => self.scroll_page(top, -48),
                Key::Down => self.scroll_page(top, 48),
                _ => {
                    if let Some(b) = self.browser_state_mut(top) {
                        b.browser.on_key(key);
                    }
                }
            },
            // Forward keystrokes to the guest (printable bytes as-is, Enter as
            // LF, Esc as 0x1B so apps like DOOM get their menu key). A WASM app is
            // closed with the title-bar button, not Esc, so the guest keeps Esc.
            Kind::WasmApp => match key {
                Key::Char(b) => crate::wasm::on_key(b as i32),
                Key::Enter => crate::wasm::on_key(10),
                Key::Esc => crate::wasm::on_key(27),
                _ => {}
            },
            Kind::Files => self.files_key(top, key),
        }
        true
    }

    fn files_key(&mut self, id: WindowId, key: Key) {
        if key == Key::Esc {
            self.request_close(id);
            return;
        }
        if matches!(key, Key::Enter | Key::Right) {
            let slot = self.files_mut(id).and_then(|f| f.primary());
            if let Some(slot) = slot {
                self.fs_load_slot(slot);
            }
            return;
        }
        let Some(f) = self.files_mut(id) else {
            return;
        };
        match key {
            Key::Up => f.move_sel(-1),
            Key::Down => f.move_sel(1),
            Key::Left | Key::Backspace => f.up(),
            Key::Tab => f.toggle_view(),
            Key::Delete => f.delete(),
            Key::Char(b'n') | Key::Char(b'N') => f.mkdir(),
            _ => {}
        }
    }

    /// Resolve a click inside browser window `id`: toolbar buttons (home /
    /// reload / search) or, on the start page, the shortcut tiles.
    pub(crate) fn browser_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        let ch = BrowserChrome::of(rect);
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        if ch.home.contains(px, py) {
            b.browser.go_home();
        } else if ch.reload.contains(px, py) {
            b.browser.reload();
        } else if ch.go.contains(px, py) {
            b.browser.submit();
        } else if b.browser.is_home() {
            let (_logo, tiles) = browser_home_layout(ch.content);
            for (i, t) in tiles.iter().enumerate() {
                if t.contains(px, py) {
                    b.browser
                        .open(osjeff_core::browser::QUICK_LINKS[i].1.as_bytes());
                    break;
                }
            }
        }
    }

    /// Resolve a click in file-manager window `id`. Geometry mirrors
    /// `files_ui::draw_files` (sidebar width 176, rows 30px high). Sidebar items
    /// switch the view; clicks in the main list select a row.
    pub(crate) fn files_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        use osjeff_core::layout::{FilesHit, files_hit};
        let Some(f) = self.files_mut(id) else {
            return;
        };
        match files_hit(rect, f.view, px, py) {
            Some(FilesHit::View(v)) => f.set_view(v),
            Some(FilesHit::Row(i)) => f.select_at(i),
            None => {}
        }
    }

    pub(crate) fn calc_input(&mut self, id: WindowId, k: u8) {
        if let Some(c) = self.calc_mut(id) {
            if k == 0x08 {
                c.backspace();
            } else {
                c.input(k);
            }
        }
    }

    // ---- terminal actions ----

    pub(crate) fn handle_terminal_action(&mut self, id: WindowId, action: Action) {
        match action {
            Action::OpenEditor => {
                self.launch(Kind::Editor);
            }
            Action::OpenTasks => {
                self.launch(Kind::TaskMgr);
            }
            Action::OpenCalc => {
                self.launch(Kind::Calculator);
            }
            Action::Reboot => crate::power::reboot(),
            Action::Shutdown => crate::power::shutdown(),
            Action::List => self.fs_list(id),
            Action::Save(f) => self.fs_save(id, f),
            Action::Load(f) => self.fs_load(id, f),
            Action::Cat(f) => self.fs_cat(id, f),
            Action::Remove(f) => self.fs_remove(id, f),
            Action::None => {}
        }
    }

    /// Copy the focused app's current text (terminal input line / editor current
    /// line / calculator display / browser URL) into the shared clipboard.
    pub(crate) fn copy_from_focused(&mut self) {
        let Some(top) = self.focused() else {
            return;
        };
        // Snapshot to a local buffer so the immutable borrow of the app ends
        // before mutably borrowing the clipboard.
        let mut tmp = [0u8; clipboard::CAP];
        let mut n = 0;
        if let Some(w) = self.wm.get(top) {
            let text: &[u8] = match &w.app.app {
                App::Terminal(t) => t.input(),
                App::Editor(e) => e.editor.line(e.editor.cursor().1),
                App::Calculator(c) => c.display(),
                App::Browser(b) => b.browser.url(),
                App::TaskMgr | App::Wasm | App::Files(_) => &[],
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

        match self.app_mut(top) {
            Some(App::Terminal(t)) => {
                for &b in data {
                    if b != b'\n' && b != b'\r' {
                        let _ = t.on_key(Key::Char(b), time);
                    }
                }
            }
            Some(App::Editor(e)) => {
                for &b in data {
                    if b == b'\n' {
                        e.editor.on_key(Key::Enter);
                    } else if b != b'\r' {
                        e.editor.on_key(Key::Char(b));
                    }
                }
            }
            Some(App::Calculator(c)) => {
                for &b in data {
                    c.input(b);
                }
            }
            Some(App::Browser(b)) => {
                for &ch in data {
                    if ch != b'\n' && ch != b'\r' {
                        b.browser.on_key(Key::Char(ch));
                    }
                }
            }
            Some(App::TaskMgr | App::Wasm | App::Files(_)) | None => {}
        }
    }

    pub(crate) fn task_key(&mut self, id: WindowId, key: Key) {
        match key {
            Key::Up => self.procs.select_prev(),
            Key::Down => self.procs.select_next(),
            Key::Enter => {
                if let Some(pid) = self.procs.selected_pid()
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.wm.activate(w);
                }
            }
            // End the selected process: really close the window of that
            // instance (its process goes with it when the animation ends).
            Key::Delete => {
                if let Some(pid) = self.procs.selected_pid()
                    && let Some(p) = self.procs.get(pid)
                    && p.kind == ProcKind::App
                    && let Some(w) = self.window_of_pid(pid)
                {
                    self.request_close(w);
                }
            }
            Key::Esc => self.request_close(id),
            _ => {}
        }
    }

    // ---- mouse ----

    /// Maximize / restore window `id` and make the next frame repaint the whole
    /// screen (the window and everything it uncovers change).
    pub(crate) fn toggle_maximize(&mut self, id: WindowId) {
        let work = self.work_area();
        if self.wm.toggle_maximize(id, work) {
            self.force_full = true;
            self.relayout_browser(id);
        }
    }

    /// A left press on window `w` at `(cx, cy)`: title-bar buttons, resize
    /// border, title (drag / double-click maximize) or app content.
    fn click_window(&mut self, w: WindowId, cx: i32, cy: i32) {
        let Some(win) = self.wm.get(w) else {
            return;
        };
        let (rect, resizable, maximized) = (win.rect, win.resizable, win.maximized);
        let kind = win.app.kind();
        if rect.on_close(cx, cy) {
            self.request_close(w);
            self.drag = None;
            return;
        }
        if rect.on_min(cx, cy) {
            self.wm.minimize(w);
            self.drag = None;
            self.clicks.reset();
            return;
        }
        if resizable && rect.on_max(cx, cy) {
            self.toggle_maximize(w);
            self.clicks.reset();
            return;
        }
        self.wm.raise(w);
        if resizable
            && !maximized
            && let Some(edge) = rect.resize_edge_at(cx, cy)
        {
            self.drag = Some(Drag {
                win: w,
                mode: DragMode::Resize {
                    edge,
                    start: rect,
                    ox: cx,
                    oy: cy,
                },
            });
            return;
        }
        if rect.on_title(cx, cy) {
            let double = self.clicks.press(crate::interrupts::ticks(), cx, cy, w);
            if double && resizable {
                self.toggle_maximize(w);
            } else if !maximized {
                self.drag = Some(Drag {
                    win: w,
                    mode: DragMode::Move {
                        grab_dx: cx - rect.x,
                        grab_dy: cy - rect.y,
                    },
                });
            }
            return;
        }
        match kind {
            Kind::Calculator => {
                if let Some(k) = calc_button_at(rect, cx, cy) {
                    self.calc_input(w, k);
                }
            }
            Kind::Browser => self.browser_click(w, rect, cx, cy),
            Kind::WasmApp => {
                // Translate the click into the guest's content-local
                // coordinates (same origin as `draw_wasm`) and deliver it.
                let pad = 14;
                let lx = cx - (rect.x + pad);
                let ly = cy - (rect.y + TITLE_H + 12);
                crate::wasm::on_pointer(lx, ly, 1);
            }
            Kind::Files => self.files_click(w, rect, cx, cy),
            Kind::Terminal | Kind::Editor | Kind::TaskMgr => {}
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

        // Right click opens a context menu at the cursor: the app menu on a dock
        // icon ("Nova janela"), the launcher elsewhere.
        if right_pressed {
            let kind = match self.dock_hit(cx, cy) {
                Some(DockAction::Open(k)) => MenuKind::Dock(k),
                _ => MenuKind::Desktop,
            };
            let (mx, my) = match kind {
                // A dock icon's menu pops up above the icon, centered on it.
                MenuKind::Dock(k) => {
                    let (_, icons) = dock_layout(self.sw, self.sh);
                    let r = icons[k.index() + 1];
                    let h = osjeff_core::layout::menu_height(kind.len());
                    self.clamp_menu(r.x + r.w / 2 - MENU_W / 2, r.y - h - 14, kind.len())
                }
                MenuKind::Desktop => self.clamp_menu(cx, cy, kind.len()),
            };
            self.menu = Some(MenuState { x: mx, y: my, kind });
            scene = true;
        }

        if left_pressed {
            if self.start_open {
                // Resolve a click on the open start panel (app / power / dismiss).
                match start_item_at(self.sw, self.sh, cx, cy) {
                    Some(StartItem::App(k)) => {
                        self.start_open = false;
                        self.launch(k);
                    }
                    Some(StartItem::Reboot) => crate::power::reboot(),
                    Some(StartItem::Shutdown) => crate::power::shutdown(),
                    None => self.start_open = false,
                }
                scene = true;
            } else if let Some(m) = self.menu {
                // A click while the menu is open selects an item or dismisses it.
                if let Some(i) = menu_item_at(m.x, m.y, cx, cy, m.kind.len())
                    && let Some((_, _, action)) = m.kind.entry(i)
                {
                    match action {
                        MenuAction::Launch(k) => {
                            self.launch(k);
                        }
                        MenuAction::NewWindow(k) => {
                            self.new_window(k);
                        }
                    }
                }
                self.menu = None;
                scene = true;
            } else if let Some(w) = self.topmost_at(cx, cy) {
                self.click_window(w, cx, cy);
                scene = true;
            } else if let Some(action) = self.dock_hit(cx, cy) {
                match action {
                    DockAction::Start => self.start_open = !self.start_open,
                    // Focus (or restore) the app's window; open one if none.
                    DockAction::Open(k) => {
                        self.launch(k);
                    }
                }
                scene = true;
            }
        }

        if released && let Some(d) = self.drag.take() {
            // A resized browser lays its page out again for the new width.
            if matches!(d.mode, DragMode::Resize { .. }) {
                self.relayout_browser(d.win);
            }
        }

        if let Some(d) = &self.drag {
            if left {
                let (w, mode) = (d.win, d.mode);
                let (sw, sh) = (self.sw, self.sh);
                match mode {
                    DragMode::Move { grab_dx, grab_dy } => {
                        self.wm.move_to(w, cx - grab_dx, cy - grab_dy, sw, sh);
                    }
                    DragMode::Resize {
                        edge,
                        start,
                        ox,
                        oy,
                    } => {
                        self.wm.resize(w, edge, start, (cx - ox, cy - oy), (sw, sh));
                    }
                }
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

        // Hover: the window under the cursor shows its minimize / maximize
        // buttons. Only an enter / leave changes pixels; skipped while dragging.
        if self.drag.is_none() {
            let hov = self.topmost_at(cx, cy);
            if hov != self.hover {
                for id in [self.hover, hov].into_iter().flatten() {
                    if let Some(r) = self.wm.get(id).map(|w| self.window_box(w)) {
                        self.mark_dirty(r);
                    }
                }
                self.hover = hov;
                scene = true;
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
