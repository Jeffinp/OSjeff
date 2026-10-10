//! addressbar (split out of `browser.rs`).

use super::*;

impl History {
    /// Record a finished navigation to `url`: drops the forward entries, ignores
    /// a reload of the same page and honors a pending back/forward replay.
    pub(super) fn record(&mut self, url: &[u8]) {
        if core::mem::take(&mut self.replay) {
            return;
        }
        if self.urls.get(self.cur).is_some_and(|u| u == url) {
            return;
        }
        if !self.urls.is_empty() {
            self.urls.truncate(self.cur + 1);
        }
        if self.urls.len() == MAX_HISTORY {
            self.urls.remove(0);
        }
        self.urls.push(url.to_vec());
        self.cur = self.urls.len() - 1;
    }
}

impl InsecureHosts {
    pub(super) const fn new() -> Self {
        Self {
            hosts: [[0; HOST_CAP]; MAX_INSECURE_HOSTS],
            lens: [0; MAX_INSECURE_HOSTS],
            n: 0,
        }
    }

    pub(super) fn find(&self, host: &[u8]) -> Option<usize> {
        (0..self.n).find(|&i| self.hosts[i][..usize::from(self.lens[i])].eq_ignore_ascii_case(host))
    }

    /// Remember `host` (the oldest entry is dropped when full).
    pub(super) fn add(&mut self, host: &[u8]) {
        if host.is_empty() || host.len() > HOST_CAP || self.find(host).is_some() {
            return;
        }
        if self.n == MAX_INSECURE_HOSTS {
            for i in 1..MAX_INSECURE_HOSTS {
                self.hosts[i - 1] = self.hosts[i];
                self.lens[i - 1] = self.lens[i];
            }
            self.n -= 1;
        }
        let i = self.n;
        self.hosts[i] = [0; HOST_CAP];
        self.hosts[i][..host.len()].copy_from_slice(host);
        self.lens[i] = host.len() as u8;
        self.n += 1;
    }
}

/// The `Accept-Language` value the browser sends: the language of the interface first, then
/// English (`pt-BR,pt;q=0.9,en;q=0.8` or `en;q=1`). It is a catalog entry, so a new language
/// brings its own.
pub fn accept_language(lang: Lang) -> &'static str {
    i18n::tr_in(lang, tk!("web.accept_language"))
}

/// Append an HTTP/1.1 `GET` request to `req` (`Connection: close`: one request per
/// connection; shared by the plain and the TLS paths), asking for the page in `lang`.
pub fn build_get_request(
    req: &mut Vec<u8>,
    lang: Lang,
    host: &str,
    path: &str,
    port: u16,
    tls: bool,
) {
    req.extend_from_slice(b"GET ");
    req.extend_from_slice(path.as_bytes());
    req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    req.extend_from_slice(host.as_bytes());
    let default_port = if tls { 443 } else { 80 };
    if port != default_port {
        req.push(b':');
        let mut digits = [0u8; 5];
        let mut n = port;
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        req.extend_from_slice(&digits[i..]);
    }
    req.extend_from_slice(
        concat!(
            "\r\nUser-Agent: Kitsune/",
            env!("CARGO_PKG_VERSION"),
            "\r\nAccept: text/html, image/png, image/bmp, */*;q=0.1\r\nAccept-Language: "
        )
        .as_bytes(),
    );
    req.extend_from_slice(accept_language(lang).as_bytes());
    req.extend_from_slice(b"\r\nAccept-Encoding: gzip, deflate\r\nConnection: close\r\n\r\n");
}

impl Browser {
    pub fn new() -> Self {
        let mut b = Self {
            url: [0; URL_CAP],
            url_len: 0,
            caret: 0,
            status: Status::Idle,
            pending: false,
            nav: [0; URL_CAP],
            nav_len: 0,
            home: true,
            security: Security::None,
            note: None,
            fail_reason: FailReason::Network,
            insecure: Rc::new(RefCell::new(InsecureHosts::new())),
            history: History::default(),
            bookmarks: Rc::new(RefCell::new(Box::new(MemoryBookmarks::default()))),
            bar_focus: true,
            sugg_sel: None,
            sugg_dismissed: false,
            internal: false,
            internal_ready: false,
            page_title: String::new(),
            bar_selected: false,
        };
        b.set_url(b"");
        b
    }

    /// A browser whose favourites live in `store` (the one place the kernel plugs persistence in).
    pub fn with_store(store: Box<dyn BookmarkStore>) -> Self {
        let mut b = Self::new();
        b.bookmarks = Rc::new(RefCell::new(store));
        b
    }

    /// A fresh browser (a new tab) that shares this one's favourites and the hosts the user
    /// allowed past a certificate error: both belong to the window, not to a tab.
    pub fn sibling(&self) -> Self {
        let mut b = Self::new();
        b.bookmarks = Rc::clone(&self.bookmarks);
        b.insecure = Rc::clone(&self.insecure);
        b
    }

    /// True while the native start page (logo + shortcuts) is shown.
    pub fn is_home(&self) -> bool {
        self.home
    }

    /// Return to the start page, clearing the address bar.
    pub fn go_home(&mut self) {
        self.home = true;
        self.status = Status::Idle;
        self.security = Security::None;
        self.note = None;
        self.set_url(b"");
    }

    /// Re-fetch the page that is shown (no-op on the start page). It is the address that was
    /// navigated to, not whatever has been typed in the bar since; an unsent edit is dropped.
    pub fn reload(&mut self) {
        if self.home || self.nav_len == 0 {
            return;
        }
        let nav = self.nav[..self.nav_len].to_vec();
        self.set_url(&nav);
        self.submit();
    }

    /// "Tentar novamente" on an error page: the same navigation again.
    pub fn retry(&mut self) {
        self.reload();
    }

    /// Navigate straight to `url` (used by the start-page shortcuts).
    pub fn open(&mut self, url: &[u8]) {
        self.set_url(url);
        self.submit();
    }

    pub(super) fn set_url(&mut self, s: &[u8]) {
        self.bar_selected = false;
        self.url_len = s.len().min(URL_CAP);
        self.url[..self.url_len].copy_from_slice(&s[..self.url_len]);
        self.caret = self.url_len;
    }

    pub fn url(&self) -> &[u8] {
        &self.url[..self.url_len]
    }
    pub fn caret(&self) -> usize {
        self.caret
    }
    pub fn status(&self) -> Status {
        self.status
    }

    /// What can be said about the connection behind the current page. While a
    /// load is in flight this reflects the *requested* scheme; once it
    /// completes ([`Browser::loaded_with`]) it reflects the final one, after
    /// any redirects. Never reports a verified connection (see [`Security`]).
    pub fn security(&self) -> Security {
        self.security
    }

    /// True when the loaded page is not the whole document (see [`Browser::note`]).
    pub fn truncated(&self) -> bool {
        self.note.is_some()
    }

    /// Why the loaded page is only part of the document, if it is.
    pub fn note(&self) -> Option<PageNote> {
        self.note
    }

    /// Why the last navigation failed (meaningful when `status()` is `Error`).
    pub fn fail_reason(&self) -> FailReason {
        self.fail_reason
    }

    /// Handle a key while the address bar has focus. Returns `true` if anything
    /// changed (so the caller repaints). ENTER submits a navigation/search.
    pub fn on_key(&mut self, key: Key) -> bool {
        // Suggestion list: Up/Down move through it, Enter opens the highlighted one.
        let n = self.suggestions().len();
        match key {
            Key::Down if n > 0 => {
                self.sugg_sel = Some(self.sugg_sel.map_or(0, |i| (i + 1).min(n - 1)));
                return true;
            }
            Key::Up if n > 0 => {
                self.sugg_sel = match self.sugg_sel {
                    Some(0) | None => None,
                    Some(i) => Some(i - 1),
                };
                return true;
            }
            Key::Enter if self.sugg_sel.is_some() => {
                let i = self.sugg_sel.unwrap_or(0);
                if let Some(s) = self.suggestions().get(i) {
                    let url = s.url.clone();
                    self.open(url.as_bytes());
                }
                return true;
            }
            Key::Esc if n > 0 => {
                self.sugg_dismissed = true;
                self.sugg_sel = None;
                return true;
            }
            Key::Char(_) | Key::Backspace | Key::Delete => {
                self.sugg_sel = None;
                self.sugg_dismissed = false;
                // A selected address is replaced by what is typed (or just deleted).
                if core::mem::take(&mut self.bar_selected) {
                    self.url_len = 0;
                    self.caret = 0;
                    if matches!(key, Key::Backspace | Key::Delete) {
                        return true;
                    }
                }
            }
            Key::Left | Key::Right | Key::Home | Key::End => self.bar_selected = false,
            _ => {}
        }
        match key {
            Key::Char(c) => {
                if self.url_len < URL_CAP {
                    // insert at caret
                    let mut i = self.url_len;
                    while i > self.caret {
                        self.url[i] = self.url[i - 1];
                        i -= 1;
                    }
                    self.url[self.caret] = c;
                    self.url_len += 1;
                    self.caret += 1;
                }
                true
            }
            Key::Backspace => {
                if self.caret > 0 {
                    for i in self.caret..self.url_len {
                        self.url[i - 1] = self.url[i];
                    }
                    self.url_len -= 1;
                    self.caret -= 1;
                }
                true
            }
            Key::Delete => {
                if self.caret < self.url_len {
                    for i in self.caret + 1..self.url_len {
                        self.url[i - 1] = self.url[i];
                    }
                    self.url_len -= 1;
                }
                true
            }
            Key::Left => {
                self.caret = self.caret.saturating_sub(1);
                true
            }
            Key::Right => {
                if self.caret < self.url_len {
                    self.caret += 1;
                }
                true
            }
            Key::Home => {
                self.caret = 0;
                true
            }
            Key::End => {
                self.caret = self.url_len;
                true
            }
            Key::Enter => {
                self.submit();
                true
            }
            _ => false,
        }
    }

    /// Resolve the address bar into a navigation target and mark a fetch pending.
    pub fn submit(&mut self) {
        let input = &self.url[..self.url_len];
        if input.trim_ascii().is_empty() {
            return;
        }
        self.sugg_sel = None;
        self.sugg_dismissed = true;
        if is_internal_input(input.trim_ascii()) {
            let url = input.trim_ascii().to_vec();
            self.open_internal(&url);
            return;
        }
        self.internal = false;
        let mut nav = [0u8; URL_CAP];
        let n = if looks_like_url(input) {
            // Normalize: prepend https:// if no scheme was given.
            if starts_with_ci(input, b"http://") || starts_with_ci(input, b"https://") {
                let n = input.len().min(URL_CAP);
                nav[..n].copy_from_slice(&input[..n]);
                n
            } else {
                let pre = b"https://";
                let mut n = pre.len();
                nav[..n].copy_from_slice(pre);
                let take = input.len().min(URL_CAP - n);
                nav[n..n + take].copy_from_slice(&input[..take]);
                n += take;
                n
            }
        } else {
            build_search_url(input, &mut nav)
        };
        self.nav_len = n;
        self.nav[..n].copy_from_slice(&nav[..n]);
        // A verified connection is only ever reported by `loaded_with`.
        self.security = if starts_with_ci(&nav[..n], b"http://") {
            Security::Http
        } else {
            Security::None
        };
        self.note = None;
        self.status = Status::Loading;
        self.pending = true;
        self.home = false;
    }

    /// Pull a pending navigation target (clears the pending flag). The kernel
    /// fetches it and reports back with [`loaded`] / [`fail`].
    pub fn take_request(&mut self) -> Option<&[u8]> {
        if self.pending {
            self.pending = false;
            Some(&self.nav[..self.nav_len])
        } else {
            None
        }
    }

    /// Mark a navigation as successfully loaded (the kernel renders the page via
    /// the `web` engine and owns the display list).
    pub fn loaded(&mut self) {
        self.status = Status::Done;
        self.home = false;
        let url = self.nav[..self.nav_len].to_vec();
        self.history.record(&url);
    }

    /// True when there is an earlier page in the history.
    pub fn can_back(&self) -> bool {
        self.history.cur > 0 && !self.history.urls.is_empty()
    }

    /// True when there is a later page in the history.
    pub fn can_forward(&self) -> bool {
        self.history.cur + 1 < self.history.urls.len()
    }

    /// Number of pages in the history.
    pub fn history_len(&self) -> usize {
        self.history.urls.len()
    }

    /// Go to the previous page of the history (no-op at the start).
    pub fn back(&mut self) {
        if self.can_back() {
            self.history.cur -= 1;
            self.replay_current();
        }
    }

    /// Go to the next page of the history (no-op at the end).
    pub fn forward(&mut self) {
        if self.can_forward() {
            self.history.cur += 1;
            self.replay_current();
        }
    }

    pub(super) fn replay_current(&mut self) {
        let url = self.history.urls[self.history.cur].clone();
        // Set before `submit`: an `kitsune://` page is "loaded" inside it.
        self.history.replay = true;
        self.set_url(&url);
        self.submit();
    }

    /// The user clicked a link whose `href` is `href` on the current page:
    /// resolve it against the page URL (relative, absolute and protocol-relative
    /// forms; `javascript:`, `data:` and an https -> http downgrade are refused)
    /// and navigate there. Returns `false` when the link was refused.
    pub fn open_link(&mut self, href: &[u8]) -> bool {
        if is_internal_input(href.trim_ascii()) {
            self.set_url(href.trim_ascii());
            self.submit();
            return true;
        }
        // The browser's own pages have no real address: resolve against a neutral http base so
        // their absolute links are not mistaken for an https -> http downgrade.
        let base = if self.internal {
            parse_url(b"http://kitsune.local/")
        } else {
            parse_url(self.nav_url())
        };
        let Some(base) = base else {
            return false;
        };
        match crate::browsing::redirect::resolve_redirect(&base, href) {
            Ok(target) => {
                self.set_url(&target);
                self.submit();
                true
            }
            Err(_) => false,
        }
    }

    /// Like [`Browser::loaded`], recording how the page actually arrived: `conn`
    /// describes the *final* connection (after redirects) and `truncated` says
    /// the response hit [`MAX_RESPONSE_BYTES`].
    pub fn loaded_with(&mut self, conn: Conn, truncated: bool) {
        self.loaded_with_note(conn, truncated.then_some(PageNote::Truncated));
    }

    /// [`Browser::loaded_with`] with the precise reason the page is partial
    /// (from [`page_body_partial`]), `None` for a whole page.
    pub fn loaded_with_note(&mut self, conn: Conn, note: Option<PageNote>) {
        self.security = match conn {
            Conn::Plain => Security::Http,
            Conn::Verified => Security::HttpsVerified,
            Conn::Insecure => Security::HttpsInvalid,
        };
        self.note = note;
        self.loaded();
    }

    /// The navigation target of the last (or pending) request.
    pub fn nav_url(&self) -> &[u8] {
        &self.nav[..self.nav_len]
    }

    /// The host of the current navigation when the user allowed it to proceed
    /// despite a certificate error (the fetcher then skips validation for that
    /// host only, on every hop of this navigation).
    pub fn insecure_host(&self) -> Option<Vec<u8>> {
        let u = parse_url(self.nav_url())?;
        if !u.https {
            return None;
        }
        let ins = self.insecure.borrow();
        let i = ins.find(u.host())?;
        Some(ins.hosts[i][..usize::from(ins.lens[i])].to_vec())
    }

    /// True when the failed navigation can be retried anyway: the failure is a
    /// certificate error (nothing else offers an unsafe override).
    pub fn can_continue_insecure(&self) -> bool {
        self.status == Status::Error && matches!(self.fail_reason, FailReason::Cert(_))
    }

    /// The user's explicit "continue anyway (insecure)": remember this host for
    /// the session and load the page again.
    pub fn continue_insecure(&mut self) {
        if !self.can_continue_insecure() {
            return;
        }
        if let Some(u) = parse_url(self.nav_url()) {
            self.insecure.borrow_mut().add(u.host());
        }
        self.pending = true;
        self.status = Status::Loading;
    }

    // ---- the browser's own pages, favourites and suggestions ----

    /// True while an `kitsune://` page is on screen.
    pub fn is_internal(&self) -> bool {
        self.internal
    }

    /// Open the internal page `url` (`kitsune://inicio`, `favoritos`, `historico`, `sobre`).
    /// Unknown names show the "about" page's list of pages.
    pub(super) fn open_internal(&mut self, url: &[u8]) {
        let text = String::from_utf8_lossy(url).to_ascii_lowercase();
        // An address with the old scheme is redirected: the page is shown as `kitsune://...`.
        let rest = text
            .trim_start_matches("kitsune:")
            .trim_start_matches("osjeff:")
            .trim_start_matches('/');
        let (name, query) = rest.split_once('?').unwrap_or((rest, ""));
        let name = name.trim_end_matches('/');
        if name == "inicio" || name.is_empty() {
            self.internal = false;
            self.go_home();
            return;
        }
        // `kitsune://favoritos?rm=N` removes the N-th favourite, then shows the list.
        let mut shown = alloc::format!("kitsune://{name}");
        let doomed = (name == "favoritos")
            .then(|| {
                query
                    .strip_prefix("rm=")
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .flatten()
            .and_then(|n| self.bookmarks.borrow().all().get(n).cloned());
        if let Some(b) = doomed {
            self.bookmarks.borrow_mut().remove(&b.url);
            shown = String::from("kitsune://favoritos");
        }
        self.set_url(shown.as_bytes());
        let n = self.url_len;
        self.nav[..n].copy_from_slice(&self.url[..n]);
        self.nav_len = n;
        self.internal = true;
        self.internal_ready = true;
        self.security = Security::None;
        self.note = None;
        self.pending = false;
        self.home = false;
        self.loaded();
    }

    /// The HTML of an internal page that was just opened (once), for the kernel to lay out.
    pub fn take_internal(&mut self) -> Option<Vec<u8>> {
        if !core::mem::take(&mut self.internal_ready) {
            return None;
        }
        self.internal_html()
    }

    /// The HTML of the internal page on screen, built again in the language in effect (the
    /// kernel calls it when the language changes); `None` when no internal page is shown.
    pub fn internal_html(&self) -> Option<Vec<u8>> {
        if !self.internal {
            return None;
        }
        self.internal_html_in(i18n::lang())
    }

    /// [`Self::internal_html`] in `lang`.
    pub fn internal_html_in(&self, lang: Lang) -> Option<Vec<u8>> {
        if !self.internal {
            return None;
        }
        let name = String::from_utf8_lossy(self.nav_url()).to_ascii_lowercase();
        let name = name
            .trim_start_matches("kitsune://")
            .trim_start_matches("osjeff://");
        Some(match name {
            "favoritos" => pages::bookmarks_in(lang, &self.bookmarks.borrow().all()),
            "historico" => pages::history_in(lang, &self.history.urls),
            _ => pages::about_in(lang),
        })
    }

    /// Remember the `<title>` of the page on screen (for favourites and the window title).
    pub fn set_page_title(&mut self, title: &str) {
        self.page_title.clear();
        // Cut at a character boundary: a title is page-controlled UTF-8.
        let mut end = title.len().min(80);
        while !title.is_char_boundary(end) {
            end -= 1;
        }
        self.page_title.push_str(&title[..end]);
    }

    /// `<title>` of the page on screen (empty when none).
    pub fn page_title(&self) -> &str {
        &self.page_title
    }

    /// Is the page on screen a favourite?
    pub fn is_bookmarked(&self) -> bool {
        !self.home
            && !self.nav_url().is_empty()
            && self
                .bookmarks
                .borrow()
                .contains(&String::from_utf8_lossy(self.nav_url()))
    }

    /// Ctrl+D / the star: add the page to the favourites, or remove it. Returns the new state
    /// (`true` = now a favourite) or `None` when there is nothing to bookmark (start page, full).
    pub fn toggle_bookmark(&mut self) -> Option<bool> {
        if self.home || self.nav_url().is_empty() {
            return None;
        }
        let url = String::from_utf8_lossy(self.nav_url()).into_owned();
        if self.bookmarks.borrow().contains(&url) {
            self.bookmarks.borrow_mut().remove(&url);
            return Some(false);
        }
        let title = if self.page_title.is_empty() {
            url.clone()
        } else {
            self.page_title.clone()
        };
        self.bookmarks
            .borrow_mut()
            .add(Bookmark { url, title })
            .then_some(true)
    }

    /// The favourites.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.bookmarks.borrow().all()
    }

    /// Select the whole address (Ctrl+L): typing replaces it.
    pub fn select_bar(&mut self) {
        self.bar_focus = true;
        self.bar_selected = self.url_len > 0;
        self.sugg_dismissed = true;
    }

    /// Is the whole address selected?
    pub fn bar_selected(&self) -> bool {
        self.bar_selected
    }

    /// Keyboard focus is in the address bar (true) or on the page (false).
    pub fn bar_focus(&self) -> bool {
        self.bar_focus
    }

    pub fn set_bar_focus(&mut self, on: bool) {
        self.bar_focus = on;
        if !on {
            self.sugg_sel = None;
        }
    }

    /// The history, oldest first (absolute URLs).
    pub fn history_urls(&self) -> impl Iterator<Item = &[u8]> {
        self.history.urls.iter().map(|u| u.as_slice())
    }

    /// Address-bar suggestions for the text typed so far: favourites then history, prefix matches
    /// before substring matches, at most [`MAX_SUGGESTIONS`]. Empty when the bar has no text, does
    /// not have the focus, was dismissed with Esc, or only the typed address itself matches.
    pub fn suggestions(&self) -> Vec<Suggestion> {
        if !self.bar_focus || self.sugg_dismissed || self.url_len == 0 {
            return Vec::new();
        }
        let q = String::from_utf8_lossy(self.url());
        let hist: Vec<String> = self
            .history
            .urls
            .iter()
            .rev()
            .map(|u| String::from_utf8_lossy(u).into_owned())
            .filter(|u| !is_internal_url(u))
            .collect();
        suggest(&q, &self.bookmarks.borrow().all(), &hist)
    }

    /// The highlighted suggestion index.
    pub fn suggestion_selected(&self) -> Option<usize> {
        self.sugg_sel
    }

    /// Choose suggestion `i` (a click): open it.
    pub fn pick_suggestion(&mut self, i: usize) -> bool {
        match self.suggestions().get(i) {
            Some(s) => {
                let url = s.url.clone();
                self.open(url.as_bytes());
                true
            }
            None => false,
        }
    }

    /// Stop loading: a request that was not taken yet is dropped, one in flight is ignored by
    /// the caller. The page on screen (or the start page) stays.
    pub fn stop(&mut self) {
        if self.status == Status::Loading {
            self.pending = false;
            self.history.replay = false;
            self.status = Status::Idle;
        }
    }

    /// Is a navigation in progress?
    pub fn is_loading(&self) -> bool {
        self.status == Status::Loading
    }

    /// Up to `n` most recently visited addresses, newest first, each once, without the
    /// browser's own pages.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for u in self.history.urls.iter().rev() {
            let u = String::from_utf8_lossy(u);
            if is_internal_url(&u) || out.iter().any(|o| *o == u) {
                continue;
            }
            out.push(u.into_owned());
            if out.len() >= n {
                break;
            }
        }
        out
    }

    /// Mark the current fetch as failed (the kernel shows the error state).
    pub fn fail(&mut self) {
        self.fail_with(FailReason::Network);
    }

    /// Mark the current fetch as failed for a specific reason.
    pub fn fail_with(&mut self, reason: FailReason) {
        self.history.replay = false;
        self.status = Status::Error;
        self.fail_reason = reason;
        self.note = None;
        self.home = false;
    }
}
