//! Fuzz target: the editor's dialogs (`kitsune_core::editor2::dialog`).
//!
//! An arbitrary folder listing (any names, including `..`, `/`, control
//! characters and huge sizes) and arbitrary keys, clicks, wheel steps, window
//! sizes, errors and overwrite questions go through the Open / Save-as
//! [`Picker`], and arbitrary keys through [`CloseAsk`]. Invariants after every
//! step: the selection and scroll stay inside the list, the caret stays inside
//! the field, the field never exceeds `MAX_FIELD`, and every path the picker
//! asks the kernel to open or list is absolute, normalized (no `.`, `..` or
//! empty component) so the VFS never sees a surprise.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use kitsune_core::editor2::{CloseAsk, MAX_FIELD, PickEvent, PickMode, PickRow, Picker};
use kitsune_core::input::{KeyCode, KeyEvent, Mods};

const MAX_ROWS: usize = 300;
const MAX_OPS: usize = 600;

#[derive(Arbitrary, Debug)]
struct Row {
    name: String,
    dir: bool,
    size: u64,
}

#[derive(Arbitrary, Debug)]
enum Op {
    Key { code: u8, ch: char, ctrl: bool, shift: bool, alt: bool },
    Click(u16, bool),
    Wheel(i8),
    Visible(u8),
    Entries(String, Vec<Row>),
    Error(String),
    Ask(String),
    AskKey { code: u8, ch: char },
}

fn code_of(code: u8, ch: char) -> KeyCode {
    match code % 16 {
        0 => KeyCode::Enter,
        1 => KeyCode::Backspace,
        2 => KeyCode::Delete,
        3 => KeyCode::Tab,
        4 => KeyCode::Esc,
        5 => KeyCode::Left,
        6 => KeyCode::Right,
        7 => KeyCode::Up,
        8 => KeyCode::Down,
        9 => KeyCode::Home,
        10 => KeyCode::End,
        11 => KeyCode::PageUp,
        12 => KeyCode::PageDown,
        _ => KeyCode::Char(ch),
    }
}

fn rows(list: Vec<Row>) -> Vec<PickRow> {
    list.into_iter()
        .take(MAX_ROWS)
        .map(|r| PickRow {
            name: r.name,
            dir: r.dir,
            size: r.size,
        })
        .collect()
}

/// A path the kernel will be asked about: absolute and normalized.
fn check_path(p: &str) {
    assert!(p.starts_with('/'), "not absolute: {p:?}");
    assert!(!p.contains("//"), "empty component: {p:?}");
    if p != "/" {
        assert!(!p.ends_with('/'), "trailing slash: {p:?}");
    }
    for c in p.split('/').skip(1) {
        assert!(c != "." && c != "..", "dot component: {p:?}");
    }
}

fuzz_target!(|input: (bool, String, String, Vec<Row>, Vec<Op>)| {
    let (save, dir, name, list, ops) = input;
    let mode = if save { PickMode::SaveAs } else { PickMode::Open };
    let mut p = Picker::new(mode, &dir, &name);
    p.set_entries(&dir, rows(list));
    let mut ask = CloseAsk::new();
    for op in ops.into_iter().take(MAX_OPS) {
        let ev = match op {
            Op::Key {
                code,
                ch,
                ctrl,
                shift,
                alt,
            } => p.key(KeyEvent::new(code_of(code, ch), Mods { ctrl, shift, alt })),
            Op::Click(i, double) => p.click(usize::from(i), double),
            Op::Wheel(n) => {
                p.scroll_by(isize::from(n));
                PickEvent::None
            }
            Op::Visible(n) => {
                p.set_visible(usize::from(n));
                PickEvent::None
            }
            Op::Entries(d, list) => {
                p.set_entries(&d, rows(list));
                check_path(p.dir());
                PickEvent::None
            }
            Op::Error(m) => {
                p.set_error(&m);
                PickEvent::None
            }
            Op::Ask(path) => {
                // The kernel only asks about paths it was given (absolute, normalized).
                p.confirm_overwrite(&kitsune_core::shell::fs::normalize("/", &path));
                PickEvent::None
            }
            Op::AskKey { code, ch } => {
                let _ = ask.key(KeyEvent::plain(code_of(code, ch)));
                PickEvent::None
            }
        };
        match &ev {
            PickEvent::Navigate(d) => check_path(d),
            PickEvent::Choose(f) | PickEvent::Overwrite(f) => check_path(f),
            _ => {}
        }
        let n = p.rows().len();
        assert!(n == 0 || p.selected() < n);
        assert!(p.scroll() <= n);
        let (text, caret) = p.field();
        assert!(caret <= text.chars().count());
        assert!(text.chars().count() <= MAX_FIELD);
        check_path(p.dir());
    }
});
