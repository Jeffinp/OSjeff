//! Per-frame state of the viewer: glides, the slideshow and the thumbnail queue.

use super::load::THUMB_EVERY;
use super::load::THUMB_LIMIT;
use super::load::make_thumb;
use crate::desktop::kit::appui::{self};
use crate::desktop::*;
use kitsune_core::viewer::ui::{self as vui};

impl Desktop {
    // ---- per-frame state ----

    /// Advance every viewer's animations by `dt`, run the slideshow clock and make a thumbnail
    /// when one is due. Returns whether anything still moves.
    pub(crate) fn step_viewers(&mut self, dt: f32) -> bool {
        let ids: Vec<WindowId> = self
            .wm
            .windows()
            .iter()
            .filter(|w| matches!(w.app.app, App::Viewer(_)) && w.shown())
            .map(|w| w.id)
            .collect();
        let now = appui::ticks();
        let mut busy = false;
        for id in ids {
            let (vpw, vph) = self.viewer_vp(id);
            // The slideshow turns the page.
            let due = self
                .viewer_mut(id)
                .is_some_and(|v| v.slideshow.due(now) && v.save.is_none());
            if due {
                self.viewer_step(id, true);
            }
            let Some(v) = self.viewer_mut(id) else {
                continue;
            };
            // A glide pans the picture, stopping at the edge.
            if v.inertia.active() {
                let (dx, dy) = v.inertia.step(dt);
                if let Some((iw, ih)) = v.image.as_ref().map(|i| (i.width(), i.height())) {
                    let before = (v.view.pan_x, v.view.pan_y);
                    v.view.pan_by(dx, dy, iw, ih, vpw, vph);
                    if (v.view.pan_x, v.view.pan_y) == before && (dx != 0 || dy != 0) {
                        v.inertia.stop();
                    }
                    v.pan_x_s.jump(v.view.pan_x as f32);
                    v.pan_y_s.jump(v.view.pan_y as f32);
                } else {
                    v.inertia.stop();
                }
            }
            busy |= v.zoom_s.step(dt);
            busy |= v.pan_x_s.step(dt);
            busy |= v.pan_y_s.step(dt);
            busy |= v.rot.step(dt);
            busy |= v.enter_t.step(dt);
            busy |= v.info_t.step(dt);
            busy |= v.sheet_t.step(dt);
            busy |= v.hover_t.step(dt);
            busy |= v.strip_scroll.step(dt);
            // One thumbnail every so often, nearest the current image first.
            if v.thumbs_pending() && now.saturating_sub(v.thumb_tick) >= THUMB_EVERY {
                v.thumb_tick = now;
                let order = vui::thumb_order(v.list.index(), v.list.len(), THUMB_LIMIT);
                if let Some(i) = order
                    .into_iter()
                    .find(|&i| matches!(v.thumbs.get(i), Some(Thumb::Pending)))
                {
                    let t = v.list.path_at(i).map_or(Thumb::Missing, |p| make_thumb(&p));
                    v.thumbs[i] = t;
                }
                // Images beyond the limit never get one: do not wait for them.
                let keep = vui::thumb_order(v.list.index(), v.list.len(), THUMB_LIMIT);
                for (i, t) in v.thumbs.iter_mut().enumerate() {
                    if matches!(t, Thumb::Pending) && !keep.contains(&i) {
                        *t = Thumb::Missing;
                    }
                }
            }
            busy |= v.animating();
        }
        busy
    }
}
