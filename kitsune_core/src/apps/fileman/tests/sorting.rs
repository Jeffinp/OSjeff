use super::*;

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
