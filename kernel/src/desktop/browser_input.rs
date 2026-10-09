//! Keys, clicks and drags in the browser window.
//!
//! Keys go, in this order, to: an open context menu or popover (Esc), Ctrl chords (tabs,
//! favourites, find, zoom, address), the find bar, a focused form control, then the page
//! (scrolling) or the omnibox. Clicks are resolved against the same geometry the window is
//! drawn with (`osjeff_core::layout`).

use super::BrowserHover as H;
use super::browser::{layout_browser, page_menu_row_at, start_items};
use super::*;
use osjeff_core::browser::Status;
use osjeff_core::layout as geo;
use osjeff_core::t;
use osjeff_core::web::form::{FieldKind, FormOutcome};
use osjeff_core::web::textops::Selection;

/// A page step with the keyboard (Page Down, Space): a view less a little overlap.
fn page_step(view_h: i32) -> i32 {
    (view_h - 48).max(48)
}

impl Desktop {
    // ---- keyboard ----

    /// Keys for the browser window `id`.
    pub(crate) fn browser_key(&mut self, id: WindowId, key: Key) {
        let ctrl = self.keymap.ctrl();
        let shift = self.keymap.shift();
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        // Whatever a key does here stays inside the client area, unless a handler below says
        // otherwise (the title changes, the window closes).
        self.client_dirty = Some(id);
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let view_h = b.chrome(rect).content.h;
        b.notice = None;
        b.notice_flash.hide();
        b.touch();
        // A menu or the popover: Esc closes it, every other key is for nobody.
        if b.ctx.is_some() || b.popover {
            if key == Key::Esc {
                b.ctx = None;
                b.popover = false;
                b.glass[1].clear();
                b.glass[3].clear();
            }
            return;
        }
        if ctrl {
            self.browser_ctrl_key(id, key, shift);
            return;
        }
        // The find bar takes the keys while it is open.
        if b.find.is_open() {
            use osjeff_core::web::find::FindOutcome;
            let t = b.tabs.active_mut();
            let out = t
                .find
                .on_key(key, shift, t.page.as_ref(), &browser_paint::KernelMetrics);
            let y = t.find.current_y();
            if out == FindOutcome::Closed {
                b.glass[2].clear();
            }
            if out == FindOutcome::Changed
                && let Some(y) = y
            {
                // Bring the match into the middle of the view.
                self.scroll_page_to(id, (y - view_h / 2).max(0));
            }
            return;
        }
        // A focused form control gets the key first.
        let has_focus = b.forms.focus().is_some() && b.page.is_some();
        if has_focus {
            let t = b.tabs.active_mut();
            let Some(page) = &t.page else { return };
            if key == Key::Tab {
                if !t.forms.tab(&page.forms, shift) {
                    t.browser.set_bar_focus(true);
                }
                self.browser_scroll_to_focus(id);
                return;
            }
            match t.forms.on_key(&page.forms, key) {
                FormOutcome::Submit { form, submitter } => {
                    self.browser_submit_form(id, form, submitter);
                }
                FormOutcome::Blur => {}
                FormOutcome::Changed => self.browser_scroll_to_focus(id),
                FormOutcome::Ignored => match key {
                    Key::PageUp => {
                        self.scroll_page(id, -page_step(view_h));
                    }
                    Key::PageDown => {
                        self.scroll_page(id, page_step(view_h));
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
        // Esc: close the suggestion list, drop the selection, stop a load. It never closes the window.
        if key == Key::Esc {
            let t = b.tabs.active_mut();
            if !t.browser.suggestions().is_empty() || t.sel.is_some() {
                t.browser.on_key(Key::Esc);
                t.sel = None;
            } else if t.browser.is_loading() {
                self.browser_stop(id);
            } else if t.browser.bar_focus() && !t.browser.is_home() {
                t.browser.set_bar_focus(false);
            }
            return;
        }
        let t = b.tabs.active_mut();
        let page_focus = !t.browser.bar_focus();
        match key {
            Key::PageUp => {
                self.scroll_page(id, -page_step(view_h));
            }
            Key::PageDown => {
                self.scroll_page(id, page_step(view_h));
            }
            Key::Home if page_focus => {
                self.scroll_page(id, i32::MIN / 2);
            }
            Key::End if page_focus => {
                self.scroll_page(id, i32::MAX / 2);
            }
            Key::Char(b' ') if page_focus => {
                self.scroll_page(
                    id,
                    if shift {
                        -page_step(view_h)
                    } else {
                        page_step(view_h)
                    },
                );
            }
            Key::Tab => {
                // Into the page's controls (if it has any).
                let has = t.page.as_ref().is_some_and(|p| !p.forms.is_empty());
                if has && let Some(page) = &t.page {
                    t.browser.set_bar_focus(false);
                    if !t.forms.tab(&page.forms, shift) {
                        t.browser.set_bar_focus(true);
                    }
                    self.browser_scroll_to_focus(id);
                }
            }
            Key::Up | Key::Down => {
                // The suggestion list uses them; on a page they scroll.
                if !t.browser.on_key(key) {
                    self.scroll_page(id, if key == Key::Up { -64 } else { 64 });
                }
            }
            Key::Left | Key::Right if page_focus => {
                self.scroll_page(id, if key == Key::Left { -64 } else { 64 });
            }
            _ => {
                t.browser.set_bar_focus(true);
                t.forms.blur();
                t.browser.on_key(key);
                // Enter hands the keyboard to the page; typing or Ctrl+L brings the bar back.
                if key == Key::Enter && (t.browser.is_loading() || !t.browser.is_home()) {
                    t.browser.set_bar_focus(false);
                }
            }
        }
    }

    /// A Ctrl chord from the menu bar for browser window `id`.
    pub(crate) fn browser_ctrl_chord(&mut self, id: WindowId, c: char) {
        if c.is_ascii() {
            self.browser_ctrl_key(id, Key::Char(c as u8), false);
        }
    }

    /// Ctrl+T/W new and close tab, Ctrl+Tab and Ctrl+1..9 switch, Ctrl+D favourite, Ctrl+F
    /// find, Ctrl+plus/minus/0 zoom, Ctrl+L and Ctrl+R.
    fn browser_ctrl_key(&mut self, id: WindowId, key: Key, shift: bool) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let content = b.chrome(rect).content;
        match key {
            Key::Char(b't' | b'T') => {
                self.browser_new_tab(id);
                if let Some(b) = self.browser_state_mut(id) {
                    b.tabs.active_mut().browser.select_bar();
                }
            }
            Key::Char(b'w' | b'W') => {
                let i = b.tabs.active_index();
                self.browser_close_tab(id, i);
            }
            Key::Tab | Key::PageDown | Key::PageUp => {
                let back = key == Key::PageUp || (key == Key::Tab && shift);
                let n = b.tabs.len();
                let cur = b.tabs.active_index();
                let to = if back {
                    (cur + n - 1) % n
                } else {
                    (cur + 1) % n
                };
                self.browser_select_tab(id, to);
            }
            Key::Char(d @ b'1'..=b'9') => {
                let n = usize::from(d - b'0');
                let cur = b.tabs.active_index();
                let to = if n == 9 { b.tabs.len() - 1 } else { n - 1 };
                if to < b.tabs.len() && to != cur {
                    self.browser_select_tab(id, to);
                }
            }
            Key::Char(b'd' | b'D') => self.browser_toggle_bookmark(id),
            Key::Char(b'f' | b'F') => {
                let t = b.tabs.active_mut();
                if t.page.is_some() {
                    t.find.open(t.page.as_ref(), &browser_paint::KernelMetrics);
                    t.browser.set_bar_focus(false);
                    b.glass[2].clear();
                }
            }
            Key::Char(b'l' | b'L') => {
                let t = b.tabs.active_mut();
                t.find.close();
                t.forms.blur();
                t.browser.select_bar();
            }
            Key::Char(b'r' | b'R') => self.browser_reload(id),
            Key::Char(b'=' | b'+' | b'-' | b'_' | b'0') => {
                use osjeff_core::web::{zoom_in, zoom_out};
                let BrowserState { tabs, images, .. } = &mut *b;
                let t = tabs.active_mut();
                t.zoom = match key {
                    Key::Char(b'=' | b'+') => zoom_in(t.zoom),
                    Key::Char(b'-' | b'_') => zoom_out(t.zoom),
                    _ => 100,
                };
                let old = t.scroll;
                t.layout_w = content.w;
                layout_browser(t, images, content.w, false);
                let max = t.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
                let y = old.clamp(0, max);
                t.scroll = y;
                t.scroll_spring.jump(y as f32);
                b.zoom_flash.show(1.2);
            }
            _ => {}
        }
        if let Some(b) = self.browser_state_mut(id) {
            b.touch();
        }
    }

    /// Reload the page of the active tab (the stop button while loading).
    pub(crate) fn browser_reload(&mut self, id: WindowId) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let t = b.tabs.active_mut();
        if t.browser.is_loading() {
            self.browser_stop(id);
            return;
        }
        t.browser.reload();
    }

    /// Ctrl+D, the star: add the page to the favourites or remove it.
    pub(crate) fn browser_toggle_bookmark(&mut self, id: WindowId) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let t = b.tabs.active_mut();
        let state = t.browser.toggle_bookmark();
        let msg = match state {
            Some(true) => t!("web.notice.bookmark_added"),
            Some(false) => t!("web.notice.bookmark_removed"),
            None => t!("web.notice.bookmark_none"),
        };
        t.star.retarget(
            f32::from(state == Some(true)),
            if state == Some(true) { 0.38 } else { 0.18 },
            osjeff_core::anim::curves::ENTER,
        );
        b.say(msg);
    }

    /// Close the active tab (Ctrl+W, the menu): the last tab closes the window.
    pub(crate) fn browser_close_active(&mut self, id: WindowId) {
        let i = self
            .browser_state_mut(id)
            .map_or(0, |b| b.tabs.active_index());
        self.browser_close_tab(id, i);
    }

    /// Submit form `form` (Enter in a field, or a button): navigate to the GET URL, or say why not.
    pub(crate) fn browser_submit_form(
        &mut self,
        id: WindowId,
        form: usize,
        submitter: Option<usize>,
    ) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let t = b.tabs.active_mut();
        let Some(page) = &t.page else {
            return;
        };
        match t.forms.target(&page.forms, form, submitter) {
            Ok(href) => {
                t.forms.blur();
                t.browser.set_bar_focus(false);
                if !t.browser.open_link(href.as_bytes()) {
                    b.say(t!("web.form.bad_action"));
                }
            }
            Err(e) => b.say(e.message()),
        }
    }

    /// Scroll the page so the focused form control is visible.
    fn browser_scroll_to_focus(&mut self, id: WindowId) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let view_h = b.chrome(rect).content.h;
        let t = b.tabs.active();
        let (Some(page), Some((f, i))) = (&t.page, t.forms.focus()) else {
            return;
        };
        let Some(fb) = page.fields.iter().find(|x| x.form == f && x.field == i) else {
            return;
        };
        let (top, bottom) = (fb.y, fb.y + fb.h);
        let scroll = t.scroll_spring.target() as i32;
        if top < scroll + 16 {
            self.scroll_page_to(id, top - 16);
        } else if bottom > scroll + view_h - 48 {
            self.scroll_page_to(id, bottom - (view_h - 48));
        }
    }

    // ---- mouse ----

    /// Resolve a click inside browser window `id`. Returns `true` when it starts selecting text
    /// (the caller then tracks the drag).
    pub(crate) fn browser_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) -> bool {
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        b.touch();
        b.notice = None;
        b.notice_flash.hide();
        let ch = b.chrome(rect);
        // An open context menu takes the click; outside it, it only closes.
        if let Some(m) = &b.ctx {
            let pick = page_menu_row_at(m, rect, px, py).and_then(|i| {
                m.items
                    .get(i)
                    .copied()
                    .filter(|(_, _, ok)| *ok)
                    .map(|(cmd, _, _)| cmd)
            });
            let link = m.link.clone();
            b.ctx = None;
            b.glass[3].clear();
            if let Some(cmd) = pick {
                self.browser_menu_command(id, cmd, link);
            }
            return false;
        }
        // The popover: a click in it does nothing, elsewhere it closes first.
        if b.popover {
            let h = 200;
            let pop = geo::browser_popover(ch.bar, rect, h);
            if ch.shield.contains(px, py) {
                b.popover = false;
                b.glass[1].clear();
                return false;
            }
            if pop.contains(px, py) {
                return false;
            }
            b.popover = false;
            b.glass[1].clear();
        }
        // The suggestion list floats over everything under the omnibox.
        let t = b.tabs.active_mut();
        let n = t.browser.suggestions().len();
        if let Some(i) = geo::browser_suggestion_at(ch.bar, n, px, py) {
            t.browser.pick_suggestion(i);
            t.browser.set_bar_focus(false);
            return false;
        }
        if ch.back.contains(px, py) {
            t.browser.back();
        } else if ch.forward.contains(px, py) {
            t.browser.forward();
        } else if ch.reload.contains(px, py) {
            self.browser_reload(id);
        } else if ch.newtab.contains(px, py) {
            self.browser_new_tab(id);
            if let Some(b) = self.browser_state_mut(id) {
                b.tabs.active_mut().browser.select_bar();
            }
        } else if ch.star.contains(px, py) && !t.browser.is_home() {
            self.browser_toggle_bookmark(id);
        } else if ch.shield.w > 0 && ch.shield.contains(px, py) {
            b.popover = true;
            b.glass[1].clear();
        } else if ch.bar.contains(px, py) {
            t.browser.select_bar();
            t.forms.blur();
        } else if ch.strip.h > 0 && ch.strip.contains(px, py) {
            let rects = geo::browser_tab_rects(ch.strip, &b.strip_weights());
            if let Some(ei) = geo::browser_tab_at(&rects, px, py)
                && let Some(&(_, ti)) = b.strip_tab_slots().iter().find(|(e, _)| *e == ei)
            {
                if geo::browser_tab_close(rects[ei]).contains(px, py) {
                    self.browser_close_tab(id, ti);
                } else {
                    self.browser_select_tab(id, ti);
                }
            }
        } else if t.find.is_open() && {
            let f = geo::browser_find_layout(ch.content);
            f.bar.contains(px, py)
        } {
            let f = geo::browser_find_layout(ch.content);
            if f.close.contains(px, py) {
                t.find.close();
                b.glass[2].clear();
            } else if f.next.contains(px, py) || f.prev.contains(px, py) {
                if f.next.contains(px, py) {
                    t.find.next();
                } else {
                    t.find.prev();
                }
                if let Some(y) = t.find.current_y() {
                    self.scroll_page_to(id, (y - ch.content.h / 2).max(0));
                }
            }
        } else if ch.content.contains(px, py) {
            return self.browser_content_click(id, ch.content, px, py);
        }
        false
    }

    /// A click in the page area: the start page, an error page or the page itself.
    fn browser_content_click(&mut self, id: WindowId, content: Rect, px: i32, py: i32) -> bool {
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        if b.tabs.active().browser.is_home() {
            let (tiles, recents) = start_items(b);
            let l = geo::browser_start_layout(content, tiles.len(), recents.len());
            let t = b.tabs.active_mut();
            if l.search.contains(px, py) {
                t.browser.select_bar();
            } else if let Some(i) = l.tiles.iter().position(|r| r.contains(px, py)) {
                t.browser.open(tiles[i].1.as_bytes());
            } else if let Some(i) = l.recents.iter().position(|r| r.contains(px, py)) {
                t.browser.open(recents[i].as_bytes());
            }
            return false;
        }
        let t = b.tabs.active_mut();
        if t.page.is_none() {
            if t.browser.status() == Status::Error {
                let cert = t.browser.can_continue_insecure();
                let e = geo::browser_error_layout(content, cert);
                if e.retry.contains(px, py) {
                    t.browser.retry();
                } else if cert && e.proceed.contains(px, py) {
                    // The explicit, per-site, per-session "continue anyway".
                    t.browser.continue_insecure();
                }
            }
            return false;
        }
        let (qx, qy) = (px - content.x, py - content.y + t.scroll);
        t.browser.set_bar_focus(false);
        t.sel = None;
        let page = t.page.as_ref();
        if let Some(f) = page.and_then(|p| p.field_at(qx, qy)).copied()
            && let Some(page) = &t.page
        {
            // A control: focus a text box, toggle a box, press a button.
            t.forms.set_focus(&page.forms, f.form, f.field);
            match f.kind {
                FieldKind::Submit => self.browser_submit_form(id, f.form, Some(f.field)),
                FieldKind::Checkbox | FieldKind::Radio => {
                    t.forms.toggle(&page.forms, f.form, f.field);
                }
                _ => {}
            }
            return false;
        }
        t.forms.blur();
        if let Some(href) = page.and_then(|p| p.link_at(qx, qy)) {
            // A click on link text: resolve it against the page and navigate.
            let href = href.as_bytes().to_vec();
            if !t.browser.open_link(&href) {
                b.say(t!("web.notice.link_bad"));
            }
            false
        } else {
            // Empty page area: start selecting text.
            t.sel_anchor = Some((qx, qy));
            true
        }
    }

    /// A double click on page text selects the word.
    pub(crate) fn browser_double_click(&mut self, id: WindowId, content: Rect, px: i32, py: i32) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let t = b.tabs.active_mut();
        let Some(page) = &t.page else { return };
        if !content.contains(px, py) {
            return;
        }
        let (qx, qy) = (px - content.x, py - content.y + t.scroll);
        if page.field_at(qx, qy).is_some() || page.link_at(qx, qy).is_some() {
            return;
        }
        t.sel = page.select_word(qx, qy, &browser_paint::KernelMetrics);
        t.sel_anchor = None;
        b.touch();
    }

    /// Right click in the page area: the context menu.
    pub(crate) fn browser_rclick(&mut self, id: WindowId, px: i32, py: i32) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let ch = b.chrome(rect);
        b.touch();
        b.popover = false;
        if !ch.content.contains(px, py) {
            b.ctx = None;
            return;
        }
        let t = b.tabs.active();
        let mut link = None;
        if let Some(page) = &t.page {
            let (qx, qy) = (px - ch.content.x, py - ch.content.y + t.scroll);
            link = page.link_at(qx, qy).map(String::from);
        }
        let has_sel = t.sel.is_some();
        let fav = t.browser.is_bookmarked();
        let can_mark = !t.browser.is_home();
        let items = alloc::vec![
            (PageCmd::Copy, t!("menu.edit.copy"), has_sel),
            (PageCmd::OpenLink, t!("web.menu.open_link"), link.is_some()),
            (PageCmd::CopyLink, t!("web.menu.copy_link"), link.is_some()),
            (PageCmd::Back, t!("web.menu.back"), t.browser.can_back()),
            (PageCmd::Reload, t!("web.menu.reload"), !t.browser.is_home()),
            (
                PageCmd::Bookmark,
                if fav {
                    t!("web.menu.bookmark_remove")
                } else {
                    t!("web.menu.bookmark_add")
                },
                can_mark,
            ),
        ];
        b.glass[3].clear();
        b.ctx = Some(PageMenu {
            x: px,
            y: py,
            link,
            items,
        });
        b.hover = H::None;
    }

    /// Run a context-menu command.
    fn browser_menu_command(&mut self, id: WindowId, cmd: PageCmd, link: Option<String>) {
        match cmd {
            PageCmd::Copy => self.copy_from_focused(),
            PageCmd::OpenLink => {
                if let (Some(href), Some(b)) = (link, self.browser_state_mut(id)) {
                    b.tabs.active_mut().browser.open_link(href.as_bytes());
                }
            }
            PageCmd::CopyLink => {
                if let Some(href) = link {
                    let n = href.len().min(clipboard::CAP);
                    self.clipboard.set(&href.as_bytes()[..n]);
                    if let Some(b) = self.browser_state_mut(id) {
                        b.say(t!("web.notice.copied"));
                    }
                }
            }
            PageCmd::Back => {
                if let Some(b) = self.browser_state_mut(id) {
                    b.tabs.active_mut().browser.back();
                }
            }
            PageCmd::Reload => self.browser_reload(id),
            PageCmd::Bookmark => self.browser_toggle_bookmark(id),
        }
    }

    /// While the left button drags on a browser page: extend the selection to the pointer.
    pub(crate) fn browser_select_drag(&mut self, id: WindowId) {
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return;
        };
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let content = b.chrome(rect).content;
        let t = b.tabs.active_mut();
        let (Some(a), Some(page)) = (t.sel_anchor, &t.page) else {
            return;
        };
        let cur = (cx - content.x, cy - content.y + t.scroll);
        let sel: Option<Selection> = page.select(a, cur, &browser_paint::KernelMetrics);
        if sel != t.sel {
            t.sel = sel;
            b.touch();
        }
        // Dragging past the top or bottom edge scrolls.
        if cy < content.y + 4 {
            self.scroll_page(id, -24);
        } else if cy > content.bottom() - 4 {
            self.scroll_page(id, 24);
        }
        self.mark_dirty(rect);
    }

    // ---- the pointer ----

    /// Is the pointer over something clickable in the browser window? Then it is a hand.
    pub(crate) fn browser_cursor_hand(&self, win: &Win, cx: i32, cy: i32) -> bool {
        let App::Browser(b) = &win.app.app else {
            return false;
        };
        let (h, link) = super::browser::browser_hover_at(b, win.rect, cx, cy);
        let t = b.tabs.active();
        match h {
            H::Back => t.browser.can_back(),
            H::Forward => t.browser.can_forward(),
            H::Reload | H::NewTab | H::Shield | H::Tab(_) | H::TabClose(_) => true,
            H::Star => !t.browser.is_home(),
            H::Suggestion(_)
            | H::MenuRow(_)
            | H::Tile(_)
            | H::Recent(_)
            | H::FindPrev
            | H::FindNext
            | H::FindClose
            | H::Retry
            | H::Proceed => true,
            H::Bar | H::None => {
                link.is_some()
                    || t.page.as_ref().is_some_and(|p| {
                        let ch = b.chrome(win.rect);
                        ch.content.contains(cx, cy)
                            && p.field_at(cx - ch.content.x, cy - ch.content.y + t.scroll)
                                .is_some_and(|f| {
                                    matches!(
                                        f.kind,
                                        FieldKind::Submit
                                            | FieldKind::PushButton
                                            | FieldKind::Checkbox
                                            | FieldKind::Radio
                                    )
                                })
                    })
            }
        }
    }

    /// Is the pointer over text the user can edit (the omnibox, a text field of the page)?
    pub(crate) fn browser_cursor_text(&self, win: &Win, cx: i32, cy: i32) -> bool {
        let App::Browser(b) = &win.app.app else {
            return false;
        };
        let ch = b.chrome(win.rect);
        if b.ctx.is_some() {
            return false;
        }
        let t = b.tabs.active();
        if ch.bar.contains(cx, cy) && !ch.star.contains(cx, cy) && !ch.shield.contains(cx, cy) {
            return true;
        }
        if t.browser.is_home() {
            let (tiles, recents) = start_items(b);
            let l = geo::browser_start_layout(ch.content, tiles.len(), recents.len());
            return l.search.contains(cx, cy);
        }
        t.page.as_ref().is_some_and(|p| {
            ch.content.contains(cx, cy)
                && p.field_at(cx - ch.content.x, cy - ch.content.y + t.scroll)
                    .is_some_and(|f| f.kind.is_text())
        })
    }
}
