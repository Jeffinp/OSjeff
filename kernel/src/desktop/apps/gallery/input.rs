//! Clicks and keys of the gallery.

use super::state::TABS;
use super::state::layout;
use crate::desktop::*;

impl Desktop {
    /// A click in gallery window `id` at screen `(x, y)`.
    pub(crate) fn gallery_click(&mut self, id: WindowId, rect: Rect, x: i32, y: i32) {
        let l = layout(rect.body());
        let Some(App::Gallery(g)) = self.app_mut(id) else {
            return;
        };
        g.field_focus = false;
        if let Some(i) = crate::desktop::wlogic::segmented_hit(l.tabs, TABS.len(), x, y) {
            g.tab = i;
            return;
        }
        if g.tab != 0 {
            return;
        }
        if let Some(i) = crate::desktop::wlogic::segmented_hit(l.segmented, 3, x, y) {
            g.segment = i;
        } else if l.switch.inflated(4).contains(x, y) {
            g.switch_on = !g.switch_on;
        } else if l.slider.contains(x, y) {
            g.slider = crate::desktop::wlogic::slider_value(l.slider, x, 0, 100);
        } else if let Some(i) = l.checks.iter().position(|r| r.contains(x, y)) {
            g.checks[i] = !g.checks[i];
        } else if let Some(i) = l.radios.iter().position(|r| r.contains(x, y)) {
            g.radio = i;
        } else if l.field.contains(x, y) {
            g.field_focus = true;
        } else if l.list.contains(x, y) {
            g.list_sel = ((y - l.list.y - 6) / 30).clamp(0, 3) as usize;
        }
    }

    /// Typing into the gallery's text field.
    pub(crate) fn gallery_key(&mut self, id: WindowId, key: Key) {
        let Some(App::Gallery(g)) = self.app_mut(id) else {
            return;
        };
        match key {
            Key::Char(b) if g.field_focus && (0x20..0x7F).contains(&b) && g.field.len() < 40 => {
                g.field.push(char::from(b));
            }
            Key::Backspace if g.field_focus => {
                g.field.pop();
            }
            Key::Esc => g.field_focus = false,
            Key::Left | Key::Right => {
                let n = TABS.len();
                g.tab = if key == Key::Right {
                    (g.tab + 1) % n
                } else {
                    (g.tab + n - 1) % n
                };
            }
            _ => {}
        }
    }
}
