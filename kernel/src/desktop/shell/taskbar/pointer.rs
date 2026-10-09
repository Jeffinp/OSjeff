//! Pointer handling of the bar: hover, press, release, click and the context menu.

use super::state::IconDrag;
use crate::desktop::shell::*;
use crate::desktop::*;
use kitsune_core::anim::curves;
use kitsune_core::taskbar::{self as tb, Click, Hit};

impl Desktop {
    /// The part of the bar under `(x, y)`.
    pub(crate) fn dock_item_at(&self, x: i32, y: i32) -> Option<Hit> {
        tb::hit(&self.task_layout(), x, y)
    }

    /// The pointer moved to `(x, y)`: track the hovered icon, the tooltip and an icon drag.
    pub(crate) fn dock_pointer(&mut self, x: i32, y: i32) {
        let l = self.task_layout();
        let hover = tb::hit(&l, x, y);
        let pinned = self.shell.task.pinned.len();
        let d = &mut self.shell.task;
        if hover != d.hover {
            d.hover = hover;
            d.rest = 0.0;
            if d.tip.target() > 0.0 {
                d.tip.retarget(0.0, 0.08, curves::EXIT);
            }
            d.dirty = true;
        }
        // A press that travelled far enough starts dragging a pinned icon.
        if d.drag.is_none()
            && let Some((i, px, _)) = d.press
            && i < pinned
            && (x - px).abs() >= tb::DRAG_SLOP
        {
            d.drag = Some(IconDrag {
                from: i,
                x,
                slot: i,
            });
            d.tip.retarget(0.0, 0.05, curves::EXIT);
        }
        if let Some(dr) = d.drag.as_mut() {
            dr.x = x;
            let slot = tb::drop_slot(&l, x, pinned);
            if slot != dr.slot {
                dr.slot = slot;
            }
            d.dirty = true;
        }
    }

    /// A left press on part `hit` of the bar. The Apps button and the sliver act at once; an app
    /// icon waits for the release (so it can be dragged instead).
    pub(crate) fn dock_press(&mut self, hit: Hit, x: i32, y: i32) {
        match hit {
            Hit::Apps => self.open_apps(),
            Hit::Sliver => self.show_desktop(),
            Hit::Item(i) => self.shell.task.press = Some((i, x, y)),
        }
        self.shell.task.dirty = true;
    }

    /// The button went up: finish an icon drag (reorder) or complete a click.
    pub(crate) fn dock_release(&mut self, x: i32, y: i32) {
        let d = &mut self.shell.task;
        if let Some(dr) = d.drag.take() {
            d.press = None;
            tb::reorder(&mut d.pinned, dr.from, dr.slot);
            d.dirty = true;
            return;
        }
        let Some((i, _, _)) = d.press.take() else {
            return;
        };
        if self.dock_item_at(x, y) == Some(Hit::Item(i)) {
            self.task_click(i, self.keymap.shift());
        }
        self.shell.task.dirty = true;
    }

    /// Complete a click on icon `i`: open, bring forward or minimise; with Shift, a new window.
    fn task_click(&mut self, i: usize, new_window: bool) {
        let Some(kind) = self.task_kinds().get(i).copied() else {
            return;
        };
        if new_window && kind.multi() {
            self.new_window(kind);
            return;
        }
        match tb::click_action(self.kind_running(kind), self.kind_focused(kind)) {
            Click::Launch | Click::Focus => {
                if kind != Kind::WasmApp {
                    self.shell.recents.note(&kind.recent_key());
                }
                self.launch(kind);
            }
            Click::Minimize => {
                if let Some(id) = self.focused() {
                    self.wm.minimize(id);
                }
            }
        }
    }

    /// *Mostrar área de trabalho*: minimise every window, or bring back the ones it hid.
    pub(crate) fn show_desktop(&mut self) {
        let visible: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| w.active())
            .map(|w| w.id)
            .collect();
        if !visible.is_empty() {
            for &id in &visible {
                self.wm.minimize(id);
            }
            self.shell.task.hidden = visible;
        } else {
            for id in core::mem::take(&mut self.shell.task.hidden) {
                if self.wm.get(id).is_some_and(|w| w.minimized) {
                    self.wm.activate(id);
                }
            }
        }
        self.force_full = true;
    }

    /// Pin or unpin `kind`.
    pub(crate) fn set_pinned(&mut self, kind: Kind, pin: bool) {
        let t = &mut self.shell.task;
        if pin {
            if !t.pinned.contains(&kind) && kind.pinnable() {
                t.pinned.push(kind);
            }
        } else {
            t.pinned.retain(|k| *k != kind);
        }
        t.dirty = true;
        self.force_full = true;
    }

    /// Right click on part `hit` at `(x, y)`: open its menu (the app's windows, new window, pin).
    pub(crate) fn dock_context(&mut self, hit: Hit, x: i32, _y: i32) {
        let Hit::Item(i) = hit else {
            return;
        };
        let Some(k) = self.task_kinds().get(i).copied() else {
            return;
        };
        let focused = self.focused();
        let mut entries: Vec<Entry> = Vec::new();
        let wins: Vec<(WindowId, String)> = self
            .wm
            .windows()
            .iter()
            .filter(|w| w.app.kind() == k && !w.is_closing())
            .map(|w| (w.id, w.app.title.clone()))
            .collect();
        for (id, title) in wins.iter().take(8) {
            let mut e = Entry::item(title, "", Cmd::Activate(*id));
            e.checked = focused == Some(*id);
            entries.push(e);
        }
        if wins.is_empty() {
            entries.push(Entry::item(
                kitsune_core::t!("common.open"),
                "",
                Cmd::Launch(k),
            ));
        }
        entries.push(Entry::sep());
        if k.multi() {
            entries.push(Entry::item(
                kitsune_core::t!("taskbar.menu.new_window"),
                "",
                Cmd::NewOf(k),
            ));
        }
        if self.shell.task.pinned.contains(&k) {
            entries.push(Entry::item(
                kitsune_core::t!("taskbar.menu.unpin"),
                "",
                Cmd::Unpin(k),
            ));
        } else if k.pinnable() {
            entries.push(Entry::item(
                kitsune_core::t!("taskbar.menu.pin"),
                "",
                Cmd::Pin(k),
            ));
        }
        if !wins.is_empty() {
            entries.push(Entry::sep());
            entries.push(Entry::item(
                if wins.len() > 1 {
                    kitsune_core::t!("taskbar.menu.close_all")
                } else {
                    kitsune_core::t!("taskbar.menu.close_window")
                },
                "",
                Cmd::QuitOf(k),
            ));
        }
        let rows = entries.len() as i32 * 24 + 12 + 8;
        let top = self.task_layout().panel.y;
        self.open_menu(MenuOrigin::Context, entries, (x - 80, top - rows - 8));
    }
}
