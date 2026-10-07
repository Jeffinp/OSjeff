//! Fuzz target: the v2 text editor (`osjeff_core::editor2`).
//!
//! The input is an initial document (arbitrary bytes, so invalid UTF-8 and
//! stray `\r` are covered) and a sequence of arbitrary operations: typing,
//! pasting, every cursor/selection movement, mouse clicks and drags, search and
//! replace, undo/redo, resizes down to 0x0, soft wrap, line numbers, tab width.
//!
//! After every operation `Editor::check_invariants` must hold: the cursor is a
//! valid position, the incremental line index equals a fresh rebuild, the
//! scroll position is inside the document, no drawn row is wider than the
//! window and the cursor is drawn inside it. At the end, undoing everything must
//! restore the initial text byte for byte, and redoing everything must return
//! the final text.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use osjeff_core::clipboard::Clipboard;
use osjeff_core::editor2::Editor;
use osjeff_core::input::{KeyCode, KeyEvent, Mods};

/// Keep the document and the work per input bounded.
const MAX_DOC: usize = 1 << 18;
const MAX_OPS: usize = 1500;

#[derive(Arbitrary, Debug)]
struct KeyArg {
    code: u8,
    ch: char,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

#[derive(Arbitrary, Debug)]
enum Op {
    Char(char),
    Str(String),
    Bytes(Vec<u8>),
    Key(KeyArg),
    Undo,
    Redo,
    Resize(u8, u8),
    Click {
        row: u8,
        col: u8,
        clicks: u8,
        shift: bool,
    },
    Drag(u8, u8),
    Scroll(i8),
    ScrollX(i16),
    SetSearch(String, bool),
    SetReplacement(String),
    FindNext,
    FindPrev,
    ReplaceOne,
    ReplaceAll,
    Goto(u32),
    SelectAll,
    SelectRange(u32, u32),
    SelectWord(u32),
    SelectLine(u32),
    SetCursor(u16, u16),
    Copy,
    Cut,
    Paste,
    SetText(Vec<u8>),
    SoftWrap(bool),
    LineNumbers(bool),
    TabWidth(u8),
    UseSpaces(bool),
    AutoIndent(bool),
    MarkSaved,
}

fn key_event(k: &KeyArg) -> KeyEvent {
    let code = match k.code % 22 {
        0 => KeyCode::Char(k.ch),
        1 => KeyCode::Enter,
        2 => KeyCode::Backspace,
        3 => KeyCode::Delete,
        4 => KeyCode::Tab,
        5 => KeyCode::Esc,
        6 => KeyCode::Left,
        7 => KeyCode::Right,
        8 => KeyCode::Up,
        9 => KeyCode::Down,
        10 => KeyCode::Home,
        11 => KeyCode::End,
        12 => KeyCode::PageUp,
        13 => KeyCode::PageDown,
        14 => KeyCode::F(3),
        15 => KeyCode::F(k.ch as u8),
        // Bias toward the shortcut letters: undo, redo, find, replace, goto,
        // select all/line, clipboard.
        16 => {
            KeyCode::Char(['z', 'y', 'f', 'h', 'g', 'a', 'l', 'x', 'c', 'v'][(k.ch as usize) % 10])
        }
        _ => KeyCode::Char(k.ch),
    };
    KeyEvent::new(
        code,
        Mods {
            ctrl: k.ctrl || (16..20).contains(&(k.code % 22)),
            shift: k.shift,
            alt: k.alt,
        },
    )
}

fuzz_target!(|input: (Vec<u8>, Vec<Op>)| {
    let (init, ops) = input;
    if init.len() > MAX_DOC {
        return;
    }
    let mut ed = Editor::from_bytes(&init);
    let mut baseline = init;
    let mut saved_used = false;
    let mut clip = Clipboard::new();

    for op in ops.into_iter().take(MAX_OPS) {
        let big = ed.len_bytes() > MAX_DOC;
        match op {
            Op::Char(c) => ed.insert_char(c),
            Op::Str(s) => {
                if !big {
                    ed.insert_str(&s)
                }
            }
            Op::Bytes(b) => {
                if !big {
                    ed.insert_bytes(&b)
                }
            }
            Op::Key(k) => {
                let _ = ed.handle_key(key_event(&k), &mut clip);
            }
            Op::Undo => {
                ed.undo();
            }
            Op::Redo => {
                ed.redo();
            }
            Op::Resize(r, c) => ed.resize(r as usize, c as usize),
            Op::Click {
                row,
                col,
                clicks,
                shift,
            } => ed.mouse_down(row as usize, col as usize, clicks, shift),
            Op::Drag(r, c) => ed.mouse_drag(r as usize, c as usize),
            Op::Scroll(d) => ed.scroll_by(d as isize),
            Op::ScrollX(d) => ed.scroll_x_by(d as isize),
            Op::SetSearch(q, case) => ed.set_search(&q, case),
            Op::SetReplacement(r) => ed.set_replacement(&r),
            Op::FindNext => {
                ed.find_next();
            }
            Op::FindPrev => {
                ed.find_prev();
            }
            Op::ReplaceOne => {
                if !big {
                    ed.replace_current();
                }
            }
            Op::ReplaceAll => {
                if ed.len_bytes() < MAX_DOC / 4 {
                    ed.replace_all();
                }
            }
            Op::Goto(n) => ed.goto_line(n as usize),
            Op::SelectAll => ed.select_all(),
            Op::SelectRange(a, b) => ed.select_range(a as usize, b as usize),
            Op::SelectWord(p) => ed.select_word_at(p as usize),
            Op::SelectLine(p) => ed.select_line_at(p as usize),
            Op::SetCursor(l, c) => ed.set_cursor(l as usize, c as usize),
            Op::Copy => {
                ed.copy(&mut clip);
            }
            Op::Cut => {
                ed.cut(&mut clip);
            }
            Op::Paste => {
                if !big {
                    ed.paste(&clip);
                }
            }
            Op::SetText(b) => {
                if b.len() <= MAX_DOC {
                    ed.set_text(&b);
                    baseline = b;
                    saved_used = false;
                }
            }
            Op::SoftWrap(on) => ed.set_soft_wrap(on),
            Op::LineNumbers(on) => ed.set_line_numbers(on),
            Op::TabWidth(w) => ed.set_tab_width(w as usize),
            Op::UseSpaces(on) => ed.set_use_spaces(on),
            Op::AutoIndent(on) => ed.set_auto_indent(on),
            Op::MarkSaved => {
                ed.mark_saved();
                saved_used = true;
            }
        }
        if let Err(msg) = ed.check_invariants() {
            panic!("invariant broken: {msg}");
        }
    }

    // Undo everything: the initial text comes back byte for byte.
    let end = ed.to_bytes();
    let mut undone = 0u32;
    while ed.undo() {
        undone += 1;
        assert!(undone < 5_000_000, "undo does not terminate");
    }
    assert_eq!(ed.to_bytes(), baseline, "undo did not restore the text");
    assert!(ed.check_invariants().is_ok());
    if !saved_used {
        assert!(!ed.is_modified(), "clean state not recognised");
    }
    // Redo exactly what was undone (earlier undo ops may have left more).
    for _ in 0..undone {
        assert!(ed.redo(), "redo ran out early");
    }
    assert_eq!(ed.to_bytes(), end, "redo did not restore the text");
    if let Err(msg) = ed.check_invariants() {
        panic!("invariant broken after redo: {msg}");
    }
});
