//! Drawing a WASM app's window content.

use crate::desktop::*;

impl Desktop {
    /// Render the resident WASM application into window `r`'s content area. The
    /// guest paints through the host drawing ABI; the engine translates and
    /// clips it to this box (see [`crate::wasm::draw_app`]).
    pub(crate) fn draw_wasm(&self, c: &mut Canvas, r: Rect, w: &WasmWin) {
        // Each app renders on the `appd` thread into its own offscreen surface; the
        // compositor just copies the latest finished frame into the window (or shows
        // why the app is not running).
        let cr = wasm_content(r);
        crate::wasm::blit(w.id, c, cr.x, cr.y, cr.w, cr.h);
    }
}
