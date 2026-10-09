//! The browser window: tabs, the hand-off with the network fetcher, laying pages out and
//! scrolling them. Drawing is in `browser_ui.rs` / `browser_paint.rs`, keys and clicks in
//! `browser_input.rs`.
//!
//! # Tabs and the single fetcher
//!
//! One window holds up to [`MAX_TABS`](kitsune_core::browser::tabs::MAX_TABS) tabs, each
//! with its own history, page, scroll, forms and find state ([`TabData`]). There is one
//! fetcher thread and it serves one request at a time, so tabs share it by queueing: the
//! main loop asks for a request, the active tab is asked first and the others after it in
//! order, and the answer goes back to the tab that asked (`req_tab`), active or not. A tab
//! that is not active when its page arrives keeps the parsed document and is laid out when
//! it is shown (inactive tabs do not hold a display list or pictures). Pictures are
//! fetched for the active tab only.

use crate::desktop::*;
use crate::text;
use kitsune_core::anim::{Tween, curves};
use kitsune_core::browser::tabs;
use kitsune_core::t;
use kitsune_core::web::imgcache::{ImageCache, PageImages, image_key};
use kitsune_core::web::{Cmd as WebCmd, Layout};

/// Wheel step: three lines of body text.
const WHEEL_PX: i32 = 3 * 24;

impl BrowserState {
    /// The tab with id `id`.
    pub(crate) fn tab_by_id(&mut self, id: u32) -> Option<(usize, &mut TabData)> {
        let i = self.tabs.iter().position(|t| t.id == id)?;
        self.tabs.get_mut(i).map(|t| (i, t))
    }

    /// Width of the security indicator for the page on screen (0: none).
    pub(crate) fn shield_width(&self) -> i32 {
        match self.security_badge() {
            Some((label, _, _)) => {
                8 + 16 + 6 + text::measure(label, text::FOOTNOTE, text::Weight::Medium) + 10
            }
            None => 0,
        }
    }

    /// The indicator of the page on screen: label, glyph, kind. `None` for the start page,
    /// the browser's own pages and a load that has not finished.
    pub(crate) fn security_badge(
        &self,
    ) -> Option<(&'static str, kitsune_core::iconart::Glyph, SecurityTone)> {
        use kitsune_core::browser::Security;
        use kitsune_core::iconart::Glyph;
        let b = &self.tabs.active().browser;
        if b.is_home() || b.is_internal() {
            return None;
        }
        match b.security() {
            Security::HttpsVerified => {
                Some((t!("web.sec.secure"), Glyph::Lock, SecurityTone::Good))
            }
            Security::Http => Some((t!("web.sec.insecure"), Glyph::Warning, SecurityTone::Warn)),
            Security::HttpsInvalid => {
                Some((t!("web.sec.invalid"), Glyph::Warning, SecurityTone::Bad))
            }
            Security::None => None,
        }
    }

    /// Chrome geometry for window `r` right now (the strip height is animated).
    pub(crate) fn chrome(&self, r: Rect) -> BrowserChrome {
        BrowserChrome::with_strip(
            r,
            self.shield_width(),
            (self.strip_h.value().max(0.0) + 0.5) as i32,
        )
    }

    /// The tab strip's entries as `(entry index, tab index)` for the real tabs.
    pub(crate) fn strip_tab_slots(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut ti = 0;
        for (i, e) in self.strip.iter().enumerate() {
            if e.id.is_some() {
                out.push((i, ti));
                ti += 1;
            }
        }
        out
    }

    /// Growth of each strip entry, 0..=256.
    pub(crate) fn strip_weights(&self) -> Vec<i32> {
        self.strip
            .iter()
            .map(|e| (e.weight.value().clamp(0.0, 1.0) * 256.0) as i32)
            .collect()
    }

    /// Does anything in the window still move (so it needs a frame every tick)?
    pub(crate) fn busy(&self) -> bool {
        let now = crate::desktop::shell::toasts::now_ms();
        self.tabs.iter().any(|t| {
            !t.scroll_spring.at_rest()
                || t.load.alpha() > 0
                || t.load.is_loading()
                || !t.star.finished()
                || t.scroll_bar.active(now)
        }) || !self.strip_h.finished()
            || self
                .strip
                .iter()
                .any(|e| !e.weight.finished() || e.id.is_none())
            || self.notice_flash.active()
            || self.zoom_flash.active()
    }

    /// Something changed that the painted page area depends on.
    pub(crate) fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// Show a line over the bottom of the page for a moment.
    pub(crate) fn say(&mut self, msg: &str) {
        self.notice = Some(String::from(msg));
        self.notice_flash.show(1.8);
    }
}

/// How alarming the security indicator is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SecurityTone {
    Good,
    Warn,
    Bad,
}

impl Desktop {
    /// The (single) browser window, if open.
    pub(crate) fn browser_id(&self) -> Option<WindowId> {
        self.wm
            .windows()
            .iter()
            .find(|w| w.app.kind() == Kind::Browser && !w.is_closing())
            .map(|w| w.id)
    }

    fn browser_win_rect(&self, id: WindowId) -> Option<Rect> {
        self.wm.get(id).map(|w| w.rect)
    }

    /// Per-frame stepping of the browser's own motion; returns whether anything moves.
    pub(crate) fn step_browser(&mut self, dt: f32) -> bool {
        let mut busy = false;
        let Some(id) = self.browser_id() else {
            return false;
        };
        if self.browser_win_rect(id).is_none() {
            return false;
        }
        let now = crate::desktop::shell::toasts::now_ms();
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        for t in b.tabs.iter_mut() {
            busy |= t.load.step(dt);
            busy |= t.star.step(dt);
            if t.scroll_spring.step(dt) || (t.scroll as f32 - t.scroll_spring.value()).abs() >= 0.5
            {
                let v = (t.scroll_spring.value().max(0.0) + 0.5) as i32;
                if v != t.scroll {
                    t.scroll = v;
                    busy = true;
                }
            }
            busy |= t.scroll_bar.active(now);
        }
        busy |= b.strip_h.step(dt);
        for e in b.strip.iter_mut() {
            busy |= e.weight.step(dt);
        }
        // Ghosts whose animation ended are gone.
        b.strip.retain(|e| e.id.is_some() || !e.weight.finished());
        busy |= b.notice_flash.step(dt);
        busy |= b.zoom_flash.step(dt);
        if !b.notice_flash.active() {
            b.notice = None;
        }
        busy
    }

    /// Does the browser window need a frame every tick right now?
    pub(crate) fn browser_busy(&self, w: &Win) -> bool {
        match &w.app.app {
            App::Browser(b) => b.busy(),
            _ => false,
        }
    }

    /// Set the window title; the title bar is outside the client area, so the whole window
    /// repaints.
    fn browser_set_title(&mut self, id: WindowId, title: String) {
        if let Some(w) = self.wm.get_mut(id)
            && w.app.title != title
        {
            w.app.title = title;
            // The title bar is outside the client area.
            self.client_dirty = None;
            self.force_full = true;
        }
    }

    /// The client area of `w` (below the title bar).
    pub(crate) fn client_rect(w_box: Rect) -> Rect {
        Rect::new(
            w_box.x,
            w_box.y + TITLE_H,
            w_box.w,
            (w_box.h - TITLE_H).max(0),
        )
    }

    // ---- network hand-off (driven by the kernel main loop) ----

    /// If a tab has a pending navigation, copy its target URL into `out`, return the length
    /// and remember the tab as the one the fetcher now works for. The active tab goes first,
    /// then the others in order. The kernel fetches it and reports back with
    /// [`browser_load`](Self::browser_load) / [`browser_fail`](Self::browser_fail).
    pub fn browser_take_request(&mut self, out: &mut [u8]) -> Option<usize> {
        let id = self.browser_id()?;
        let b = self.browser_state_mut(id)?;
        let n = b.tabs.len();
        let act = b.tabs.active_index();
        let order = core::iter::once(act).chain((0..n).filter(|&i| i != act));
        for i in order {
            let Some(t) = b.tabs.get_mut(i) else { continue };
            if let Some(url) = t.browser.take_request() {
                let len = url.len().min(out.len());
                out[..len].copy_from_slice(&url[..len]);
                t.cancelled = false;
                t.load.start();
                let tid = t.id;
                b.req_tab = Some(tid);
                b.touch();
                return Some(len);
            }
        }
        None
    }

    /// The host the user allowed past a certificate error for the navigation just taken by
    /// [`browser_take_request`](Self::browser_take_request) (empty when none).
    pub fn browser_insecure_host(&mut self, out: &mut [u8]) -> usize {
        let Some(id) = self.browser_id() else {
            return 0;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return 0;
        };
        let Some(tid) = b.req_tab else {
            return 0;
        };
        let Some((_, t)) = b.tab_by_id(tid) else {
            return 0;
        };
        match t.browser.insecure_host() {
            Some(h) => {
                let n = h.len().min(out.len());
                out[..n].copy_from_slice(&h[..n]);
                n
            }
            None => 0,
        }
    }

    /// Show a fetched raw HTTP response in the tab that asked for it. `conn` says how the
    /// final connection was authenticated, `truncated` that the response hit the size cap and
    /// `cert` summarises the server certificate (https only). Dropped when the tab was closed
    /// or the load was stopped meanwhile.
    pub fn browser_load(
        &mut self,
        resp: &[u8],
        conn: kitsune_core::browser::Conn,
        truncated: bool,
        cert: Option<kitsune_core::browser::CertInfo>,
    ) {
        let Some(id) = self.browser_id() else {
            return;
        };
        let Some(rect) = self.browser_win_rect(id) else {
            return;
        };
        let page = kitsune_core::browser::page_body_partial(resp, truncated);
        let body = page.body;
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let Some(tid) = b.req_tab.take() else {
            return;
        };
        let content_w = b.chrome(rect).content.w;
        let active_id = b.tabs.active().id;
        let BrowserState { tabs, images, .. } = &mut *b;
        let Some(t) = tabs.iter_mut().find(|t| t.id == tid) else {
            return;
        };
        if core::mem::take(&mut t.cancelled) {
            return;
        }
        let t0 = crate::trace::t();
        t.doc = Some(kitsune_core::web::Doc::parse(&body));
        crate::trace::note("parse", t0, body.len() as u64);
        t.trace_t0.set(t0);
        t.page = None;
        t.cert = cert;
        t.sel = None;
        t.sel_anchor = None;
        t.forms = kitsune_core::web::form::FormState::default();
        t.find.close();
        t.browser.loaded_with_note(conn, page.note);
        t.load.finish();
        jump_scroll(t, 0);
        if t.id == active_id {
            images.begin_page();
            t.layout_w = content_w;
            layout_browser(t, images, content_w, true);
        } else {
            t.layout_w = 0;
        }
        let tid = t.id;
        self.browser_page_changed(id, tid);
    }

    /// Lay out and show the HTML of a page the browser generated itself (`kitsune://...`), if
    /// one was just opened in any tab. Called every frame by the main loop, right next to the
    /// network hand-off.
    pub fn browser_poll_internal(&mut self) -> bool {
        let Some(id) = self.browser_id() else {
            return false;
        };
        let Some(rect) = self.browser_win_rect(id) else {
            return false;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        let content_w = b.chrome(rect).content.w;
        let active_id = b.tabs.active().id;
        let BrowserState { tabs, images, .. } = &mut *b;
        let mut changed: Option<u32> = None;
        for t in tabs.iter_mut() {
            let Some(html) = t.browser.take_internal() else {
                continue;
            };
            t.doc = Some(kitsune_core::web::Doc::parse(&html));
            t.page = None;
            t.cert = None;
            t.sel = None;
            t.sel_anchor = None;
            t.forms = kitsune_core::web::form::FormState::default();
            t.find.close();
            t.load.finish();
            jump_scroll(t, 0);
            if t.id == active_id {
                images.begin_page();
                t.layout_w = content_w;
                layout_browser(t, images, content_w, true);
            } else {
                t.layout_w = 0;
            }
            changed = Some(t.id);
        }
        match changed {
            Some(tid) => {
                self.browser_page_changed(id, tid);
                true
            }
            None => false,
        }
    }

    /// A new page is on screen in tab `tid`: remember its title, reset selection and find, set
    /// the window title when the tab is the active one.
    fn browser_page_changed(&mut self, id: WindowId, tid: u32) {
        let mut title = String::from(t!("app.browser"));
        let mut active = false;
        if let Some(b) = self.browser_state_mut(id) {
            b.touch();
            let active_id = b.tabs.active().id;
            if let Some((_, t)) = b.tab_by_id(tid) {
                let tt = t
                    .doc
                    .as_ref()
                    .map(|d| String::from(d.title()))
                    .unwrap_or_default();
                t.browser.set_page_title(&tt);
                t.sel = None;
                t.sel_anchor = None;
                let (page, find) = (&t.page, &mut t.find);
                find.refresh(page.as_ref(), &apps::browser::paint::KernelMetrics);
                // The star fills when the page is a favourite.
                let fav = t.browser.is_bookmarked();
                t.star.retarget(f32::from(fav as u8), 0.3, curves::ENTER);
                if !tt.is_empty() {
                    title = tt;
                }
                active = t.id == active_id;
            }
            b.popover = false;
            b.ctx = None;
            b.notice = None;
        }
        if active {
            self.browser_set_title(id, title);
        }
    }

    /// Mark the in-flight browser fetch as failed.
    pub fn browser_fail(&mut self, reason: kitsune_core::browser::FailReason) {
        let Some(id) = self.browser_id() else {
            return;
        };
        let mut set_title = false;
        if let Some(b) = self.browser_state_mut(id) {
            b.touch();
            let active_id = b.tabs.active().id;
            let Some(tid) = b.req_tab.take() else {
                return;
            };
            if let Some((_, t)) = b.tab_by_id(tid) {
                if core::mem::take(&mut t.cancelled) {
                    return;
                }
                t.page = None;
                t.doc = None;
                t.sel = None;
                t.cert = None;
                t.load.finish();
                t.browser.fail_with(reason);
                set_title = t.id == active_id;
            }
        }
        if set_title {
            self.browser_set_title(id, String::from(t!("app.browser")));
        }
    }

    /// The language changed: the browser's own pages are built again, the active page is
    /// laid out again (a few words are baked into a layout, like the line under a picture
    /// that failed to load) and an open context menu closes. Error pages and the chrome
    /// ask the catalog at every frame and need nothing.
    pub(crate) fn browser_language_changed(&mut self) {
        let Some(id) = self.browser_id() else {
            return;
        };
        let Some(rect) = self.browser_win_rect(id) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        b.ctx = None;
        b.glass[3].clear();
        let content = b.chrome(rect).content;
        let active_id = b.tabs.active().id;
        let BrowserState { tabs, images, .. } = &mut *b;
        let mut rebuilt: Vec<u32> = Vec::new();
        for t in tabs.iter_mut() {
            if let Some(html) = t.browser.internal_html() {
                t.doc = Some(kitsune_core::web::Doc::parse(&html));
                t.page = None;
                t.find.close();
                if t.id == active_id {
                    layout_browser(t, images, content.w, true);
                    let max = t.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
                    clamp_scroll(t, max);
                } else {
                    t.layout_w = 0;
                }
                rebuilt.push(t.id);
            } else if t.id == active_id && t.page.is_some() {
                layout_browser(t, images, content.w, false);
            }
        }
        b.touch();
        for tid in rebuilt {
            self.browser_page_changed(id, tid);
        }
        self.client_dirty = Some(id);
    }

    // ---- scrolling ----

    /// Largest scroll of the page on screen in window `id`.
    fn browser_max_scroll(&mut self, id: WindowId) -> i32 {
        let Some(rect) = self.browser_win_rect(id) else {
            return 0;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return 0;
        };
        let view_h = b.chrome(rect).content.h;
        b.page.as_ref().map_or(0, |p| (p.height - view_h).max(0))
    }

    /// Scroll the page of window `id` by `dy` pixels (eased, clamped to the page).
    pub(crate) fn scroll_page(&mut self, id: WindowId, dy: i32) -> bool {
        let max = self.browser_max_scroll(id);
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        let now = crate::desktop::shell::toasts::now_ms();
        let t = b.tabs.active_mut();
        let target = (t.scroll_spring.target() as i32)
            .saturating_add(dy)
            .clamp(0, max);
        let moved = target != t.scroll_spring.target() as i32;
        t.scroll_spring.set_target(target as f32);
        t.scroll_bar.touch(now);
        moved
    }

    /// Put the page at scroll `y` at once (find, focus: the jump is part of another change).
    pub(crate) fn scroll_page_to(&mut self, id: WindowId, y: i32) {
        let max = self.browser_max_scroll(id);
        if let Some(b) = self.browser_state_mut(id) {
            let now = crate::desktop::shell::toasts::now_ms();
            let t = b.tabs.active_mut();
            let y = y.clamp(0, max);
            t.scroll_spring.set_target(y as f32);
            t.scroll_bar.touch(now);
        }
    }

    /// Wheel over browser window `id`: three text lines per notch.
    pub(crate) fn browser_wheel(&mut self, id: WindowId, notches: i32) -> bool {
        // Over an open menu or popover the wheel does nothing to the page.
        if let Some(b) = self.browser_state_mut(id)
            && (b.ctx.is_some() || b.popover)
        {
            return false;
        }
        self.scroll_page(id, notches * WHEEL_PX)
    }

    /// Lay the page of the active tab out again for the window's current width (after a
    /// resize or maximize). Cheap no-op when the width did not change.
    pub(crate) fn relayout_browser(&mut self, id: WindowId) {
        let Some(rect) = self.browser_win_rect(id) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let content = b.chrome(rect).content;
        let BrowserState { tabs, images, .. } = &mut *b;
        let t = tabs.active_mut();
        if t.doc.is_some() && (t.layout_w != content.w || t.page.is_none()) {
            t.layout_w = content.w;
            let register = t.page.is_none();
            layout_browser(t, images, content.w, register);
            let max = t.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
            clamp_scroll(t, max);
        }
        b.touch();
    }

    // ---- pictures ----

    /// The next picture of the page that has to be downloaded, if the fetcher is free: copies its
    /// URL into `out` and returns the length and the column width to scale it to. Inline `data:`
    /// pictures are decoded right here (they are small) and never reach the fetcher.
    pub fn browser_next_image(&mut self, out: &mut [u8]) -> Option<(usize, usize)> {
        let id = self.browser_id()?;
        let rect = self.browser_win_rect(id)?;
        let b = self.browser_state_mut(id)?;
        let content = b.chrome(rect).content;
        if b.img_inflight.is_some() {
            return None;
        }
        let fit_w = (content.w - 16).max(16) as usize;
        let BrowserState {
            tabs,
            images,
            img_inflight,
            ..
        } = &mut *b;
        let t = tabs.active_mut();
        t.page.as_ref()?;
        let mut inline_done = false;
        let mut result = None;
        while let Some((key, data)) = images.next_pending() {
            if let Some(uri) = data {
                let r = kitsune_core::web::imgcache::decode_data_uri(&uri, fit_w);
                images.finish(&key, r);
                inline_done = true;
                continue;
            }
            let n = key.len().min(out.len());
            out[..n].copy_from_slice(&key.as_bytes()[..n]);
            *img_inflight = Some((t.id, key));
            result = Some((n, fit_w));
            break;
        }
        if inline_done {
            layout_browser(t, images, content.w, false);
            b.touch();
        }
        result
    }

    /// A picture request finished: store it and lay the page out again (images change sizes).
    pub fn browser_image_done(
        &mut self,
        res: Result<kitsune_core::web::imgcache::Loaded, kitsune_core::web::imgcache::ImgFail>,
    ) {
        let Some(id) = self.browser_id() else {
            return;
        };
        let Some(rect) = self.browser_win_rect(id) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let content = b.chrome(rect).content;
        let Some((tid, key)) = b.img_inflight.take() else {
            return;
        };
        let active_id = b.tabs.active().id;
        let BrowserState { tabs, images, .. } = &mut *b;
        images.finish(&key, res);
        if tid == active_id {
            let t = tabs.active_mut();
            layout_browser(t, images, content.w, false);
            let max = t.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
            clamp_scroll(t, max);
        }
        b.touch();
    }

    // ---- tabs ----

    /// Open a new tab (the start page) and show it. Returns whether there was room.
    pub(crate) fn browser_new_tab(&mut self, id: WindowId) -> bool {
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        if !b.tabs.can_open() {
            b.say(&t!("web.tab.limit", n = tabs::MAX_TABS));
            return false;
        }
        let tid = b.next_id;
        b.next_id += 1;
        let browser = b.tabs.active().browser.sibling();
        let old_active = b.tabs.active_index();
        let Some(_) = b.tabs.open(TabData::new(tid, browser)) else {
            return false;
        };
        // The strip entry goes right after the active tab's.
        let slots = b.strip_tab_slots();
        let entry_at = slots
            .iter()
            .find(|(_, ti)| *ti == old_active)
            .map_or(b.strip.len(), |(ei, _)| ei + 1);
        let mut weight = Tween::at(0.0);
        weight.retarget(1.0, 0.24, curves::ENTER);
        b.strip.insert(
            entry_at,
            StripEntry {
                id: Some(tid),
                weight,
                title: String::new(),
                badge: ' ',
            },
        );
        self.browser_after_switch(id, Some(old_active));
        true
    }

    /// Close tab `i`; the last one closes the window.
    pub(crate) fn browser_close_tab(&mut self, id: WindowId, i: usize) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        if b.tabs.len() <= 1 {
            self.client_dirty = None;
            self.request_close(id);
            return;
        }
        let old_active = b.tabs.active_index();
        let Some(t) = b.tabs.get(i) else {
            return;
        };
        let (tid, title, badge) = (
            t.id,
            tabs::tab_title(
                t.browser.page_title(),
                &String::from_utf8_lossy(t.browser.nav_url()),
            ),
            tabs::tab_badge(
                t.browser.page_title(),
                &String::from_utf8_lossy(t.browser.nav_url()),
            ),
        );
        // A fetch for it keeps running; its answer is dropped.
        if let Some(t) = b.tabs.get_mut(i).filter(|_| b.req_tab == Some(tid)) {
            t.cancelled = true;
        }
        if b.img_inflight.as_ref().is_some_and(|(x, _)| *x == tid) {
            b.img_inflight = None;
        }
        let Some(_closed) = b.tabs.close(i) else {
            return;
        };
        // Its strip entry becomes a ghost that shrinks away.
        if let Some(e) = b.strip.iter_mut().find(|e| e.id == Some(tid)) {
            e.id = None;
            e.title = title;
            e.badge = badge;
            e.weight.retarget(0.0, 0.18, curves::EXIT);
        }
        // If the closed tab was the shown one, the one now active has to be laid out.
        self.browser_after_switch(
            id,
            if i == old_active {
                None
            } else {
                Some(old_active)
            },
        );
    }

    /// Show tab `i`.
    pub(crate) fn browser_select_tab(&mut self, id: WindowId, i: usize) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let old = b.tabs.active_index();
        if i == old || !b.tabs.select(i) {
            return;
        }
        // The tab we leave frees its display list: only its document stays.
        if let Some(t) = b.tabs.get_mut(old) {
            t.page = None;
            t.hover_link = None;
            t.sel_anchor = None;
        }
        self.browser_after_switch(id, None);
    }

    /// The active tab changed (or was just opened): lay its page out, fix the window title,
    /// strip and focus state.
    fn browser_after_switch(&mut self, id: WindowId, leaving: Option<usize>) {
        let Some(rect) = self.browser_win_rect(id) else {
            return;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        if let Some(old) = leaving
            && let Some(t) = b.tabs.get_mut(old)
        {
            t.page = None;
            t.hover_link = None;
            t.sel_anchor = None;
        }
        b.popover = false;
        b.ctx = None;
        b.hover = BrowserHover::None;
        for s in b.glass.iter() {
            s.clear();
        }
        let strip_target = if b.tabs.len() >= 2 { 36.0 } else { 0.0 };
        b.strip_h.retarget(strip_target, 0.22, curves::ENTER);
        let content = b.chrome(rect).content;
        let BrowserState { tabs, images, .. } = &mut *b;
        let t = tabs.active_mut();
        let was = t.scroll;
        if t.doc.is_some() {
            images.begin_page();
            t.layout_w = content.w;
            layout_browser(t, images, content.w, true);
            let max = t.page.as_ref().map_or(0, |p| (p.height - content.h).max(0));
            jump_scroll(t, was.clamp(0, max));
        }
        // A tab that is the start page has the omnibox ready for typing.
        let fresh = t.browser.is_home();
        let fav = t.browser.is_bookmarked();
        t.star.retarget(f32::from(fav as u8), 0.01, curves::ENTER);
        if fresh {
            t.browser.set_bar_focus(true);
        }
        let title = {
            let tt = t.browser.page_title();
            if tt.is_empty() {
                String::from(t!("app.browser"))
            } else {
                String::from(tt)
            }
        };
        b.touch();
        b.cache.borrow_mut().invalidate();
        self.browser_set_title(id, title);
    }

    /// Stop the load of the active tab.
    pub(crate) fn browser_stop(&mut self, id: WindowId) {
        let Some(b) = self.browser_state_mut(id) else {
            return;
        };
        let req = b.req_tab;
        let t = b.tabs.active_mut();
        if !t.browser.is_loading() {
            return;
        }
        if req == Some(t.id) {
            t.cancelled = true;
        }
        t.browser.stop();
        if t.doc.is_none() {
            t.browser.go_home();
        }
        t.load.finish();
        b.touch();
    }

    /// The pointer moved: what is under it? Returns whether that changed (the window repaints
    /// the hover looks and the underline of a link).
    pub(crate) fn browser_hover_update(&mut self, cx: i32, cy: i32) -> bool {
        let Some(id) = self.browser_id() else {
            return false;
        };
        let over =
            self.drag.is_none() && !self.overlay_open() && self.topmost_at(cx, cy) == Some(id);
        let Some(rect) = self.browser_win_rect(id) else {
            return false;
        };
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        let target = if over {
            browser_hover_at(b, rect, cx, cy)
        } else {
            (BrowserHover::None, None)
        };
        let changed = target.0 != b.hover || target.1 != b.tabs.active().hover_link;
        b.hover = target.0;
        b.tabs.active_mut().hover_link = target.1;
        // The hovered link is part of the page cache key, so a new underline repaints by itself.
        changed
    }
}

/// What is under `(cx, cy)` in browser window `r`, and which link of the page.
pub(crate) fn browser_hover_at(
    b: &BrowserState,
    r: Rect,
    cx: i32,
    cy: i32,
) -> (BrowserHover, Option<usize>) {
    let ch = b.chrome(r);
    let t = b.tabs.active();
    if let Some(m) = &b.ctx {
        if let Some(i) = page_menu_row_at(m, r, cx, cy) {
            return (BrowserHover::MenuRow(i), None);
        }
        return (BrowserHover::None, None);
    }
    let n = t.browser.suggestions().len();
    if let Some(i) =
        kitsune_core::layout::browser_suggestion_at(ch.bar, n, cx, cy).filter(|_| n > 0)
    {
        return (BrowserHover::Suggestion(i), None);
    }
    if ch.back.contains(cx, cy) {
        return (BrowserHover::Back, None);
    }
    if ch.forward.contains(cx, cy) {
        return (BrowserHover::Forward, None);
    }
    if ch.reload.contains(cx, cy) {
        return (BrowserHover::Reload, None);
    }
    if ch.newtab.contains(cx, cy) {
        return (BrowserHover::NewTab, None);
    }
    if ch.star.contains(cx, cy) {
        return (BrowserHover::Star, None);
    }
    if ch.shield.w > 0 && ch.shield.contains(cx, cy) {
        return (BrowserHover::Shield, None);
    }
    if ch.bar.contains(cx, cy) {
        return (BrowserHover::Bar, None);
    }
    if ch.strip.h > 0 && ch.strip.contains(cx, cy) {
        let rects = kitsune_core::layout::browser_tab_rects(ch.strip, &b.strip_weights());
        let hit = kitsune_core::layout::browser_tab_at(&rects, cx, cy).and_then(|ei| {
            let slots = b.strip_tab_slots();
            slots
                .iter()
                .find(|(e, _)| *e == ei)
                .map(|&(_, ti)| (ei, ti))
        });
        if let Some((ei, ti)) = hit {
            if kitsune_core::layout::browser_tab_close(rects[ei]).contains(cx, cy) {
                return (BrowserHover::TabClose(ti), None);
            }
            return (BrowserHover::Tab(ti), None);
        }
        return (BrowserHover::None, None);
    }
    if ch.content.contains(cx, cy) {
        if t.find.is_open() {
            let f = kitsune_core::layout::browser_find_layout(ch.content);
            if f.prev.contains(cx, cy) {
                return (BrowserHover::FindPrev, None);
            }
            if f.next.contains(cx, cy) {
                return (BrowserHover::FindNext, None);
            }
            if f.close.contains(cx, cy) {
                return (BrowserHover::FindClose, None);
            }
            if f.bar.contains(cx, cy) {
                return (BrowserHover::None, None);
            }
        }
        if t.browser.is_home() {
            return (start_hover_at(b, ch.content, cx, cy), None);
        }
        if t.page.is_none() && t.browser.status() == kitsune_core::browser::Status::Error {
            let e = kitsune_core::layout::browser_error_layout(
                ch.content,
                t.browser.can_continue_insecure(),
            );
            if e.retry.contains(cx, cy) {
                return (BrowserHover::Retry, None);
            }
            if t.browser.can_continue_insecure() && e.proceed.contains(cx, cy) {
                return (BrowserHover::Proceed, None);
            }
            return (BrowserHover::None, None);
        }
        if let Some(p) = &t.page {
            let (qx, qy) = (cx - ch.content.x, cy - ch.content.y + t.scroll);
            return (BrowserHover::None, p.link_index_at(qx, qy));
        }
    }
    (BrowserHover::None, None)
}

/// The tile or row of the start page under the pointer.
fn start_hover_at(b: &BrowserState, content: Rect, cx: i32, cy: i32) -> BrowserHover {
    let (tiles, recents) = start_items(b);
    let l = kitsune_core::layout::browser_start_layout(content, tiles.len(), recents.len());
    if let Some(i) = l.tiles.iter().position(|r| r.contains(cx, cy)) {
        return BrowserHover::Tile(i);
    }
    if let Some(i) = l.recents.iter().position(|r| r.contains(cx, cy)) {
        return BrowserHover::Recent(i);
    }
    BrowserHover::None
}

/// What the new-tab page lists: tiles `(label, url)` and recent addresses.
pub(crate) fn start_items(b: &BrowserState) -> (Vec<(String, String)>, Vec<String>) {
    let t = b.tabs.active();
    let bm = t.browser.bookmarks();
    let mut tiles: Vec<(String, String)> = bm
        .iter()
        .take(10)
        .map(|k| (tabs::tab_title(&k.title, &k.url), k.url.clone()))
        .collect();
    if tiles.is_empty() {
        tiles = kitsune_core::browser::QUICK_LINKS
            .iter()
            .map(|(l, u)| (String::from(kitsune_core::i18n::tr(l)), String::from(*u)))
            .collect();
    }
    // Recents belong to the window: every tab's history, the shown tab's first.
    let mut recents = t.browser.recent(6);
    for other in b.tabs.iter().filter(|o| o.id != t.id) {
        for u in other.browser.recent(6) {
            if recents.len() < 6 && !recents.contains(&u) {
                recents.push(u);
            }
        }
    }
    (tiles, recents)
}

/// Rows of the context menu of window `r` under `(cx, cy)`.
pub(crate) fn page_menu_row_at(m: &PageMenu, r: Rect, cx: i32, cy: i32) -> Option<usize> {
    let g = page_menu_geom(m, r);
    g.rows
        .iter()
        .zip(&m.items)
        .position(|(row, _)| row.contains(cx, cy))
}

/// Placement of the context menu, kept inside window `r`.
pub(crate) fn page_menu_geom(m: &PageMenu, r: Rect) -> kitsune_core::chrome::MenuGeom {
    let rows: Vec<kitsune_core::chrome::MenuRow> = m
        .items
        .iter()
        .map(|(_, label, _)| kitsune_core::chrome::MenuRow::Item {
            label_w: text::measure(label, text::BODY, text::Weight::Regular),
            shortcut_w: 0,
        })
        .collect();
    let mut g = kitsune_core::chrome::menu_geom(&rows, (m.x, m.y), r.right() + 4, r.bottom() + 4);
    // The menu must not leave the window: shift it back inside if it was clamped to the screen.
    let dx = (r.right() - 8 - g.rect.right()).min(0);
    let dy = (r.bottom() - 8 - g.rect.bottom()).min(0);
    if dx != 0 || dy != 0 {
        g.rect = Rect::new(g.rect.x + dx, g.rect.y + dy, g.rect.w, g.rect.h);
        for row in g.rows.iter_mut() {
            *row = Rect::new(row.x + dx, row.y + dy, row.w, row.h);
        }
    }
    g
}

/// Put tab `t` at scroll `y` without easing.
fn jump_scroll(t: &mut TabData, y: i32) {
    t.scroll = y;
    t.scroll_spring.jump(y as f32);
}

/// Keep the scroll of `t` inside `0..=max` (after the page got shorter).
fn clamp_scroll(t: &mut TabData, max: i32) {
    let target = (t.scroll_spring.target() as i32).clamp(0, max);
    t.scroll_spring.set_target(target as f32);
    if t.scroll > max {
        t.scroll = max;
        t.scroll_spring.jump(target as f32);
    }
}

/// Lay the document of `t` out for `width` pixels with its zoom and whatever the image cache
/// knows. `register` (a new page) first lays out with the images unknown to learn which
/// pictures the page has, asks the cache for them, then lays out again.
pub(crate) fn layout_browser(t: &mut TabData, images: &mut ImageCache, width: i32, register: bool) {
    let Some(doc) = &t.doc else {
        return;
    };
    let base = t.browser.nav_url().to_vec();
    let zoom = t.zoom;
    let lay = |images: &ImageCache| {
        doc.layout(&Layout {
            width,
            zoom,
            images: &PageImages {
                cache: images,
                base: &base,
            },
            metrics: &apps::browser::paint::KernelMetrics,
        })
    };
    let t0 = crate::trace::t();
    let mut page = lay(images);
    crate::trace::note("layout", t0, page.cmds.len() as u64);
    if register {
        t.forms = kitsune_core::web::form::FormState::new(&page.forms);
        t.img_keys.clear();
        for r in &page.images {
            let key = image_key(&base, &r.src);
            if let Some(k) = &key {
                let data = (k.starts_with("data:#")).then_some(r.src.as_str());
                images.want(k, data);
            }
            t.img_keys.push(key);
        }
        if page.images.len() > kitsune_core::web::imgcache::MAX_PAGE_IMAGES {
            page = lay(images);
        }
    }
    // Make each stored picture exactly the size of its box so painting is a plain copy.
    for c in &page.cmds {
        if let WebCmd::Image { w, h, idx, .. } = c
            && let Some(Some(k)) = t.img_keys.get(*idx)
        {
            images.fit_to(k, *w as usize, *h as usize);
        }
    }
    t.page = Some(page);
    // The text moved: the selection and the find matches follow the new layout.
    t.find
        .refresh(t.page.as_ref(), &apps::browser::paint::KernelMetrics);
    if let Some(p) = &t.page
        && t.sel.is_some_and(|sel| !p.selection_valid(&sel))
    {
        t.sel = None;
    }
}
