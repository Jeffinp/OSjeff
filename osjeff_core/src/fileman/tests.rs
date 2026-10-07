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
    let c = breadcrumbs(b"/");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].label, b"Raiz");
    let c = breadcrumbs(b"/a/b c/d");
    let labels: Vec<_> = c.iter().map(|x| x.label.clone()).collect();
    assert_eq!(
        labels,
        vec![
            b"Raiz".to_vec(),
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
    let c = breadcrumbs(TRASH_PATH);
    assert_eq!(c.len(), 2);
    assert_eq!(c[1].label, b"Lixeira");
    assert_eq!(c[1].path, TRASH_PATH);
}

#[test]
fn long_paths_fold_leading_crumbs() {
    assert_eq!(first_visible_crumb(&[4, 3, 5], 100), 0);
    // "Raiz > a > bbbbb" = 4+3+3+3+5 = 18 columns.
    assert_eq!(first_visible_crumb(&[4, 3, 5], 18), 0);
    // One less: fold "Raiz" ("... a > bbbbb" = 4 + 3 + 3 + 5 = 15).
    assert_eq!(first_visible_crumb(&[4, 3, 5], 17), 1);
    // The last crumb always stays.
    assert_eq!(first_visible_crumb(&[4, 3, 50], 5), 2);
    assert_eq!(first_visible_crumb(&[], 5), 0);
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
    assert_eq!(format_datetime(0, 0), "--");
    assert_eq!(format_datetime(1_700_000_000, 0), "14/11/2023 22:13");
    assert_eq!(
        format_datetime(1_700_000_000, -3 * 3600),
        "14/11/2023 19:13"
    );
    // Crossing midnight backwards.
    assert_eq!(format_datetime(86_400 + 60, -3600), "01/01/1970 23:01");
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
        assert!(m.iter().all(|(_, l)| l.is_ascii()));
    }
}

// ---- layout ----

fn layout() -> Layout {
    Layout::of(Rect::new(100, 100, 780, 520))
}

#[test]
fn layout_regions_do_not_overlap_and_fit() {
    let l = layout();
    let w = l.window;
    for r in [
        l.back,
        l.forward,
        l.up,
        l.address,
        l.sidebar,
        l.header,
        l.list,
        l.status,
        l.scrollbar,
    ] {
        assert!(r.x >= w.x && r.right() <= w.right(), "{r:?}");
        assert!(r.y >= w.y + TITLE_H && r.bottom() <= w.bottom(), "{r:?}");
    }
    assert!(l.list.bottom() <= l.status.y);
    assert!(l.header.bottom() <= l.list.y);
    assert!(l.sidebar.right() <= l.header.x);
    assert!(l.list.right() <= l.scrollbar.x);
    assert!(
        l.back.right() < l.forward.x && l.forward.right() < l.up.x && l.up.right() < l.address.x
    );
    assert!(l.name_x < l.size_x && l.size_x < l.date_x);
    assert!(l.visible_rows() > 8);
}

#[test]
fn layout_survives_tiny_windows() {
    let l = Layout::of(Rect::new(0, 0, 100, 60));
    assert!(l.list.h >= 0 && l.sidebar.h >= 0 && l.address.w >= 0);
    assert_eq!(l.visible_rows(), 0);
    assert_eq!(l.scroll_for(500, 100), 100 * 500 / 1000);
}

#[test]
fn hit_testing_finds_each_region() {
    let l = layout();
    let c = |r: Rect| (r.x + r.w / 2, r.y + r.h / 2);
    let h = |p: (i32, i32)| l.hit(p.0, p.1, 0, &[4, 3]);
    assert_eq!(h(c(l.back)), Some(Hit::Back));
    assert_eq!(h(c(l.forward)), Some(Hit::Forward));
    assert_eq!(h(c(l.up)), Some(Hit::Up));
    for (p, r) in l.places() {
        assert_eq!(h(c(r)), Some(Hit::Place(p)));
    }
    assert_eq!(
        h((l.size_x + 10, l.header.y + 5)),
        Some(Hit::Header(SortKey::Size))
    );
    assert_eq!(
        h((l.date_x + 10, l.header.y + 5)),
        Some(Hit::Header(SortKey::Modified))
    );
    assert_eq!(
        h((l.name_x, l.header.y + 5)),
        Some(Hit::Header(SortKey::Name))
    );
    assert_eq!(h((l.list.x + 5, l.list.y + 1)), Some(Hit::Row(0)));
    assert_eq!(
        h((l.list.x + 5, l.list.y + ROW_H * 3 + 1)),
        Some(Hit::Row(3))
    );
    assert_eq!(
        l.hit(l.list.x + 5, l.list.y + ROW_H * 3 + 1, 10, &[]),
        Some(Hit::Row(13))
    );
    assert!(matches!(h(c(l.scrollbar)), Some(Hit::Scroll(_))));
    assert_eq!(l.hit(0, 0, 0, &[]), None);
}

#[test]
fn clicking_a_crumb_and_the_blank_address() {
    let l = layout();
    let labels = [4usize, 3, 5];
    let (spans, folded) = l.crumb_spans(&labels);
    assert!(!folded);
    assert_eq!(spans.len(), 3);
    for (i, x, w) in &spans {
        assert_eq!(
            l.hit(x + w / 2, l.address.y + 5, 0, &labels),
            Some(Hit::Crumb(*i))
        );
    }
    assert_eq!(
        l.hit(l.address.right() - 5, l.address.y + 5, 0, &labels),
        Some(Hit::Address)
    );
}

#[test]
fn crumbs_fold_in_a_narrow_bar() {
    let l = Layout::of(Rect::new(0, 0, 440, 260));
    let labels = [4usize, 20, 20, 20];
    let (spans, folded) = l.crumb_spans(&labels);
    assert!(folded);
    assert_eq!(spans.last().unwrap().0, 3);
    assert!(spans.first().unwrap().0 > 0);
}

#[test]
fn scroll_mapping_round_trips() {
    let l = layout();
    let rows = 5000;
    assert_eq!(l.scroll_for(0, rows), 0);
    assert_eq!(l.scroll_for(1000, rows), rows - l.visible_rows());
    let mid = l.scroll_for(500, rows);
    let (y, h) = l.thumb(mid, rows);
    assert!(y > l.scrollbar.y && y + h < l.scrollbar.bottom());
    assert_eq!(l.thumb(0, 3), (l.scrollbar.y, l.scrollbar.h));
    let (y_end, h_end) = l.thumb(rows - l.visible_rows(), rows);
    assert_eq!(y_end + h_end, l.scrollbar.bottom());
    assert!(h >= 24);
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
    assert_eq!(one.summary(), "0 itens");
}

#[test]
fn select_name_scrolls_it_into_view() {
    let mut fs = fresh();
    for i in 0..100 {
        let p = alloc::format!("/f{i:03}");
        fs.write_file(p.as_str(), b"", NOW).unwrap();
    }
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    v.select_name(b"f090", 10);
    assert_eq!(v.sel.cursor(), 90);
    assert!(v.scroll <= 90 && v.scroll + 10 > 90);
    v.select_name(b"f001", 10);
    assert_eq!(v.scroll, 1);
    v.scroll_by(1000, 10);
    assert_eq!(v.scroll, 90);
    v.scroll_by(-1000, 10);
    assert_eq!(v.scroll, 0);
}

#[test]
fn five_thousand_entries_load_sorted() {
    let mut fs = Fs3::format(RamDisk::new(32 * 2048), &{
        let mut o = FormatOptions::new(*b"0123456789abcdef", NOW);
        o.inode_count = Some(8192);
        o
    })
    .unwrap();
    fs.mkdir("/many", NOW).unwrap();
    for i in 0..5000u32 {
        let p = alloc::format!("/many/file{i}.txt");
        fs.write_file(p.as_str(), b"x", NOW + i as u64).unwrap();
    }
    let mut v = FileView::new();
    v.navigate(&mut fs, b"/many").unwrap();
    assert_eq!(v.rows.len(), 5000);
    assert_eq!(v.rows[0].name, b"file0.txt");
    assert_eq!(v.rows[4999].name, b"file4999.txt");
    v.click_header(SortKey::Modified);
    v.click_header(SortKey::Modified);
    assert_eq!(v.rows[0].name, b"file4999.txt");
    v.sel.select_all();
    assert_eq!(v.selected_paths().len(), 5000);
    assert_eq!(v.summary(), "5000 selecionados (4,8 KiB)");
}

#[test]
fn file_names_of_255_bytes_list_and_fold() {
    let mut fs = fresh();
    let name = "é".repeat(127);
    fs.write_file(alloc::format!("/{name}").as_str(), b"", NOW)
        .unwrap();
    let mut v = FileView::new();
    v.refresh(&mut fs).unwrap();
    assert_eq!(v.rows[0].name.len(), 254);
    assert_eq!(display_ascii(&v.rows[0].name).len(), 127);
    assert_eq!(ellipsize(&display_ascii(&v.rows[0].name), 20).len(), 20);
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
    v.select_names(&[b"d".to_vec(), b"b".to_vec()], 10);
    assert_eq!(v.sel.selected(), vec![1, 3]);
    assert_eq!(v.sel.cursor(), 1);
    v.select_names(&[b"zzz".to_vec()], 10);
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
