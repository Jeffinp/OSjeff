//! names (split out of `ui.rs`).

use super::*;

/// A name folded for searching: ASCII lowercase with accents removed, so `acao` finds `Ação`.
pub fn search_key(name: &[u8]) -> String {
    let folded = super::super::display_ascii(name);
    folded
        .iter()
        .map(|&b| b.to_ascii_lowercase() as char)
        .collect()
}

/// The folded, trimmed form of a search query, ready for [`matches_key`].
pub fn query_key(query: &[u8]) -> String {
    String::from(search_key(query).trim())
}

/// Whether `name` contains the folded query `key` (an empty key matches everything).
pub fn matches_key(name: &[u8], key: &str) -> bool {
    key.is_empty() || search_key(name).contains(key)
}

/// Whether `name` matches the search `query` (an empty query matches everything).
pub fn matches_query(name: &[u8], query: &[u8]) -> bool {
    matches_key(name, &query_key(query))
}

/// How a row is described in the preview pane and the file icons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewKind {
    Image,
    Text,
    Folder,
    App,
    Other,
}

/// What a name (and whether it is a folder) previews as.
pub fn preview_kind(name: &[u8], is_dir: bool) -> PreviewKind {
    use super::super::FileClass;
    if is_dir {
        return PreviewKind::Folder;
    }
    match super::super::classify(name) {
        FileClass::Image => PreviewKind::Image,
        FileClass::Wasm => PreviewKind::App,
        FileClass::Text => PreviewKind::Text,
        FileClass::Other => PreviewKind::Other,
    }
}

/// The file icon a name uses.
pub fn icon_kind(name: &[u8], is_dir: bool) -> crate::ui::appart::FileKind {
    use crate::ui::appart::FileKind;
    match preview_kind(name, is_dir) {
        PreviewKind::Folder => FileKind::Folder,
        PreviewKind::Image => FileKind::Image,
        PreviewKind::App => FileKind::App,
        PreviewKind::Text => FileKind::Text,
        PreviewKind::Other => FileKind::Generic,
    }
}

/// A human description of a file by its name, in the language in effect: `Pasta`, `Imagem PNG`,
/// `Texto`, `Aplicativo`... (`Folder`, `PNG image`, `Text`, `App`...).
pub fn kind_label(name: &[u8], is_dir: bool) -> String {
    if is_dir {
        return String::from(crate::t!("files.kind.folder"));
    }
    let ext = super::super::extension(name);
    let upper: String = ext
        .iter()
        .map(|&b| (b as char).to_ascii_uppercase())
        .collect();
    match preview_kind(name, false) {
        PreviewKind::Image => crate::t!("files.kind.image_ext", ext = &upper),
        PreviewKind::App => String::from(crate::t!("files.kind.app")),
        PreviewKind::Text if ext.is_empty() => String::from(crate::t!("files.kind.text")),
        PreviewKind::Text => crate::t!("files.kind.text_ext", ext = &upper),
        _ if ext.is_empty() => String::from(crate::t!("files.kind.file")),
        _ => crate::t!("files.kind.file_ext", ext = &upper),
    }
}

/// Largest file the preview decodes as an image.
pub const PREVIEW_MAX_IMAGE: u64 = 3 * 1024 * 1024;

/// Bytes read from the start of a text file for the preview.
pub const PREVIEW_TEXT_BYTES: usize = 6 * 1024;

/// The first lines of a text for the preview: invalid UTF-8 shown as replacement characters,
/// tabs as four spaces, other control characters as spaces, at most `max_lines` lines of
/// `max_chars` characters each. A file that is not text gives no lines.
pub fn text_preview(bytes: &[u8], max_lines: usize, max_chars: usize) -> Vec<String> {
    if !super::super::looks_like_text(bytes) {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if out.len() >= max_lines {
            break;
        }
        let mut line = String::new();
        let mut n = 0;
        for ch in raw.trim_end_matches('\r').chars() {
            if n >= max_chars {
                break;
            }
            match ch {
                '\t' => {
                    line.push_str("    ");
                    n += 4;
                }
                c if c.is_control() => {
                    line.push(' ');
                    n += 1;
                }
                c => {
                    line.push(c);
                    n += 1;
                }
            }
        }
        out.push(line);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// The modified time as people say it: `Hoje, 14:32`, `Ontem, 09:10`, else the date and time
/// (`Today, 2:32 PM` ... in English). `0` (the clock was not set when the file was written)
/// shows `--`.
pub fn format_modified(unix: u64, now: u64, tz_secs: i32, clock24: bool) -> String {
    if unix == 0 {
        return String::from("--");
    }
    let day = |t: u64| (t as i64 + tz_secs as i64).div_euclid(86_400);
    let secs = (unix as i64 + tz_secs as i64).rem_euclid(86_400);
    let civil = crate::i18n::Civil {
        year: 2000,
        month: 1,
        day: 1,
        weekday: 0,
        hour: (secs / 3600) as u8,
        minute: ((secs % 3600) / 60) as u8,
        second: (secs % 60) as u8,
    };
    let time = crate::i18n::format_time(civil, clock24, false);
    match day(now) - day(unix) {
        0 => crate::t!("files.when.today", time = &time),
        1 => crate::t!("files.when.yesterday", time = &time),
        _ => super::super::format_datetime(unix, tz_secs, clock24),
    }
}
