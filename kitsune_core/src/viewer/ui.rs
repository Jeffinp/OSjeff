//! The pixels of Imagens as numbers: window regions and hit testing, the filmstrip maths, pan
//! inertia, the slideshow clock and the sampler that draws a rotating picture.
//!
//! The kernel draws and routes input; everything it needs to decide lives here so it runs on the
//! host under test. Integer maths throughout, except the inertia (a handful of `f32` per frame).

use super::View;
use crate::window::Rect;
use alloc::vec::Vec;

pub const TITLE_H: i32 = crate::window::TITLE_H;
pub const TOOLBAR_H: i32 = 44;
pub const BTN: i32 = 28;
/// Height of the bottom band with the filmstrip (several images) and the caption.
pub const STRIP_H: i32 = 92;
/// Height of the bottom band with only the caption (one image).
pub const CAPTION_H: i32 = 32;
/// Side of a filmstrip thumbnail and the gap between two.
pub const THUMB: i32 = 56;
pub const THUMB_GAP: i32 = 8;
/// Width of the information inspector and the margin around it.
pub const INFO_W: i32 = 248;
pub const INFO_MARGIN: i32 = 12;
/// Height of one inspector row, and the padding around the rows.
pub const INFO_ROW_H: i32 = 26;
pub const INFO_PAD: i32 = 14;
/// Seconds one slide stays up.
pub const SLIDE_SECS: u64 = 3;

/// How the picture sits in the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FitMode {
    /// All of it inside the window.
    Fit,
    /// Covering the window (the edges are cropped).
    Fill,
    /// One image pixel per screen pixel.
    Actual,
}

impl FitMode {
    pub const ALL: [FitMode; 3] = [FitMode::Fit, FitMode::Fill, FitMode::Actual];

    pub fn index(self) -> usize {
        match self {
            FitMode::Fit => 0,
            FitMode::Fill => 1,
            FitMode::Actual => 2,
        }
    }
}

/// The mode a view is in, if it is one of the three (a wheel zoom leaves none selected).
pub fn mode_of(v: &View) -> Option<FitMode> {
    if v.fit {
        Some(FitMode::Fit)
    } else if v.fill {
        Some(FitMode::Fill)
    } else if v.zoom == 1000 {
        Some(FitMode::Actual)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Window regions
// ---------------------------------------------------------------------------

/// Geometry of a viewer window, in screen coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    pub body: Rect,
    pub toolbar: Rect,
    pub rot_left: Rect,
    pub rot_right: Rect,
    pub flip: Rect,
    /// The three-segment fit control.
    pub modes: Rect,
    pub slideshow: Rect,
    pub info_btn: Rect,
    pub save: Rect,
    /// Where the picture lives (below the toolbar, above the bottom band).
    pub canvas: Rect,
    /// The filmstrip, when the folder has more than one image.
    pub strip: Option<Rect>,
    /// The caption line under the filmstrip (or alone).
    pub caption: Rect,
    /// The inspector, when open, for `info_rows` rows.
    pub info: Option<Rect>,
}

impl Layout {
    /// Geometry for a window at `r`. `multi`: more than one image (the filmstrip shows).
    /// `info_rows`: the inspector is open and has this many rows.
    pub fn of(r: Rect, multi: bool, info_rows: Option<usize>) -> Layout {
        let body = Rect::new(r.x, r.y + TITLE_H, r.w, (r.h - TITLE_H).max(0));
        let toolbar = Rect::new(body.x, body.y, body.w, TOOLBAR_H.min(body.h));
        let by = toolbar.y + (TOOLBAR_H - BTN) / 2;
        let left = body.x + 12;
        let rot_left = Rect::new(left, by, BTN, BTN);
        let rot_right = Rect::new(rot_left.right() + 4, by, BTN, BTN);
        let flip = Rect::new(rot_right.right() + 4, by, BTN, BTN);
        let save = Rect::new(body.right() - 12 - BTN, by, BTN, BTN);
        let info_btn = Rect::new(save.x - 4 - BTN, by, BTN, BTN);
        let slideshow = Rect::new(info_btn.x - 4 - BTN, by, BTN, BTN);
        // The fit control sits in the middle of the toolbar when there is room, else it is
        // squeezed between the two groups.
        let modes_w = 204.min((slideshow.x - flip.right() - 24).max(90));
        let mid = body.x + body.w / 2 - modes_w / 2;
        let min_x = flip.right() + 12;
        let max_x = (slideshow.x - 12 - modes_w).max(min_x);
        let modes = Rect::new(mid.clamp(min_x, max_x), by, modes_w, BTN);
        let band_h = if multi { STRIP_H } else { CAPTION_H };
        let band_h = band_h.min((body.h - toolbar.h).max(0));
        let canvas = Rect::new(
            body.x,
            toolbar.bottom(),
            body.w,
            (body.h - toolbar.h - band_h).max(0),
        );
        let band = Rect::new(body.x, canvas.bottom(), body.w, band_h);
        let (strip, caption) = if multi {
            (
                Some(Rect::new(band.x, band.y, band.w, band.h - 28)),
                Rect::new(band.x, band.bottom() - 28, band.w, 28),
            )
        } else {
            (None, band)
        };
        let info = info_rows.map(|n| {
            let h =
                (2 * INFO_PAD + n as i32 * INFO_ROW_H).min((canvas.h - 2 * INFO_MARGIN).max(40));
            let w = INFO_W.min((canvas.w - 2 * INFO_MARGIN).max(80));
            Rect::new(
                canvas.right() - w - INFO_MARGIN,
                canvas.y + INFO_MARGIN,
                w,
                h,
            )
        });
        Layout {
            window: r,
            body,
            toolbar,
            rot_left,
            rot_right,
            flip,
            modes,
            slideshow,
            info_btn,
            save,
            canvas,
            strip,
            caption,
            info,
        }
    }
}

/// What a press landed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    RotateLeft,
    RotateRight,
    Flip,
    Mode(FitMode),
    Slideshow,
    Info,
    Save,
    /// A filmstrip thumbnail (image index).
    Thumb(usize),
    /// The picture area (pan, double click).
    Canvas,
    /// The inspector.
    InfoPanel,
    /// Toolbar or caption space that does nothing.
    Dead,
}

impl Layout {
    /// Resolve a press at `(px, py)`. `strip_scroll` is the filmstrip's scroll, `count` the
    /// number of images.
    pub fn hit(&self, px: i32, py: i32, strip_scroll: i32, count: usize) -> Option<Hit> {
        if !self.window.contains(px, py) {
            return None;
        }
        let pairs = [
            (self.rot_left, Hit::RotateLeft),
            (self.rot_right, Hit::RotateRight),
            (self.flip, Hit::Flip),
            (self.slideshow, Hit::Slideshow),
            (self.info_btn, Hit::Info),
            (self.save, Hit::Save),
        ];
        for (r, h) in pairs {
            if r.contains(px, py) {
                return Some(h);
            }
        }
        if self.modes.contains(px, py) {
            let seg = (((px - self.modes.x) * 3) / self.modes.w.max(1)).clamp(0, 2) as usize;
            return Some(Hit::Mode(FitMode::ALL[seg]));
        }
        if self.toolbar.contains(px, py) || self.caption.contains(px, py) {
            return Some(Hit::Dead);
        }
        if self.info.is_some_and(|i| i.contains(px, py)) {
            return Some(Hit::InfoPanel);
        }
        if let Some(s) = self.strip
            && s.contains(px, py)
        {
            return Some(
                strip_item_at(s, count, strip_scroll, px, py).map_or(Hit::Dead, Hit::Thumb),
            );
        }
        if self.canvas.contains(px, py) {
            return Some(Hit::Canvas);
        }
        Some(Hit::Dead)
    }
}

// ---------------------------------------------------------------------------
// The filmstrip
// ---------------------------------------------------------------------------

/// Total width of `count` thumbnails with their gaps and the end padding.
pub fn strip_content_w(count: usize) -> i32 {
    if count == 0 {
        return 0;
    }
    2 * 16 + count as i32 * THUMB + (count as i32 - 1) * THUMB_GAP
}

/// The rectangle of thumbnail `i` for a filmstrip `strip` scrolled by `scroll`.
pub fn thumb_rect(strip: Rect, scroll: i32, i: usize) -> Rect {
    Rect::new(
        strip.x + 16 + i as i32 * (THUMB + THUMB_GAP) - scroll,
        strip.y + (strip.h - THUMB) / 2,
        THUMB,
        THUMB,
    )
}

/// The scroll that centres thumbnail `current` (clamped: the strip never scrolls past its ends,
/// and a strip that fits is not scrolled at all).
pub fn strip_scroll_for(strip_w: i32, count: usize, current: usize) -> i32 {
    let max = (strip_content_w(count) - strip_w).max(0);
    let centre = 16 + current as i32 * (THUMB + THUMB_GAP) + THUMB / 2;
    (centre - strip_w / 2).clamp(0, max)
}

/// The thumbnails (first, one past the last) that show in the strip.
pub fn strip_visible(strip_w: i32, count: usize, scroll: i32) -> (usize, usize) {
    if count == 0 || strip_w <= 0 {
        return (0, 0);
    }
    let pitch = THUMB + THUMB_GAP;
    let first = ((scroll - 16).max(0) / pitch) as usize;
    let end = (((scroll + strip_w - 16 + pitch - 1).max(0) / pitch) as usize + 1).min(count);
    (first.min(count), end)
}

/// The thumbnail under `(px, py)`.
pub fn strip_item_at(strip: Rect, count: usize, scroll: i32, px: i32, py: i32) -> Option<usize> {
    let (a, b) = strip_visible(strip.w, count, scroll);
    (a..b).find(|&i| thumb_rect(strip, scroll, i).contains(px, py))
}

/// Size of an `iw x ih` picture scaled so its shorter side is `side` (a thumbnail that covers its
/// square; the longer side is cropped when drawn).
pub fn cover_dims(iw: usize, ih: usize, side: usize) -> (usize, usize) {
    if iw == 0 || ih == 0 || side == 0 {
        return (side.max(1), side.max(1));
    }
    if iw <= ih {
        (side, (ih * side).div_ceil(iw).max(side))
    } else {
        ((iw * side).div_ceil(ih).max(side), side)
    }
}

/// Image indices ordered by distance from `current` (the order thumbnails are made in, so the
/// ones around the picture shown appear first), at most `limit` of them.
pub fn thumb_order(current: usize, count: usize, limit: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..count).collect();
    v.sort_by_key(|&i| (i.abs_diff(current), i));
    v.truncate(limit);
    v
}

// ---------------------------------------------------------------------------
// Pan inertia
// ---------------------------------------------------------------------------

/// Velocity of a dragged picture and the glide after it is let go.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inertia {
    vx: f32,
    vy: f32,
    /// Sub-pixel remainder carried between steps.
    rx: f32,
    ry: f32,
    gliding: bool,
}

/// Fastest glide, pixels per second.
const MAX_SPEED: f32 = 3200.0;
/// Below this speed the glide stops.
const REST_SPEED: f32 = 14.0;
/// Exponential friction: the speed halves every `ln 2 / FRICTION` seconds (about 0.17 s).
const FRICTION: f32 = 4.2;

impl Inertia {
    pub const fn new() -> Inertia {
        Inertia {
            vx: 0.0,
            vy: 0.0,
            rx: 0.0,
            ry: 0.0,
            gliding: false,
        }
    }

    /// The button went down: stop any glide and forget the velocity.
    pub fn grab(&mut self) {
        *self = Inertia::new();
    }

    /// The pointer moved by `(dx, dy)` pixels in `dt` seconds while dragging.
    pub fn push(&mut self, dx: i32, dy: i32, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let (ix, iy) = (dx as f32 / dt, dy as f32 / dt);
        // Smooth over the last few events; a long pause weighs the old speed down first.
        let keep = if dt > 0.12 { 0.0 } else { 0.55 };
        self.vx = self.vx * keep + ix * (1.0 - keep);
        self.vy = self.vy * keep + iy * (1.0 - keep);
        self.vx = self.vx.clamp(-MAX_SPEED, MAX_SPEED);
        self.vy = self.vy.clamp(-MAX_SPEED, MAX_SPEED);
    }

    /// The button went up after `idle` seconds without movement: start gliding unless the
    /// pointer had stopped, or the speed is too small to matter.
    pub fn release(&mut self, idle: f32) {
        if idle > 0.08 || (self.vx * self.vx + self.vy * self.vy) < REST_SPEED * REST_SPEED * 4.0 {
            *self = Inertia::new();
            return;
        }
        self.gliding = true;
    }

    /// Whether a glide is running (the window needs frames).
    pub fn active(&self) -> bool {
        self.gliding
    }

    /// Advance the glide by `dt` seconds and return the whole pixels to pan by.
    pub fn step(&mut self, dt: f32) -> (i32, i32) {
        if !self.gliding {
            return (0, 0);
        }
        let ax = self.vx * dt + self.rx;
        let ay = self.vy * dt + self.ry;
        let (mx, my) = (ax as i32, ay as i32);
        self.rx = ax - mx as f32;
        self.ry = ay - my as f32;
        let k = (1.0 - FRICTION * dt).max(0.0);
        self.vx *= k;
        self.vy *= k;
        if self.vx * self.vx + self.vy * self.vy < REST_SPEED * REST_SPEED {
            *self = Inertia::new();
        }
        (mx, my)
    }

    /// Stop the glide (the picture hit an edge, or something else took over).
    pub fn stop(&mut self) {
        *self = Inertia::new();
    }
}

// ---------------------------------------------------------------------------
// Slideshow
// ---------------------------------------------------------------------------

/// The slideshow clock: it runs on timer ticks (250 per second).
#[derive(Clone, Copy, Debug, Default)]
pub struct Slideshow {
    running: bool,
    since: u64,
}

impl Slideshow {
    pub const fn new() -> Slideshow {
        Slideshow {
            running: false,
            since: 0,
        }
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Start or stop at tick `now`.
    pub fn toggle(&mut self, now: u64) {
        self.running = !self.running;
        self.since = now;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    /// The slide just changed (by the clock or by hand): restart the wait.
    pub fn restart(&mut self, now: u64) {
        self.since = now;
    }

    /// Whether it is time for the next slide at tick `now`.
    pub fn due(&self, now: u64) -> bool {
        self.running && now.saturating_sub(self.since) >= SLIDE_SECS * 250
    }
}

// ---------------------------------------------------------------------------
// Drawing a rotating picture
// ---------------------------------------------------------------------------

/// Maps viewport pixels to image coordinates (Q16) for a picture of `iw x ih` shown at `zoom`
/// permille, panned by `(pan_x, pan_y)` and turned by `angle` degrees (clockwise) about its
/// centre. Walking along a row adds a constant step, so no division happens per pixel.
#[derive(Clone, Copy, Debug)]
pub struct RotMap {
    /// Image coordinates (Q16) of viewport pixel `(0, 0)`.
    u0: i64,
    v0: i64,
    du_dx: i64,
    dv_dx: i64,
    du_dy: i64,
    dv_dy: i64,
}

impl RotMap {
    pub fn new(
        vw: i32,
        vh: i32,
        pan: (i32, i32),
        iw: usize,
        ih: usize,
        zoom: u32,
        angle_deg: i32,
    ) -> RotMap {
        let c = crate::iconart::cos_q14(angle_deg) as i64;
        let s = crate::iconart::sin_q14(angle_deg) as i64;
        // Q16 image pixels per screen pixel, before the rotation.
        let f = ((1000i64) << 16) / zoom.max(1) as i64;
        let (cx, cy) = (vw as i64 / 2 + pan.0 as i64, vh as i64 / 2 + pan.1 as i64);
        // u = (dx*c + dy*s) / 2^14 * f + iw/2 ;  v = (-dx*s + dy*c) / 2^14 * f + ih/2
        let du_dx = (c * f) >> 14;
        let dv_dx = (-s * f) >> 14;
        let du_dy = (s * f) >> 14;
        let dv_dy = (c * f) >> 14;
        let u0 = (iw as i64 * 65536) / 2 - cx * du_dx - cy * du_dy;
        let v0 = (ih as i64 * 65536) / 2 - cx * dv_dx - cy * dv_dy;
        RotMap {
            u0,
            v0,
            du_dx,
            dv_dx,
            du_dy,
            dv_dy,
        }
    }

    /// Image coordinates (Q16) of viewport pixel `(x, y)`.
    pub fn at(&self, x: i32, y: i32) -> (i64, i64) {
        (
            self.u0 + x as i64 * self.du_dx + y as i64 * self.du_dy,
            self.v0 + x as i64 * self.dv_dx + y as i64 * self.dv_dy,
        )
    }

    /// The change in image coordinates for one step right.
    pub fn step_x(&self) -> (i64, i64) {
        (self.du_dx, self.dv_dx)
    }

    /// The integer pixel inside an `iw x ih` image at Q16 `(u, v)`, if any.
    pub fn pixel(u: i64, v: i64, iw: usize, ih: usize) -> Option<(usize, usize)> {
        if u < 0 || v < 0 {
            return None;
        }
        let (x, y) = ((u >> 16) as usize, (v >> 16) as usize);
        (x < iw && y < ih).then_some((x, y))
    }
}

/// The bounding box (viewport coordinates) of an `iw x ih` picture shown at `zoom` permille,
/// panned and turned by `angle` degrees, clipped to `vw x vh`: where pixels may need drawing.
pub fn rotated_bounds(
    vw: i32,
    vh: i32,
    pan: (i32, i32),
    iw: usize,
    ih: usize,
    zoom: u32,
    angle_deg: i32,
) -> Rect {
    let (w, h) = (
        (iw as i64 * zoom as i64 / 1000) as i32,
        (ih as i64 * zoom as i64 / 1000) as i32,
    );
    let (c, s) = (
        crate::iconart::cos_q14(angle_deg).abs() as i64,
        crate::iconart::sin_q14(angle_deg).abs() as i64,
    );
    let bw = ((w as i64 * c + h as i64 * s) >> 14) as i32 + 2;
    let bh = ((w as i64 * s + h as i64 * c) >> 14) as i32 + 2;
    let (cx, cy) = (vw / 2 + pan.0, vh / 2 + pan.1);
    Rect::new(cx - bw / 2, cy - bh / 2, bw, bh)
        .intersection(&Rect::new(0, 0, vw, vh))
        .unwrap_or(Rect::new(0, 0, 0, 0))
}

#[cfg(test)]
mod tests;
