//! Stepping the shell's animations and answering what layer is open.

use super::model::MENU_FADE;
use super::model::fade_out;
use crate::desktop::*;

impl Desktop {
    /// Advance the shell's animations by `dt` seconds; `true` while any still runs.
    pub(crate) fn step_shell(&mut self, dt: f32) -> bool {
        let mut busy = false;
        let sh = &mut self.shell;
        if let Some(m) = sh.menu.as_mut() {
            busy |= m.t.step(dt);
        }
        if let Some(p) = sh.pop.as_mut() {
            busy |= p.t.step(dt);
        }
        if let Some(d) = sh.dialog.as_mut() {
            busy |= d.t.step(dt);
        }
        if let Some(a) = sh.apps.as_mut() {
            busy |= a.t.step(dt);
        }
        if let Some(s) = sh.search.as_mut() {
            busy |= s.t.step(dt);
        }
        for k in sh.knobs.iter_mut() {
            busy |= k.step(dt);
        }
        if let Some(sp) = sh.snap.as_mut() {
            busy |= sp.t.step(dt);
        }
        // Closing overlays disappear when their fade-out ends.
        if sh
            .menu
            .as_ref()
            .is_some_and(|m| m.closing && m.t.finished())
        {
            sh.menu = None;
            self.force_full = true;
        }
        if sh.pop.as_ref().is_some_and(|p| p.closing && p.t.finished()) {
            sh.pop = None;
            self.force_full = true;
        }
        if sh
            .dialog
            .as_ref()
            .is_some_and(|d| d.closing && d.t.finished())
        {
            sh.dialog = None;
            self.force_full = true;
        }
        if sh
            .apps
            .as_ref()
            .is_some_and(|a| a.closing && a.t.finished())
        {
            sh.apps = None;
            self.force_full = true;
        }
        if sh
            .search
            .as_ref()
            .is_some_and(|s| s.closing && s.t.finished())
        {
            sh.search = None;
            self.force_full = true;
        }
        busy | self.step_dock(dt)
    }

    /// Any shell animation or overlay transition is running (needs frames).
    pub(crate) fn shell_animating(&self) -> bool {
        let sh = &self.shell;
        sh.menu.as_ref().is_some_and(|m| !m.t.finished())
            || sh.pop.as_ref().is_some_and(|p| !p.t.finished())
            || sh.dialog.as_ref().is_some_and(|d| !d.t.finished())
            || sh.apps.as_ref().is_some_and(|a| !a.t.finished())
            || sh.search.as_ref().is_some_and(|s| !s.t.finished())
            || sh.knobs.iter().any(|k| !k.finished())
            || sh.snap.as_ref().is_some_and(|p| !p.t.finished())
            || self.dock_animating()
    }

    // ---- overlay bookkeeping ----

    /// True while a transient overlay (menu, popover, sheet, Apps, Busca, Alt+Tab) is shown.
    pub fn overlay_open(&self) -> bool {
        let sh = &self.shell;
        sh.menu.is_some()
            || sh.pop.is_some()
            || sh.dialog.is_some()
            || sh.apps.is_some()
            || sh.search.is_some()
            || self.switcher.is_some()
    }

    /// The compositor painted the dirty region.
    pub(crate) fn overlay_painted(&self) {
        self.shell.dirty.set(Rect::new(0, 0, 0, 0));
    }

    /// Mark the whole screen dirty for the Apps overlay.
    pub(crate) fn apps_dirty_all(&self) {
        self.shell.dirty.set(Rect::new(0, 0, self.sw, self.sh));
    }

    /// Close every overlay that a click elsewhere dismisses (fade out).
    pub(crate) fn close_transients(&mut self) {
        let sh = &mut self.shell;
        if let Some(m) = sh.menu.as_mut()
            && !m.closing
        {
            m.closing = true;
            fade_out(&mut m.t, MENU_FADE);
        }
        if let Some(p) = sh.pop.as_mut()
            && !p.closing
        {
            p.closing = true;
            fade_out(&mut p.t, MENU_FADE);
        }
        self.force_full = true;
    }

    /// Is a modal layer (sheet, Apps, Busca) up? Mouse and keys go to it exclusively.
    pub(crate) fn modal_open(&self) -> bool {
        let sh = &self.shell;
        sh.dialog.as_ref().is_some_and(|d| !d.closing)
            || sh.apps.as_ref().is_some_and(|a| !a.closing)
            || sh.search.as_ref().is_some_and(|s| !s.closing)
    }

    // ---- commands ----

    /// The window the panel acts on: the focused one.
    pub(super) fn target_window(&self) -> Option<(WindowId, Kind)> {
        let id = self.focused()?;
        Some((id, self.kind_of(id)?))
    }
}
