//! Changing the interface language while the desktop runs.
//!
//! `crate::settings::set` already switches the catalog (`osjeff_core::i18n::set_lang`). What is
//! left for the desktop is everything that holds text built with the old language: window titles
//! (stored per window), the Apps and Busca overlays (their tiles and results are built when
//! they open) and open menus and popovers. Text drawn each frame from the catalog needs
//! nothing: the full repaint requested here redraws it.

use super::*;
use osjeff_core::i18n::Lang;

impl Desktop {
    /// The language changed from `old` to the one in effect: refresh what was built with `old`
    /// and repaint everything.
    pub(crate) fn language_changed(&mut self, old: Lang) {
        // Titles are "<name>[ N][ - detail]": swap the name, keep the rest.
        let wins: Vec<(WindowId, instance::Kind, u8)> = self
            .wm
            .windows()
            .iter()
            .map(|w| (w.id, w.app.kind(), w.app.index))
            .collect();
        for (id, kind, index) in wins {
            let old_base = instance::base_title_in(old, kind, index);
            let new_base = instance::base_title(kind, index);
            if let Some(w) = self.wm.get_mut(id)
                && let Some(rest) = w.app.title.strip_prefix(old_base.as_str())
            {
                let mut t = new_base;
                t.push_str(rest);
                w.app.title = t;
            }
        }
        self.files_language_changed_all();
        self.viewer_language_changed_all();
        self.editor_language_changed_all();
        // Tarefas keeps its rows (friendly names) and a footer message.
        self.tarefas_language_changed();
        // Transient layers hold strings: drop them (they are cheap to open again).
        let sh = &mut self.shell;
        sh.menu = None;
        sh.pop = None;
        sh.apps = None;
        sh.search = None;
        self.force_full = true;
    }
}
