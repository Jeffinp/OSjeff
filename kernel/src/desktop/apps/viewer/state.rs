//! State of an image-viewer window.

use crate::desktop::*;

/// A filmstrip thumbnail: not made yet, made, or impossible (too big or unreadable).
pub(crate) enum Thumb {
    Pending,
    Ready(kitsune_core::raster::Surface),
    Missing,
}

/// An image-viewer window.
pub(crate) struct ViewerState {
    pub path: Vec<u8>,
    pub image: Option<kitsune_core::image::Image>,
    /// Box-filtered copy for zooms below 100 %: `(zoom, image)`.
    pub scaled: Option<(u32, kitsune_core::image::Image)>,
    pub opaque: bool,
    /// Where the zoom and pan are heading (the drawn values follow with springs).
    pub view: kitsune_core::viewer::View,
    pub list: kitsune_core::viewer::ImageList,
    pub format: Option<kitsune_core::image::Format>,
    pub file_bytes: u64,
    /// Why the file could not be shown.
    pub error: Option<[String; 2]>,
    pub show_info: bool,
    /// The "save as" sheet.
    pub save: Option<kitsune_core::fileman::TextInput>,
    pub msg: Option<(String, bool)>,
    /// The zoom (permille) and pan being drawn.
    pub zoom_s: kitsune_core::anim::Spring,
    pub pan_x_s: kitsune_core::anim::Spring,
    pub pan_y_s: kitsune_core::anim::Spring,
    /// Extra turn (degrees, clockwise) of the drawn picture while a rotation animates.
    pub rot: kitsune_core::anim::Tween,
    /// The picture fading in after a change of image.
    pub enter_t: kitsune_core::anim::Tween,
    pub info_t: kitsune_core::anim::Tween,
    pub sheet_t: kitsune_core::anim::Tween,
    pub hover: Option<kitsune_core::viewer::ui::Hit>,
    pub hover_t: kitsune_core::anim::Tween,
    pub inertia: kitsune_core::viewer::ui::Inertia,
    /// Tick of the last drag event (for the speed of a flick).
    pub drag_tick: u64,
    pub slideshow: kitsune_core::viewer::ui::Slideshow,
    pub thumbs: Vec<Thumb>,
    pub strip_scroll: kitsune_core::anim::Spring,
    /// Tick the last thumbnail was made.
    pub thumb_tick: u64,
    /// The viewport the zoom was last fitted for: a change jumps instead of animating.
    pub vp_seen: (i32, i32),
}

impl ViewerState {
    pub(crate) fn new() -> Self {
        use kitsune_core::anim::{Spring, Tween};
        ViewerState {
            path: Vec::new(),
            image: None,
            scaled: None,
            opaque: true,
            view: kitsune_core::viewer::View::default(),
            list: kitsune_core::viewer::ImageList::default(),
            format: None,
            file_bytes: 0,
            error: None,
            show_info: false,
            save: None,
            msg: None,
            zoom_s: Spring::pixels(1000.0, 240.0, 31.0),
            pan_x_s: Spring::pixels(0.0, 240.0, 31.0),
            pan_y_s: Spring::pixels(0.0, 240.0, 31.0),
            rot: Tween::at(0.0),
            enter_t: Tween::at(1.0),
            info_t: Tween::at(0.0),
            sheet_t: Tween::at(0.0),
            hover: None,
            hover_t: Tween::at(1.0),
            inertia: kitsune_core::viewer::ui::Inertia::new(),
            drag_tick: 0,
            slideshow: kitsune_core::viewer::ui::Slideshow::new(),
            thumbs: Vec::new(),
            strip_scroll: Spring::pixels(0.0, 260.0, 32.0),
            thumb_tick: 0,
            vp_seen: (0, 0),
        }
    }

    /// Whether something in the window moves on its own and needs frames.
    pub(crate) fn animating(&self) -> bool {
        !self.zoom_s.at_rest()
            || !self.pan_x_s.at_rest()
            || !self.pan_y_s.at_rest()
            || !self.rot.finished()
            || !self.enter_t.finished()
            || !self.info_t.finished()
            || !self.sheet_t.finished()
            || !self.hover_t.finished()
            || !self.strip_scroll.at_rest()
            || self.inertia.active()
            || self.thumbs_pending()
    }

    /// Thumbnails still to make (the window keeps running frames until they are done).
    pub(crate) fn thumbs_pending(&self) -> bool {
        self.list.len() > 1 && self.thumbs.iter().any(|t| matches!(t, Thumb::Pending))
    }
}
