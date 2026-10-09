//! One drawing entry point per shell layer (see `compositor/layers.rs`).

use crate::desktop::*;

impl Desktop {
    // ---- drawing entry: one function per shell layer (see `compositor/layers.rs`) ----

    pub(crate) fn draw_apps_layer(&self, c: &mut Canvas) {
        if let Some(a) = &self.shell.apps {
            self.draw_apps(c, a);
        }
    }

    pub(crate) fn draw_search_layer(&self, c: &mut Canvas) {
        if let Some(s) = &self.shell.search {
            self.draw_search(c, s);
        }
    }

    pub(crate) fn draw_switcher_layer(&self, c: &mut Canvas) {
        if let Some(sw) = &self.switcher {
            self.draw_switcher(c, sw);
        }
    }

    pub(crate) fn draw_dialog_layer(&self, c: &mut Canvas) {
        if let Some(d) = &self.shell.dialog {
            self.draw_dialog(c, d);
        }
    }
}
