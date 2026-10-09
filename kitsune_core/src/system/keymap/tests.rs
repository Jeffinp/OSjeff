use super::*;

fn down(km: &mut Keymap, scan: u8) -> Option<Key> {
    km.process(scan, false, true)
}

#[test]
fn lowercase_by_default() {
    let mut km = Keymap::new();
    assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
    assert_eq!(down(&mut km, 0x32), Some(Key::Char(b'm')));
}

#[test]
fn shift_uppercases_letters() {
    let mut km = Keymap::new();
    assert_eq!(km.process(0x2A, false, true), None); // shift down
    assert!(km.shift());
    assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'A')));
    assert_eq!(km.process(0x2A, false, false), None); // shift up
    assert!(!km.shift());
    assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
}

#[test]
fn right_shift_also_works() {
    let mut km = Keymap::new();
    km.process(0x36, false, true);
    assert_eq!(down(&mut km, 0x1F), Some(Key::Char(b'S')));
}

#[test]
fn alt_tracks_press_and_release_and_tab_still_translates() {
    let mut km = Keymap::new();
    assert!(!km.alt());
    assert_eq!(km.process(0x38, false, true), None); // left alt down
    assert!(km.alt());
    assert_eq!(down(&mut km, 0x0F), Some(Key::Tab)); // desktop reads alt()
    assert_eq!(km.process(0x38, false, false), None);
    assert!(!km.alt());
    assert_eq!(km.process(0x38, true, true), None); // right alt (extended)
    assert!(km.alt());
    assert_eq!(km.process(0x38, true, false), None);
    assert!(!km.alt());
}

#[test]
fn ctrl_tracks_press_and_release() {
    let mut km = Keymap::new();
    assert!(!km.ctrl());
    assert_eq!(km.process(0x1D, false, true), None); // left ctrl down
    assert!(km.ctrl());
    // The letter still translates; the desktop reads ctrl() to intercept it.
    assert_eq!(down(&mut km, 0x2E), Some(Key::Char(b'c')));
    assert_eq!(km.process(0x1D, true, false), None); // right ctrl up (extended)
    assert!(!km.ctrl());
}

#[test]
fn caps_lock_toggles_letters_only() {
    let mut km = Keymap::new();
    km.process(0x3A, false, true); // caps on
    assert!(km.caps());
    assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'A')));
    // digit unaffected by caps
    assert_eq!(down(&mut km, 0x02), Some(Key::Char(b'1')));
    km.process(0x3A, false, true); // caps off
    assert!(!km.caps());
    assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
}

#[test]
fn shift_plus_caps_is_lowercase_letter() {
    let mut km = Keymap::new();
    km.process(0x3A, false, true); // caps on
    km.process(0x2A, false, true); // shift on
    assert_eq!(down(&mut km, 0x1E), Some(Key::Char(b'a')));
}

#[test]
fn shifted_symbols() {
    let mut km = Keymap::new();
    km.process(0x2A, false, true);
    assert_eq!(down(&mut km, 0x02), Some(Key::Char(b'!')));
    assert_eq!(down(&mut km, 0x0C), Some(Key::Char(b'_')));
    assert_eq!(down(&mut km, 0x35), Some(Key::Char(b'?')));
    assert_eq!(down(&mut km, 0x34), Some(Key::Char(b'>')));
}

#[test]
fn control_keys() {
    let mut km = Keymap::new();
    assert_eq!(down(&mut km, 0x1C), Some(Key::Enter));
    assert_eq!(down(&mut km, 0x0E), Some(Key::Backspace));
    assert_eq!(down(&mut km, 0x0F), Some(Key::Tab));
    assert_eq!(down(&mut km, 0x01), Some(Key::Esc));
    assert_eq!(down(&mut km, 0x39), Some(Key::Char(b' ')));
}

#[test]
fn extended_arrows_and_edit_keys() {
    let mut km = Keymap::new();
    assert_eq!(km.process(0x48, true, true), Some(Key::Up));
    assert_eq!(km.process(0x50, true, true), Some(Key::Down));
    assert_eq!(km.process(0x4B, true, true), Some(Key::Left));
    assert_eq!(km.process(0x4D, true, true), Some(Key::Right));
    assert_eq!(km.process(0x47, true, true), Some(Key::Home));
    assert_eq!(km.process(0x4F, true, true), Some(Key::End));
    assert_eq!(km.process(0x53, true, true), Some(Key::Delete));
}

#[test]
fn key_up_produces_nothing() {
    let mut km = Keymap::new();
    assert_eq!(km.process(0x1E, false, false), None);
}

#[test]
fn unmapped_scancode_is_none() {
    let mut km = Keymap::new();
    assert_eq!(down(&mut km, 0x7E), None);
    assert_eq!(km.process(0x99, true, true), None);
}

#[test]
fn default_impl_matches_new() {
    let km = Keymap::default();
    assert!(!km.shift() && !km.caps());
}

const MODIFIERS: [u8; 3] = [0x2A, 0x36, 0x3A];

#[test]
fn whole_table_translates_both_cases() {
    // Exercise every mapped scancode unshifted and shifted (skipping the
    // modifier keys so caps/shift state stays controlled), asserting the
    // shifted form differs for everything except space.
    let mut shifted_km = Keymap::new();
    shifted_km.process(0x2A, false, true); // shift held

    for scan in 0x00u8..0x60 {
        if MODIFIERS.contains(&scan) {
            continue;
        }
        let unshifted = Keymap::new().process(scan, false, true);
        let shifted = shifted_km.process(scan, false, true);

        if let (Some(Key::Char(u)), Some(Key::Char(s))) = (unshifted, shifted) {
            assert!(u.is_ascii_graphic() || u == b' ');
            if u != b' ' {
                assert_ne!(u, s, "scan {:#x} should change under shift", scan);
            }
        }
    }
}

// ---------------------------------------------------------------- ABNT2

fn abnt2() -> Keymap {
    let mut km = Keymap::new();
    km.set_layout(Layout::Abnt2);
    km
}

fn type_keys(km: &mut Keymap, scans: &[(u8, bool)]) -> alloc::vec::Vec<u8> {
    // (scancode, shifted) pairs; returns the typed bytes.
    let mut out = alloc::vec::Vec::new();
    for &(scan, shifted) in scans {
        if shifted {
            km.process(0x2A, false, true);
        }
        if let Some(Key::Char(c)) = km.process(scan, false, true) {
            out.push(c);
        }
        while let Some(Key::Char(c)) = km.take_pending() {
            out.push(c);
        }
        if shifted {
            km.process(0x2A, false, false);
        }
    }
    out
}

#[test]
fn layout_names() {
    assert_eq!(Layout::from_name(b"abnt2"), Some(Layout::Abnt2));
    assert_eq!(Layout::from_name(b"US"), Some(Layout::Us));
    assert_eq!(Layout::from_name(b"dvorak"), None);
    assert_eq!(Layout::default(), Layout::Us);
    assert_eq!(Keymap::new().layout(), Layout::Us);
}

#[test]
fn abnt2_c_cedilla() {
    let mut km = abnt2();
    assert_eq!(type_keys(&mut km, &[(0x27, false)]), [0xE7]); // ç
    assert_eq!(type_keys(&mut km, &[(0x27, true)]), [0xC7]); // Ç
    km.process(0x3A, false, true); // caps lock
    assert_eq!(type_keys(&mut km, &[(0x27, false)]), [0xC7]);
    // The US layout types `;` / `:` there.
    let mut us = Keymap::new();
    assert_eq!(type_keys(&mut us, &[(0x27, false), (0x27, true)]), *b";:");
}

#[test]
fn abnt2_differing_symbols() {
    let mut km = abnt2();
    let cases: [(u8, bool, u8); 12] = [
        (0x03, true, b'@'),
        (0x29, false, b'\''),
        (0x29, true, b'"'),
        (0x1B, false, b'['),
        (0x1B, true, b'{'),
        (0x2B, false, b']'),
        (0x2B, true, b'}'),
        (0x56, false, b'\\'),
        (0x56, true, b'|'),
        (0x35, false, b';'),
        (0x73, false, b'/'),
        (0x73, true, b'?'),
    ];
    for (scan, shifted, want) in cases {
        assert_eq!(
            type_keys(&mut km, &[(scan, shifted)]),
            [want],
            "scan {scan:#x}"
        );
    }
    // Shared keys are untouched: digits, comma, period.
    assert_eq!(
        type_keys(&mut km, &[(0x02, false), (0x33, false), (0x34, true)]),
        *b"1,>"
    );
    // 6 is a digit unshifted (the shifted form is a dead diaeresis).
    assert_eq!(type_keys(&mut km, &[(0x07, false)]), *b"6");
}

#[test]
fn abnt2_dead_accents_combine() {
    let mut km = abnt2();
    // acute + a/e/i/o/u
    for (letter, want) in [
        (0x1E, 0xE1),
        (0x12, 0xE9),
        (0x17, 0xED),
        (0x18, 0xF3),
        (0x16, 0xFA),
    ] {
        assert_eq!(
            type_keys(&mut km, &[(0x1A, false), (letter, false)]),
            [want]
        );
    }
    // grave + a, tilde + a/o/n, circumflex + e, diaeresis + u
    assert_eq!(type_keys(&mut km, &[(0x1A, true), (0x1E, false)]), [0xE0]);
    assert_eq!(type_keys(&mut km, &[(0x28, false), (0x1E, false)]), [0xE3]);
    assert_eq!(type_keys(&mut km, &[(0x28, false), (0x18, false)]), [0xF5]);
    assert_eq!(type_keys(&mut km, &[(0x28, false), (0x31, false)]), [0xF1]);
    assert_eq!(type_keys(&mut km, &[(0x28, true), (0x12, false)]), [0xEA]);
    assert_eq!(type_keys(&mut km, &[(0x07, true), (0x16, false)]), [0xFC]);
    // Uppercase: acute + Shift+A, tilde + Shift+O.
    assert_eq!(type_keys(&mut km, &[(0x1A, false), (0x1E, true)]), [0xC1]);
    assert_eq!(type_keys(&mut km, &[(0x28, false), (0x18, true)]), [0xD5]);
}

#[test]
fn abnt2_dead_accent_edge_cases() {
    let mut km = abnt2();
    // Accent + space types the accent alone.
    assert_eq!(type_keys(&mut km, &[(0x1A, false), (0x39, false)]), [0xB4]);
    assert_eq!(type_keys(&mut km, &[(0x28, false), (0x39, false)]), *b"~");
    assert_eq!(type_keys(&mut km, &[(0x28, true), (0x39, false)]), *b"^");
    // Accent + a letter it cannot combine with: both come out, in order.
    assert_eq!(
        type_keys(&mut km, &[(0x1A, false), (0x2D, false)]),
        [0xB4, b'x']
    );
    // The same dead key twice types the accent once.
    assert_eq!(type_keys(&mut km, &[(0x1A, false), (0x1A, false)]), [0xB4]);
    // A different dead key flushes the first and arms the second.
    assert_eq!(
        type_keys(&mut km, &[(0x1A, false), (0x28, false), (0x1E, false)]),
        [0xB4, 0xE3]
    );
    // Enter cancels a pending accent (and still passes through).
    km.process(0x1A, false, true);
    assert_eq!(km.process(0x1C, false, true), Some(Key::Enter));
    assert_eq!(type_keys(&mut km, &[(0x1E, false)]), *b"a");
    // Switching layout drops a pending accent.
    km.process(0x1A, false, true);
    km.set_layout(Layout::Us);
    assert_eq!(type_keys(&mut km, &[(0x1E, false)]), *b"a");
}

#[test]
fn abnt2_letters_and_modifiers_unchanged() {
    let mut km = abnt2();
    assert_eq!(type_keys(&mut km, &[(0x1E, false), (0x1E, true)]), *b"aA");
    // Ctrl+C still arrives as 'c' for the desktop shortcut.
    km.process(0x1D, false, true);
    assert_eq!(type_keys(&mut km, &[(0x2E, false)]), *b"c");
    assert!(km.ctrl());
}

#[test]
fn us_layout_never_produces_latin1() {
    let mut km = Keymap::new();
    for scan in 0x00u8..0x80 {
        if MODIFIERS.contains(&scan) || scan == 0x1D || scan == 0x38 {
            continue;
        }
        if let Some(Key::Char(c)) = km.process(scan, false, true) {
            assert!(c < 0x80, "scan {scan:#x}");
        }
        assert_eq!(km.take_pending(), None);
    }
}
