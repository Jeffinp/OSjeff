//! Debugging aid (Ctrl+Alt+V): after every composed frame, paint the same scene from scratch with
//! the reference plan into a scratch buffer and compare it with the back buffer. Any difference
//! is a bug in the scene description (a footprint, an opaque area or a look that is not truthful)
//! or in a painter (drawing outside its footprint, depending on the clip). The first mismatches
//! are logged on the serial port with the rectangle that differs.
//!
//! The reference plan trusts nothing: every layer is painted over the whole screen, so a layer that
//! draws outside its declared footprint shows up here even though the incremental plans, which
//! clip to the footprint, hide it.

use super::super::*;
use super::paint::DeskPainter;
use super::present::Screen;
use osjeff_core::compositor::{Engine, Scene};

/// How many mismatching frames are logged before going quiet.
const MAX_REPORTS: u32 = 40;

pub(super) struct Verifier {
    reports: u32,
    frames: u32,
}

impl Verifier {
    pub fn new() -> Self {
        Self {
            reports: 0,
            frames: 0,
        }
    }

    pub fn check(
        &mut self,
        desk: &Desktop,
        scr: &mut Screen,
        time: Time,
        scene: &Scene,
        engine: &Engine,
    ) {
        if !desk.verify_mode() {
            return;
        }
        let n = scr.n;
        let plan = engine.full_plan(scene);
        let mut painter = DeskPainter {
            desk,
            back: scr.check,
            bg: scr.bg,
            info: scr.info,
            time,
        };
        plan.paint(&mut painter);
        self.frames += 1;
        let (got, want) = (&scr.back[..n], &scr.check[..n]);
        if got == want {
            return;
        }
        // A WASM guest draws into a buffer the compositor reads while the guest thread keeps
        // writing, and a caret blinks with the clock: two paints of the same scene can legitimately
        // differ inside such content.
        let skip: Vec<Rect> = desk
            .wm
            .windows()
            .iter()
            .filter(|w| w.shown())
            .filter_map(|w| match &w.app.app {
                App::Wasm(_) => Some(wasm_content(desk.window_box(w))),
                // A running terminal's caret blinks with the clock.
                App::Terminal(t) if t.term.is_running() => Some(desk.window_box(w)),
                // Live windows (Tarefas) read the clock for their glide and scroll bars.
                _ if desk.live_dynamic(w) => Some(desk.window_box(w)),
                _ => None,
            })
            .collect();
        let info = scr.info;
        let bpp = info.bytes_per_pixel;
        let (mut count, mut x0, mut y0, mut x1, mut y1) = (0u32, i32::MAX, i32::MAX, 0, 0);
        for y in 0..info.height {
            let row = y * info.stride * bpp;
            let len = info.width * bpp;
            if got[row..row + len] == want[row..row + len] {
                continue;
            }
            for x in 0..info.width {
                let o = row + x * bpp;
                if got[o..o + bpp] != want[o..o + bpp]
                    && !skip.iter().any(|r| r.contains(x as i32, y as i32))
                {
                    count += 1;
                    x0 = x0.min(x as i32);
                    x1 = x1.max(x as i32);
                    y0 = y0.min(y as i32);
                    y1 = y1.max(y as i32);
                }
            }
        }
        if count > 0 && self.reports < MAX_REPORTS {
            self.reports += 1;
            crate::serial_println!(
                "compositor-verify: MISMATCH frame {}: {} px differ in ({x0},{y0})..({x1},{y1}), {} layers",
                self.frames,
                count,
                scene.len()
            );
        }
    }
}
