//! Executing a shell command, power actions, the dialog and the appearance poll.

use super::model::Cmd;
use super::model::Dialog;
use super::model::MENU_FADE;
use super::model::fade_in;
use super::model::fade_out;
use crate::desktop::*;

impl Desktop {
    /// Run a menu or button command.
    pub(crate) fn execute(&mut self, cmd: Cmd) {
        use kitsune_core::input::{KeyCode, KeyEvent, Mods};
        let target = self.target_window();
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), Mods::CTRL);
        match cmd {
            Cmd::Sep => {}
            Cmd::About => self.open_settings(crate::desktop::apps::ajustes::ABOUT),
            Cmd::Settings => self.open_settings(0),
            Cmd::Lock => self.lock_screen(),
            Cmd::SignOut => self.sign_out(),
            Cmd::Reboot => self.ask_power(false),
            Cmd::Shutdown => self.ask_power(true),
            Cmd::NewWindow => {
                let kind = target.map_or(Kind::Terminal, |(_, k)| k);
                if let Some((id, Kind::Browser)) = target {
                    // One browser window: "new" opens a tab.
                    self.menu_ctrl_key_browser(id, 't');
                } else {
                    self.new_window(kind);
                }
            }
            Cmd::CloseWindow => {
                if let Some((id, kind)) = target {
                    if kind == Kind::Browser {
                        self.browser_close_active(id);
                    } else {
                        self.request_close(id);
                    }
                }
            }
            Cmd::Minimize => {
                if let Some((id, _)) = target {
                    self.wm.minimize(id);
                }
            }
            Cmd::Zoom => {
                if let Some((id, _)) = target {
                    self.toggle_maximize(id);
                }
            }
            Cmd::Copy | Cmd::Paste | Cmd::Cut | Cmd::SelectAll
                if target.is_some_and(|(_, k)| k == Kind::Files) =>
            {
                use kitsune_core::fileman::Cmd as F;
                self.files_run_focused(match cmd {
                    Cmd::Copy => F::Copy,
                    Cmd::Paste => F::Paste,
                    Cmd::Cut => F::Cut,
                    _ => F::SelectAll,
                });
            }
            Cmd::Copy => self.copy_from_focused(),
            Cmd::Paste => self.paste_into_focused(),
            Cmd::Cut => self.menu_ctrl(ctrl('x')),
            Cmd::SelectAll => self.menu_ctrl(ctrl('a')),
            Cmd::Undo => self.menu_ctrl(ctrl('z')),
            Cmd::Redo => self.menu_ctrl(ctrl('y')),
            Cmd::OpenFile => self.menu_ctrl(ctrl('o')),
            Cmd::SaveFile => self.menu_ctrl(ctrl('s')),
            Cmd::BrowserZoomIn => self.menu_ctrl(ctrl('=')),
            Cmd::BrowserZoomOut => self.menu_ctrl(ctrl('-')),
            Cmd::BrowserZoomReset => self.menu_ctrl(ctrl('0')),
            Cmd::ShowApps => self.open_apps(),
            Cmd::ShowSearch => self.open_search(),
            Cmd::Gallery => {
                self.launch(Kind::Gallery);
            }
            Cmd::MoveToWorkspace(n) => {
                if let Some((id, _)) = target {
                    self.move_window_to_workspace(id, n);
                }
            }
            Cmd::Pin(k) => self.set_pinned(k, true),
            Cmd::Unpin(k) => self.set_pinned(k, false),
            Cmd::ShowDesktop => self.show_desktop(),
            Cmd::Snap(zone) => {
                if let Some((id, _)) = target {
                    self.snap_window(id, zone);
                }
            }
            Cmd::Activate(id) => {
                self.wm.activate(id);
            }
            Cmd::Launch(k) => {
                self.launch(k);
            }
            Cmd::NewOf(k) => {
                self.new_window(k);
            }
            Cmd::Files(c) => self.files_run_focused(c),
            Cmd::QuitOf(k) => {
                let ids: Vec<WindowId> = self
                    .wm
                    .windows()
                    .iter()
                    .filter(|w| w.app.kind() == k)
                    .map(|w| w.id)
                    .collect();
                for id in ids {
                    self.request_close(id);
                }
            }
        }
        self.force_full = true;
    }

    /// A Ctrl chord for browser window `id` from a menu entry.
    fn menu_ctrl_key_browser(&mut self, id: WindowId, c: char) {
        self.browser_ctrl_chord(id, c);
    }

    /// Send a Ctrl chord to the focused app as if typed (menu entries reuse the keys).
    fn menu_ctrl(&mut self, ev: kitsune_core::input::KeyEvent) {
        let Some((id, kind)) = self.target_window() else {
            return;
        };
        match kind {
            Kind::Editor => {
                self.editor_event(id, ev);
            }
            Kind::Terminal => {
                self.term_event(id, ev);
            }
            Kind::Browser => {
                use kitsune_core::input::KeyCode;
                if let KeyCode::Char(c) = ev.code {
                    self.browser_ctrl_chord(id, c);
                }
            }
            _ => {}
        }
    }

    /// Open the Settings window on `section`.
    pub(crate) fn open_settings(&mut self, section: u8) {
        if let Some(id) = self.launch(Kind::Settings)
            && let Some(App::Settings(s)) = self.app_mut(id)
        {
            s.select_section(section);
        }
    }

    /// Ask before restarting / shutting down.
    pub(crate) fn ask_power(&mut self, shutdown: bool) {
        self.close_transients();
        self.shell.dialog = Some(Dialog {
            title: String::from(if shutdown {
                kitsune_core::t!("power.shutdown_title")
            } else {
                kitsune_core::t!("power.restart_title")
            }),
            body: String::from(kitsune_core::t!("power.body")),
            ok: String::from(if shutdown {
                kitsune_core::t!("power.shutdown")
            } else {
                kitsune_core::t!("power.restart")
            }),
            cmd: if shutdown { Cmd::Shutdown } else { Cmd::Reboot },
            t: fade_in(MENU_FADE),
            closing: false,
            focus: 0,
        });
    }

    /// The sheet's confirm button: really do it.
    pub(crate) fn power_now(&mut self, cmd: Cmd) {
        if self.guard_unsaved() {
            return;
        }
        match cmd {
            Cmd::Shutdown => crate::power::shutdown(),
            _ => crate::power::reboot(),
        }
    }

    /// Fade the sheet out (cancel or after confirming).
    pub(crate) fn close_dialog(&mut self) {
        if let Some(d) = self.shell.dialog.as_mut()
            && !d.closing
        {
            d.closing = true;
            fade_out(&mut d.t, MENU_FADE);
        }
    }

    /// The HUD toggle (Ctrl+Alt+H).
    pub fn hud_visible(&self) -> bool {
        self.shell.hud
    }

    /// Resolve the appearance setting for the current local hour (Auto follows the
    /// clock) and tell the compositor when the look changed.
    pub(crate) fn poll_appearance(&mut self, hour: u8) {
        let s = crate::settings::get();
        let want = s.appearance.resolve(hour);
        if crate::theme::set_appearance(want) {
            self.bg_dirty = true;
            self.force_full = true;
            // Backdrops of open overlays were captured in the old look.
            self.close_transients();
        }
        self.shell.last_hour = hour;
    }
}
