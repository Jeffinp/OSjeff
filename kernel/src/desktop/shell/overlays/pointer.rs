//! Pointer handling of every shell layer (menus, popovers, the overlays) and the desktop menu.

use super::dialog::sheet_geom;
use crate::desktop::shell::*;
use crate::desktop::*;
use kitsune_core::chrome::{self, launcher_cell_at, spotlight_geom};
use kitsune_core::style::PANEL_H;

impl Desktop {
    // ---- pointer ----

    /// The pointer moved to `(x, y)`: update hover states of the shell layers.
    pub(crate) fn shell_pointer(&mut self, x: i32, y: i32) -> bool {
        let mut changed = false;
        // Panel item under the pointer.
        let item = self.panel_item_at(x, y).map(|(i, _)| i);
        if item != self.shell.panel_hover {
            self.shell.panel_hover = item;
            self.mark_dirty(self.panel_rect());
            changed = true;
        }
        // (The hover washes inside a popover repaint with the overlay on every pointer move.)
        if let Some(m) = self.shell.menu.as_mut() {
            let hover = kitsune_core::chrome::menu_row_at(&m.geom, &m.rows, x, y);
            if hover != m.hover {
                m.hover = hover;
            }
        }
        if self.shell.apps.is_some() {
            let (hit, rail) = {
                let a = self.shell.apps.as_ref().expect("checked");
                let g = self.apps_geom(a);
                (
                    launcher_cell_at(&g, self.sh, x, y),
                    chrome::launcher_rail_at(&g, x, y),
                )
            };
            self.apps_hover_to(hit);
            if let Some(a) = self.shell.apps.as_mut()
                && a.rail_hover != rail
            {
                a.rail_hover = rail;
                self.apps_dirty_all();
            }
        }
        if let Some(s) = self.shell.search.as_mut() {
            let g = spotlight_geom(self.sw, self.sh, s.hits.len());
            if let Some(i) = g.rows.iter().position(|r| r.contains(x, y))
                && i != s.sel
            {
                s.sel = i;
            }
        }
        if let Some(d) = self.shell.dialog.as_mut() {
            let (_, cancel, ok) = sheet_geom(self.sw, self.sh);
            if cancel.contains(x, y) {
                d.focus = 0;
            } else if ok.contains(x, y) {
                d.focus = 1;
            }
        }
        changed
    }

    /// A left press while a shell layer is up. Returns `true` when the layers consumed it.
    pub(crate) fn shell_click(&mut self, x: i32, y: i32) -> bool {
        // Sheet: modal.
        if let Some(d) = self.shell.dialog.as_ref().filter(|d| !d.closing) {
            let (_, cancel, ok) = sheet_geom(self.sw, self.sh);
            let cmd = d.cmd;
            if ok.contains(x, y) {
                self.close_dialog();
                self.power_now(cmd);
            } else if cancel.contains(x, y) {
                self.close_dialog();
            }
            return true;
        }
        if self.shell.apps.as_ref().is_some_and(|a| !a.closing) {
            enum Pick {
                Launch(Target),
                Rail(usize),
                Nothing,
            }
            let pick = {
                let a = self.shell.apps.as_ref().expect("checked");
                let g = self.apps_geom(a);
                if g.field.contains(x, y)
                    || g.rail.contains(x, y) && chrome::launcher_rail_at(&g, x, y).is_none()
                {
                    Pick::Nothing
                } else if let Some(i) = chrome::launcher_rail_at(&g, x, y) {
                    Pick::Rail(i)
                } else if let Some(i) = chrome::launcher_recent_at(&g, x, y) {
                    self.recent_tiles(a)
                        .get(i)
                        .and_then(|&ti| a.tiles.get(ti))
                        .map_or(Pick::Nothing, |t| Pick::Launch(t.target))
                } else {
                    launcher_cell_at(&g, self.sh, x, y)
                        .and_then(|pos| a.shown.get(pos).and_then(|&i| a.tiles.get(i)))
                        .map_or(Pick::Nothing, |t| Pick::Launch(t.target))
                }
            };
            if y < PANEL_H && self.panel_item_at(x, y).is_some() {
                self.close_apps();
                return false;
            }
            match pick {
                Pick::Rail(i) => self.apps_set_category(i),
                Pick::Launch(t) => {
                    self.close_apps();
                    self.launch_target(t);
                }
                // The field, the rail's padding: stay; a click on empty space closes.
                Pick::Nothing => {
                    let inside = {
                        let a = self.shell.apps.as_ref().expect("checked");
                        let g = self.apps_geom(a);
                        g.field.contains(x, y) || g.rail.contains(x, y)
                    };
                    if !inside {
                        self.close_apps();
                    }
                }
            }
            return true;
        }
        if self.shell.search.as_ref().is_some_and(|s| !s.closing) {
            let (hit, inside) = {
                let s = self.shell.search.as_ref().expect("checked");
                let g = spotlight_geom(self.sw, self.sh, s.hits.len());
                (
                    g.rows
                        .iter()
                        .position(|r| r.contains(x, y))
                        .and_then(|i| s.hits.get(i).cloned()),
                    g.panel.contains(x, y),
                )
            };
            if !inside {
                self.close_search();
                return y >= PANEL_H;
            }
            if let Some(h) = hit {
                self.close_search();
                self.activate_hit(h);
            }
            return true;
        }
        // Menu.
        if self.shell.menu.as_ref().is_some_and(|m| !m.closing) {
            let (row, inside) = {
                let m = self.shell.menu.as_ref().expect("checked");
                (
                    kitsune_core::chrome::menu_row_at(&m.geom, &m.rows, x, y),
                    m.geom.rect.contains(x, y),
                )
            };
            if let Some(i) = row {
                self.menu_pick(i);
                return true;
            }
            if inside {
                return true;
            }
            // A click on a panel item closes the menu and acts on the item.
            if let Some((item, _)) = self.panel_item_at(x, y) {
                self.close_transients();
                self.panel_click(item, x, y);
                return true;
            }
            self.close_transients();
            return true;
        }
        // Popover.
        if self.shell.pop.as_ref().is_some_and(|p| !p.closing) {
            if self.popover_click(x, y) {
                return true;
            }
            let was = self.shell.pop.as_ref().map(|p| p.kind);
            self.close_transients();
            if let Some((item, _)) = self.panel_item_at(x, y) {
                let same = matches!(
                    (was, item),
                    (Some(PopKind::Quick), PanelItem::Tray)
                        | (Some(PopKind::Centre), PanelItem::Clock)
                );
                if !same {
                    self.panel_click(item, x, y);
                }
                return true;
            }
            return false;
        }
        // The panel.
        if let Some((item, _)) = self.panel_item_at(x, y) {
            self.panel_click(item, x, y);
            return true;
        }
        false
    }

    /// A left click on panel item `item`.
    pub(crate) fn panel_click(&mut self, item: PanelItem, x: i32, y: i32) {
        match item {
            PanelItem::Workspaces => {
                let n = self.wm.visible_workspaces();
                let cur = self.wm.workspace();
                if let Some((_, r)) = self.panel_items().into_iter().find(|(i, _)| *i == item)
                    && let Some(ws) = chrome::workspace_at(r, n, cur, x, y)
                {
                    self.go_workspace(ws);
                }
            }
            PanelItem::Apps => self.open_apps(),
            PanelItem::Search => self.open_search(),
            PanelItem::Tray => self.open_popover(PopKind::Quick),
            PanelItem::Clock => self.open_popover(PopKind::Centre),
        }
        self.force_full = true;
    }

    /// A right click on the Apps button: the system menu.
    pub(crate) fn panel_context(&mut self, item: PanelItem) {
        if item != PanelItem::Apps {
            return;
        }
        let Some((_, rect)) = self.panel_items().into_iter().find(|(i, _)| *i == item) else {
            return;
        };
        let entries = self.system_menu();
        self.open_menu(MenuOrigin::Context, entries, (rect.x, PANEL_H));
    }

    /// A wheel step while Apps is up: scroll its grid.
    pub(crate) fn shell_wheel(&mut self, dz: i32) -> bool {
        let Some(a) = self.shell.apps.as_ref() else {
            return false;
        };
        let g = self.apps_geom(a);
        let max = g.total_rows.saturating_sub(g.visible_rows) as i32;
        let next = (a.scroll as i32 + dz.signum()).clamp(0, max) as usize;
        let changed = next != a.scroll;
        if let Some(a) = self.shell.apps.as_mut() {
            a.scroll = next;
        }
        if changed {
            self.apps_dirty_all();
        }
        changed
    }

    /// Context menu on the empty desktop.
    pub(crate) fn desktop_context(&mut self, x: i32, y: i32) {
        let entries = alloc::vec![
            Entry::item(kitsune_core::t!("menu.view.show_apps"), "", Cmd::ShowApps),
            Entry::item(
                kitsune_core::t!("menu.view.search"),
                kitsune_core::t!("menu.shortcut.search"),
                Cmd::ShowSearch,
            ),
            Entry::item(
                kitsune_core::t!("menu.desktop.show_desktop"),
                "Ctrl+Alt+D",
                Cmd::ShowDesktop,
            ),
            Entry::sep(),
            Entry::item(
                &kitsune_core::t!("common.open_app", app = Kind::Files.label()),
                "",
                Cmd::Launch(Kind::Files),
            ),
            Entry::item(
                &kitsune_core::t!("common.open_app", app = Kind::Terminal.label()),
                "",
                Cmd::Launch(Kind::Terminal),
            ),
            Entry::sep(),
            Entry::item(kitsune_core::t!("menu.system.settings"), "", Cmd::Settings),
        ];
        self.open_menu(MenuOrigin::Context, entries, (x, y));
    }
}
