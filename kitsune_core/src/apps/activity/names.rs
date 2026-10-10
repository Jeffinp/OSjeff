//! names (split out of `activity.rs`).

use super::*;

/// The name a person sees for an internal thread or process name. Numbered instances
/// (`shell 2`) keep their number (`Terminal 2`). Unknown names are returned as they
/// are, so nothing disappears from the list.
pub fn friendly_name(raw: &[u8]) -> String {
    let (base, num) = split_number(raw);
    // Catalog keys; the text is looked up in the language in effect.
    let fixed: Option<&str> = match base {
        b"compositor" => Some(tk!("tasks.name.interface")),
        b"fetcher" => Some(tk!("tasks.name.net_fetch")),
        b"appd" => Some(tk!("tasks.name.apps")),
        b"shelld" | b"shelld2" => Some(tk!("tasks.name.shell_exec")),
        b"logd" => Some(tk!("app.log")),
        b"kernel" => Some(tk!("tasks.name.system")),
        b"(idle)" => Some(tk!("tasks.name.idle")),
        b"shell" => Some(tk!("app.terminal")),
        b"editor" => Some(tk!("app.editor")),
        b"taskmgr" | b"monitor" => Some(tk!("app.tasks")),
        b"calc" => Some(tk!("app.calculator")),
        b"browser" => Some(tk!("app.browser")),
        b"wasmapp" => Some(tk!("app.wasm_title")),
        b"files" => Some(tk!("app.files")),
        b"settings" => Some(tk!("app.settings")),
        b"syslog" => Some(tk!("app.log")),
        b"viewer" => Some(tk!("app.viewer")),
        b"gallery" => Some(tk!("app.gallery")),
        _ => None,
    };
    let mut out = String::new();
    match fixed {
        Some(key) => out.push_str(i18n::tr(key)),
        None => {
            // Raw names are ASCII in practice; anything else is shown 1:1 as Latin-1.
            out.extend(base.iter().map(|&b| b as char));
        }
    }
    if let Some(n) = num {
        let _ = write!(out, " {n}");
    }
    out
}

/// `"shell 2"` -> (`"shell"`, `Some(2)`); anything else -> (`name`, `None`).
pub(super) fn split_number(raw: &[u8]) -> (&[u8], Option<u32>) {
    if let Some(sp) = raw.iter().rposition(|&b| b == b' ') {
        let tail = &raw[sp + 1..];
        if !tail.is_empty() && tail.len() <= 3 && tail.iter().all(u8::is_ascii_digit) {
            let n = tail.iter().fold(0u32, |a, &d| a * 10 + (d - b'0') as u32);
            return (&raw[..sp], Some(n));
        }
    }
    (raw, None)
}

/// Is `raw` the internal name of a system thread or process (one the user should
/// think twice before ending)?
pub fn is_system_name(raw: &[u8]) -> bool {
    let (base, _) = split_number(raw);
    matches!(
        base,
        b"compositor" | b"fetcher" | b"appd" | b"shelld" | b"shelld2" | b"logd" | b"kernel"
    )
}
