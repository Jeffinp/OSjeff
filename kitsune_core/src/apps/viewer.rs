//! Image viewer logic: zoom, pan, fit box, the folder's image list, info text.
//!
//! Pure integer math (the kernel is soft-float). The window draws; this module
//! answers "how big is the image on screen", "where does its top-left corner go",
//! "which source column feeds screen column `x`" and "what is the next image".
//!
//! Zoom is in permille (1000 = 100 %). The pan is the offset of the image centre from
//! the viewport centre, in screen pixels; it is clamped so an image larger than the
//! viewport never leaves a gap at its edges, and an image that fits is centred.
//!
//! [`ui`] holds the window geometry, the filmstrip maths, pan inertia, the slideshow clock and the
//! rotation sampler that the kernel draws with.

pub mod ui;

use crate::apps::fileman::{format_size, is_image, natural_cmp};
use crate::format::image::{DecodeError, Format, ImageError};
use crate::storage::vfs::{self, Entry, EntryKind};
use alloc::string::String;
use alloc::vec::Vec;

/// Zoom steps in percent, ascending; `+`/`-` and the wheel move along them.
pub const ZOOM_STEPS: [u32; 18] = [
    5, 10, 15, 25, 33, 50, 67, 75, 100, 125, 150, 200, 300, 400, 600, 800, 1200, 1600,
];
/// Smallest zoom, permille.
pub const MIN_ZOOM: u32 = 50;
/// Largest zoom, permille.
pub const MAX_ZOOM: u32 = 16_000;
/// Pixels moved by an arrow key / pan step.
pub const PAN_STEP: i32 = 48;
/// Side of one transparency checkerboard square.
pub const CHECKER: i32 = 8;

/// Size of `n` pixels at `zoom` permille (at least 1).
pub fn scaled_dim(n: usize, zoom: u32) -> usize {
    ((n as u64 * zoom as u64 + 500) / 1000).max(1) as usize
}

/// The zoom (permille) at which an `iw x ih` image fits in `vw x vh`, never above
/// 100 % (small images are not blown up) and never below [`MIN_ZOOM`].
pub fn fit_zoom(iw: usize, ih: usize, vw: i32, vh: i32) -> u32 {
    if iw == 0 || ih == 0 || vw <= 0 || vh <= 0 {
        return 1000;
    }
    let zw = vw as u64 * 1000 / iw as u64;
    let zh = vh as u64 * 1000 / ih as u64;
    (zw.min(zh) as u32).clamp(MIN_ZOOM, 1000)
}

/// The zoom (permille) at which an `iw x ih` image covers the whole `vw x vh` viewport (the
/// larger of the two fit ratios), at most [`MAX_ZOOM`] and at least [`MIN_ZOOM`].
pub fn fill_zoom(iw: usize, ih: usize, vw: i32, vh: i32) -> u32 {
    if iw == 0 || ih == 0 || vw <= 0 || vh <= 0 {
        return 1000;
    }
    // Round up so no sliver of background shows at an edge.
    let zw = (vw as u64 * 1000).div_ceil(iw as u64);
    let zh = (vh as u64 * 1000).div_ceil(ih as u64);
    (zw.max(zh) as u32).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Pan limit on one axis: an image no larger than the viewport is centred.
pub fn clamp_pan(pan: i32, scaled: i32, viewport: i32) -> i32 {
    if scaled <= viewport {
        return 0;
    }
    let limit = (scaled - viewport + 1) / 2;
    pan.clamp(-limit, limit)
}

/// Top-left corner (viewport coordinates) of an image of `sw x sh` pixels on screen.
pub fn image_origin(vw: i32, vh: i32, sw: i32, sh: i32, pan_x: i32, pan_y: i32) -> (i32, i32) {
    ((vw - sw) / 2 + pan_x, (vh - sh) / 2 + pan_y)
}

/// The next zoom step above `zoom` permille (or the cap).
pub fn next_zoom(zoom: u32) -> u32 {
    ZOOM_STEPS
        .iter()
        .map(|&p| p * 10)
        .find(|&z| z > zoom)
        .unwrap_or(MAX_ZOOM)
        .min(MAX_ZOOM)
}

/// The next zoom step below `zoom` permille (or the floor).
pub fn prev_zoom(zoom: u32) -> u32 {
    ZOOM_STEPS
        .iter()
        .rev()
        .map(|&p| p * 10)
        .find(|&z| z < zoom)
        .unwrap_or(MIN_ZOOM)
        .max(MIN_ZOOM)
}

/// `"100%"`, `"67%"`: the zoom as a percentage (rounded).
pub fn zoom_label(zoom: u32) -> String {
    alloc::format!("{}%", (zoom + 5) / 10)
}

/// For each screen column in `x0..x1` (viewport coordinates), the source column it
/// shows, with the image's left edge at `origin_x` and `zoom` permille. Columns
/// outside the image map to `None`.
pub fn column_map(x0: i32, x1: i32, origin_x: i32, zoom: u32, iw: usize) -> Vec<Option<u32>> {
    (x0..x1)
        .map(|x| {
            let d = x - origin_x;
            if d < 0 {
                return None;
            }
            let s = (d as u64 * 1000 / zoom.max(1) as u64) as usize;
            (s < iw).then_some(s as u32)
        })
        .collect()
}

/// Whether the transparency checkerboard square at `(x, y)` is the dark one.
pub fn checker_dark(x: i32, y: i32) -> bool {
    ((x.div_euclid(CHECKER) + y.div_euclid(CHECKER)) & 1) != 0
}

/// Zoom and pan of one viewer window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct View {
    /// Permille.
    pub zoom: u32,
    /// Keep the image fitted when the window or the image changes.
    pub fit: bool,
    /// Keep the image covering the viewport (never together with `fit`).
    pub fill: bool,
    pub pan_x: i32,
    pub pan_y: i32,
}

impl Default for View {
    fn default() -> Self {
        View {
            zoom: 1000,
            fit: true,
            fill: false,
            pan_x: 0,
            pan_y: 0,
        }
    }
}

impl View {
    /// Fit the image to the viewport (key `0`).
    pub fn fit_to(&mut self, iw: usize, ih: usize, vw: i32, vh: i32) {
        self.fit = true;
        self.fill = false;
        self.zoom = fit_zoom(iw, ih, vw, vh);
        self.pan_x = 0;
        self.pan_y = 0;
    }

    /// Make the image cover the viewport (key `9`).
    pub fn fill_to(&mut self, iw: usize, ih: usize, vw: i32, vh: i32) {
        self.fit = false;
        self.fill = true;
        self.zoom = fill_zoom(iw, ih, vw, vh);
        self.pan_x = 0;
        self.pan_y = 0;
    }

    /// Show the image at 100 % (key `1`).
    pub fn actual(&mut self) {
        self.fit = false;
        self.fill = false;
        self.zoom = 1000;
        self.pan_x = 0;
        self.pan_y = 0;
    }

    /// The window or image changed: refit when in fit mode, else re-clamp the pan.
    pub fn relayout(&mut self, iw: usize, ih: usize, vw: i32, vh: i32) {
        if self.fit {
            self.fit_to(iw, ih, vw, vh);
        } else if self.fill {
            // The pan survives a resize; only the zoom follows the viewport.
            let (px, py) = (self.pan_x, self.pan_y);
            self.fill_to(iw, ih, vw, vh);
            self.pan_x = px;
            self.pan_y = py;
            self.clamp(iw, ih, vw, vh);
        } else {
            self.clamp(iw, ih, vw, vh);
        }
    }

    /// Re-clamp the pan to the current zoom.
    pub fn clamp(&mut self, iw: usize, ih: usize, vw: i32, vh: i32) {
        let (sw, sh) = (
            scaled_dim(iw, self.zoom) as i32,
            scaled_dim(ih, self.zoom) as i32,
        );
        self.pan_x = clamp_pan(self.pan_x, sw, vw);
        self.pan_y = clamp_pan(self.pan_y, sh, vh);
    }

    /// Move to `zoom`, keeping the image point under `(ax, ay)` (viewport
    /// coordinates relative to its centre) fixed.
    pub fn set_zoom_at(
        &mut self,
        zoom: u32,
        anchor: (i32, i32),
        iw: usize,
        ih: usize,
        vw: i32,
        vh: i32,
    ) {
        let (ax, ay) = anchor;
        let new = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let old = self.zoom.max(1);
        if new == old {
            return;
        }
        let scale = |p: i32, a: i32| -> i32 {
            // pan' = a - (a - pan) * new / old
            let v = a as i64 - (a as i64 - p as i64) * new as i64 / old as i64;
            v.clamp(-1_000_000, 1_000_000) as i32
        };
        self.pan_x = scale(self.pan_x, ax);
        self.pan_y = scale(self.pan_y, ay);
        self.zoom = new;
        self.fit = false;
        self.fill = false;
        self.clamp(iw, ih, vw, vh);
    }

    /// Zoom in one step around the viewport centre (`+`).
    pub fn zoom_in(&mut self, iw: usize, ih: usize, vw: i32, vh: i32) {
        self.set_zoom_at(next_zoom(self.zoom), (0, 0), iw, ih, vw, vh);
    }

    /// Zoom out one step around the viewport centre (`-`).
    pub fn zoom_out(&mut self, iw: usize, ih: usize, vw: i32, vh: i32) {
        self.set_zoom_at(prev_zoom(self.zoom), (0, 0), iw, ih, vw, vh);
    }

    /// Drag or arrow-pan by `(dx, dy)` screen pixels.
    pub fn pan_by(&mut self, dx: i32, dy: i32, iw: usize, ih: usize, vw: i32, vh: i32) {
        self.pan_x = self.pan_x.saturating_add(dx);
        self.pan_y = self.pan_y.saturating_add(dy);
        self.clamp(iw, ih, vw, vh);
    }

    /// On-screen size of the image.
    pub fn scaled(&self, iw: usize, ih: usize) -> (i32, i32) {
        (
            scaled_dim(iw, self.zoom) as i32,
            scaled_dim(ih, self.zoom) as i32,
        )
    }

    /// Top-left corner of the image in a `vw x vh` viewport.
    pub fn origin(&self, iw: usize, ih: usize, vw: i32, vh: i32) -> (i32, i32) {
        let (sw, sh) = self.scaled(iw, ih);
        image_origin(vw, vh, sw, sh, self.pan_x, self.pan_y)
    }
}

// ---------------------------------------------------------------------------
// The folder's images
// ---------------------------------------------------------------------------

/// The image files of one folder, in natural name order, with the current one.
#[derive(Clone, Debug, Default)]
pub struct ImageList {
    dir: Vec<u8>,
    names: Vec<Vec<u8>>,
    idx: usize,
}

impl ImageList {
    /// Build from a folder listing; `current` is the file being shown (it is
    /// listed even if the listing no longer has it).
    pub fn from_entries(dir: &[u8], entries: &[Entry], current: &[u8]) -> Self {
        let mut names: Vec<Vec<u8>> = entries
            .iter()
            .filter(|e| e.kind == EntryKind::File && is_image(&e.name))
            .map(|e| e.name.clone())
            .collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        let idx = match names.iter().position(|n| n == current) {
            Some(i) => i,
            None => {
                names.push(current.to_vec());
                names.sort_by(|a, b| natural_cmp(a, b));
                names.iter().position(|n| n == current).unwrap_or(0)
            }
        };
        ImageList {
            dir: dir.to_vec(),
            names,
            idx,
        }
    }

    /// Number of images.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// 0-based position of the current image.
    pub fn index(&self) -> usize {
        self.idx
    }

    /// Path of the current image.
    pub fn current(&self) -> Option<Vec<u8>> {
        self.names.get(self.idx).map(|n| vfs::join(&self.dir, n))
    }

    /// Path of image `i`.
    pub fn path_at(&self, i: usize) -> Option<Vec<u8>> {
        self.names.get(i).map(|n| vfs::join(&self.dir, n))
    }

    /// Make image `i` the current one and return its path (`None` for an index past the end).
    pub fn go_to(&mut self, i: usize) -> Option<Vec<u8>> {
        if i >= self.names.len() {
            return None;
        }
        self.idx = i;
        self.current()
    }

    /// Step to the next image (wraps) and return its path.
    pub fn go_next(&mut self) -> Option<Vec<u8>> {
        if self.names.is_empty() {
            return None;
        }
        self.idx = (self.idx + 1) % self.names.len();
        self.current()
    }

    /// Step to the previous image (wraps) and return its path.
    pub fn go_prev(&mut self) -> Option<Vec<u8>> {
        if self.names.is_empty() {
            return None;
        }
        self.idx = (self.idx + self.names.len() - 1) % self.names.len();
        self.current()
    }

    /// Drop the current image (it could not be opened or was deleted).
    pub fn remove_current(&mut self) {
        if self.idx < self.names.len() {
            self.names.remove(self.idx);
            if self.idx >= self.names.len() {
                self.idx = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// The rows of the information inspector: label and value, in the language in effect.
#[allow(clippy::too_many_arguments)]
pub fn info_rows(
    name: &[u8],
    w: usize,
    h: usize,
    format: Option<Format>,
    file_bytes: u64,
    zoom: u32,
    position: (usize, usize),
    transparent: bool,
) -> Vec<(String, String)> {
    let mut v = Vec::new();
    v.push((
        String::from(crate::t!("viewer.info.name")),
        String::from_utf8_lossy(name).into_owned(),
    ));
    v.push((
        String::from(crate::t!("viewer.info.dimensions")),
        crate::t!("viewer.dims", w = w, h = h),
    ));
    let mpx = (w as u64 * h as u64 * 10 + 500_000) / 1_000_000;
    v.push((
        String::from(crate::t!("viewer.info.resolution")),
        crate::t!("viewer.mpx", mp = crate::i18n::dec(mpx as i64, 1)),
    ));
    v.push((
        String::from(crate::t!("viewer.info.format")),
        String::from(match format {
            Some(Format::Png) => "PNG",
            Some(Format::Bmp) => "BMP",
            Some(Format::Ppm) => "PPM",
            Some(Format::Gif) => "GIF",
            Some(Format::Jpeg) => "JPEG",
            None => "—",
        }),
    ));
    v.push((
        String::from(crate::t!("viewer.info.size")),
        format_size(file_bytes),
    ));
    v.push((
        String::from(crate::t!("viewer.info.transparency")),
        String::from(if transparent {
            crate::t!("viewer.yes")
        } else {
            crate::t!("viewer.no")
        }),
    ));
    v.push((
        String::from(crate::t!("viewer.info.zoom")),
        zoom_label(zoom),
    ));
    if position.1 > 1 {
        v.push((
            String::from(crate::t!("viewer.info.position")),
            crate::t!("viewer.position", n = position.0 + 1, total = position.1),
        ));
    }
    v
}

/// What to tell the user when an image cannot be opened: a headline and one plain sentence,
/// in the language in effect.
pub fn decode_error_message(e: &DecodeError) -> [String; 2] {
    let kind = |f: &str| crate::t!("viewer.err.damaged", format = f);
    let (head, detail) = match e {
        DecodeError::UnknownFormat => (
            crate::t!("viewer.err.unknown_format"),
            String::from(crate::t!("viewer.err.supported_formats")),
        ),
        DecodeError::Png(_) => (crate::t!("viewer.err.cannot_open"), kind("PNG")),
        DecodeError::Bmp(_) => (crate::t!("viewer.err.cannot_open"), kind("BMP")),
        DecodeError::Ppm(_) => (crate::t!("viewer.err.cannot_open"), kind("PPM")),
        DecodeError::Gif(_) => (crate::t!("viewer.err.cannot_open"), kind("GIF")),
        DecodeError::Jpeg(crate::format::jpeg::JpegError::Unsupported(_)) => (
            crate::t!("viewer.err.cannot_open"),
            String::from(crate::t!("viewer.err.jpeg_unsupported")),
        ),
        DecodeError::Jpeg(_) => (crate::t!("viewer.err.cannot_open"), kind("JPEG")),
    };
    [String::from(head), detail]
}

/// Message for an image operation that failed (rotate, encode), in the language in effect.
pub fn image_error_message(e: ImageError) -> &'static str {
    match e {
        ImageError::TooLarge => crate::t!("viewer.err.too_large"),
        ImageError::OutOfMemory => crate::t!("viewer.err.no_memory"),
        ImageError::Unsupported => crate::t!("viewer.err.cannot_write"),
        ImageError::ZeroSize | ImageError::BadBuffer | ImageError::OutOfBounds => {
            crate::t!("viewer.err.invalid")
        }
    }
}

/// The format a "save as" name asks for, by extension (`png`, `bmp`, `ppm`).
pub fn save_format(name: &[u8]) -> Option<Format> {
    match &crate::apps::fileman::extension(name)[..] {
        b"png" => Some(Format::Png),
        b"bmp" => Some(Format::Bmp),
        b"ppm" => Some(Format::Ppm),
        _ => None,
    }
}

/// A first guess for the "save as" name: `stem (cópia).png` (`stem (copy).png` in English).
pub fn suggest_save_name(orig: &[u8]) -> Vec<u8> {
    let (stem, _) = vfs::split_ext(orig);
    let mut n = stem.to_vec();
    n.push(b' ');
    n.extend_from_slice(crate::t!("viewer.save_suffix").as_bytes());
    n
}

#[cfg(test)]
mod tests;
