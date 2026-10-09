//! Mouse and key input forwarded to a WASM app.

use crate::desktop::*;
use crate::wasm::{self};

impl Desktop {
    /// Any WASM window on screen? (keeps the per-frame damage path running)
    pub(crate) fn wasm_pointer_target(&self, px: i32, py: i32) -> Option<(WindowId, i32, i32)> {
        let id = self.topmost_at(px, py)?;
        let w = self.wm.get(id)?;
        if !matches!(w.app.app, App::Wasm(_)) {
            return None;
        }
        let c = wasm_content(w.rect);
        (px >= c.x && py >= c.y && px < c.x + c.w && py < c.y + c.h).then_some((
            id,
            px - c.x,
            py - c.y,
        ))
    }

    /// Deliver the mouse state to the WASM window under the cursor (or the one that
    /// grabbed the button), as content-local coordinates.
    pub(crate) fn wasm_pointer(&mut self, left: bool, right: bool, moved: bool) {
        let buttons = left as i32 | ((right as i32) << 1);
        let changed = left != self.prev_left || right != self.prev_right;
        if !moved && !changed {
            return;
        }
        if buttons == 0
            && let Some(g) = self.wasm_grab.take()
        {
            // release: send it to the window that owns the press, even outside it
            if let (Some(h), Some(r)) = (self.wasm_handle(g), self.wm.get(g).map(|w| w.rect)) {
                let c = wasm_content(r);
                wasm::pointer(h, self.cursor_x - c.x, self.cursor_y - c.y, 0);
            }
            return;
        }
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        if let Some(g) = self.wasm_grab {
            if let (Some(h), Some(r)) = (self.wasm_handle(g), self.wm.get(g).map(|w| w.rect)) {
                let c = wasm_content(r);
                wasm::pointer(h, cx - c.x, cy - c.y, buttons);
            }
            return;
        }
        if self.overlay_open() {
            return;
        }
        if let Some((id, lx, ly)) = self.wasm_pointer_target(cx, cy)
            && let Some(h) = self.wasm_handle(id)
        {
            if buttons != 0 && changed {
                self.wasm_grab = Some(id);
            }
            wasm::pointer(h, lx, ly, buttons);
        }
    }
}

/// ABI key code and modifier bits of a logical key: ASCII, 10 Enter, 27 Esc,
/// 8 Backspace, 9 Tab, 127 Delete, 0x100.. arrows / Home / End / PageUp / PageDown.
pub(crate) fn wasm_key_code(key: Key) -> i32 {
    match key {
        Key::Char(b) => b as i32,
        Key::Enter => 10,
        Key::Esc => 27,
        Key::Backspace => 8,
        Key::Tab => 9,
        Key::Delete => 127,
        Key::Left => 0x100,
        Key::Right => 0x101,
        Key::Up => 0x102,
        Key::Down => 0x103,
        Key::Home => 0x104,
        Key::End => 0x105,
        Key::PageUp => 0x106,
        Key::PageDown => 0x107,
    }
}
