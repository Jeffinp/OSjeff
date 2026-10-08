//! System settings model and its `key=value` text form.
//!
//! [`Settings`] is plain data (`Copy`, no heap) that the kernel applies at boot
//! and the settings app edits. It is stored as text, one `key=value` per line:
//!
//! ```text
//! # OSjeff settings
//! version=1
//! wallpaper=2
//! wallpaper_path=fotos/praia.png
//! accent=3
//! clock=24
//! tz=-180
//! keyboard=abnt2
//! toasts=1
//! appearance=auto
//! reduce_motion=0
//! ```
//!
//! Parsing is total: [`Settings::parse`] never fails. Blank lines, `#`
//! comments, unknown keys and malformed or out-of-range values are skipped and
//! the field keeps its default, so a damaged or newer file still yields a usable
//! configuration. A file with no `version` is read as version 1.

use crate::hw::rtc::{TZ_MAX, TZ_MIN};
use crate::keymap::Layout;
use crate::style::AppearanceSetting;
use crate::wallpaper::PRESETS;
use alloc::vec::Vec;

/// Version written by [`Settings::to_text`].
pub const VERSION: u32 = 1;
/// Longest wallpaper file path (bytes).
pub const PATH_CAP: usize = 40;
/// Name of the settings file in the FS (the FS v3 front maps it to
/// `/etc/osjeff.conf`).
pub const FILE_NAME: &[u8] = b"osjeff.conf";

/// Accent palette (`0xRRGGBB`). Entry 0 is the default indigo and must stay equal
/// to `theme::ACCENT_DEFAULT`.
pub const ACCENTS: [u32; 8] = [
    0x5B5CF6, // indigo (default)
    0x14B8C4, // turquoise
    0xA855F7, // violet
    0xEC4899, // rose
    0xFB6F4B, // coral
    0xF59E0B, // amber
    0x22C55E, // green
    0x8B90A0, // graphite
];

/// Names of the [`ACCENTS`] for the settings page.
pub const ACCENT_NAMES: [&str; 8] = [
    "Indigo", "Turquesa", "Violeta", "Rosa", "Coral", "Ambar", "Verde", "Grafite",
];

/// Which wallpaper is active.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallpaperChoice {
    /// An entry of [`PRESETS`].
    Preset(u8),
    /// The user's image at `Settings::image_path`.
    Image,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    pub wallpaper: WallpaperChoice,
    image_path: [u8; PATH_CAP],
    image_len: u8,
    /// Index into [`ACCENTS`].
    pub accent: u8,
    /// 24-hour clock (otherwise 12-hour with AM/PM).
    pub clock24: bool,
    /// Minutes east of UTC.
    pub tz_minutes: i16,
    pub layout: Layout,
    /// Show notification toasts.
    pub toasts: bool,
    /// Light, dark, or by the time of day.
    pub appearance: AppearanceSetting,
    /// Skip animations: every transition jumps to its end.
    pub reduce_motion: bool,
}

impl Default for Settings {
    /// The defaults: dynamic wallpaper, indigo accent, 24-hour clock, Brasilia time
    /// (UTC-3), US keyboard, toasts on, appearance by the time of day, motion on.
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    pub const fn new() -> Self {
        Self {
            wallpaper: WallpaperChoice::Preset(0),
            image_path: [0; PATH_CAP],
            image_len: 0,
            accent: 0,
            clock24: true,
            tz_minutes: -180,
            layout: Layout::Us,
            toasts: true,
            appearance: AppearanceSetting::Auto,
            reduce_motion: false,
        }
    }

    pub fn image_path(&self) -> &[u8] {
        &self.image_path[..self.image_len as usize]
    }

    /// Set the wallpaper image path. Refused (`false`, nothing changed) when it
    /// is empty, too long, or has characters other than `A-Z a-z 0-9 . _ - /`.
    pub fn set_image_path(&mut self, path: &[u8]) -> bool {
        if !valid_path(path) {
            return false;
        }
        self.image_path = [0; PATH_CAP];
        self.image_path[..path.len()].copy_from_slice(path);
        self.image_len = path.len() as u8;
        true
    }

    /// The accent as `0xRRGGBB`.
    pub fn accent_rgb(&self) -> u32 {
        ACCENTS[(self.accent as usize).min(ACCENTS.len() - 1)]
    }

    /// Build settings from text. Total (see the module docs).
    pub fn parse(text: &[u8]) -> Settings {
        let mut s = Settings::new();
        for line in text.split(|&b| b == b'\n') {
            let line = trim(line);
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            let Some(eq) = line.iter().position(|&b| b == b'=') else {
                continue;
            };
            let (key, val) = (trim(&line[..eq]), trim(&line[eq + 1..]));
            s.apply(key, val);
        }
        // `wallpaper=image` with no usable path means the default wallpaper.
        if s.wallpaper == WallpaperChoice::Image && s.image_len == 0 {
            s.wallpaper = WallpaperChoice::Preset(0);
        }
        s
    }

    /// Apply one `key=value`; anything invalid is ignored.
    fn apply(&mut self, key: &[u8], val: &[u8]) {
        match key {
            b"wallpaper" => {
                if val.eq_ignore_ascii_case(b"image") {
                    // Only meaningful with a path; `wallpaper_path` may come later.
                    self.wallpaper = WallpaperChoice::Image;
                } else if let Some(n) = parse_u32(val).filter(|&n| (n as usize) < PRESETS.len()) {
                    self.wallpaper = WallpaperChoice::Preset(n as u8);
                }
            }
            b"wallpaper_path" => {
                self.set_image_path(val);
            }
            b"accent" => {
                if let Some(n) = parse_u32(val).filter(|&n| (n as usize) < ACCENTS.len()) {
                    self.accent = n as u8;
                }
            }
            b"clock" => match val {
                b"24" => self.clock24 = true,
                b"12" => self.clock24 = false,
                _ => {}
            },
            b"tz" => {
                if let Some(m) = parse_i32(val).filter(|m| (TZ_MIN..=TZ_MAX).contains(m)) {
                    self.tz_minutes = m as i16;
                }
            }
            b"keyboard" => {
                if let Some(l) = Layout::from_name(val) {
                    self.layout = l;
                }
            }
            b"toasts" => match val {
                b"1" | b"on" => self.toasts = true,
                b"0" | b"off" => self.toasts = false,
                _ => {}
            },
            b"appearance" => {
                if let Some(a) = AppearanceSetting::from_name(val) {
                    self.appearance = a;
                }
            }
            b"reduce_motion" => match val {
                b"1" | b"on" => self.reduce_motion = true,
                b"0" | b"off" => self.reduce_motion = false,
                _ => {}
            },
            // `version` and anything unknown: nothing to do (forward compatible).
            _ => {}
        }
    }

    /// The text form (see the module docs).
    pub fn to_text(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(200);
        out.extend_from_slice(b"# OSjeff settings\n");
        push_kv(&mut out, b"version", VERSION);
        match self.wallpaper {
            WallpaperChoice::Preset(n) => push_kv(&mut out, b"wallpaper", n as u32),
            WallpaperChoice::Image => out.extend_from_slice(b"wallpaper=image\n"),
        }
        if self.image_len > 0 {
            out.extend_from_slice(b"wallpaper_path=");
            out.extend_from_slice(self.image_path());
            out.push(b'\n');
        }
        push_kv(&mut out, b"accent", self.accent as u32);
        push_kv(&mut out, b"clock", if self.clock24 { 24 } else { 12 });
        out.extend_from_slice(b"tz=");
        push_i32(&mut out, self.tz_minutes as i32);
        out.push(b'\n');
        out.extend_from_slice(b"keyboard=");
        out.extend_from_slice(self.layout.name().as_bytes());
        out.push(b'\n');
        push_kv(&mut out, b"toasts", self.toasts as u32);
        out.extend_from_slice(b"appearance=");
        out.extend_from_slice(self.appearance.name().as_bytes());
        out.push(b'\n');
        push_kv(&mut out, b"reduce_motion", self.reduce_motion as u32);
        out
    }
}

/// The volume path a stored wallpaper path means: the settings file and the
/// settings page accept a bare name (`papel.png`, what the old flat file system
/// used), the volume wants an absolute one (`/papel.png`). An absolute path is
/// returned unchanged.
pub fn absolute_path(p: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.len() + 1);
    if p.first() != Some(&b'/') {
        out.push(b'/');
    }
    out.extend_from_slice(p);
    out
}

fn valid_path(p: &[u8]) -> bool {
    !p.is_empty()
        && p.len() <= PATH_CAP
        && p.iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if first.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if last.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() || s.len() > 9 || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0u32, |a, &d| a * 10 + (d - b'0') as u32))
}

fn parse_i32(s: &[u8]) -> Option<i32> {
    match s {
        [b'-', rest @ ..] => parse_u32(rest).map(|v| -(v as i32)),
        [b'+', rest @ ..] => parse_u32(rest).map(|v| v as i32),
        _ => parse_u32(s).map(|v| v as i32),
    }
}

fn push_i32(out: &mut Vec<u8>, v: i32) {
    if v < 0 {
        out.push(b'-');
    }
    let mut n = v.unsigned_abs();
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

fn push_kv(out: &mut Vec<u8>, key: &[u8], v: u32) {
    out.extend_from_slice(key);
    out.push(b'=');
    push_i32(out, v as i32);
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_new_desktop() {
        let s = Settings::default();
        assert_eq!(s.wallpaper, WallpaperChoice::Preset(0));
        assert_eq!(s.accent, 0);
        assert!(s.clock24);
        assert_eq!(s.tz_minutes, -180);
        assert_eq!(s.layout, Layout::Us);
        assert!(s.toasts);
        assert_eq!(s.accent_rgb(), 0x5B5CF6);
        assert_eq!(s.appearance, AppearanceSetting::Auto);
        assert!(!s.reduce_motion);
        assert!(s.image_path().is_empty());
    }

    #[test]
    fn roundtrip() {
        let mut s = Settings {
            wallpaper: WallpaperChoice::Image,
            accent: 5,
            clock24: false,
            tz_minutes: 330,
            layout: Layout::Abnt2,
            toasts: false,
            appearance: AppearanceSetting::Dark,
            reduce_motion: true,
            ..Settings::default()
        };
        assert!(s.set_image_path(b"fotos/praia-1.png"));
        let text = s.to_text();
        assert_eq!(Settings::parse(&text), s);
        // Idempotent.
        assert_eq!(Settings::parse(&text).to_text(), text);
    }

    #[test]
    fn default_roundtrips_and_keeps_text_small() {
        let s = Settings::default();
        let text = s.to_text();
        assert_eq!(Settings::parse(&text), s);
        assert!(text.len() < 240, "{}", text.len());
        assert!(text.starts_with(b"# OSjeff settings\nversion=1\n"));
    }

    #[test]
    fn negative_timezone_roundtrip() {
        let mut s = Settings::default();
        for tz in [-720, -180, 0, 60, 330, 840] {
            s.tz_minutes = tz;
            assert_eq!(Settings::parse(&s.to_text()).tz_minutes, tz);
        }
    }

    #[test]
    fn empty_and_garbage_give_defaults() {
        assert_eq!(Settings::parse(b""), Settings::default());
        assert_eq!(Settings::parse(b"\n\n   \n"), Settings::default());
        assert_eq!(Settings::parse(b"\xFF\xFE\x00garbage"), Settings::default());
        assert_eq!(
            Settings::parse(b"no equals here\n=\n==\n"),
            Settings::default()
        );
    }

    #[test]
    fn invalid_values_keep_the_default() {
        let s = Settings::parse(
            b"accent=99\nclock=13\ntz=99999\nkeyboard=klingon\ntoasts=maybe\nwallpaper=77\nwallpaper_path=bad path!\n",
        );
        assert_eq!(s, Settings::default());
        let s = Settings::parse(b"accent=abc\ntz=--5\ntz=\naccent=\nwallpaper=-1\n");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn valid_lines_survive_invalid_neighbours() {
        let s = Settings::parse(b"accent=zzz\naccent=4\nclock=24\nclock=12\ntz=bad\n");
        assert_eq!(s.accent, 4);
        assert!(!s.clock24);
        assert_eq!(s.tz_minutes, -180);
    }

    #[test]
    fn comments_whitespace_crlf_and_unknown_keys() {
        let s = Settings::parse(
            b"# comment\r\n  accent = 2  \r\nfuture_key=whatever\nversion=99\n  # indented comment\nclock=12\r\n",
        );
        assert_eq!(s.accent, 2);
        assert!(!s.clock24);
    }

    #[test]
    fn later_lines_win() {
        assert_eq!(Settings::parse(b"accent=1\naccent=6\n").accent, 6);
    }

    #[test]
    fn image_wallpaper_needs_a_path() {
        // No path anywhere: default wallpaper.
        assert_eq!(
            Settings::parse(b"wallpaper=image\n").wallpaper,
            WallpaperChoice::Preset(0)
        );
        // Path before or after the choice both work.
        let a = Settings::parse(b"wallpaper=image\nwallpaper_path=a.png\n");
        let b = Settings::parse(b"wallpaper_path=a.png\nwallpaper=image\n");
        assert_eq!(a.wallpaper, WallpaperChoice::Image);
        assert_eq!(a, b);
        assert_eq!(a.image_path(), b"a.png");
        // A preset choice keeps a stored path around for later.
        let c = Settings::parse(b"wallpaper_path=a.png\nwallpaper=3\n");
        assert_eq!(c.wallpaper, WallpaperChoice::Preset(3));
        assert_eq!(c.image_path(), b"a.png");
    }

    #[test]
    fn path_validation() {
        let mut s = Settings::default();
        assert!(!s.set_image_path(b""));
        assert!(!s.set_image_path(b"has space.png"));
        assert!(!s.set_image_path(b"../etc/passwd\n"));
        assert!(!s.set_image_path(&[b'a'; PATH_CAP + 1]));
        assert!(s.set_image_path(&[b'a'; PATH_CAP]));
        assert!(s.set_image_path(b"a/b_c-d.PNG"));
        assert_eq!(s.image_path(), b"a/b_c-d.PNG");
        // A refused path leaves the previous one.
        assert!(!s.set_image_path(b"bad path"));
        assert_eq!(s.image_path(), b"a/b_c-d.PNG");
    }

    #[test]
    fn accent_palette_head_is_the_theme_indigo() {
        assert_eq!(ACCENTS[0], 0x5B5CF6);
        assert_eq!(ACCENTS.len(), ACCENT_NAMES.len());
        let s = Settings {
            accent: 200,
            ..Settings::default()
        };
        assert_eq!(s.accent_rgb(), ACCENTS[7]);
    }

    #[test]
    fn appearance_and_reduce_motion_parse_and_stay_total() {
        let s = Settings::parse(b"appearance=dark\nreduce_motion=1\n");
        assert_eq!(s.appearance, AppearanceSetting::Dark);
        assert!(s.reduce_motion);
        // Bad values keep the default, good neighbours survive.
        let s = Settings::parse(b"appearance=sepia\nreduce_motion=maybe\nappearance=light\n");
        assert_eq!(s.appearance, AppearanceSetting::Light);
        assert!(!s.reduce_motion);
        assert_eq!(
            Settings::parse(b"appearance=\n=dark\n").appearance,
            AppearanceSetting::Auto
        );
        // Old files without the new keys read as the defaults.
        let old = Settings::parse(b"version=1\nwallpaper=2\naccent=3\nclock=12\n");
        assert_eq!(old.appearance, AppearanceSetting::Auto);
        assert!(!old.reduce_motion);
        assert!(
            old.to_text()
                .ends_with(b"appearance=auto\nreduce_motion=0\n")
        );
    }

    #[test]
    fn every_possible_input_byte_parses_without_panic() {
        // Poor man's fuzz: short strings over a nasty alphabet.
        let alphabet = b"=\n#- 0123456789abcxyz\r\xFF";
        let mut buf = [0u8; 5];
        fn walk(depth: usize, buf: &mut [u8; 5], alphabet: &[u8]) {
            if depth == buf.len() {
                let _ = Settings::parse(buf);
                return;
            }
            for &a in alphabet {
                buf[depth] = a;
                walk(depth + 1, buf, alphabet);
            }
        }
        walk(0, &mut buf, alphabet);
    }

    #[test]
    fn stored_wallpaper_paths_become_absolute_volume_paths() {
        assert_eq!(absolute_path(b"papel.png"), b"/papel.png");
        assert_eq!(absolute_path(b"fotos/praia.png"), b"/fotos/praia.png");
        assert_eq!(absolute_path(b"/Imagens/a.png"), b"/Imagens/a.png");
        // Whatever the settings file holds, the result is a single leading slash
        // followed by the stored text: nothing is resolved or removed here (the
        // volume rejects `.`/`..` components itself).
        let mut s = Settings::new();
        assert!(s.set_image_path(b"a/b.png"));
        assert_eq!(absolute_path(s.image_path()), b"/a/b.png");
    }
}
