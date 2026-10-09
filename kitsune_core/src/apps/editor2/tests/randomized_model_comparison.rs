use super::*;

#[test]
fn model_comparison_empty_start() {
    for seed in 1..=20u64 {
        random_session(seed * 7919, 600, "");
    }
}

#[test]
fn model_comparison_multibyte_start() {
    for seed in 1..=20u64 {
        random_session(seed * 104_729, 600, "héllo wörld\n€uro\n\n😀 end");
    }
}

#[test]
fn model_comparison_long_run() {
    random_session(0xDEADBEEF, 6000, "seed text\nsecond line\n");
}

#[test]
fn random_keys_never_break_invariants() {
    let codes = [
        KeyCode::Char('a'),
        KeyCode::Char('Z'),
        KeyCode::Char(' '),
        KeyCode::Char('é'),
        KeyCode::Char('f'),
        KeyCode::Char('g'),
        KeyCode::Char('h'),
        KeyCode::Char('z'),
        KeyCode::Char('y'),
        KeyCode::Char('x'),
        KeyCode::Char('v'),
        KeyCode::Char('c'),
        KeyCode::Char('9'),
        KeyCode::Enter,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Tab,
        KeyCode::Esc,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::F(3),
        KeyCode::F(5),
    ];
    for seed in 1..=15u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut e = Editor::from_bytes("a\tb\r\nc€\n\n😀 word word".as_bytes());
        let mut clip = Clipboard::new();
        e.resize(1 + rng.below(8), 1 + rng.below(25));
        e.set_line_numbers(rng.below(2) == 0);
        e.set_soft_wrap(rng.below(2) == 0);
        for _ in 0..1500 {
            let code = codes[rng.below(codes.len())];
            let r = rng.below(8);
            let mods = Mods {
                ctrl: r & 1 != 0,
                shift: r & 2 != 0,
                alt: r & 4 != 0 && rng.below(4) == 0,
            };
            let ev = e.handle_key(KeyEvent::new(code, mods), &mut clip);
            if ev == Event::SaveRequested {
                e.mark_saved();
            }
            if rng.below(40) == 0 {
                e.resize(rng.below(10), rng.below(30));
            }
            if rng.below(30) == 0 {
                e.mouse_down(rng.below(12), rng.below(35), 1 + rng.below(3) as u8, false);
            }
            check(&e);
            for row in e.visible_rows() {
                assert!(row.cells().count() <= e.text_cols());
            }
            if let Some((r, c)) = e.cursor_screen() {
                let (rows, cols) = e.viewport();
                assert!(r < rows && c < cols);
            }
        }
        while e.undo() {}
        while e.redo() {}
        check(&e);
    }
}

#[test]
fn undo_redo_roundtrip_property() {
    let mut rng = Rng(424242);
    let mut e = Editor::new();
    for _ in 0..800 {
        match rng.below(6) {
            0 | 1 => e.insert_char(ALPHABET[rng.below(ALPHABET.len())]),
            2 => e.newline(),
            3 => e.backspace(),
            4 => e.move_left(false),
            _ => e.delete_forward(),
        }
    }
    let end = e.to_bytes();
    let k = 1 + rng.below(50);
    let mut steps = 0;
    for _ in 0..k {
        if e.undo() {
            steps += 1;
        }
    }
    for _ in 0..steps {
        assert!(e.redo());
    }
    assert_eq!(e.to_bytes(), end);
}

#[test]
fn random_replace_and_search_stay_consistent() {
    let mut rng = Rng(777);
    let mut e = ed("abcabc ABCabc\nabc");
    for _ in 0..300 {
        match rng.below(6) {
            0 => {
                e.set_search(
                    ["a", "bc", "abc", "é", "C"][rng.below(5)],
                    rng.below(2) == 0,
                );
                e.find_next();
            }
            1 => {
                e.find_prev();
            }
            2 => {
                e.set_replacement(["", "x", "éé", "\n", "abc"][rng.below(5)]);
                e.replace_current();
            }
            3 => {
                if rng.below(10) == 0 {
                    e.replace_all();
                }
            }
            4 => {
                e.undo();
            }
            _ => {
                e.redo();
            }
        }
        check(&e);
    }
}

#[test]
fn modified_flag_matches_text_difference_after_random_undo_redo() {
    let mut rng = Rng(99);
    let base = "start\ntext";
    let mut e = ed(base);
    for i in 0..500 {
        match rng.below(5) {
            0 => e.insert_char('k'),
            1 => e.backspace(),
            2 => {
                e.undo();
            }
            3 => {
                e.redo();
            }
            _ => {
                if i % 50 == 0 {
                    e.mark_saved();
                }
                e.move_right(false);
            }
        }
    }
    // Undoing everything lands on the initial text: clean only if the save
    // point was never moved.
    while e.undo() {}
    assert_eq!(text(&e), base);
}

#[test]
fn check_invariants_hold_on_fresh_and_edited_editors() {
    assert_eq!(Editor::new().check_invariants(), Ok(()));
    let mut e = ed("héllo\r\nwörld\n\tx");
    e.set_soft_wrap(true);
    e.resize(3, 7);
    e.move_doc_end(false);
    type_str(&mut e, "€€€");
    assert_eq!(e.check_invariants(), Ok(()));
    e.resize(0, 0);
    assert_eq!(e.check_invariants(), Ok(()));
}
