//! Menus: the system menu, a window's menu, context menus, and drawing the open menu.

use crate::desktop::kit::glass::panel as glass_panel;
use crate::desktop::shell::*;
use crate::desktop::*;
use crate::text::{self, BODY, Weight};
use kitsune_core::chrome::{MenuRow, menu_geom};
use kitsune_core::snap::SnapZone;
use kitsune_core::style::R_MENU;
use kitsune_core::t;

impl Desktop {
    // ---- menus ----

    pub(crate) fn system_menu(&self) -> Vec<Entry> {
        alloc::vec![
            Entry::item(t!("menu.system.about"), "", Cmd::About),
            Entry::sep(),
            Entry::item(t!("menu.system.settings"), "", Cmd::Settings),
            Entry::item(t!("menu.system.gallery"), "Ctrl+Alt+G", Cmd::Gallery),
            Entry::sep(),
            Entry::item(t!("menu.system.lock"), "Ctrl+Alt+L", Cmd::Lock),
            Entry::item(t!("menu.system.sign_out"), "", Cmd::SignOut),
            Entry::sep(),
            Entry::item(t!("menu.system.restart"), "", Cmd::Reboot),
            Entry::item(t!("menu.system.shutdown"), "", Cmd::Shutdown),
        ]
    }

    /// The focused app's menu `name` (`file`, `edit`, `view`; anything else is the window menu).
    fn named_menu(&self, name: &str) -> Vec<Entry> {
        let kind = self.focused().and_then(|id| self.kind_of(id));
        let wasm = kind == Some(Kind::WasmApp);
        match name {
            "file" => {
                let browser = kind == Some(Kind::Browser);
                let mut v = alloc::vec![Entry::item(
                    if browser {
                        t!("menu.file.new_tab")
                    } else {
                        t!("menu.file.new_window")
                    },
                    if wasm {
                        ""
                    } else if browser {
                        "Ctrl+T"
                    } else {
                        "Ctrl+N"
                    },
                    Cmd::NewWindow
                )];
                match kind {
                    Some(Kind::Editor) => {
                        v.push(Entry::item(t!("menu.file.open"), "Ctrl+O", Cmd::OpenFile));
                        v.push(Entry::item(t!("menu.file.save"), "Ctrl+S", Cmd::SaveFile));
                    }
                    None => {}
                    _ => {}
                }
                v.push(Entry::sep());
                v.push(
                    Entry::item(
                        if browser {
                            t!("menu.file.close_tab")
                        } else {
                            t!("menu.file.close_window")
                        },
                        "Ctrl+W",
                        Cmd::CloseWindow,
                    )
                    .disabled_if(kind.is_none()),
                );
                v
            }
            "edit" => {
                let editor = kind == Some(Kind::Editor);
                let files = kind == Some(Kind::Files);
                alloc::vec![
                    Entry::item(t!("menu.edit.undo"), "Ctrl+Z", Cmd::Undo).disabled_if(!editor),
                    Entry::item(t!("menu.edit.redo"), "Ctrl+Y", Cmd::Redo).disabled_if(!editor),
                    Entry::sep(),
                    Entry::item(t!("menu.edit.cut"), "Ctrl+X", Cmd::Cut)
                        .disabled_if(!editor && !files),
                    Entry::item(
                        t!("menu.edit.copy"),
                        if kind == Some(Kind::Terminal) {
                            "Ctrl+Shift+C"
                        } else {
                            "Ctrl+C"
                        },
                        Cmd::Copy
                    )
                    .disabled_if(kind.is_none() || wasm),
                    Entry::item(t!("menu.edit.paste"), "Ctrl+V", Cmd::Paste)
                        .disabled_if(kind.is_none() || wasm),
                    Entry::sep(),
                    Entry::item(t!("menu.edit.select_all"), "Ctrl+A", Cmd::SelectAll)
                        .disabled_if(!editor && !files),
                ]
            }
            "view" => {
                let browser = kind == Some(Kind::Browser);
                let mut v = alloc::vec![];
                if browser {
                    v.push(Entry::item(
                        t!("menu.view.zoom_in"),
                        "Ctrl++",
                        Cmd::BrowserZoomIn,
                    ));
                    v.push(Entry::item(
                        t!("menu.view.zoom_out"),
                        "Ctrl+-",
                        Cmd::BrowserZoomOut,
                    ));
                    v.push(Entry::item(
                        t!("menu.view.zoom_reset"),
                        "Ctrl+0",
                        Cmd::BrowserZoomReset,
                    ));
                    v.push(Entry::sep());
                }
                if let Some(fid) = self.focused().filter(|_| kind == Some(Kind::Files))
                    && let Some(App::Files(f)) = self.wm.get(fid).map(|w| &w.app.app)
                {
                    use kitsune_core::fileman::{Cmd as FCmd, ui::ViewMode};
                    let mut list = Entry::item(
                        t!("menu.view.as_list"),
                        "Ctrl+1",
                        Cmd::Files(FCmd::SetView(ViewMode::List)),
                    );
                    list.checked = f.mode == ViewMode::List;
                    let mut icons = Entry::item(
                        t!("menu.view.as_icons"),
                        "Ctrl+2",
                        Cmd::Files(FCmd::SetView(ViewMode::Icons)),
                    );
                    icons.checked = f.mode == ViewMode::Icons;
                    v.push(list);
                    v.push(icons);
                    let mut pv = Entry::item(
                        t!("menu.view.preview"),
                        t!("menu.shortcut.preview"),
                        Cmd::Files(FCmd::TogglePreview),
                    );
                    pv.checked = f.preview_open;
                    v.push(pv);
                    v.push(Entry::sep());
                }
                v.push(Entry::item(t!("menu.view.window_zoom"), "", Cmd::Zoom));
                v.push(Entry::sep());
                v.push(Entry::item(t!("menu.view.show_apps"), "", Cmd::ShowApps));
                v.push(Entry::item(
                    t!("menu.view.search"),
                    t!("menu.shortcut.search"),
                    Cmd::ShowSearch,
                ));
                v
            }
            _ => {
                let mut v = alloc::vec![
                    Entry::item(t!("menu.window.minimize"), "Ctrl+M", Cmd::Minimize)
                        .disabled_if(kind.is_none()),
                    Entry::item(t!("menu.window.zoom"), "", Cmd::Zoom).disabled_if(kind.is_none()),
                ];
                let list = self.wm.switch_list();
                if !list.is_empty() {
                    v.push(Entry::sep());
                    let focused = self.focused();
                    for id in list.into_iter().take(10) {
                        if let Some(w) = self.wm.get(id) {
                            let mut e = Entry::item(&w.app.title, "", Cmd::Activate(id));
                            e.checked = focused == Some(id);
                            v.push(e);
                        }
                    }
                }
                v
            }
        }
    }

    /// The menu behind a window's title-bar button: the app's File, Edit and View entries
    /// (disabled ones left out), then the window commands. `id` must be the focused window.
    fn window_menu(&self, id: WindowId) -> Vec<Entry> {
        let mut v: Vec<Entry> = Vec::new();
        let section = |v: &mut Vec<Entry>, items: Vec<Entry>, keep_disabled: bool| {
            let items: Vec<Entry> = items
                .into_iter()
                .filter(|e| e.cmd == Cmd::Sep || e.enabled || keep_disabled)
                .collect();
            // Drop separators that would lead, trail or double up.
            let mut out: Vec<Entry> = Vec::new();
            for e in items {
                if e.cmd == Cmd::Sep && out.last().is_none_or(|l| l.cmd == Cmd::Sep) {
                    continue;
                }
                out.push(e);
            }
            while out.last().is_some_and(|l| l.cmd == Cmd::Sep) {
                out.pop();
            }
            if out.is_empty() {
                return;
            }
            if !v.is_empty() {
                v.push(Entry::sep());
            }
            v.extend(out);
        };
        section(&mut v, self.named_menu("file"), true);
        section(&mut v, self.named_menu("edit"), false);
        let view: Vec<Entry> = self
            .named_menu("view")
            .into_iter()
            .filter(|e| !matches!(e.cmd, Cmd::Zoom | Cmd::ShowApps | Cmd::ShowSearch))
            .collect();
        section(&mut v, view, false);
        let state = self
            .wm
            .get(id)
            .map(|w| (w.snap_state().is_some(), w.resizable));
        let (tiled, resizable) = state.unwrap_or((false, false));
        let mut win = alloc::vec![Entry::item(
            t!("menu.window.minimize"),
            "Ctrl+M",
            Cmd::Minimize
        )];
        let mut zoom = Entry::item(
            if tiled {
                t!("menu.window.restore")
            } else {
                t!("menu.window.maximize")
            },
            "Alt+↑",
            Cmd::Zoom,
        );
        zoom.enabled = resizable;
        win.push(zoom);
        let mut left = Entry::item(
            t!("menu.window.snap_left"),
            "Alt+←",
            Cmd::Snap(SnapZone::Left),
        );
        left.enabled = resizable;
        let mut right = Entry::item(
            t!("menu.window.snap_right"),
            "Alt+→",
            Cmd::Snap(SnapZone::Right),
        );
        right.enabled = resizable;
        win.push(left);
        win.push(right);
        section(&mut v, win, true);
        // Move the window to another workspace.
        let cur = self.wm.workspace();
        let moves: Vec<Entry> = (0..self.wm.visible_workspaces())
            .filter(|&i| i != cur)
            .map(|i| {
                Entry::item(
                    &t!("menu.window.move_to_workspace", n = i + 1),
                    "",
                    Cmd::MoveToWorkspace(i),
                )
            })
            .collect();
        section(&mut v, moves, true);
        v
    }

    /// Open the menu button's menu of window `id`, under the button, right edge aligned.
    pub(crate) fn open_window_menu(&mut self, id: WindowId) {
        if self
            .shell
            .menu
            .as_ref()
            .is_some_and(|m| !m.closing && m.origin == MenuOrigin::Window(id))
        {
            self.close_transients();
            return;
        }
        let Some((rect, resizable)) = self.wm.get(id).map(|w| (w.rect, w.resizable)) else {
            return;
        };
        let Some(btn) = rect.title_layout(resizable, true).menu else {
            return;
        };
        let entries = self.window_menu(id);
        self.open_menu_at(
            MenuOrigin::Window(id),
            entries,
            (btn.right(), btn.bottom() + 2),
            true,
        );
    }

    /// Open a menu with its top-left near `at`.
    pub(crate) fn open_menu(&mut self, origin: MenuOrigin, entries: Vec<Entry>, at: (i32, i32)) {
        self.open_menu_at(origin, entries, at, false);
    }

    /// Open a menu near `at`: with its top-left there, or (`right_edge`) its top-right.
    pub(crate) fn open_menu_at(
        &mut self,
        origin: MenuOrigin,
        entries: Vec<Entry>,
        at: (i32, i32),
        right_edge: bool,
    ) {
        let rows: Vec<MenuRow> = entries
            .iter()
            .map(|e| {
                if e.cmd == Cmd::Sep {
                    MenuRow::Separator
                } else {
                    MenuRow::Item {
                        label_w: text::measure(&e.label, BODY, Weight::Regular),
                        shortcut_w: text::measure(e.shortcut, BODY, Weight::Regular),
                    }
                }
            })
            .collect();
        let mut geom = menu_geom(&rows, at, self.sw, self.sh);
        if right_edge {
            geom = menu_geom(&rows, (at.0 - geom.rect.w, at.1), self.sw, self.sh);
        }
        // Opening a menu closes whatever else was transient.
        if let Some(p) = self.shell.pop.as_mut() {
            p.closing = true;
            p.t = fade_in(0.01);
        }
        self.shell.pop = None;
        self.shell.menu = Some(OpenMenu {
            origin,
            entries,
            rows,
            geom,
            hover: None,
            t: fade_in(MENU_FADE),
            closing: false,
            glass: Default::default(),
        });
        self.force_full = true;
    }

    /// Run row `i` of the open menu and close it.
    pub(crate) fn menu_pick(&mut self, i: usize) {
        let cmd = match self.shell.menu.as_ref().and_then(|m| m.entries.get(i)) {
            Some(e) if e.enabled => e.cmd,
            _ => return,
        };
        self.close_transients();
        self.execute(cmd);
    }

    pub(super) fn draw_open_menu(&self, c: &mut Canvas, m: &OpenMenu) {
        let p = theme::pal();
        let fade = level(&m.t);
        let slide = ((256 - fade) as i32 * 6) / 256;
        let mut g = m.geom.rect;
        g.y -= slide;
        glass_panel(
            c,
            g,
            R_MENU,
            &m.glass,
            10,
            p.menu_tint,
            p.separator,
            Shadow {
                blur: 12,
                dy: 8,
                alpha: 80,
            },
            fade,
        );
        let a = fade as u16;
        for (i, (rect, e)) in m.geom.rows.iter().zip(&m.entries).enumerate() {
            let mut r = *rect;
            r.y -= slide;
            if e.cmd == Cmd::Sep {
                let line = Rect::new(r.x + 8, r.y + r.h / 2, r.w - 16, 1);
                let (sc, sa) = theme::tint(p.separator);
                c.blend_rect(line, sc, (sa as u32 * fade / 256) as u16);
                continue;
            }
            // Items fade with the panel through the snapshot-free route: draw at full
            // strength once the panel is mostly in.
            if a > 100 {
                ui::menu_item(
                    c,
                    r,
                    &e.label,
                    e.shortcut,
                    m.hover == Some(i),
                    e.enabled,
                    e.checked,
                );
            }
        }
    }
}

impl Entry {
    fn disabled_if(mut self, off: bool) -> Entry {
        if off {
            self.enabled = false;
        }
        self
    }
}
