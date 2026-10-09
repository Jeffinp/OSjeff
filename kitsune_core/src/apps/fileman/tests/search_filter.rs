use super::*;

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
