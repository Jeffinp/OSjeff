use super::*;

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
    crate::storage::vfs::remove(&mut fs, b"/z.txt", NOW + 50).unwrap();
    crate::storage::vfs::remove(&mut fs, b"/Docs/a.txt", NOW + 60).unwrap();
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
