//! Copy and paste between the focused window and the clipboard.

use crate::desktop::*;

impl Desktop {
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
            let text = page.selection_text(&sel);
            let n = text.len().min(clipboard::CAP);
            tmp[..n].copy_from_slice(&text.as_bytes()[..n]);
            self.clipboard.set(&tmp[..n]);
            return;
        }
        let typed;
        if let Some(w) = self.wm.get(top) {
            let text: &[u8] = match &w.app.app {
                App::Terminal(t) => {
                    // The selected text, else the typed line.
                    typed = t.selection_text().unwrap_or_else(|| t.input()).into_bytes();
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
                b.touch();
                let t = b.tabs.active_mut();
                if t.forms.focus().is_some()
                    && let Some(page) = &t.page
                {
                    let text = String::from_utf8_lossy(data).into_owned();
                    t.forms.insert_str(&page.forms, &text);
                } else {
                    t.browser.set_bar_focus(true);
                    for &ch in data {
                        if ch != b'\n' && ch != b'\r' {
                            t.browser.on_key(Key::Char(ch));
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
}
