//! Tests of the file manager logic.

use super::*;
use crate::blockdev::RamDisk;
use crate::fs3::{FormatOptions, Fs3};
use alloc::vec;

const NOW: u64 = 1_700_000_000;

fn fresh() -> Fs3<RamDisk> {
    Fs3::format(
        RamDisk::new(8 * 2048),
        &FormatOptions::new(*b"0123456789abcdef", NOW),
    )
    .unwrap()
}

fn row(name: &str, dir: bool, size: u64, mtime: u64) -> Row {
    Row {
        name: name.as_bytes().to_vec(),
        kind: if dir { EntryKind::Dir } else { EntryKind::File },
        size,
        mtime,
        id: Vec::new(),
        installed: false,
    }
}

fn names(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|r| String::from_utf8_lossy(&r.name).into_owned())
        .collect()
}

// ---- natural order ----

#[test]
fn natural_order_compares_numbers_by_value() {
    assert_eq!(natural_cmp(b"f2", b"f10"), Ordering::Less);
    assert_eq!(natural_cmp(b"f10", b"f2"), Ordering::Greater);
    assert_eq!(natural_cmp(b"f2", b"f2"), Ordering::Equal);
    assert_eq!(natural_cmp(b"a1b", b"a1c"), Ordering::Less);
}

#[test]
fn natural_order_ignores_case_but_is_total() {
    assert_eq!(natural_cmp(b"apple", b"Banana"), Ordering::Less);
    assert_ne!(natural_cmp(b"abc", b"ABC"), Ordering::Equal);
    assert_eq!(
        natural_cmp(b"abc", b"ABC").reverse(),
        natural_cmp(b"ABC", b"abc")
    );
}

#[test]
fn natural_order_leading_zeros_and_prefixes() {
    assert_eq!(natural_cmp(b"a01", b"a1"), Ordering::Greater);
    assert_eq!(natural_cmp(b"a1", b"a01"), Ordering::Less);
    assert_eq!(natural_cmp(b"a007", b"a8"), Ordering::Less);
    assert_eq!(natural_cmp(b"a", b"ab"), Ordering::Less);
    assert_eq!(natural_cmp(b"", b"a"), Ordering::Less);
    assert_eq!(natural_cmp(b"000", b"00"), Ordering::Greater);
}

#[test]
fn natural_order_handles_huge_digit_runs() {
    let a = [b'9'; 40];
    let mut b = vec![b'1'];
    b.extend_from_slice(&[b'0'; 40]);
    assert_eq!(natural_cmp(&a, &b), Ordering::Less);
}

// ---- sorting ----

#[test]
fn folders_come_first_in_every_sort() {
    let mut rows = vec![
        row("b.txt", false, 5, 10),
        row("zdir", true, 0, 5),
        row("a.txt", false, 1, 20),
        row("adir", true, 0, 50),
    ];
    for key in [SortKey::Name, SortKey::Size, SortKey::Modified] {
        for asc in [true, false] {
            sort_rows(&mut rows, Sort { key, asc });
            assert!(rows[0].is_dir() && rows[1].is_dir(), "{key:?} {asc}");
            assert!(!rows[2].is_dir() && !rows[3].is_dir());
        }
    }
}

#[test]
fn sort_by_name_ascending_and_descending() {
    let mut rows = vec![
        row("file10", false, 0, 0),
        row("file2", false, 0, 0),
        row("File1", false, 0, 0),
    ];
    sort_rows(&mut rows, Sort::DEFAULT);
    assert_eq!(names(&rows), ["File1", "file2", "file10"]);
    sort_rows(
        &mut rows,
        Sort {
            key: SortKey::Name,
            asc: false,
        },
    );
    assert_eq!(names(&rows), ["file10", "file2", "File1"]);
}

#[test]
fn sort_by_size_breaks_ties_by_name() {
    let mut rows = vec![
        row("c", false, 100, 0),
        row("b", false, 5, 0),
        row("a", false, 100, 0),
    ];
    sort_rows(
        &mut rows,
        Sort {
            key: SortKey::Size,
            asc: true,
        },
    );
    assert_eq!(names(&rows), ["b", "a", "c"]);
    sort_rows(
        &mut rows,
        Sort {
            key: SortKey::Size,
            asc: false,
        },
    );
    assert_eq!(names(&rows), ["a", "c", "b"]);
}

#[test]
fn sort_by_modified() {
    let mut rows = vec![
        row("old", false, 0, 1),
        row("new", false, 0, 9),
        row("mid", false, 0, 5),
    ];
    sort_rows(
        &mut rows,
        Sort {
            key: SortKey::Modified,
            asc: false,
        },
    );
    assert_eq!(names(&rows), ["new", "mid", "old"]);
}

#[test]
fn header_click_flips_then_switches() {
    let mut s = Sort::DEFAULT;
    s.click(SortKey::Name);
    assert!(!s.asc);
    s.click(SortKey::Size);
    assert_eq!((s.key, s.asc), (SortKey::Size, true));
    s.click(SortKey::Size);
    assert!(!s.asc);
}

// ---- selection ----

fn sel(n: usize) -> Selection {
    let mut s = Selection::new();
    s.reset(n);
    s
}

#[test]
fn plain_click_selects_one() {
    let mut s = sel(5);
    s.click(2, false, false);
    assert_eq!(s.selected(), vec![2]);
    s.click(4, false, false);
    assert_eq!(s.selected(), vec![4]);
    assert_eq!(s.count(), 1);
    assert_eq!(s.cursor(), 4);
}

#[test]
fn ctrl_click_toggles() {
    let mut s = sel(5);
    s.click(1, true, false);
    s.click(3, true, false);
    assert_eq!(s.selected(), vec![1, 3]);
    s.click(1, true, false);
    assert_eq!(s.selected(), vec![3]);
    assert_eq!(s.count(), 1);
}

#[test]
fn shift_click_selects_a_range_from_the_anchor() {
    let mut s = sel(10);
    s.click(2, false, false);
    s.click(5, false, true);
    assert_eq!(s.selected(), vec![2, 3, 4, 5]);
    s.click(0, false, true);
    assert_eq!(s.selected(), vec![0, 1, 2]);
}

#[test]
fn ctrl_shift_click_adds_the_range() {
    let mut s = sel(10);
    s.click(1, false, false);
    s.click(5, true, false);
    s.click(8, true, true);
    assert_eq!(s.selected(), vec![1, 5, 6, 7, 8]);
}

#[test]
fn select_all_and_clear() {
    let mut s = sel(4);
    s.select_all();
    assert_eq!(s.count(), 4);
    assert_eq!(s.selected(), vec![0, 1, 2, 3]);
    s.clear();
    assert_eq!(s.count(), 0);
}

#[test]
fn arrow_keys_move_and_shift_extends() {
    let mut s = sel(6);
    s.move_cursor(1, false);
    assert_eq!(s.selected(), vec![1]);
    s.move_cursor(2, true);
    assert_eq!(s.selected(), vec![1, 2, 3]);
    s.move_cursor(-1, true);
    assert_eq!(s.selected(), vec![1, 2]);
    s.move_cursor(100, false);
    assert_eq!(s.selected(), vec![5]);
    s.move_cursor(-100, false);
    assert_eq!(s.selected(), vec![0]);
}

#[test]
fn selection_ignores_out_of_range_and_empty() {
    let mut s = sel(3);
    s.click(7, false, false);
    assert_eq!(s.count(), 0);
    let mut e = sel(0);
    e.move_cursor(1, false);
    e.only(0);
    e.select_all();
    assert_eq!(e.count(), 0);
    assert!(e.is_empty());
    assert!(!e.is_selected(0));
}

#[test]
fn selection_count_matches_the_mask_under_random_clicks() {
    let mut s = sel(50);
    let mut x = 12345u32;
    for _ in 0..500 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let i = (x >> 8) as usize % 50;
        s.click(i, x & 1 != 0, x & 2 != 0);
        assert_eq!(s.count(), s.selected().len());
    }
}

// ---- breadcrumbs, history ----

#[test]
fn breadcrumbs_of_paths() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let c = breadcrumbs(b"/");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].label, b"Disco");
    let c = breadcrumbs(b"/a/b c/d");
    let labels: Vec<_> = c.iter().map(|x| x.label.clone()).collect();
    assert_eq!(
        labels,
        vec![
            b"Disco".to_vec(),
            b"a".to_vec(),
            b"b c".to_vec(),
            b"d".to_vec()
        ]
    );
    let paths: Vec<_> = c.iter().map(|x| x.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            b"/".to_vec(),
            b"/a".to_vec(),
            b"/a/b c".to_vec(),
            b"/a/b c/d".to_vec()
        ]
    );
}

#[test]
fn breadcrumbs_of_the_trash() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let c = breadcrumbs(TRASH_PATH);
    assert_eq!(c.len(), 2);
    assert_eq!(c[1].label, b"Lixeira");
    assert_eq!(c[1].path, TRASH_PATH);
}

#[test]
fn history_back_and_forward() {
    let mut h = History::new(b"/");
    assert!(!h.can_back() && !h.can_forward());
    h.push(b"/a");
    h.push(b"/a/b");
    assert_eq!(h.current(), b"/a/b");
    assert_eq!(h.back(), Some(&b"/a"[..]));
    assert_eq!(h.back(), Some(&b"/"[..]));
    assert_eq!(h.back(), None);
    assert_eq!(h.forward(), Some(&b"/a"[..]));
    h.push(b"/c");
    assert!(!h.can_forward());
    assert_eq!(h.back(), Some(&b"/a"[..]));
}

#[test]
fn history_ignores_a_repeat_and_is_bounded() {
    let mut h = History::new(b"/");
    h.push(b"/");
    assert!(!h.can_back());
    for i in 0..200 {
        let p = alloc::format!("/d{i}");
        h.push(p.as_bytes());
    }
    let mut steps = 0;
    while h.back().is_some() {
        steps += 1;
    }
    assert!(steps <= HISTORY_MAX);
}

// ---- formatting ----

#[test]
fn sizes_are_formatted_with_binary_units() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1023), "1023 B");
    assert_eq!(format_size(1024), "1,0 KiB");
    assert_eq!(format_size(1536), "1,5 KiB");
    assert_eq!(format_size(3 * 1024 * 1024), "3,0 MiB");
    assert_eq!(format_size(1024 * 1024 - 1), "1023,9 KiB");
    assert_eq!(
        format_size(5 * 1024 * 1024 * 1024 + 512 * 1024 * 1024),
        "5,5 GiB"
    );
    assert_eq!(format_size(u64::MAX), "16777215,9 TiB");
}

#[test]
fn civil_dates() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(59), (1970, 3, 1));
    assert_eq!(civil_from_days(10_957), (2000, 1, 1));
    assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // leap day
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
}

#[test]
fn datetimes_are_local() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert_eq!(format_datetime(0, 0, true), "--");
    assert_eq!(format_datetime(1_700_000_000, 0, true), "14/11/2023 22:13");
    assert_eq!(
        format_datetime(1_700_000_000, -3 * 3600, true),
        "14/11/2023 19:13"
    );
    // Crossing midnight backwards.
    assert_eq!(
        format_datetime(86_400 + 60, -3600, true),
        "01/01/1970 23:01"
    );
}

#[test]
fn datetimes_follow_the_language_and_the_clock() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let t = 1_700_000_000; // 2023-11-14 22:13 UTC, a Tuesday
    {
        let _g = LangGuard::new(Lang::En);
        assert_eq!(format_datetime(t, 0, false), "11/14/2023 10:13 PM");
        assert_eq!(format_datetime(t, 0, true), "11/14/2023 22:13");
        assert_eq!(format_datetime(0, 0, false), "--");
        // 1970-01-01 00:01 local (the first minute of the clock's epoch) keeps its AM.
        assert_eq!(format_datetime(60, 0, false), "01/01/1970 12:01 AM");
    }
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(format_datetime(t, 0, false), "14/11/2023 10:13 PM");
    assert_eq!(format_datetime(t, 0, true), "14/11/2023 22:13");
}

#[test]
fn sizes_follow_the_language() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let big = 1234 * 1024 + 512 * 1024 / 10; // 1234,05 KiB -> 1,2 MiB
    {
        let _g = LangGuard::new(Lang::En);
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(big), "1.2 MiB");
        assert_eq!(format_size(1024 * 1024 - 1), "1023.9 KiB");
    }
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(format_size(1536), "1,5 KiB");
    assert_eq!(format_size(big), "1,2 MiB");
}

#[test]
fn display_folds_accents_and_unknowns() {
    assert_eq!(display_ascii("relatório.txt".as_bytes()), b"relatorio.txt");
    assert_eq!(display_ascii("AÇÃO".as_bytes()), b"ACAO");
    assert_eq!(display_ascii("日本.png".as_bytes()), b"??.png");
    assert_eq!(display_ascii(&[b'a', 0xFF, b'b']), b"a?b");
    assert_eq!(display_ascii(b"a\x01b"), b"a?b");
    assert_eq!(display_ascii(&[0xE6, 0x97]), b"?"); // truncated sequence
    for name in ["ñandú", "über", "naïve café", "😀x"] {
        assert!(display_ascii(name.as_bytes()).len() <= name.len());
    }
}

#[test]
fn ellipsize_cuts_long_text() {
    assert_eq!(ellipsize(b"short", 10), b"short");
    assert_eq!(ellipsize(b"0123456789", 8), b"01234...");
    assert_eq!(ellipsize(b"0123456789", 3), b"012");
    assert_eq!(ellipsize(b"0123456789", 10), b"0123456789");
}

// ---- text input ----

#[test]
fn text_input_edits() {
    let mut t = TextInput::new(b"abc", 255);
    assert_eq!(t.text(), b"abc");
    t.insert(b'd');
    t.left();
    t.left();
    t.insert(b'X');
    assert_eq!(t.text(), b"abXcd");
    t.backspace();
    assert_eq!(t.text(), b"abcd");
    t.delete();
    assert_eq!(t.text(), b"abd");
    t.home();
    t.backspace();
    assert_eq!(t.text(), b"abd");
    t.end();
    t.delete();
    assert_eq!(t.text(), b"abd");
    assert_eq!(t.caret(), 3);
    t.clear();
    assert_eq!(t.text(), b"");
}

#[test]
fn text_input_refuses_slash_controls_and_overflow() {
    let mut t = TextInput::new(b"", 4);
    for b in *b"a/b\n\0cdef" {
        t.insert(b);
    }
    assert_eq!(t.text(), b"abcd");
    let t = TextInput::new(b"abcdef", 3);
    assert_eq!(t.text(), b"abc");
}

#[test]
fn text_input_moves_over_utf8_characters() {
    let mut t = TextInput::new("aé日".as_bytes(), 255);
    t.left();
    assert_eq!(t.caret(), 3); // before 日
    t.left();
    assert_eq!(t.caret(), 1); // before é
    t.right();
    assert_eq!(t.caret(), 3);
    t.backspace();
    assert_eq!(t.text(), "a日".as_bytes());
    t.delete();
    assert_eq!(t.text(), b"a");
    assert_eq!(t.caret_column(), 1);
}

// ---- clipboard ----

#[test]
fn path_clip_copy_and_cut() {
    let mut c = PathClip::new();
    assert!(c.is_empty());
    c.set(vec![b"/a".to_vec()], false);
    assert!(!c.is_cut_path(b"/a"));
    c.after_paste();
    assert!(!c.is_empty()); // a copy can be pasted again
    c.set(vec![b"/a".to_vec(), b"/b".to_vec()], true);
    assert!(c.is_cut_path(b"/b"));
    assert!(!c.is_cut_path(b"/c"));
    c.after_paste();
    assert!(c.is_empty() && !c.is_cut());
}

// ---- classification ----

#[test]
fn files_are_classified_by_extension() {
    assert_eq!(classify(b"a.PNG"), FileClass::Image);
    assert_eq!(classify(b"a.bmp"), FileClass::Image);
    assert_eq!(classify(b"x.ppm"), FileClass::Image);
    assert_eq!(classify(b"app.wasm"), FileClass::Wasm);
    assert_eq!(classify(b"notes.txt"), FileClass::Text);
    assert_eq!(classify(b"Makefile"), FileClass::Text);
    assert_eq!(classify(b"a.xyz"), FileClass::Other);
    assert!(is_image(b"foto.png"));
    assert!(!is_image(b"foto.png.txt"));
}

#[test]
fn text_sniffing() {
    assert!(looks_like_text(b"hello\nworld\t!"));
    assert!(looks_like_text("relatório".as_bytes()));
    assert!(!looks_like_text(b"abc\0def"));
    assert!(!looks_like_text(&[1u8; 100]));
    assert!(looks_like_text(b""));
}

// ---- context menu ----

#[test]
fn context_menu_adapts_to_the_selection() {
    let ctx = |in_trash, selected, image, clip| MenuCtx {
        in_trash,
        in_apps: false,
        app_installed: false,
        selected,
        image,
        clip_has_items: clip,
    };
    let cmds = |m: Vec<(Cmd, &str)>| m.into_iter().map(|(c, _)| c).collect::<Vec<_>>();
    let blank = cmds(context_menu(ctx(false, 0, false, false)));
    assert!(blank.contains(&Cmd::NewFolder) && !blank.contains(&Cmd::Paste));
    let blank = cmds(context_menu(ctx(false, 0, false, true)));
    assert!(blank.contains(&Cmd::Paste));
    let one = cmds(context_menu(ctx(false, 1, true, false)));
    assert!(one.contains(&Cmd::Rename) && one.contains(&Cmd::SetWallpaper));
    let two = cmds(context_menu(ctx(false, 2, false, false)));
    assert!(!two.contains(&Cmd::Rename) && !two.contains(&Cmd::Open));
    let trash = cmds(context_menu(ctx(true, 1, false, false)));
    assert!(trash.contains(&Cmd::Restore) && trash.contains(&Cmd::EmptyTrash));
    assert!(!trash.contains(&Cmd::NewFile));
    for m in [
        context_menu(ctx(false, 0, false, true)),
        context_menu(ctx(false, 1, true, false)),
        context_menu(ctx(true, 1, false, false)),
    ] {
        assert!(m.iter().all(|(_, l)| !l.is_empty()));
        // Entries come grouped: the group number never goes back.
        let groups: Vec<u8> = m.iter().map(|(c, _)| c.group()).collect();
        let mut sorted = groups.clone();
        sorted.sort();
        assert_eq!(groups, sorted, "{m:?}");
    }
}

// ---- the view over a real volume ----

fn populated() -> Fs3<RamDisk> {
    let mut fs = fresh();
    fs.mkdir("/Docs", NOW).unwrap();
    fs.mkdir("/Docs/inner", NOW).unwrap();
    fs.write_file("/Docs/a.txt", b"aaa", NOW + 1).unwrap();
    fs.write_file("/Docs/b.png", b"x", NOW + 2).unwrap();
    fs.write_file("/z.txt", b"zz", NOW + 3).unwrap();
    fs
}

#[test]
fn view_lists_sorted_rows_with_folders_first() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    assert_eq!(names(&v.rows), ["Docs", "z.txt"]);
    v.navigate(&mut fs, b"/Docs").unwrap();
    assert_eq!(names(&v.rows), ["inner", "a.txt", "b.png"]);
    assert_eq!(v.cwd, b"/Docs");
}

#[test]
fn view_navigation_and_history() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v.navigate(&mut fs, b"/Docs").unwrap();
    v.navigate(&mut fs, b"/Docs/inner").unwrap();
    v.go_up(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/Docs");
    v.go_back(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/Docs/inner");
    v.go_forward(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/Docs");
    v.go_up(&mut fs).unwrap();
    v.go_up(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/");
}

#[test]
fn view_refuses_bad_destinations_and_stays() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    assert_eq!(v.navigate(&mut fs, b"/nope"), Err(VfsError::NotFound));
    assert_eq!(v.navigate(&mut fs, b"/z.txt"), Err(VfsError::NotDir));
    assert_eq!(v.cwd, b"/");
    assert_eq!(names(&v.rows), ["Docs", "z.txt"]);
}

#[test]
fn activating_a_folder_enters_it_and_a_file_opens() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    assert_eq!(v.activate(&mut fs, 0), Activation::Entered);
    assert_eq!(v.cwd, b"/Docs");
    assert_eq!(
        v.activate(&mut fs, 2),
        Activation::Open(b"/Docs/b.png".to_vec(), FileClass::Image)
    );
    assert_eq!(
        v.activate(&mut fs, 1),
        Activation::Open(b"/Docs/a.txt".to_vec(), FileClass::Text)
    );
    assert_eq!(v.activate(&mut fs, 99), Activation::None);
}

#[test]
fn refresh_keeps_the_selection_by_name() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.navigate(&mut fs, b"/Docs").unwrap();
    v.sel.click(2, false, false); // b.png
    fs.write_file("/Docs/0first.txt", b"", NOW).unwrap();
    v.refresh(&mut fs).unwrap();
    assert_eq!(names(&v.rows), ["inner", "0first.txt", "a.txt", "b.png"]);
    let rows: Vec<_> = v.selected_rows().iter().map(|r| r.name.clone()).collect();
    assert_eq!(rows, vec![b"b.png".to_vec()]);
}

#[test]
fn resorting_keeps_the_selection() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.navigate(&mut fs, b"/Docs").unwrap();
    v.sel.click(1, false, false); // a.txt
    v.click_header(SortKey::Name); // now descending
    assert_eq!(names(&v.rows), ["inner", "b.png", "a.txt"]);
    assert_eq!(v.selected_paths(), vec![b"/Docs/a.txt".to_vec()]);
}

#[test]
fn a_vanished_folder_falls_back_to_its_parent() {
    let mut fs = populated();
    let mut v = FileView::new();
    v.navigate(&mut fs, b"/Docs").unwrap();
    v.navigate(&mut fs, b"/Docs/inner").unwrap();
    fs.remove_all("/Docs/inner").unwrap();
    v.refresh(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/Docs");
    fs.remove_all("/Docs").unwrap();
    v.refresh(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/");
}

#[test]
fn trash_view_lists_deleted_items() {
    let mut fs = populated();
    crate::vfs::remove(&mut fs, b"/z.txt", NOW + 50).unwrap();
    crate::vfs::remove(&mut fs, b"/Docs/a.txt", NOW + 60).unwrap();
    let mut v = FileView::new();
    v.navigate(&mut fs, TRASH_PATH).unwrap();
    assert!(v.in_trash());
    assert_eq!(v.rows.len(), 2);
    assert!(v.rows.iter().all(|r| !r.id.is_empty()));
    assert_eq!(v.path_of(0), None);
    assert!(v.selected_paths().is_empty());
    assert_eq!(v.activate(&mut fs, 0), Activation::None);
    v.go_up(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/");
    assert!(!v.in_trash());
}

#[test]
fn summary_counts_items_and_selection() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let mut fs = populated();
    let mut v = FileView::new();
    v.navigate(&mut fs, b"/Docs").unwrap();
    assert_eq!(v.summary(), "1 selecionado (0 B)"); // navigating selects the first row
    v.sel.clear();
    assert_eq!(v.summary(), "3 itens");
    v.sel.click(1, false, false);
    assert_eq!(v.summary(), "1 selecionado (3 B)");
    v.sel.select_all();
    assert_eq!(v.summary(), "3 selecionados (4 B)");
    let one = FileView::new();
    assert_eq!(one.summary(), "0 item");
}

#[test]
fn select_name_puts_the_cursor_on_it() {
    let mut fs = fresh();
    for i in 0..100 {
        let p = alloc::format!("/f{i:03}");
        fs.write_file(p.as_str(), b"", NOW).unwrap();
    }
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v.select_name(b"f090");
    assert_eq!(v.sel.cursor(), 90);
    assert_eq!(v.sel.count(), 1);
    v.select_name(b"f001");
    assert_eq!(v.sel.selected(), vec![1]);
    v.select_name(b"nope");
    assert_eq!(v.sel.selected(), vec![1]);
}

#[test]
fn select_names_selects_the_new_items() {
    let mut fs = fresh();
    for n in ["a", "b", "c", "d"] {
        fs.write_file(alloc::format!("/{n}").as_str(), b"", NOW)
            .unwrap();
    }
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v.select_names(&[b"d".to_vec(), b"b".to_vec()]);
    assert_eq!(v.sel.selected(), vec![1, 3]);
    assert_eq!(v.sel.cursor(), 1);
    v.select_names(&[b"zzz".to_vec()]);
    assert_eq!(v.sel.selected(), vec![1, 3]);
}

#[test]
fn select_set_sets_cursor_and_anchor() {
    let mut s = sel(6);
    s.select_set(&[2, 4]);
    assert_eq!(s.selected(), vec![2, 4]);
    assert_eq!(s.cursor(), 2);
    s.click(5, false, true);
    assert_eq!(s.selected(), vec![2, 3, 4, 5]);
}

// ---- the Apps place ----

fn items() -> Vec<apps::AppItem> {
    let it = |id: &str, name: &str, installed: bool, size: u64| apps::AppItem {
        id: id.into(),
        name: name.into(),
        installed,
        size,
    };
    alloc::vec![
        it("snake", "Snake", true, 3000),
        it("notes", "Notas", true, 5200),
        it("paint", "Pintura", false, 9000),
        it("clock", "Relógio", true, 2100),
    ]
}

#[test]
fn apps_rows_carry_id_state_and_size() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let rows = apps::rows(&items());
    assert_eq!(rows.len(), 4);
    let r = &rows[2];
    assert_eq!(r.name, b"Pintura");
    assert_eq!(r.id, b"paint");
    assert!(!r.installed && r.mtime == 0 && r.size == 9000 && !r.is_dir());
    assert!(rows[0].installed && rows[0].mtime == 1);
    assert_eq!(apps::status_label(true), "instalado");
    assert_eq!(apps::status_label(false), "não instalado");
}

#[test]
fn apps_place_is_a_pseudo_path_that_never_touches_the_volume() {
    let mut fs = fresh();
    fs.write_file("/a.txt", b"x", NOW).unwrap();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    assert!(!v.in_apps());
    assert!(!v.rows.is_empty(), "the root has rows");
    v.navigate(&mut fs, APPS_PATH).unwrap();
    assert!(v.in_apps() && !v.in_trash());
    assert_eq!(v.cwd, APPS_PATH);
    // The volume has no such folder: no row of the volume leaks in.
    assert!(v.rows.is_empty());
    let crumbs = breadcrumbs(APPS_PATH);
    assert_eq!(crumbs.len(), 2);
    assert_eq!(
        (&crumbs[1].label[..], &crumbs[1].path[..]),
        (&b"Apps"[..], APPS_PATH)
    );
    v.set_apps(&items());
    // Name order (the default sort): Notas, Pintura, Relógio, Snake.
    assert_eq!(names(&v.rows), ["Notas", "Pintura", "Relógio", "Snake"]);
    assert_eq!(v.sel.cursor(), 0);
    assert!(v.sel.is_selected(0), "the first app is selected");
    v.set_apps(&items());
    assert!(v.sel.is_selected(0), "and stays selected on a reload");
    // A reload (the volume changed) keeps the app rows.
    v.refresh(&mut fs).unwrap();
    assert_eq!(v.rows.len(), 4);
    // No file paths in this place.
    assert_eq!(v.path_of(0), None);
    assert!(v.selected_paths().is_empty());
    // Up goes to the root; history remembers the Apps place.
    v.go_up(&mut fs).unwrap();
    assert_eq!(v.cwd, b"/");
    v.go_back(&mut fs).unwrap();
    assert!(v.in_apps());
}

#[test]
fn apps_selection_follows_the_app_across_catalog_changes() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let mut fs = fresh();
    let mut v = FileView::new();
    v.navigate(&mut fs, APPS_PATH).unwrap();
    v.set_apps(&items());
    v.sel.only(2); // Relógio
    assert_eq!(v.rows[2].id, b"clock");
    // Pintura gets installed and Notas removed: the cursor stays on the clock.
    let mut now = items();
    now[2].installed = true;
    now.retain(|a| a.id != "notes");
    v.set_apps(&now);
    assert_eq!(v.rows[v.sel.cursor()].id, b"clock");
    // The selected app disappears: the cursor falls back to the first row.
    now.retain(|a| a.id != "clock");
    v.set_apps(&now);
    assert_eq!(v.sel.cursor(), 0);
    v.set_apps(&[]);
    assert!(v.rows.is_empty());
    assert_eq!(v.summary(), "0 item");
}

#[test]
fn activating_an_app_row_asks_for_the_app_not_a_file() {
    let mut fs = fresh();
    let mut v = FileView::new();
    v.navigate(&mut fs, APPS_PATH).unwrap();
    v.set_apps(&items());
    match v.activate(&mut fs, 1) {
        Activation::App { id, installed } => {
            assert_eq!(id, b"paint");
            assert!(!installed);
        }
        a => panic!("{a:?}"),
    }
    assert_eq!(v.activate(&mut fs, 99), Activation::None);
}

#[test]
fn app_keys_install_remove_and_run() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let rows = apps::rows(&items());
    let installed = rows.iter().find(|r| r.id == b"notes").unwrap();
    let missing = rows.iter().find(|r| r.id == b"paint").unwrap();
    use apps::{AppAction, AppKey, app_action};
    assert_eq!(
        app_action(installed, AppKey::Enter),
        Ok(AppAction::Launch("notes".into()))
    );
    assert_eq!(
        app_action(missing, AppKey::Enter),
        Ok(AppAction::InstallAndLaunch("paint".into()))
    );
    assert_eq!(
        app_action(missing, AppKey::Install),
        Ok(AppAction::Install("paint".into()))
    );
    assert_eq!(app_action(installed, AppKey::Install), Err("Já instalado"));
    {
        let _en = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::En);
        assert_eq!(
            app_action(installed, AppKey::Install),
            Err("Already installed")
        );
        assert_eq!(app_action(missing, AppKey::Remove), Err("Not installed"));
    }
    assert_eq!(
        app_action(installed, AppKey::Remove),
        Ok(AppAction::Remove("notes".into()))
    );
    assert_eq!(app_action(missing, AppKey::Remove), Err("Não instalado"));
}

#[test]
fn apps_context_menu_offers_what_applies() {
    let ctx = |selected, installed| MenuCtx {
        in_trash: false,
        in_apps: true,
        app_installed: installed,
        selected,
        image: false,
        clip_has_items: true,
    };
    let cmds = |c| {
        context_menu(c)
            .into_iter()
            .map(|(c, _)| c)
            .collect::<Vec<_>>()
    };
    let on_installed = cmds(ctx(1, true));
    assert_eq!(on_installed[0], Cmd::Open);
    assert!(on_installed.contains(&Cmd::RemoveApp) && !on_installed.contains(&Cmd::InstallApp));
    let on_missing = cmds(ctx(1, false));
    assert!(on_missing.contains(&Cmd::InstallApp) && !on_missing.contains(&Cmd::RemoveApp));
    // None of the file commands (they would act on paths that do not exist here).
    for c in [
        &on_installed,
        &on_missing,
        &cmds(ctx(0, false)),
        &cmds(ctx(3, false)),
    ] {
        for bad in [
            Cmd::NewFile,
            Cmd::NewFolder,
            Cmd::Cut,
            Cmd::Copy,
            Cmd::Paste,
            Cmd::Delete,
            Cmd::DeletePermanent,
            Cmd::Rename,
            Cmd::SetWallpaper,
        ] {
            assert!(!c.contains(&bad), "{bad:?}");
        }
    }
    assert!(
        context_menu(ctx(1, false))
            .iter()
            .all(|(_, l)| !l.is_empty())
    );
}

#[test]
fn manifest_lines_show_every_permission() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    use crate::appmanifest::{ClipPerm, FsPerm, NetPerm};
    let mut m = crate::appmanifest::Manifest::legacy("demo", "Demo");
    m.fs = FsPerm::Own;
    m.net = NetPerm::Http;
    m.clipboard = ClipPerm::Rw;
    let lines = apps::manifest_lines(&m);
    let all = lines.join("\n");
    assert!(all.contains("Demo (demo)"), "{all}");
    assert!(all.contains("/data/demo"), "{all}");
    assert!(all.contains("HTTP"), "{all}");
    assert!(all.contains("ler e escrever"), "{all}");
    assert!(all.contains("Memória:") && all.contains("Janela "), "{all}");
    {
        let _en = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::En);
        let en = apps::manifest_lines(&m).join("\n");
        assert!(en.contains("Files: only /data/demo"), "{en}");
        assert!(
            en.contains("read and write") && en.contains("Window "),
            "{en}"
        );
        assert!(en.contains("Clipboard:"), "{en}");
    }
    m.fs = FsPerm::None;
    m.net = NetPerm::None;
    m.clipboard = ClipPerm::None;
    let none = apps::manifest_lines(&m).join("\n");
    assert!(
        none.contains("Arquivos: nenhum") && none.contains("Rede: nenhuma"),
        "{none}"
    );
    // Each line is a short "label: value" the information sheet can split at the colon.
    assert!(
        lines
            .iter()
            .all(|l| l.contains(": ") && l.chars().count() <= 60),
        "{lines:?}"
    );
    m.fs = FsPerm::Home;
    assert!(apps::manifest_lines(&m).join("\n").contains("/home"));
}

// ---- search filter ----

fn searchable() -> FileView {
    let mut fs = fresh();
    for n in [
        "Ação.txt",
        "relatorio.pdf",
        "RELATÓRIO final.txt",
        "foto.png",
    ] {
        fs.write_file(alloc::format!("/{n}").as_str(), b"x", NOW)
            .unwrap();
    }
    fs.mkdir("/Relatórios", NOW).unwrap();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v
}

#[test]
fn the_filter_narrows_the_rows_and_comes_back() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    let mut v = searchable();
    assert_eq!(v.rows.len(), 5);
    v.set_filter(b"relat");
    assert_eq!(
        names(&v.rows),
        vec!["Relatórios", "relatorio.pdf", "RELATÓRIO final.txt"]
    );
    assert_eq!(v.total_rows(), 5);
    assert_eq!(v.filter(), b"relat");
    v.set_filter("AÇÃO".as_bytes());
    assert_eq!(names(&v.rows), vec!["Ação.txt"]);
    v.set_filter(b"zzz");
    assert!(v.rows.is_empty());
    assert_eq!(v.summary(), "0 item");
    v.clear_filter();
    assert_eq!(v.rows.len(), 5);
    assert_eq!(v.total_rows(), 5);
    assert!(v.filter().is_empty());
}

#[test]
fn filtering_keeps_the_selection_by_name_and_follows_sorts() {
    let mut v = searchable();
    v.select_name("foto.png".as_bytes());
    v.set_filter(b"o");
    assert!(v.selected_rows().iter().any(|r| r.name == b"foto.png"));
    v.set_filter(b"txt");
    // The selected row no longer shows: nothing selected, nothing lost.
    assert_eq!(v.sel.count(), 0);
    v.click_header(SortKey::Name); // descending
    assert_eq!(
        names(&v.rows),
        vec!["RELATÓRIO final.txt", "Ação.txt"],
        "sort applies to the filtered rows"
    );
    v.clear_filter();
    // The hidden rows were sorted too.
    assert_eq!(v.rows[0].name, "Relatórios".as_bytes());
    assert_eq!(v.rows.last().unwrap().name, "Ação.txt".as_bytes());
}

#[test]
fn a_refresh_under_a_filter_reloads_and_refilters() {
    let mut fs = fresh();
    fs.write_file("/a.txt", b"", NOW).unwrap();
    fs.write_file("/b.png", b"", NOW).unwrap();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v.set_filter(b"txt");
    assert_eq!(names(&v.rows), vec!["a.txt"]);
    fs.write_file("/c.txt", b"", NOW).unwrap();
    fs.remove("/b.png").ok();
    v.refresh(&mut fs).unwrap();
    assert_eq!(names(&v.rows), vec!["a.txt", "c.txt"]);
    assert_eq!(v.total_rows(), 2);
}

#[test]
fn navigating_drops_the_filter_and_bumps_the_generation() {
    let mut fs = fresh();
    fs.mkdir("/d", NOW).unwrap();
    fs.write_file("/d/in.txt", b"", NOW).unwrap();
    fs.write_file("/x.txt", b"", NOW).unwrap();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v.set_filter(b"x");
    let g = v.nav_gen;
    v.navigate(&mut fs, b"/d").unwrap();
    assert!(v.filter().is_empty());
    assert_eq!(names(&v.rows), vec!["in.txt"]);
    assert_ne!(v.nav_gen, g);
    let g = v.nav_gen;
    v.go_back(&mut fs).unwrap();
    assert_ne!(v.nav_gen, g);
    assert_eq!(v.rows.len(), 2);
    // A failed navigation changes nothing.
    v.set_filter(b"x");
    let g = v.nav_gen;
    assert!(v.navigate(&mut fs, b"/nope").is_err());
    assert_eq!(v.nav_gen, g);
    assert_eq!(v.filter(), b"x");
    assert_eq!(names(&v.rows), vec!["x.txt"]);
}

#[test]
fn set_sort_picks_a_column_without_flipping() {
    let mut v = searchable();
    v.set_sort(SortKey::Size);
    assert_eq!(
        v.sort,
        Sort {
            key: SortKey::Size,
            asc: true
        }
    );
    v.set_sort(SortKey::Size);
    assert!(v.sort.asc);
    v.set_sort(SortKey::Name);
    assert_eq!(v.sort.key, SortKey::Name);
}

// ---- places ----

#[test]
fn places_map_to_paths_and_back() {
    assert_eq!(Place::Home.path(), b"/home");
    assert_eq!(Place::Disk.path(), b"/");
    assert_eq!(Place::Trash.path(), TRASH_PATH);
    assert_eq!(Place::Apps.path(), APPS_PATH);
    assert_eq!(Place::of_path(b"/"), Place::Disk);
    assert_eq!(Place::of_path(b"/Documentos"), Place::Documents);
    assert_eq!(Place::of_path(b"/Documentos/a/b"), Place::Documents);
    assert_eq!(Place::of_path(b"/DocumentosX"), Place::Disk);
    assert_eq!(Place::of_path(b"/Imagens/ferias"), Place::Images);
    assert_eq!(Place::of_path(b"/home"), Place::Home);
    assert_eq!(Place::of_path(b"/outra"), Place::Disk);
    assert_eq!(Place::of_path(TRASH_PATH), Place::Trash);
    assert_eq!(Place::of_path(APPS_PATH), Place::Apps);
    assert!(Place::Home.is_folder() && Place::Images.is_folder());
    assert!(!Place::Trash.is_folder() && !Place::Apps.is_folder() && !Place::Disk.is_folder());
}

// ---- text input selection ----

#[test]
fn renaming_selects_the_stem() {
    let mut t = TextInput::new(b"relatorio.final.pdf", 255);
    assert_eq!(t.selection(), None);
    t.select_stem();
    assert_eq!(t.selection(), Some((0, 15)));
    t.insert(b'X');
    assert_eq!(t.text(), b"X.pdf");
    assert_eq!(t.caret(), 1);
    assert_eq!(t.selection(), None);
    // No extension, or a leading dot: everything.
    let mut t = TextInput::new(b"Nova pasta", 255);
    t.select_stem();
    assert_eq!(t.selection(), Some((0, 10)));
    let mut t = TextInput::new(b".config", 255);
    t.select_stem();
    assert_eq!(t.selection(), Some((0, 7)));
}

#[test]
fn selected_text_is_replaced_deleted_or_collapsed() {
    let mut t = TextInput::new(b"abcdef", 255);
    t.select_all();
    assert_eq!(t.selection(), Some((0, 6)));
    t.backspace();
    assert_eq!(t.text(), b"");
    let mut t = TextInput::new(b"abcdef", 255);
    t.select_all();
    t.delete();
    assert_eq!(t.text(), b"");
    let mut t = TextInput::new(b"abcdef", 255);
    t.select_all();
    t.left();
    assert_eq!((t.caret(), t.selection()), (0, None));
    let mut t = TextInput::new(b"abcdef", 255);
    t.select_all();
    t.right();
    assert_eq!((t.caret(), t.selection()), (6, None));
    let mut t = TextInput::new(b"abcdef", 255);
    t.select_all();
    t.home();
    assert_eq!(t.selection(), None);
    // Replacing does not overflow the limit even when the selection is bigger than the key.
    let mut t = TextInput::new(b"abcd", 4);
    t.select_all();
    t.insert(b'z');
    assert_eq!(t.text(), b"z");
    t.insert(b'a');
    t.insert(b'b');
    t.insert(b'c');
    t.insert(b'd');
    assert_eq!(t.text(), b"zabc");
}

#[test]
fn latin1_keys_are_stored_as_utf8() {
    let mut t = TextInput::new(b"", 255);
    for b in [b'a', 0xE7, 0xE3, b'o'] {
        t.insert(b);
    }
    assert_eq!(t.text(), "açãо".replace('о', "o").as_bytes());
    assert_eq!(t.to_string_lossy(), "ação");
    t.backspace();
    t.backspace();
    assert_eq!(t.to_string_lossy(), "aç");
    // The limit counts bytes, and a character never splits.
    let mut t = TextInput::new(b"abc", 4);
    t.insert(0xE7); // needs two bytes: no room
    assert_eq!(t.text(), b"abc");
    let t = TextInput::new("aç".as_bytes(), 2);
    assert_eq!(t.text(), b"a");
}

#[test]
fn counts_and_selection_text_in_both_languages() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let mut v = searchable();
    let n = v.rows.len();
    assert!(n > 1);
    for (l, none, all_sel) in [
        (
            Lang::Pt,
            std::format!("{n} itens"),
            std::format!("{n} selecionados"),
        ),
        (
            Lang::En,
            std::format!("{n} items"),
            std::format!("{n} selected"),
        ),
    ] {
        let _g = LangGuard::new(l);
        v.sel.clear();
        assert_eq!(v.summary(), none, "{l:?}");
        v.sel.select_all();
        assert!(v.summary().starts_with(&all_sel), "{l:?}: {}", v.summary());
    }
    let _g = LangGuard::new(Lang::En);
    assert_eq!(crate::tp!("files.count", 1u32), "1 item");
    assert_eq!(crate::tp!("files.count", 0u32), "0 items");
    drop(_g);
    // pt-BR: zero is singular too.
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(crate::tp!("files.count", 0u32), "0 item");
    assert_eq!(crate::tp!("files.count", 1u32), "1 item");
    assert_eq!(crate::tp!("files.count", 2u32), "2 itens");
    assert_eq!(
        crate::tp!("files.selected", 1u32, size = crate::i18n::bytes(1536)),
        "1 selecionado (1,5 KiB)"
    );
}

#[test]
fn context_menu_labels_follow_the_language() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let blank = MenuCtx {
        in_trash: false,
        in_apps: false,
        app_installed: false,
        selected: 0,
        image: false,
        clip_has_items: true,
    };
    let labels = || {
        context_menu(blank)
            .into_iter()
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
    };
    {
        let _g = LangGuard::new(Lang::Pt);
        assert_eq!(
            labels(),
            [
                "Novo arquivo",
                "Nova pasta",
                "Colar",
                "Selecionar tudo",
                "Atualizar",
                "Informações"
            ]
        );
        assert_eq!(Cmd::TogglePreview.shortcut(), "Espaço");
    }
    let _g = LangGuard::new(Lang::En);
    assert_eq!(
        labels(),
        [
            "New file",
            "New folder",
            "Paste",
            "Select all",
            "Refresh",
            "Properties"
        ]
    );
    assert_eq!(Cmd::TogglePreview.shortcut(), "Space");
    let trash = MenuCtx {
        in_trash: true,
        selected: 1,
        ..blank
    };
    let l: Vec<_> = context_menu(trash).into_iter().map(|(_, l)| l).collect();
    assert_eq!(
        l,
        [
            "Restore",
            "Delete permanently",
            "Empty trash",
            "Properties",
            "Select all"
        ]
    );
}

#[test]
fn breadcrumbs_special_labels_follow_the_language() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let _g = LangGuard::new(Lang::En);
    assert_eq!(breadcrumbs(b"/")[0].label, b"Disk");
    assert_eq!(breadcrumbs(TRASH_PATH)[1].label, b"Trash");
    assert_eq!(breadcrumbs(b"/Projetos/x")[1].label, b"Projetos");
}
