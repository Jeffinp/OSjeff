//! Pointer handling: clicks, hover and the wheel.

use crate::desktop::apps::files::labels::crumbs_of;
use crate::desktop::*;
use kitsune_core::fileman::ui::{self, Layout, crumb_layout};
use kitsune_core::fileman::{self, Cmd};

impl Desktop {
    // ---- mouse ----

    /// A press in file-manager window `id` at `(px, py)` (screen coordinates).
    /// `right` is the right button. Geometry is `fileman::ui::Layout`, the same the
    /// renderer uses.
    pub(crate) fn files_click(&mut self, id: WindowId, _rect: Rect, px: i32, py: i32, right: bool) {
        let ctrl = self.keymap.ctrl();
        let shift = self.keymap.shift();
        let Some((lay, hit)) = self.files_hit(id, px, py) else {
            return;
        };
        // A sheet takes the click first.
        if self.files_mut(id).is_some_and(|f| f.sheet_open()) {
            self.files_sheet_click(id, &lay, px, py);
            return;
        }
        // Clicking away from the inline name field commits it.
        if self.files_mut(id).is_some_and(|f| f.input.is_some()) {
            self.files_commit_input(id);
        }
        // Clicking away from the search field gives the keyboard back to the list.
        if !matches!(hit, ui::Hit::Search)
            && let Some(f) = self.files_mut(id)
            && f.search.focused
        {
            f.search.focused = false;
        }
        // The scrollbar sits over the right edge of the rows and wins the press there.
        if !right
            && matches!(hit, ui::Hit::Item(_) | ui::Hit::Blank)
            && self.files_scrollbar_press(id, &lay, px, py)
        {
            return;
        }
        let now = appui::ticks();
        match hit {
            ui::Hit::Back => self.files_history(id, 0),
            ui::Hit::Forward => self.files_history(id, 1),
            ui::Hit::Crumb(i) => {
                let target = self
                    .files_mut(id)
                    .map(|f| fileman::breadcrumbs(&f.view.cwd))
                    .and_then(|c| c.get(i).map(|c| c.path.clone()));
                if let Some(p) = target {
                    self.files_go(id, &p);
                }
            }
            ui::Hit::CrumbFold => {
                // Up to the parent of the first crumb that is shown.
                let cwd = self.files_mut(id).map(|f| f.view.cwd.clone());
                if let Some(cwd) = cwd {
                    let crumbs = fileman::breadcrumbs(&cwd);
                    let first = {
                        let (_, _, w) = crumbs_of(&cwd);
                        crumb_layout(lay.path, &w).first
                    };
                    if let Some(c) = crumbs.get(first.saturating_sub(1)) {
                        self.files_go(id, &c.path.clone());
                    }
                }
            }
            ui::Hit::PathBlank | ui::Hit::Dead | ui::Hit::PreviewPane => {}
            ui::Hit::View(m) => self.files_set_view(id, m),
            ui::Hit::SortButton => {
                let at = (lay.sort.x, lay.sort.bottom() + 4);
                self.files_sort_menu(id, at);
            }
            ui::Hit::Search => {
                let on_clear = self.files_mut(id).is_some_and(|f| {
                    !f.search.input.text().is_empty()
                        && lay.search_is_field()
                        && appui::field_clear_rect(lay.search).contains(px, py)
                });
                if on_clear {
                    self.files_set_search(id, b"");
                    if let Some(f) = self.files_mut(id) {
                        f.search.focused = false;
                    }
                } else if let Some(f) = self.files_mut(id) {
                    f.search.focused = true;
                    f.search.last_input = now;
                }
            }
            ui::Hit::PreviewButton => self.files_cmd(id, Cmd::TogglePreview),
            ui::Hit::Place(p) => {
                if right {
                    return;
                }
                self.files_go_place(id, p);
            }
            ui::Hit::Header(k) => {
                if let Some(f) = self.files_mut(id) {
                    f.view.click_header(k);
                }
            }
            ui::Hit::Item(i) => self.files_item_press(id, i, ctrl, shift, right, px, py),
            ui::Hit::Blank => {
                if right {
                    if let Some(f) = self.files_mut(id) {
                        f.view.sel.clear();
                    }
                    self.files_context_menu(id, px, py);
                    return;
                }
                if self.files_scrollbar_press(id, &lay, px, py) {
                    return;
                }
                let Some(f) = self.files_mut(id) else {
                    return;
                };
                let base = if ctrl {
                    f.view.sel.selected()
                } else {
                    Vec::new()
                };
                if !ctrl {
                    f.view.sel.clear();
                }
                let anchor = (px - lay.list.x, py - lay.list.y + f.scroller.pos());
                f.gesture = Gesture::Band {
                    anchor,
                    cur: (px, py),
                    base,
                    additive: ctrl,
                };
                self.drag = Some(Drag {
                    win: id,
                    mode: DragMode::Files,
                });
            }
        }
        self.files_sync_preview(id);
    }

    /// A press on the overlay scrollbar's track: start dragging the thumb. `true` when it
    /// was on the scrollbar.
    fn files_scrollbar_press(&mut self, id: WindowId, lay: &Layout, px: i32, py: i32) -> bool {
        let Some(f) = self.files_mut(id) else {
            return false;
        };
        let n = f.view.rows.len();
        let content = ui::content_height(f.mode, lay.list.w, n);
        if content <= lay.list.h || px < lay.list.right() - 14 {
            return false;
        }
        let (off, len) = kitsune_core::widgets::scroll_thumb(
            lay.list.h,
            content as usize,
            lay.list.h as usize,
            f.scroller.pos().max(0) as usize,
            28,
        );
        let thumb_top = lay.list.y + off;
        let grab = if py >= thumb_top && py < thumb_top + len {
            py - thumb_top
        } else {
            len / 2
        };
        f.gesture = Gesture::Thumb { grab };
        f.scroll_fade.touch(appui::now_ms());
        self.files_thumb_to(id, py);
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Files,
        });
        true
    }

    /// Scroll so the thumb follows pointer `py`.
    pub(super) fn files_thumb_to(&mut self, id: WindowId, py: i32) {
        let Some(lay) = self.files_layout(id) else {
            return;
        };
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let Gesture::Thumb { grab } = f.gesture else {
            return;
        };
        let n = f.view.rows.len();
        let content = ui::content_height(f.mode, lay.list.w, n);
        let (_, len) = kitsune_core::widgets::scroll_thumb(
            lay.list.h,
            content as usize,
            lay.list.h as usize,
            0,
            28,
        );
        let span = (lay.list.h - len).max(1) as i64;
        let rel = (py - grab - lay.list.y).clamp(0, span as i32) as i64;
        let max = ui::max_scroll(f.mode, lay.list.w, lay.list.h, n) as i64;
        f.scroller.jump((max * rel / span) as i32);
        f.scroll_fade.touch(appui::now_ms());
    }

    #[allow(clippy::too_many_arguments)]
    fn files_item_press(
        &mut self,
        id: WindowId,
        i: usize,
        ctrl: bool,
        shift: bool,
        right: bool,
        px: i32,
        py: i32,
    ) {
        if right {
            if let Some(f) = self.files_mut(id)
                && !f.view.sel.is_selected(i)
            {
                f.view.sel.only(i);
            }
            self.files_context_menu(id, px, py);
            return;
        }
        let double = !ctrl && !shift && self.clicks.press(crate::interrupts::ticks(), px, py, id);
        if double {
            if let Some(f) = self.files_mut(id) {
                f.gesture = Gesture::None;
                f.view.sel.only(i);
            }
            self.drag = None;
            self.files_activate(id, i);
            return;
        }
        let Some(f) = self.files_mut(id) else {
            return;
        };
        let was_selected = f.view.sel.is_selected(i);
        if !was_selected || ctrl || shift {
            f.view.sel.click(i, ctrl, shift);
        }
        f.gesture = Gesture::Press {
            item: i,
            at: (px, py),
            collapse: was_selected && !ctrl && !shift,
        };
        self.drag = Some(Drag {
            win: id,
            mode: DragMode::Files,
        });
    }

    /// The pointer moved over a file manager (no button held): the item under it lights up.
    pub(crate) fn files_hover(&mut self, id: WindowId, px: i32, py: i32) -> bool {
        let hit = self.files_hit(id, px, py).map(|(_, h)| h);
        // The scrollbar wakes up when the pointer is near it.
        let Some(f) = self.files_mut(id) else {
            return false;
        };
        let hit = match hit {
            Some(ui::Hit::Blank | ui::Hit::Dead | ui::Hit::PathBlank | ui::Hit::PreviewPane) => {
                None
            }
            other => other,
        };
        if f.hover == hit {
            return false;
        }
        f.hover = hit;
        f.hover_t = kitsune_core::anim::Tween::at(0.0);
        f.hover_t
            .retarget(1.0, 0.12, kitsune_core::anim::curves::STANDARD);
        true
    }

    /// The pointer left window `id` (or moved to another window).
    pub(crate) fn files_unhover(&mut self, id: WindowId) {
        if let Some(f) = self.files_mut(id)
            && f.hover.is_some()
        {
            f.hover = None;
        }
    }

    /// Mouse wheel over a file manager: three rows per notch (`notches` > 0 scrolls down).
    pub(crate) fn files_wheel(&mut self, id: WindowId, notches: i32) {
        if let Some(f) = self.files_mut(id) {
            f.scroller.scroll_by(notches * ui::WHEEL_STEP);
            f.scroll_fade.touch(appui::now_ms());
        }
    }
}
