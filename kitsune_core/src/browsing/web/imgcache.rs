//! Images in pages: what the layout needs to know about each `<img>`
//! ([`ImageLookup`]), the decoder that turns a downloaded body into a pixel
//! buffer within the page's budgets ([`decode_for_page`]) and the small LRU
//! cache that holds the results ([`ImageCache`]).
//!
//! The network part lives in the kernel (`fetch.rs`): it downloads one image at
//! a time on the background `fetcher` thread, calls [`decode_for_page`] there
//! (so the UI never waits for a decode) and hands the result to the compositor,
//! which puts it in the cache and lays the page out again. Everything decided
//! here is pure and tested on the host.
//!
//! # Limits
//!
//! | what | limit |
//! |---|---|
//! | images fetched per page | [`MAX_PAGE_IMAGES`] = 8 (the rest show "limite de imagens") |
//! | bytes downloaded per image | [`MAX_IMAGE_BYTES`] = 512 KiB |
//! | pixels decoded | [`MAX_DECODE_PIXELS`] = 2 Mpx (8 MiB of RGBA), checked on the header |
//! | `data:` image payload | [`MAX_DATA_IMAGE_BYTES`] = 64 KiB |
//! | pixel memory kept | [`CACHE_BYTES`] = 6 MiB, least recently used goes first |
//!
//! A decoded image is scaled to the page's column width on the spot
//! ([`crate::format::image::Image::fit`]) and flattened over the page background, so
//! the cache keeps one opaque, already-small buffer per image.

use crate::format::base64;
use crate::format::image::{self, Filter, Format, Image};
use crate::tk;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

/// Images fetched for one page; later ones are shown as "limite de imagens".
pub const MAX_PAGE_IMAGES: usize = 8;
/// Largest response body accepted for one image.
pub const MAX_IMAGE_BYTES: usize = 512 * 1024;
/// Largest decoded size (width x height) before any pixel memory is reserved.
pub const MAX_DECODE_PIXELS: usize = 2 * 1024 * 1024;
/// Decoded payload limit for a `data:` image.
pub const MAX_DATA_IMAGE_BYTES: usize = 64 * 1024;
/// Longest `data:` URI kept in a page (`src` attribute), in bytes.
pub const MAX_DATA_URI_LEN: usize = 96 * 1024;
/// Pixel memory the cache may hold (RGBA, 4 bytes per pixel).
pub const CACHE_BYTES: usize = 6 * 1024 * 1024;
/// Entries kept (loaded or not).
pub const MAX_ENTRIES: usize = 24;
/// Background the pictures are flattened over (the page's white-ish).
pub const PAGE_BG: u32 = 0xFFF7_F9FC;

/// What the layout may know about an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImgState {
    /// Not downloaded yet: the layout reserves a grey box.
    Pending,
    /// Decoded; `w`x`h` are the picture's own (original) dimensions.
    Ready { w: i32, h: i32 },
    /// Not a PNG/BMP/PPM (JPEG, GIF, WebP, SVG, ...).
    Unsupported,
    /// Could not be downloaded or decoded.
    Failed,
    /// Over the byte or pixel limit.
    TooBig,
    /// Past [`MAX_PAGE_IMAGES`].
    TooMany,
}

impl ImgState {
    /// Catalog key of the line shown under the alt text of a box that has no picture;
    /// `None` for pending / ready.
    pub fn message_key(self) -> Option<&'static str> {
        match self {
            ImgState::Pending | ImgState::Ready { .. } => None,
            ImgState::Unsupported => Some(tk!("web.img.unsupported")),
            ImgState::Failed => Some(tk!("web.img.failed")),
            ImgState::TooBig => Some(tk!("web.img.too_big")),
            ImgState::TooMany => Some(tk!("web.img.too_many")),
        }
    }

    /// The line in the language in effect (the layout bakes it into the page, so the kernel
    /// lays a page out again when the language changes).
    pub fn message(self) -> Option<&'static str> {
        self.message_key().map(crate::i18n::tr)
    }
}

/// How the layout asks about an image by its `src` attribute (as written in
/// the page: the resolver turns it into a key).
pub trait ImageLookup {
    fn lookup(&self, src: &str) -> ImgState;
}

/// A lookup that knows no images: everything is [`ImgState::Pending`].
pub struct NoImages;

impl ImageLookup for NoImages {
    fn lookup(&self, _src: &str) -> ImgState {
        ImgState::Pending
    }
}

/// Why an image could not be turned into pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImgFail {
    Unsupported,
    TooBig,
    Failed,
}

impl ImgFail {
    fn state(self) -> ImgState {
        match self {
            ImgFail::Unsupported => ImgState::Unsupported,
            ImgFail::TooBig => ImgState::TooBig,
            ImgFail::Failed => ImgState::Failed,
        }
    }
}

/// A decoded, column-sized, opaque picture plus its original dimensions.
pub struct Loaded {
    pub orig_w: usize,
    pub orig_h: usize,
    pub img: Image,
}

/// Width and height from the file header, without decoding. `None` for an
/// unknown or truncated header.
pub fn peek_dims(bytes: &[u8]) -> Option<(usize, usize)> {
    match image::detect(bytes)? {
        Format::Png => {
            let h = crate::format::png::read_header(bytes).ok()?;
            Some((h.width as usize, h.height as usize))
        }
        Format::Bmp => {
            let hs = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?);
            if hs == 12 {
                let w = u16::from_le_bytes(bytes.get(18..20)?.try_into().ok()?);
                let h = u16::from_le_bytes(bytes.get(20..22)?.try_into().ok()?);
                Some((usize::from(w), usize::from(h)))
            } else {
                let w = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
                let h = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
                Some((w.unsigned_abs() as usize, h.unsigned_abs() as usize))
            }
        }
        Format::Ppm => {
            let mut pos = 2;
            let mut nums = [0usize; 2];
            for n in &mut nums {
                loop {
                    match bytes.get(pos)? {
                        b'#' => {
                            while *bytes.get(pos)? != b'\n' {
                                pos += 1;
                            }
                        }
                        b if b.is_ascii_whitespace() => pos += 1,
                        _ => break,
                    }
                }
                let mut v = 0usize;
                let start = pos;
                while let Some(d) = bytes.get(pos).filter(|b| b.is_ascii_digit()) {
                    v = v.checked_mul(10)?.checked_add(usize::from(d - b'0'))?;
                    pos += 1;
                }
                if pos == start {
                    return None;
                }
                *n = v;
            }
            Some((nums[0], nums[1]))
        }
    }
}

/// Decode a downloaded body for a page whose column is `fit_w` pixels wide.
/// Checks the header against [`MAX_DECODE_PIXELS`] before reserving memory,
/// scales down to `fit_w` and flattens over [`PAGE_BG`]. Never panics.
pub fn decode_for_page(body: &[u8], fit_w: usize) -> Result<Loaded, ImgFail> {
    if body.len() > MAX_IMAGE_BYTES {
        return Err(ImgFail::TooBig);
    }
    if image::detect(body).is_none() {
        return Err(ImgFail::Unsupported);
    }
    let (w, h) = peek_dims(body).ok_or(ImgFail::Failed)?;
    if w == 0 || h == 0 {
        return Err(ImgFail::Failed);
    }
    if w.checked_mul(h).is_none_or(|n| n > MAX_DECODE_PIXELS) {
        return Err(ImgFail::TooBig);
    }
    let img = image::decode(body).map_err(|_| ImgFail::Failed)?;
    let (ow, oh) = (img.width(), img.height());
    let fit_w = fit_w.clamp(16, 4096);
    let mut img = if ow > fit_w {
        img.fit(fit_w, 8192, false, Filter::Auto)
            .map_err(|_| ImgFail::Failed)?
    } else {
        img
    };
    img.flatten(PAGE_BG);
    Ok(Loaded {
        orig_w: ow,
        orig_h: oh,
        img,
    })
}

/// Decode a `data:image/...;base64,` URI (small inline pictures).
pub fn decode_data_uri(uri: &str, fit_w: usize) -> Result<Loaded, ImgFail> {
    if uri.len() > MAX_DATA_URI_LEN {
        return Err(ImgFail::TooBig);
    }
    match base64::decode_data_image(uri, MAX_DATA_IMAGE_BYTES) {
        Ok(bytes) => decode_for_page(&bytes, fit_w),
        Err(base64::DataImageError::Base64(base64::Base64Error::TooLarge)) => Err(ImgFail::TooBig),
        Err(base64::DataImageError::Unsupported) => Err(ImgFail::Unsupported),
        Err(_) => Err(ImgFail::Failed),
    }
}

/// FNV-1a 64-bit, for naming `data:` images without keeping the URI as a key.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The cache key of an image `src` found on the page at `base`: the absolute
/// URL, or `data:#<hash>-<len>` for an inline image. `None` when the address
/// cannot be fetched (not http(s), an https page asking for http: mixed
/// content is refused, malformed).
pub fn image_key(base: &[u8], src: &str) -> Option<String> {
    let src = src.trim();
    if src.is_empty() {
        return None;
    }
    if src.len() >= 5 && src.as_bytes()[..5].eq_ignore_ascii_case(b"data:") {
        let d = base64::parse_data_uri(src)?;
        let _ = d;
        return Some(alloc::format!(
            "data:#{:016x}-{}",
            fnv1a(src.as_bytes()),
            src.len()
        ));
    }
    let b = crate::browsing::browser::parse_url(base)?;
    let abs = crate::browsing::redirect::resolve_redirect(&b, src.as_bytes()).ok()?;
    String::from_utf8(abs).ok()
}

struct Entry {
    key: String,
    slot: Slot,
    used: Cell<u64>,
    /// The `data:` URI to decode (inline images only), dropped once decoded.
    data: Option<String>,
}

enum Slot {
    Pending,
    Loading,
    Ready { ow: usize, oh: usize, img: Image },
    Bad(ImgState),
}

/// The images of the page on screen plus a few recent ones (LRU), bounded by
/// [`CACHE_BYTES`] and [`MAX_ENTRIES`].
pub struct ImageCache {
    entries: Vec<Entry>,
    /// Keys of the page being shown, in document order (at most [`MAX_PAGE_IMAGES`]).
    page: Vec<String>,
    tick: Cell<u64>,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            page: Vec::new(),
            tick: Cell::new(0),
        }
    }

    fn touch(&self, e: &Entry) {
        self.tick.set(self.tick.get() + 1);
        e.used.set(self.tick.get());
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }

    /// Start a new page: nothing is wanted yet (loaded images stay cached).
    pub fn begin_page(&mut self) {
        self.page.clear();
    }

    /// Declare that the current page shows `key` (and, for an inline image, its
    /// `data` URI). Returns `false` when the page already has
    /// [`MAX_PAGE_IMAGES`] images or the cache cannot take another entry.
    pub fn want(&mut self, key: &str, data: Option<&str>) -> bool {
        if self.page.iter().any(|k| k == key) {
            return true;
        }
        if self.page.len() >= MAX_PAGE_IMAGES {
            return false;
        }
        if self.find(key).is_none() {
            if self.entries.len() >= MAX_ENTRIES && !self.evict_one(false) {
                return false;
            }
            self.entries.push(Entry {
                key: key.into(),
                slot: Slot::Pending,
                used: Cell::new(0),
                data: data.map(String::from),
            });
        }
        self.page.push(key.into());
        if let Some(i) = self.find(key) {
            let e = &self.entries[i];
            self.touch(e);
        }
        true
    }

    /// Is `key` one of the (at most 8) images of the current page?
    pub fn on_page(&self, key: &str) -> bool {
        self.page.iter().any(|k| k == key)
    }

    /// What the layout should do with `key`.
    pub fn state(&self, key: &str) -> ImgState {
        match self.find(key) {
            Some(i) => match &self.entries[i].slot {
                Slot::Pending | Slot::Loading => ImgState::Pending,
                Slot::Ready { ow, oh, .. } => {
                    self.touch(&self.entries[i]);
                    ImgState::Ready {
                        w: (*ow).min(65535) as i32,
                        h: (*oh).min(65535) as i32,
                    }
                }
                Slot::Bad(s) => *s,
            },
            None if self.on_page(key) || self.page.len() < MAX_PAGE_IMAGES => ImgState::Pending,
            None => ImgState::TooMany,
        }
    }

    /// An image of this page that still has to be fetched or decoded, marked as
    /// in flight. `None` while one is already loading (one at a time) or when
    /// nothing is left. The second value is the `data:` URI of an inline image.
    pub fn next_pending(&mut self) -> Option<(String, Option<String>)> {
        if self.entries.iter().any(|e| matches!(e.slot, Slot::Loading)) {
            return None;
        }
        for k in self.page.clone() {
            if let Some(i) = self.find(&k)
                && matches!(self.entries[i].slot, Slot::Pending)
            {
                self.entries[i].slot = Slot::Loading;
                return Some((k, self.entries[i].data.clone()));
            }
        }
        None
    }

    /// True while some image of the page is pending or in flight.
    pub fn busy(&self) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e.slot, Slot::Pending | Slot::Loading) && self.on_page(&e.key))
    }

    /// Record the outcome for `key`.
    pub fn finish(&mut self, key: &str, res: Result<Loaded, ImgFail>) {
        let Some(i) = self.find(key) else {
            return;
        };
        let e = &mut self.entries[i];
        e.data = None;
        e.slot = match res {
            Ok(l) => Slot::Ready {
                ow: l.orig_w,
                oh: l.orig_h,
                img: l.img,
            },
            Err(f) => Slot::Bad(f.state()),
        };
        self.tick.set(self.tick.get() + 1);
        self.entries[i].used.set(self.tick.get());
        self.evict_to_budget();
    }

    /// The pixels of `key`, if loaded.
    pub fn image(&self, key: &str) -> Option<&Image> {
        let i = self.find(key)?;
        match &self.entries[i].slot {
            Slot::Ready { img, .. } => {
                self.touch(&self.entries[i]);
                Some(img)
            }
            _ => None,
        }
    }

    /// Scale the stored picture to exactly `w`x`h` (the box the layout gave
    /// it) so painting is a plain copy. Returns whether it changed.
    pub fn fit_to(&mut self, key: &str, w: usize, h: usize) -> bool {
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return false;
        }
        let Some(i) = self.find(key) else {
            return false;
        };
        if let Slot::Ready { img, .. } = &mut self.entries[i].slot {
            if img.width() == w && img.height() == h {
                return false;
            }
            if let Ok(n) = img.resize(w, h, Filter::Auto) {
                *img = n;
                self.evict_to_budget();
                return true;
            }
        }
        false
    }

    /// Bytes of pixel memory held.
    pub fn bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|e| match &e.slot {
                Slot::Ready { img, .. } => img.width() * img.height() * 4,
                _ => 0,
            })
            .sum()
    }

    /// Number of entries (any state).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop everything (a navigation that should not keep pictures around).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.page.clear();
    }

    /// Put an in-flight image back to pending (its answer will not come).
    pub fn requeue_loading(&mut self) {
        for e in &mut self.entries {
            if matches!(e.slot, Slot::Loading) {
                e.slot = Slot::Pending;
            }
        }
    }

    /// Remove the least recently used entry that is not loading. With
    /// `allow_page` false the page's own entries are spared. Returns whether
    /// one was removed.
    fn evict_one(&mut self, allow_page: bool) -> bool {
        let victim = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !matches!(e.slot, Slot::Loading))
            .filter(|(_, e)| allow_page || !self.page.contains(&e.key))
            .min_by_key(|(_, e)| e.used.get())
            .map(|(i, _)| i);
        match victim {
            Some(i) => {
                let on_page = self.page.iter().any(|k| *k == self.entries[i].key);
                if on_page {
                    // Still on the page: keep a stub so it does not download again.
                    self.entries[i].slot = Slot::Bad(ImgState::TooBig);
                } else {
                    self.entries.remove(i);
                }
                true
            }
            None => false,
        }
    }

    fn evict_to_budget(&mut self) {
        while self.bytes() > CACHE_BYTES {
            // Prefer entries of other pages; then the oldest picture of this one.
            let victim = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| matches!(e.slot, Slot::Ready { .. }))
                .min_by_key(|(_, e)| (self.page.contains(&e.key), e.used.get()))
                .map(|(i, _)| i);
            let Some(i) = victim else { break };
            if self.page.iter().any(|k| *k == self.entries[i].key) {
                self.entries[i].slot = Slot::Bad(ImgState::TooBig);
            } else {
                self.entries.remove(i);
            }
        }
    }
}

/// The layout's view of the cache for one page: resolves `src` against the
/// page URL, then asks the cache.
pub struct PageImages<'a> {
    pub cache: &'a ImageCache,
    /// Absolute URL of the page (the base for relative `src`).
    pub base: &'a [u8],
}

impl ImageLookup for PageImages<'_> {
    fn lookup(&self, src: &str) -> ImgState {
        match image_key(self.base, src) {
            Some(k) => self.cache.state(&k),
            None => ImgState::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: usize, h: usize) -> Vec<u8> {
        let img = Image::new(w, h, 0xFF30_60C0).unwrap();
        crate::format::png::encode(&img).unwrap()
    }

    fn loaded(w: usize, h: usize) -> Loaded {
        Loaded {
            orig_w: w,
            orig_h: h,
            img: Image::new(w, h, 0xFF00_0000).unwrap(),
        }
    }

    #[test]
    fn decode_small_png_keeps_size() {
        let l = decode_for_page(&png(40, 30), 300).unwrap();
        assert_eq!((l.orig_w, l.orig_h), (40, 30));
        assert_eq!((l.img.width(), l.img.height()), (40, 30));
        assert!(l.img.is_opaque());
    }

    #[test]
    fn decode_scales_down_to_the_column() {
        let l = decode_for_page(&png(400, 200), 100).unwrap();
        assert_eq!((l.orig_w, l.orig_h), (400, 200));
        assert_eq!((l.img.width(), l.img.height()), (100, 50));
    }

    #[test]
    fn small_images_are_not_enlarged() {
        let l = decode_for_page(&png(20, 20), 800).unwrap();
        assert_eq!(l.img.width(), 20);
    }

    #[test]
    fn bmp_and_ppm_decode() {
        let img = Image::new(8, 6, 0xFFFF_0000).unwrap();
        let bmp = image::encode(&img, Format::Bmp).unwrap();
        assert_eq!(decode_for_page(&bmp, 100).unwrap().orig_w, 8);
        let ppm = image::encode(&img, Format::Ppm).unwrap();
        assert_eq!(decode_for_page(&ppm, 100).unwrap().orig_h, 6);
    }

    #[test]
    fn jpeg_gif_webp_svg_are_unsupported() {
        for sig in [
            &b"\xFF\xD8\xFF\xE0\0\x10JFIF"[..],
            b"GIF89a....",
            b"RIFF\0\0\0\0WEBPVP8 ",
            b"<svg xmlns=",
            b"",
        ] {
            assert_eq!(decode_for_page(sig, 100).err(), Some(ImgFail::Unsupported));
        }
    }

    #[test]
    fn truncated_png_fails_cleanly() {
        let p = png(50, 50);
        assert_eq!(
            decode_for_page(&p[..p.len() / 2], 100).err(),
            Some(ImgFail::Failed)
        );
        assert_eq!(decode_for_page(&p[..9], 100).err(), Some(ImgFail::Failed));
    }

    #[test]
    fn body_over_the_byte_limit_is_too_big() {
        let mut p = png(10, 10);
        p.resize(MAX_IMAGE_BYTES + 1, 0);
        assert_eq!(decode_for_page(&p, 100).err(), Some(ImgFail::TooBig));
    }

    #[test]
    fn pixel_limit_is_checked_on_the_header() {
        // A 4000x3000 PNG header with no pixel data: refused as too big before
        // anything is allocated or decoded.
        let big = png(2100, 1000); // 2.1 Mpx, valid and small once compressed
        assert!(big.len() < MAX_IMAGE_BYTES);
        assert_eq!(decode_for_page(&big, 100).err(), Some(ImgFail::TooBig));
        let ok = png(1400, 1000); // 1.4 Mpx
        assert!(decode_for_page(&ok, 700).is_ok());
    }

    #[test]
    fn peek_dims_of_each_format() {
        assert_eq!(peek_dims(&png(7, 9)), Some((7, 9)));
        let img = Image::new(5, 4, 0xFF00_FF00).unwrap();
        assert_eq!(
            peek_dims(&image::encode(&img, Format::Bmp).unwrap()),
            Some((5, 4))
        );
        assert_eq!(
            peek_dims(&image::encode(&img, Format::Ppm).unwrap()),
            Some((5, 4))
        );
        assert_eq!(peek_dims(b"P6 # c\n12 34\n255\n"), Some((12, 34)));
        assert_eq!(peek_dims(b"junk"), None);
        assert_eq!(peek_dims(b"BM"), None);
        assert_eq!(peek_dims(b"P6 99999999999999999999999 1"), None);
    }

    #[test]
    fn transparent_png_is_flattened() {
        let mut img = Image::new(4, 4, 0x0000_0000).unwrap();
        img.set(0, 0, 0xFF10_2030);
        let bytes = image::encode(&img, Format::Png).unwrap();
        let l = decode_for_page(&bytes, 100).unwrap();
        assert!(l.img.is_opaque());
        assert_eq!(l.img.get(1, 1), Some(PAGE_BG));
        assert_eq!(l.img.get(0, 0), Some(0xFF10_2030));
    }

    #[test]
    fn data_uri_decodes_a_png() {
        let uri = alloc::format!("data:image/png;base64,{}", base64::encode(&png(10, 10)));
        let l = decode_data_uri(&uri, 100).unwrap();
        assert_eq!(l.orig_w, 10);
    }

    #[test]
    fn data_uri_errors() {
        assert_eq!(
            decode_data_uri("data:image/jpeg;base64,/9j/4AAQ", 100).err(),
            Some(ImgFail::Unsupported)
        );
        assert_eq!(
            decode_data_uri("data:image/png;base64,!!!!", 100).err(),
            Some(ImgFail::Failed)
        );
        assert_eq!(
            decode_data_uri("data:text/plain,hello", 100).err(),
            Some(ImgFail::Unsupported)
        );
        let huge = alloc::format!("data:image/png;base64,{}", "A".repeat(MAX_DATA_URI_LEN));
        assert_eq!(decode_data_uri(&huge, 100).err(), Some(ImgFail::TooBig));
        let over = alloc::format!(
            "data:image/png;base64,{}",
            base64::encode(&alloc::vec![0u8; MAX_DATA_IMAGE_BYTES + 3])
        );
        assert_eq!(decode_data_uri(&over, 100).err(), Some(ImgFail::TooBig));
    }

    #[test]
    fn keys_resolve_against_the_page() {
        let base = b"http://h.test/a/b.html";
        assert_eq!(
            image_key(base, "p.png").as_deref(),
            Some("http://h.test/a/p.png")
        );
        assert_eq!(
            image_key(base, "/x/y.png").as_deref(),
            Some("http://h.test/x/y.png")
        );
        assert_eq!(
            image_key(base, "//cdn.test/z.png").as_deref(),
            Some("http://cdn.test/z.png")
        );
        assert_eq!(
            image_key(base, "http://o.test/i.png").as_deref(),
            Some("http://o.test/i.png")
        );
    }

    #[test]
    fn mixed_content_and_junk_have_no_key() {
        let https = b"https://h.test/";
        assert_eq!(image_key(https, "http://h.test/i.png"), None);
        assert_eq!(image_key(https, ""), None);
        assert_eq!(image_key(https, "   "), None);
        assert_eq!(image_key(https, "javascript:alert(1)"), None);
        assert_eq!(image_key(https, "ftp://h/i.png"), None);
        assert_eq!(image_key(b"", "a.png"), None);
    }

    #[test]
    fn data_keys_are_short_and_stable() {
        let uri = alloc::format!("data:image/png;base64,{}", "QUJD".repeat(5000));
        let k = image_key(b"http://h/", &uri).unwrap();
        assert!(k.starts_with("data:#") && k.len() < 40);
        assert_eq!(image_key(b"http://other/", &uri).unwrap(), k);
        let other = alloc::format!("data:image/png;base64,{}", "QUJE".repeat(5000));
        assert_ne!(image_key(b"http://h/", &other).unwrap(), k);
        assert_eq!(image_key(b"http://h/", "data:nocomma"), None);
    }

    #[test]
    fn want_registers_pending_and_caps_the_page() {
        let mut c = ImageCache::new();
        c.begin_page();
        for i in 0..MAX_PAGE_IMAGES {
            assert!(c.want(&alloc::format!("http://h/{i}.png"), None));
        }
        assert!(!c.want("http://h/extra.png", None));
        assert_eq!(c.state("http://h/extra.png"), ImgState::TooMany);
        assert_eq!(c.state("http://h/0.png"), ImgState::Pending);
        // Asking again for one already there is fine.
        assert!(c.want("http://h/3.png", None));
    }

    #[test]
    fn pending_images_are_handed_out_one_at_a_time_in_order() {
        let mut c = ImageCache::new();
        c.begin_page();
        c.want("a", None);
        c.want("b", None);
        assert_eq!(c.next_pending().unwrap().0, "a");
        assert!(c.next_pending().is_none(), "one in flight at a time");
        c.finish("a", Ok(loaded(4, 4)));
        assert_eq!(c.next_pending().unwrap().0, "b");
        c.finish("b", Err(ImgFail::Failed));
        assert!(c.next_pending().is_none());
        assert!(!c.busy());
    }

    #[test]
    fn states_follow_the_outcome() {
        let mut c = ImageCache::new();
        c.begin_page();
        for k in ["ok", "jpg", "big", "bad"] {
            c.want(k, None);
        }
        c.finish("ok", Ok(loaded(30, 20)));
        c.finish("jpg", Err(ImgFail::Unsupported));
        c.finish("big", Err(ImgFail::TooBig));
        c.finish("bad", Err(ImgFail::Failed));
        assert_eq!(c.state("ok"), ImgState::Ready { w: 30, h: 20 });
        assert_eq!(c.state("jpg"), ImgState::Unsupported);
        assert_eq!(c.state("big"), ImgState::TooBig);
        assert_eq!(c.state("bad"), ImgState::Failed);
        assert!(c.image("ok").is_some() && c.image("jpg").is_none());
    }

    #[test]
    fn messages_exist_only_for_failures() {
        assert_eq!(ImgState::Pending.message(), None);
        assert_eq!(ImgState::Ready { w: 1, h: 1 }.message(), None);
        assert_eq!(
            ImgState::Unsupported.message_key(),
            Some("web.img.unsupported")
        );
        for s in [
            ImgState::Unsupported,
            ImgState::Failed,
            ImgState::TooBig,
            ImgState::TooMany,
        ] {
            for l in crate::i18n::Lang::ALL {
                let t = crate::i18n::tr_in(l, s.message_key().unwrap());
                assert!(!t.is_empty() && !t.starts_with("web."), "{l:?}");
            }
        }
        assert_eq!(
            crate::i18n::tr_in(crate::i18n::Lang::En, "web.img.unsupported"),
            "unsupported format"
        );
        assert_eq!(
            crate::i18n::tr_in(crate::i18n::Lang::Pt, "web.img.unsupported"),
            "formato não suportado"
        );
    }

    #[test]
    fn byte_budget_evicts_the_least_recently_used() {
        let mut c = ImageCache::new();
        // Each picture is 1000x600 = 2.4 MB; four do not fit in 8 MiB.
        for i in 0..4 {
            c.begin_page();
            let k = alloc::format!("p{i}");
            c.want(&k, None);
            c.finish(&k, Ok(loaded(1000, 600)));
        }
        assert!(c.bytes() <= CACHE_BYTES);
        assert!(c.image("p3").is_some(), "the newest survives");
        assert!(c.image("p0").is_none(), "the oldest was evicted");
    }

    #[test]
    fn evicting_a_page_image_leaves_a_stub_not_a_refetch() {
        let mut c = ImageCache::new();
        c.begin_page();
        for i in 0..5 {
            let k = alloc::format!("q{i}");
            c.want(&k, None);
            c.finish(&k, Ok(loaded(1000, 700)));
        }
        assert!(c.bytes() <= CACHE_BYTES);
        // Nothing of this page went back to Pending (that would download again).
        assert!(!c.busy());
        assert!((0..5).any(|i| c.state(&alloc::format!("q{i}")) == ImgState::TooBig));
    }

    #[test]
    fn entry_count_is_bounded() {
        let mut c = ImageCache::new();
        for i in 0..100 {
            c.begin_page();
            c.want(&alloc::format!("e{i}"), None);
        }
        assert!(c.len() <= MAX_ENTRIES);
    }

    #[test]
    fn fit_to_rescales_once() {
        let mut c = ImageCache::new();
        c.begin_page();
        c.want("a", None);
        c.finish("a", Ok(loaded(100, 50)));
        assert!(c.fit_to("a", 50, 25));
        assert!(!c.fit_to("a", 50, 25));
        let i = c.image("a").unwrap();
        assert_eq!((i.width(), i.height()), (50, 25));
        assert_eq!(c.state("a"), ImgState::Ready { w: 100, h: 50 });
        assert!(!c.fit_to("a", 0, 5));
        assert!(!c.fit_to("a", 5000, 5));
        assert!(!c.fit_to("zzz", 5, 5));
    }

    #[test]
    fn data_uri_is_kept_until_decoded() {
        let mut c = ImageCache::new();
        c.begin_page();
        c.want("data:#1-2", Some("data:image/png;base64,AA=="));
        let (k, d) = c.next_pending().unwrap();
        assert_eq!(d.as_deref(), Some("data:image/png;base64,AA=="));
        c.finish(&k, Err(ImgFail::Failed));
        c.begin_page();
        c.want(&k, None);
        // After finishing, the URI text is gone.
        assert!(c.next_pending().is_none());
    }

    #[test]
    fn loaded_images_survive_into_the_next_page() {
        let mut c = ImageCache::new();
        c.begin_page();
        c.want("u", None);
        c.finish("u", Ok(loaded(10, 10)));
        c.begin_page();
        assert!(c.want("u", None));
        assert_eq!(c.state("u"), ImgState::Ready { w: 10, h: 10 });
        assert!(c.next_pending().is_none(), "no second download");
    }

    #[test]
    fn requeue_puts_loading_back() {
        let mut c = ImageCache::new();
        c.begin_page();
        c.want("u", None);
        assert!(c.next_pending().is_some());
        assert!(c.next_pending().is_none());
        c.requeue_loading();
        assert!(c.next_pending().is_some());
    }

    #[test]
    fn clear_empties_everything() {
        let mut c = ImageCache::new();
        c.begin_page();
        c.want("u", None);
        c.finish("u", Ok(loaded(5, 5)));
        c.clear();
        assert!(c.is_empty() && c.bytes() == 0);
    }

    #[test]
    fn page_images_resolve_then_look_up() {
        let mut c = ImageCache::new();
        c.begin_page();
        let k = image_key(b"http://h/a/", "i.png").unwrap();
        c.want(&k, None);
        c.finish(&k, Ok(loaded(12, 8)));
        let pi = PageImages {
            cache: &c,
            base: b"http://h/a/index.html",
        };
        assert_eq!(pi.lookup("i.png"), ImgState::Ready { w: 12, h: 8 });
        // Not registered yet, and the page still has room: it will be fetched.
        assert_eq!(pi.lookup("other.png"), ImgState::Pending);
        assert_eq!(pi.lookup(""), ImgState::Failed);
        assert_eq!(NoImages.lookup("x"), ImgState::Pending);
    }

    #[test]
    fn finish_of_an_unknown_key_is_ignored() {
        let mut c = ImageCache::new();
        c.finish("nope", Ok(loaded(5, 5)));
        assert!(c.is_empty());
    }
}
