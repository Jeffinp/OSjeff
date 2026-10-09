//! The painter: turns the engine's steps into pixels in the back buffer.
//!
//! Contract with the engine (`kitsune_core::compositor::Layer`): painting a layer with a clip gives,
//! inside the clip, the pixels a full paint would give; nothing is drawn outside the footprint
//! `layers.rs` declared. Everything here goes through [`Canvas`] primitives, which honour the clip
//! rectangle, so a layer paints only the part the engine asked for. Reading what is under a layer
//! (window corners, glass backdrops) is safe: the engine always paints the layers below first.

use super::super::*;
use super::layers::{Slot, slot_of};
use crate::desktop::widgets::copy_region;
use bootloader_api::info::FrameBufferInfo;
use kitsune_core::compositor::{LayerId, Painter};

/// Paints layers of `desk` into `back`.
pub(super) struct DeskPainter<'a> {
    pub desk: &'a Desktop,
    pub back: &'a mut [u8],
    pub bg: &'a [u8],
    pub info: FrameBufferInfo,
    pub time: Time,
}

impl DeskPainter<'_> {
    /// Copy the wallpaper (with the panel's glass strip) over `clip`.
    fn restore_wallpaper(&mut self, clip: Rect) {
        let t0 = crate::trace::t();
        copy_region(self.back, self.bg, self.info, clip);
        crate::trace::prim(crate::trace::Prim::BgCopy, t0);
    }

    fn window(&mut self, id: WindowId, clip: Rect) {
        let desk = self.desk;
        let Some(w) = desk.wm.get(id) else { return };
        let mut c = Canvas::new(self.back, self.info);
        c.set_clip(clip);
        let focused = desk.focused() == Some(id);
        if w.anim.is_some() || w.zoom.is_some() {
            desk.draw_animating(&mut c, w, focused);
        } else {
            desk.draw_window(&mut c, w, desk.window_box(w), focused, !w.maximized);
        }
    }
}

impl Painter for DeskPainter<'_> {
    fn paint(&mut self, layer: LayerId, clip: Rect) {
        let desk = self.desk;
        match slot_of(layer) {
            Slot::Wallpaper => self.restore_wallpaper(clip),
            Slot::Window(id) => self.window(id, clip),
            Slot::Panel => {
                // The panel is opaque: its glass strip is baked into the wallpaper, so a shadow
                // that reached it is wiped by restoring the strip before the items go on.
                let Some(clip) = clip.intersection(&desk.panel_rect()) else {
                    return;
                };
                self.restore_wallpaper(clip);
                let mut c = Canvas::new(self.back, self.info);
                c.set_clip(clip);
                desk.draw_panel(&mut c, self.time);
            }
            other => {
                let mut c = Canvas::new(self.back, self.info);
                c.set_clip(clip);
                match other {
                    Slot::Dock => desk.draw_dock(&mut c),
                    Slot::Snap => desk.draw_snap_preview(&mut c),
                    Slot::Apps => desk.draw_apps_layer(&mut c),
                    Slot::Search => desk.draw_search_layer(&mut c),
                    Slot::Menu => desk.draw_menu_layer(&mut c),
                    Slot::Popover => desk.draw_popover_layer(&mut c),
                    Slot::Switcher => desk.draw_switcher_layer(&mut c),
                    Slot::Dialog => desk.draw_dialog_layer(&mut c),
                    Slot::Wallpaper | Slot::Window(_) | Slot::Panel => {}
                }
            }
        }
    }
}
