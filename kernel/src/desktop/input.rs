//! `Desktop` methods: input. Keyboard routing to the focused window's app,
//! global shortcuts (Ctrl+C/V/S/N, Alt+Tab), mouse hit-testing, window drag /
//! resize / maximize / minimize and the dock and context menus.

use super::shell::Cmd;
use super::*;
use osjeff_core::window::TitleBtn;

/// Keys the [`Keymap`] has no `Key` for; read from the raw scancode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Special {
    F2,
    F3,
    F5,
    PageUp,
    PageDown,
    KpPlus,
    KpMinus,
}

/// The special key of a scancode, if it is one.
fn special_of(scan: u8, extended: bool) -> Option<Special> {
    match (scan, extended) {
        (0x3C, false) => Some(Special::F2),
        (0x3D, false) => Some(Special::F3),
        (0x3F, false) => Some(Special::F5),
        (0x49, true) => Some(Special::PageUp),
        (0x51, true) => Some(Special::PageDown),
        (0x4E, false) => Some(Special::KpPlus),
        (0x4A, false) => Some(Special::KpMinus),
        _ => None,
    }
}

impl Desktop {
    // ---- keyboard ----

    /// A special key for the focused file manager or viewer. `true` when consumed.
    fn handle_special(&mut self, sp: Special) -> bool {
        let Some(top) = self.focused() else {
            return false;
        };
        match self.kind_of(top) {
            Some(Kind::Files) => self.files_special(top, sp),
            Some(Kind::Viewer) => self.viewer_special(top, sp),
            Some(k @ (Kind::Terminal | Kind::Editor)) => self.text_special(top, k, sp),
            _ => false,
        }
    }

    /// PageUp, PageDown and F3 for a terminal (scrollback) or an editor.
    fn text_special(&mut self, id: WindowId, kind: Kind, sp: Special) -> bool {
        let code = match sp {
            Special::PageUp => osjeff_core::input::KeyCode::PageUp,
            Special::PageDown => osjeff_core::input::KeyCode::PageDown,
            Special::F3 => osjeff_core::input::KeyCode::F(3),
            _ => return false,
        };
        let ev = osjeff_core::input::KeyEvent::new(code, self.mods());
        if kind == Kind::Terminal {
            self.term_event(id, ev);
        } else {
            self.editor_event(id, ev);
        }
        true
    }

    pub fn handle_key(&mut self, scan: u8, extended: bool, pressed: bool, _time: Time) -> bool {
        let alt_before = self.keymap.alt();
        if pressed
            && !alt_before
            && let Some(sp) = special_of(scan, extended)
            && self.handle_special(sp)
        {
            return true;
        }
        let key = self.keymap.process(scan, extended, pressed);
        let alt_now = self.keymap.alt();

        // Alt released: commit the Alt+Tab selection (restoring a minimized
        // window), or just drop a stale switcher.
        if alt_before
            && !alt_now
            && let Some(sw) = self.switcher.take()
        {
            self.shell.switcher_glass.clear();
            self.wm.activate(sw.selected());
            self.force_full = true;
            return true;
        }
        let Some(key) = key else {
            return false;
        };
        if !alt_now && self.switcher.take().is_some() {
            self.shell.switcher_glass.clear();
            self.force_full = true;
        }

        // System shortcuts that work everywhere.
        if self.keymap.ctrl() {
            match key {
                // Ctrl+Alt+H: the performance HUD. Ctrl+Alt+G: the component gallery.
                Key::Char(b'h' | b'H') if alt_now => {
                    self.shell.hud = !self.shell.hud;
                    self.force_full = true;
                    return true;
                }
                Key::Char(b'g' | b'G') if alt_now => {
                    self.execute(Cmd::Gallery);
                    return true;
                }
                // Ctrl+Alt+Left / Right: the previous / next workspace; with Shift they carry the
                // focused window along.
                Key::Left | Key::Right if alt_now => {
                    let cur = self.wm.workspace();
                    let to = if key == Key::Left {
                        cur.saturating_sub(1)
                    } else {
                        (cur + 1).min(self.wm.visible_workspaces() - 1)
                    };
                    if self.keymap.shift() {
                        if let Some(id) = self.focused() {
                            self.move_window_to_workspace(id, to);
                        }
                    } else {
                        self.go_workspace(to);
                    }
                    return true;
                }
                // Ctrl+Alt+D: show the desktop (or bring the windows back).
                Key::Char(b'd' | b'D') if alt_now => {
                    self.execute(Cmd::ShowDesktop);
                    return true;
                }
                // Ctrl+Space: Busca.
                Key::Char(b' ') if !alt_now => {
                    self.open_search();
                    return true;
                }
                _ => {}
            }
        }

        if alt_now {
            // Alt+Tab / Alt+Shift+Tab opens the switcher, repeats advance it.
            match key {
                Key::Tab => {
                    let back = self.keymap.shift();
                    match self.switcher.as_mut() {
                        Some(s) => s.advance(back),
                        None => {
                            self.close_transients();
                            self.shell.switcher_glass.clear();
                            self.switcher = Switcher::start(self.wm.switch_list(), back);
                        }
                    }
                    self.force_full = true;
                    return true;
                }
                Key::Esc if self.switcher.take().is_some() => {
                    self.shell.switcher_glass.clear();
                    self.force_full = true;
                    return true;
                }
                // Alt+arrows tile, maximise, restore or minimise the focused window (Alt+Shift+
                // arrows always do; plain Alt+Left / Alt+Right go back / forward in the browser).
                Key::Left | Key::Right | Key::Up | Key::Down => {
                    if self.switcher.is_some() {
                        return true;
                    }
                    use osjeff_core::snap::Arrow;
                    let shift = self.keymap.shift();
                    if let Some(f) = self.focused()
                        && !shift
                        && matches!(key, Key::Left | Key::Right)
                        && self.kind_of(f) == Some(Kind::Browser)
                        && let Some(b) = self.browser_state_mut(f)
                    {
                        if key == Key::Left {
                            b.browser.back();
                        } else {
                            b.browser.forward();
                        }
                        return true;
                    }
                    let arrow = match key {
                        Key::Left => Arrow::Left,
                        Key::Right => Arrow::Right,
                        Key::Up => Arrow::Up,
                        _ => Arrow::Down,
                    };
                    return self.snap_key(arrow);
                }
                // The editor's find bar uses Alt+A (replace all), Alt+R and Alt+C (case).
                Key::Char(_) if self.switcher.is_none() => {
                    if let Some(f) = self.focused()
                        && self.kind_of(f) == Some(Kind::Editor)
                    {
                        return self.editor_key(f, key);
                    }
                    return false;
                }
                // Other Alt+key chords belong to no app: do not type them.
                _ => return self.switcher.is_some(),
            }
        }

        // Menus, popovers, sheets, Apps and Busca own the keyboard while they are up.
        if self.shell_key(key) {
            return true;
        }
        // An ABNT2 accent followed by a letter it cannot combine with types both.
        let mut changed = self.dispatch_key(key);
        while let Some(k) = self.keymap.take_pending() {
            changed |= self.dispatch_key(k);
        }
        changed
    }

    /// Deliver one logical key to the focused app (after the Alt+Tab handling):
    /// Ctrl shortcuts first, then the app's own key handler.
    fn dispatch_key(&mut self, key: Key) -> bool {
        let (Some(top), Some(kind)) =
            (self.focused(), self.focused().and_then(|f| self.kind_of(f)))
        else {
            return false;
        };
        // Ctrl+C / Ctrl+V / Ctrl+S / Ctrl+N are intercepted before the app sees the key
        // (a WASM app gets every chord itself).
        if self.keymap.ctrl() && kind != Kind::WasmApp {
            // The file manager owns Ctrl+A/C/X/V/R (files, not text).
            if kind == Kind::Files
                && let Key::Char(ch) = key
                && ch != b'n'
                && ch != b'N'
                && ch != b's'
                && ch != b'S'
                && self.files_ctrl(top, ch)
            {
                return true;
            }
            // The editor does its own copy, paste and save (the text engine owns Ctrl+C/X/V/S);
            // in the terminal Ctrl+C interrupts, Ctrl+Shift+C copies the typed line.
            match key {
                Key::Char(b'c' | b'C')
                    if kind != Kind::Editor && (kind != Kind::Terminal || self.keymap.shift()) =>
                {
                    self.copy_from_focused();
                    return true;
                }
                Key::Char(b'v' | b'V') if kind != Kind::Editor => {
                    self.paste_into_focused();
                    return true;
                }
                Key::Char(b's' | b'S') if kind == Kind::Viewer => {
                    self.viewer_save_prompt(top);
                    return true;
                }
                // Ctrl+N: another window of the focused app (single-instance
                // apps only re-focus; the WASM guest and the task manager keep
                // the key).
                Key::Char(b'n') | Key::Char(b'N')
                    if !matches!(kind, Kind::WasmApp | Kind::TaskMgr | Kind::Gallery) =>
                {
                    self.new_window(kind);
                    return true;
                }
                // Ctrl+W closes the window, Ctrl+M minimises it.
                Key::Char(b'w' | b'W') => {
                    self.request_close(top);
                    return true;
                }
                Key::Char(b'm' | b'M') => {
                    self.wm.minimize(top);
                    return true;
                }
                _ => {}
            }
        }
        match kind {
            Kind::Terminal => {
                self.term_key(top, key);
            }
            Kind::Editor => {
                self.editor_key(top, key);
            }
            Kind::TaskMgr => self.tarefas_key(top, key),
            Kind::Calculator => match key {
                Key::Char(b) => self.calc_input(top, b),
                Key::Enter => self.calc_input(top, b'='),
                Key::Backspace => self.calc_input(top, 0x08),
                Key::Delete => self.calc_input(top, b'C'),
                Key::Esc => self.request_close(top),
                _ => {}
            },
            Kind::Browser => self.browser_key(top, key),
            // Forward keystrokes to the guest (printable bytes as-is, Enter as
            // LF, Esc as 0x1B so apps like DOOM get their menu key). A WASM app is
            // closed with the title-bar button, not Esc, so the guest keeps Esc.
            Kind::WasmApp => {
                if let Some(h) = self.wasm_handle(top) {
                    // Hand the app the current clipboard (it may read it with `clip_get`).
                    crate::wasm::clip_load(self.clipboard.get());
                    let mods = self.keymap.shift() as i32
                        | ((self.keymap.ctrl() as i32) << 1)
                        | ((self.keymap.alt() as i32) << 2);
                    crate::wasm::key(h, wasm_key_code(key), mods);
                }
            }
            Kind::Files => self.files_key(top, key),
            Kind::Settings => self.settings_key(top, key),
            Kind::LogViewer => self.log_key(top, key),
            Kind::Viewer => self.viewer_key(top, key),
            Kind::Gallery => self.gallery_key(top, key),
        }
        true
    }

    /// Keys for the browser window `id`: Ctrl chords (favourite, find, zoom), the find bar, a
    /// focused form control, then scrolling keys (when the page has the focus) and the address bar.
    fn browser_key(&mut self, id: WindowId, key: Key) {
        let ctrl = self.keymap.ctrl();
        let shift = self.keymap.shift();
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let view_h = BrowserChrome::of(rect).content.h;
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        b.notice = None;
        if ctrl {
            self.browser_ctrl_key(id, key);
            return;
        }
        // The find bar takes the keys while it is open.
        if b.find.is_open() {
            use osjeff_core::web::find::FindOutcome;
            let out = b.find.on_key(key, shift, b.page.as_ref());
            if out == FindOutcome::Changed
                && let Some(y) = b.find.current_y()
            {
                // Bring the match into the middle of the view.
                let target = (y - view_h / 2).max(0);
                let cur = b.scroll;
                self.scroll_page(id, target - cur);
            }
            let _ = out;
            return;
        }
        // A focused form control gets the key first.
        if b.forms.focus().is_some()
            && let Some(page) = &b.page
        {
            use osjeff_core::web::form::FormOutcome;
            if key == Key::Tab {
                let forms = &page.forms;
                if !b.forms.tab(forms, shift) {
                    b.browser.set_bar_focus(true);
                }
                self.browser_scroll_to_focus(id);
                return;
            }
            match b.forms.on_key(&page.forms, key) {
                FormOutcome::Submit { form, submitter } => {
                    self.browser_submit_form(id, form, submitter)
                }
                FormOutcome::Blur => {}
                FormOutcome::Changed => self.browser_scroll_to_focus(id),
                FormOutcome::Ignored => match key {
                    Key::PageUp => {
                        self.scroll_page(id, -(view_h - 40));
                    }
                    Key::PageDown => {
                        self.scroll_page(id, view_h - 40);
                    }
                    Key::Up => {
                        self.scroll_page(id, -48);
                    }
                    Key::Down => {
                        self.scroll_page(id, 48);
                    }
                    _ => {}
                },
            }
            return;
        }
        // Esc: close the suggestion list, drop the selection, else close the window.
        if key == Key::Esc {
            if b.browser.suggestions().is_empty() && b.sel.is_none() {
                self.request_close(id);
            } else if let Some(b) = self.browser_state_mut(id) {
                b.browser.on_key(Key::Esc);
                b.sel = None;
            }
            return;
        }
        let page_focus = !b.browser.bar_focus();
        match key {
            Key::PageUp => {
                self.scroll_page(id, -(view_h - 40));
            }
            Key::PageDown => {
                self.scroll_page(id, view_h - 40);
            }
            Key::Home if page_focus => {
                self.scroll_page(id, i32::MIN / 2);
            }
            Key::End if page_focus => {
                self.scroll_page(id, i32::MAX / 2);
            }
            Key::Char(b' ') if page_focus => {
                self.scroll_page(id, if shift { -(view_h - 40) } else { view_h - 40 });
            }
            Key::Tab => {
                // Into the page's controls (if it has any).
                let has = b.page.as_ref().is_some_and(|p| !p.forms.is_empty());
                if has && let Some(page) = &b.page {
                    b.browser.set_bar_focus(false);
                    if !b.forms.tab(&page.forms, shift) {
                        b.browser.set_bar_focus(true);
                    }
                    self.browser_scroll_to_focus(id);
                }
            }
            Key::Up | Key::Down => {
                // The suggestion list uses them; otherwise they scroll.
                if !b.browser.on_key(key) {
                    self.scroll_page(id, if key == Key::Up { -48 } else { 48 });
                }
            }
            _ => {
                b.browser.set_bar_focus(true);
                b.browser.on_key(key);
            }
        }
    }

    /// A Ctrl chord from a menu for browser window `id`.
    pub(crate) fn browser_ctrl_chord(&mut self, id: WindowId, c: char) {
        if c.is_ascii() {
            self.browser_ctrl_key(id, Key::Char(c as u8));
        }
    }

    /// Ctrl+D favourite, Ctrl+F find, Ctrl+plus/minus/0 zoom, Ctrl+L address bar.
    fn browser_ctrl_key(&mut self, id: WindowId, key: Key) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let content = BrowserChrome::of(rect).content;
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        match key {
            Key::Char(b'd') | Key::Char(b'D') => {
                b.notice = Some(String::from(match b.browser.toggle_bookmark() {
                    Some(true) => "Favorito adicionado",
                    Some(false) => "Favorito removido",
                    None => "Nada para guardar aqui",
                }));
            }
            Key::Char(b'f') | Key::Char(b'F') => {
                b.find.open(b.page.as_ref());
                b.browser.set_bar_focus(false);
            }
            Key::Char(b'l') | Key::Char(b'L') => {
                b.find.close();
                b.forms.blur();
                b.browser.select_bar();
            }
            Key::Char(b'=')
            | Key::Char(b'+')
            | Key::Char(b'-')
            | Key::Char(b'_')
            | Key::Char(b'0') => {
                use osjeff_core::web::{zoom_in, zoom_out};
                b.zoom = match key {
                    Key::Char(b'=') | Key::Char(b'+') => zoom_in(b.zoom),
                    Key::Char(b'-') | Key::Char(b'_') => zoom_out(b.zoom),
                    _ => 100,
                };
                b.notice = Some(alloc::format!("Zoom {}%", b.zoom));
                let old = b.scroll;
                b.layout_w = content.w;
                super::layout_browser(b, content.w, false);
                let max = b.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
                b.scroll = old.clamp(0, max);
            }
            _ => {}
        }
    }

    /// Submit form `form` (Enter in a field, or a button): navigate to the GET URL, or say why not.
    fn browser_submit_form(&mut self, id: WindowId, form: usize, submitter: Option<usize>) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let Some(page) = &b.page else {
            return;
        };
        match b.forms.target(&page.forms, form, submitter) {
            Ok(href) => {
                b.forms.blur();
                b.browser.set_bar_focus(false);
                if !b.browser.open_link(href.as_bytes()) {
                    b.notice = Some(String::from("endereco do formulario invalido"));
                }
            }
            Err(e) => b.notice = Some(String::from(e.message())),
        }
    }

    /// Scroll the page so the focused form control is visible.
    fn browser_scroll_to_focus(&mut self, id: WindowId) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let view_h = BrowserChrome::of(rect).content.h;
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let (Some(page), Some((f, i))) = (&b.page, b.forms.focus()) else {
            return;
        };
        let Some(fb) = page.fields.iter().find(|x| x.form == f && x.field == i) else {
            return;
        };
        let (top, bottom, scroll) = (fb.y, fb.y + fb.h, b.scroll);
        if top < scroll + 8 {
            self.scroll_page(id, top - 8 - scroll);
        } else if bottom > scroll + view_h - 40 {
            self.scroll_page(id, bottom - (scroll + view_h - 40));
        }
    }

    /// Resolve a click inside browser window `id`: toolbar buttons (home /
    /// reload / search) or, on the start page, the shortcut tiles.
    pub(crate) fn browser_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) -> bool {
        let ch = BrowserChrome::of(rect);
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        b.notice = None;
        // The suggestion list floats over everything below the bar.
        let n = b.browser.suggestions().len();
        if let Some(i) = osjeff_core::layout::browser_suggestion_at(ch.bar, n, px, py) {
            b.browser.pick_suggestion(i);
            b.browser.set_bar_focus(false);
            return false;
        }
        if ch.back.contains(px, py) {
            b.browser.back();
        } else if ch.forward.contains(px, py) {
            b.browser.forward();
        } else if ch.home.contains(px, py) {
            b.browser.go_home();
            b.page = None;
            b.doc = None;
            b.browser.set_bar_focus(true);
        } else if ch.reload.contains(px, py) {
            b.browser.reload();
        } else if ch.go.contains(px, py) {
            b.browser.submit();
        } else if ch.star.contains(px, py) {
            b.notice = Some(String::from(match b.browser.toggle_bookmark() {
                Some(true) => "Favorito adicionado",
                Some(false) => "Favorito removido",
                None => "Nada para guardar aqui",
            }));
        } else if ch.bar.contains(px, py) {
            b.browser.select_bar();
            b.forms.blur();
        } else if b.browser.can_continue_insecure()
            && osjeff_core::layout::browser_continue_button(ch.content).contains(px, py)
        {
            // The explicit, per-origin, per-session "continue anyway".
            b.browser.continue_insecure();
        } else if !b.browser.is_home() && ch.content.contains(px, py) && b.page.is_some() {
            let (qx, qy) = (px - ch.content.x, py - ch.content.y + b.scroll);
            b.browser.set_bar_focus(false);
            b.sel = None;
            let page = b.page.as_ref();
            if let Some(f) = page.and_then(|p| p.field_at(qx, qy)).copied()
                && let Some(page) = &b.page
            {
                // A control: focus a text box, press a button.
                if f.kind == osjeff_core::web::form::FieldKind::Submit {
                    b.forms.set_focus(&page.forms, f.form, f.field);
                    self.browser_submit_form(id, f.form, Some(f.field));
                } else {
                    b.forms.set_focus(&page.forms, f.form, f.field);
                }
                return false;
            }
            b.forms.blur();
            if let Some(href) = page.and_then(|p| p.link_at(qx, qy)) {
                // A click on link text: resolve it against the page and navigate.
                let href = href.as_bytes().to_vec();
                b.browser.open_link(&href);
            } else {
                // Empty page area: start selecting text.
                b.sel_anchor = Some((qx, qy));
                return true;
            }
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
        false
    }

    /// While the left button drags on a browser page: extend the selection to the pointer.
    fn browser_select_drag(&mut self, id: WindowId) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let content = BrowserChrome::of(rect).content;
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let (Some(a), Some(page)) = (b.sel_anchor, &b.page) else {
            return;
        };
        let cur = (cx - content.x, cy - content.y + b.scroll);
        let sel = page.select(a, cur);
        if sel != b.sel {
            b.sel = sel;
        }
        // Dragging past the top or bottom edge scrolls.
        if cy < content.y + 4 {
            self.scroll_page(id, -24);
        } else if cy > content.bottom() - 4 {
            self.scroll_page(id, 24);
        }
        self.mark_dirty(rect);
    }

    /// Copy the focused app's current text (terminal input line / calculator display /
    /// browser URL; the editor copies its selection itself) into the shared clipboard.
    pub(crate) fn copy_from_focused(&mut self) {
        let Some(top) = self.focused() else {
            return;
        };
        // Snapshot to a local buffer so the immutable borrow of the app ends
        // before mutably borrowing the clipboard.
        let mut tmp = [0u8; clipboard::CAP];
        let mut n = 0;
        // Selected page text wins over the address bar.
        if let Some(w) = self.wm.get(top)
            && let App::Browser(b) = &w.app.app
            && let (Some(sel), Some(page)) = (b.sel, &b.page)
        {
            let text = page.selection_text(sel);
            let n = text.len().min(clipboard::CAP);
            tmp[..n].copy_from_slice(&text.as_bytes()[..n]);
            self.clipboard.set(&tmp[..n]);
            return;
        }
        let typed;
        if let Some(w) = self.wm.get(top) {
            let text: &[u8] = match &w.app.app {
                App::Terminal(t) => {
                    typed = t.input().into_bytes();
                    &typed
                }
                App::Calculator(c) => c.display(),
                App::Browser(b) => b.browser.url(),
                App::Editor(_)
                | App::Tarefas(_)
                | App::Wasm(_)
                | App::Files(_)
                | App::Viewer(_)
                | App::Settings(_)
                | App::Gallery(_)
                | App::Log(_) => &[],
            };
            n = text.len().min(clipboard::CAP);
            tmp[..n].copy_from_slice(&text[..n]);
        }
        self.clipboard.set(&tmp[..n]);
    }

    /// Paste the clipboard into the focused app by replaying its bytes through
    /// the app's normal key handler (so editor line breaks, etc. just work).
    pub(crate) fn paste_into_focused(&mut self) {
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

        if self.kind_of(top) == Some(Kind::Terminal) {
            self.term_paste(top, data);
            return;
        }
        match self.app_mut(top) {
            Some(App::Calculator(c)) => {
                for &b in data {
                    c.input(b);
                }
            }
            Some(App::Browser(b)) => {
                if b.forms.focus().is_some()
                    && let Some(page) = &b.page
                {
                    let text = String::from_utf8_lossy(data).into_owned();
                    b.forms.insert_str(&page.forms, &text);
                } else {
                    for &ch in data {
                        if ch != b'\n' && ch != b'\r' {
                            b.browser.on_key(Key::Char(ch));
                        }
                    }
                }
            }
            Some(
                App::Terminal(_)
                | App::Editor(_)
                | App::Tarefas(_)
                | App::Wasm(_)
                | App::Files(_)
                | App::Viewer(_)
                | App::Settings(_)
                | App::Gallery(_)
                | App::Log(_),
            )
            | None => {}
        }
    }

    // ---- mouse ----

    /// Is the pointer over something clickable in a browser page (a link, a button, a
    /// suggestion)? Then the cursor is drawn as a hand.
    pub(crate) fn cursor_is_hand(&self) -> bool {
        if self.drag.is_some() || self.overlay_open() {
            return false;
        }
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        let Some(w) = self.topmost_at(cx, cy) else {
            return false;
        };
        let Some(win) = self.wm.get(w) else {
            return false;
        };
        let App::Browser(b) = &win.app.app else {
            return false;
        };
        let ch = BrowserChrome::of(win.rect);
        // Toolbar buttons and the suggestion list.
        let n = b.browser.suggestions().len();
        if osjeff_core::layout::browser_suggestion_at(ch.bar, n, cx, cy).is_some() {
            return true;
        }
        if [ch.back, ch.forward, ch.reload, ch.home, ch.go, ch.star]
            .iter()
            .any(|r| r.contains(cx, cy))
        {
            return true;
        }
        if b.browser.is_home() || !ch.content.contains(cx, cy) {
            return false;
        }
        let Some(page) = &b.page else {
            return false;
        };
        let (qx, qy) = (cx - ch.content.x, cy - ch.content.y + b.scroll);
        page.link_at(qx, qy).is_some()
            || page
                .field_at(qx, qy)
                .is_some_and(|f| f.kind == osjeff_core::web::form::FieldKind::Submit)
    }

    /// A mouse-wheel step (`dz` > 0 = wheel toward the user = scroll down). As on desktop
    /// systems the window *under the pointer* gets it, focused or not, and the wheel does not
    /// change focus. Returns whether anything changed (the caller repaints). Ignored while a menu,
    /// the start panel or the Alt+Tab switcher is up, or a window is being dragged.
    pub fn handle_wheel(&mut self, dz: i32) -> bool {
        if dz != 0 && self.shell.apps.is_some() {
            return self.shell_wheel(dz);
        }
        if dz == 0 || self.overlay_open() || self.drag.is_some() {
            return false;
        }
        let Some(w) = self.topmost_at(self.cursor_x, self.cursor_y) else {
            return false;
        };
        let Some(kind) = self.kind_of(w) else {
            return false;
        };
        let notches = dz.clamp(-8, 8);
        let changed = match kind {
            Kind::Browser => self.browser_wheel(w, notches),
            Kind::TaskMgr => {
                self.tarefas_wheel(w, notches);
                true
            }
            Kind::Files => {
                self.files_wheel(w, notches);
                true
            }
            Kind::Viewer => {
                // Wheel away from the user zooms in.
                self.viewer_wheel(w, -notches);
                true
            }
            Kind::Editor => {
                self.editor_wheel(w, -notches);
                true
            }
            Kind::Terminal => {
                // Wheel away from the user scrolls back in time.
                self.term_wheel(w, -notches);
                true
            }
            // Nothing to scroll (the WASM guest has no wheel ABI).
            Kind::LogViewer => {
                self.log_wheel(w, notches);
                true
            }
            Kind::Settings => {
                self.settings_wheel(w, notches);
                true
            }
            Kind::Calculator | Kind::WasmApp | Kind::Gallery => false,
        };
        if changed && let Some(r) = self.wm.get(w).map(|win| self.window_box(win)) {
            // The target may not be the focused window: make sure it is uploaded.
            self.mark_dirty(r);
        }
        changed
    }

    /// Maximize / restore window `id` and make the next frame repaint the whole
    /// screen (the window and everything it uncovers change).
    pub(crate) fn toggle_maximize(&mut self, id: WindowId) {
        let work = self.work_area();
        if self.wm.toggle_maximize(id, work) {
            self.geometry_changed(id);
        }
    }

    /// A window's rectangle changed by more than a drag step (maximise, snap, restore): repaint
    /// the screen and lay out the apps whose content depends on their width.
    pub(crate) fn geometry_changed(&mut self, id: WindowId) {
        self.force_full = true;
        self.relayout_browser(id);
        self.relayout_viewer(id);
    }

    /// Show workspace `to` (windows slide) and put overlays away.
    pub(crate) fn go_workspace(&mut self, to: u8) {
        self.close_transients();
        self.shell.snap = None;
        if self.wm.switch_workspace(to) {
            self.drag = None;
            self.title_hover = None;
            self.force_full = true;
        }
    }

    /// Move window `id` to workspace `to` and go there with it.
    pub(crate) fn move_window_to_workspace(&mut self, id: WindowId, to: u8) {
        if self.wm.move_to_workspace(id, to) {
            self.force_full = true;
            self.go_workspace(to);
            self.wm.activate(id);
        }
    }

    /// Tile window `id` to `zone` of the work area (animated).
    pub(crate) fn snap_window(&mut self, id: WindowId, zone: osjeff_core::snap::SnapZone) {
        let work = self.work_area();
        if self.wm.snap_to(id, zone, work) {
            self.geometry_changed(id);
        }
    }

    /// `Alt+arrow` on the focused window. `true` when there was a window to act on.
    fn snap_key(&mut self, arrow: osjeff_core::snap::Arrow) -> bool {
        use osjeff_core::snap::{SnapAct, key_action};
        let Some(id) = self.focused() else {
            return false;
        };
        let Some(state) = self.wm.get(id).map(|w| w.snap_state()) else {
            return false;
        };
        match key_action(state, arrow) {
            SnapAct::Zone(z) => self.snap_window(id, z),
            SnapAct::Restore => {
                if self.wm.unmaximize(id) {
                    self.geometry_changed(id);
                }
            }
            SnapAct::Minimize => {
                self.wm.minimize(id);
                self.force_full = true;
            }
            SnapAct::Stay => {}
        }
        true
    }

    /// A left press on window `w` at `(cx, cy)`: title-bar buttons, resize
    /// border, title (drag / double-click maximize) or app content.
    fn click_window(&mut self, w: WindowId, cx: i32, cy: i32) {
        let Some(win) = self.wm.get(w) else {
            return;
        };
        let (rect, resizable, maximized) = (win.rect, win.resizable, win.maximized);
        let tiled = win.snap_state().is_some();
        let kind = win.app.kind();
        match rect.title_button_at(resizable, true, cx, cy) {
            Some(TitleBtn::Close) => {
                self.request_close(w);
                self.drag = None;
                return;
            }
            Some(TitleBtn::Minimize) => {
                self.wm.minimize(w);
                self.drag = None;
                self.clicks.reset();
                return;
            }
            Some(TitleBtn::Maximize) => {
                self.toggle_maximize(w);
                self.clicks.reset();
                return;
            }
            Some(TitleBtn::Menu) => {
                self.wm.raise(w);
                self.clicks.reset();
                self.open_window_menu(w);
                return;
            }
            None => {}
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
        if cy >= rect.y && cy < rect.y + TITLE_H {
            let double = self.clicks.press(crate::interrupts::ticks(), cx, cy, w);
            if double && resizable {
                self.toggle_maximize(w);
            } else if maximized || tiled {
                // Dragging a maximised or tiled window frees it once the pointer has moved.
                self.drag = Some(Drag {
                    win: w,
                    mode: DragMode::Unsnap { ox: cx, oy: cy },
                });
            } else {
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
            Kind::Calculator => self.calc_click(w, rect, cx, cy),
            Kind::Browser => {
                if self.browser_click(w, rect, cx, cy) {
                    self.drag = Some(Drag {
                        win: w,
                        mode: DragMode::PageSelect,
                    });
                }
            }
            Kind::WasmApp => {
                // The press is delivered by `wasm_pointer` (content-local coordinates);
                // the window grabs the button so drags and the release reach it.
                let c = wasm_content(rect);
                if cx >= c.x && cy >= c.y && cx < c.x + c.w && cy < c.y + c.h {
                    self.wasm_grab = Some(w);
                }
            }
            Kind::Files => self.files_click(w, rect, cx, cy, false),
            Kind::Viewer => self.viewer_click(w, rect, cx, cy),
            Kind::TaskMgr => self.tarefas_click(w, rect, cx, cy),
            Kind::Settings => self.settings_click(w, rect, cx, cy),
            Kind::Gallery => self.gallery_click(w, rect, cx, cy),
            Kind::LogViewer => self.log_click(w, rect, cx, cy),
            Kind::Editor => self.editor_click(w, rect, cx, cy),
            Kind::Terminal => {}
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

        // Hover of the shell layers (panel, menus, Apps, Busca) and the app bar.
        if cursor_moved {
            if self.shell_pointer(cx, cy) {
                scene = true;
            }
            self.dock_pointer(cx, cy);
        }

        // Right click: an app-bar icon's menu, the file manager's own menu, or the
        // desktop menu on empty space.
        if right_pressed {
            let files_right = !self.overlay_open()
                && self.topmost_at(cx, cy).is_some_and(|w| {
                    self.kind_of(w) == Some(Kind::Files)
                        && self.wm.get(w).is_some_and(|win| !win.rect.on_title(cx, cy))
                });
            if let Some((item, _)) = self.panel_item_at(cx, cy).filter(|_| !self.modal_open()) {
                self.close_transients();
                self.panel_context(item);
                scene = true;
            } else if let Some(h) = self.dock_item_at(cx, cy).filter(|_| !self.modal_open()) {
                self.close_transients();
                self.dock_context(h, cx, cy);
                scene = true;
            } else if self.overlay_open() {
                self.close_transients();
                scene = true;
            } else if files_right && let Some(w) = self.topmost_at(cx, cy) {
                self.wm.raise(w);
                if let Some(rect) = self.wm.get(w).map(|win| win.rect) {
                    self.files_click(w, rect, cx, cy, true);
                }
                scene = true;
            } else if self.wasm_pointer_target(cx, cy).is_none()
                && self.topmost_at(cx, cy).is_none()
                && cy >= MENUBAR_H
            {
                self.desktop_context(cx, cy);
                scene = true;
            }
        }

        if left_pressed {
            if self
                .toasts
                .click(cx, cy, self.sw, self.sh, toasts_ui::now_ms())
            {
                // A click on a toast only dismisses it.
                self.toast_dirty = true;
            } else if self.shell_click(cx, cy) {
                scene = true;
            } else if let Some(h) = self.dock_item_at(cx, cy) {
                self.dock_press(h, cx, cy);
                scene = true;
            } else if let Some(w) = self.topmost_at(cx, cy) {
                self.click_window(w, cx, cy);
                scene = true;
            }
        }

        if released {
            self.dock_release(cx, cy);
        }
        if released && let Some(d) = self.drag.take() {
            if matches!(d.mode, DragMode::Ui) {
                self.live_drop(d.win);
            }
            // A resized browser lays its page out again for the new width.
            if matches!(d.mode, DragMode::Resize { .. }) {
                self.relayout_browser(d.win);
                self.relayout_viewer(d.win);
            }
            // Dropped on an edge: tile the window where the preview showed.
            if let Some(p) = self.shell.snap.take() {
                self.snap_window(d.win, p.zone);
                self.force_full = true;
            }
            // A press in a file manager ends: a click, a drop or the end of a band.
            if matches!(d.mode, DragMode::Files) {
                self.files_release(d.win, cx, cy);
                scene = true;
            }
            // A dragged picture glides on after the button is released.
            if matches!(d.mode, DragMode::Pan { .. }) {
                self.viewer_release(d.win);
            }
        }

        if let Some(d) = &self.drag {
            if left {
                let (w, mode) = (d.win, d.mode);
                let (sw, sh) = (self.sw, self.sh);
                match mode {
                    DragMode::Pan { last_x, last_y } => {
                        self.viewer_pan(w, cx - last_x, cy - last_y);
                        self.drag = Some(Drag {
                            win: w,
                            mode: DragMode::Pan {
                                last_x: cx,
                                last_y: cy,
                            },
                        });
                    }
                    DragMode::Select => self.editor_drag(w, cx, cy),
                    DragMode::Move { grab_dx, grab_dy } => {
                        self.wm.move_to(w, cx - grab_dx, cy - grab_dy, sw, sh);
                        self.update_snap_preview(w, cx, cy);
                    }
                    DragMode::Unsnap { ox, oy } => {
                        if (cx - ox).abs() >= 4 || (cy - oy).abs() >= 4 {
                            if let Some((gx, gy)) = self.wm.restore_for_drag(w, cx, cy) {
                                self.drag = Some(Drag {
                                    win: w,
                                    mode: DragMode::Move {
                                        grab_dx: gx,
                                        grab_dy: gy,
                                    },
                                });
                                self.geometry_changed(w);
                                self.wm.move_to(w, cx - gx, cy - gy, sw, sh);
                            } else {
                                self.drag = None;
                            }
                        }
                    }
                    DragMode::Resize {
                        edge,
                        start,
                        ox,
                        oy,
                    } => {
                        self.wm.resize(w, edge, start, (cx - ox, cy - oy), (sw, sh));
                    }
                    DragMode::PageSelect => self.browser_select_drag(w),
                    DragMode::Ui => self.live_drag(w, cx, cy),
                    DragMode::Files => self.files_drag(w, cx, cy),
                }
                // NOT scene_dirty: a drag is driven by the per-frame damage path
                // (keyed on `cursor_moved`), which repaints only the window's
                // old+new rect. Marking the whole scene dirty would force a full
                // recompose + 8 MiB blit every mouse step — the very cost we're
                // removing. The dragged window is kept out of the static layer
                // (see `is_dynamic`), so moving it never touches the others.
            } else {
                self.drag = None;
                self.shell.snap = None;
            }
        }

        // The system apps repaint when what the pointer is over (or the button state) changes.
        if self.drag.is_none()
            && (cursor_moved || left != self.prev_left)
            && self.live_hover(cx, cy, left)
        {
            scene = true;
        }

        // Hover: the window under the cursor shows its minimize / maximize
        // buttons. Only an enter / leave changes pixels; skipped while dragging.
        if self.drag.is_none() {
            let hov = self.topmost_at(cx, cy);
            // The item or button under the pointer in a file manager or viewer lights up.
            if let Some(h) = hov
                && cursor_moved
                && !self.overlay_open()
            {
                let changed = match self.kind_of(h) {
                    Some(Kind::Files) => self.files_hover(h, cx, cy),
                    Some(Kind::Viewer) => self.viewer_hover(h, cx, cy),
                    Some(Kind::Editor) => self.editor_hover(h, cx, cy),
                    _ => false,
                };
                if changed && let Some(win) = self.wm.get(h) {
                    let b = self.window_box(win);
                    self.mark_dirty(b);
                }
            }
            if hov != self.hover {
                match self.hover.and_then(|o| self.kind_of(o).map(|k| (o, k))) {
                    Some((o, Kind::Files)) => self.files_unhover(o),
                    Some((o, Kind::Viewer)) => self.viewer_unhover(o),
                    Some((o, Kind::Editor)) => self.editor_unhover(o),
                    _ => {}
                }
                for id in [self.hover, hov].into_iter().flatten() {
                    if let Some(r) = self.wm.get(id).map(|w| self.window_box(w)) {
                        self.mark_dirty(r);
                    }
                }
                self.hover = hov;
                scene = true;
            }
            // Which title-bar button the pointer is on (its hover fill).
            let btn = hov
                .and_then(|id| self.wm.get(id))
                .and_then(|w| {
                    w.rect
                        .title_button_at(w.resizable, true, cx, cy)
                        .map(|b| (w.id, b))
                })
                .filter(|_| !self.overlay_open());
            if btn != self.title_hover {
                for id in [self.title_hover.map(|t| t.0), btn.map(|t| t.0)]
                    .into_iter()
                    .flatten()
                {
                    if let Some(r) = self.wm.get(id).map(|w| self.window_box(w)) {
                        self.mark_dirty(r);
                    }
                }
                self.title_hover = btn;
                scene = true;
            }
        }

        // Moving over an open menu / start panel updates the hover highlight,
        // but it is NOT a full-scene change: the compositor repaints only the
        // overlay's rectangle on `cursor_moved` (see the overlay path in the
        // main loop), so we deliberately do not set `scene` here.

        self.wasm_pointer(left, right, cursor_moved);
        self.prev_left = left;
        self.prev_right = right;
        MouseResult {
            scene_dirty: scene,
            cursor_moved,
        }
    }
}
