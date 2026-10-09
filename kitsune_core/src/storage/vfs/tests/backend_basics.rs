use super::*;

#[test]
fn seed_writes_the_welcome_files_once() {
    let mut fs = fresh(4);
    seed_welcome(&mut fs, NOW).unwrap();
    let root = list(&mut fs, b"/").unwrap();
    assert_eq!(names(&root), vec!["Documentos", "leiame.txt", "notas.txt"]);
    assert_eq!(
        fs.read_file("/Documentos/projeto.txt").unwrap(),
        b"Arquivo dentro de uma pasta."
    );
    // The seed fits the editor grid: 44 columns by 18 rows.
    for (_, c) in WELCOME_FILES {
        assert!(c.split(|&b| b == b'\n').all(|l| l.len() <= 44));
        assert!(c.split(|&b| b == b'\n').count() <= 18);
    }
}

#[test]
fn new_file_and_folder_validate_and_refuse_duplicates() {
    let mut fs = fresh(4);
    assert_eq!(
        new_file(&mut fs, b"/", b"  a.txt ", NOW).unwrap(),
        b"/a.txt"
    );
    assert_eq!(
        new_file(&mut fs, b"/", b"a.txt", NOW),
        Err(VfsError::Exists)
    );
    assert_eq!(
        new_file(&mut fs, b"/", b"a/b", NOW),
        Err(VfsError::InvalidName)
    );
    assert_eq!(new_folder(&mut fs, b"/", b"Pasta", NOW).unwrap(), b"/Pasta");
    assert_eq!(
        new_file(&mut fs, b"/Pasta", b"x", NOW).unwrap(),
        b"/Pasta/x"
    );
    assert_eq!(
        new_folder(&mut fs, b"/nope", b"x", NOW),
        Err(VfsError::NotFound)
    );
    assert_eq!(
        new_file(&mut fs, b"/a.txt", b"x", NOW),
        Err(VfsError::NotDir)
    );
}

#[test]
fn rename_in_changes_only_the_name() {
    let mut fs = fresh(4);
    fs.mkdir("/d", NOW).unwrap();
    fs.write_file("/d/a.txt", b"hi", NOW).unwrap();
    assert_eq!(
        rename_in(&mut fs, b"/d/a.txt", b"b.txt", NOW).unwrap(),
        b"/d/b.txt"
    );
    assert_eq!(fs.read_file("/d/b.txt").unwrap(), b"hi");
    assert_eq!(
        rename_in(&mut fs, b"/d/b.txt", b"b.txt", NOW).unwrap(),
        b"/d/b.txt"
    );
    fs.write_file("/d/c.txt", b"", NOW).unwrap();
    assert_eq!(
        rename_in(&mut fs, b"/d/b.txt", b"c.txt", NOW),
        Err(VfsError::Exists)
    );
    assert_eq!(
        rename_in(&mut fs, b"/d/b.txt", b"", NOW),
        Err(VfsError::InvalidName)
    );
    assert_eq!(
        rename_in(&mut fs, b"/d/zzz", b"y", NOW),
        Err(VfsError::NotFound)
    );
}

#[test]
fn remove_goes_to_the_trash_and_restores() {
    let mut fs = fresh(4);
    fs.write_file("/a.txt", b"data", NOW).unwrap();
    remove(&mut fs, b"/a.txt", NOW + 5).unwrap();
    assert!(!exists(&mut fs, b"/a.txt"));
    let items = Backend::trash_list(&mut fs).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, b"a.txt");
    assert_eq!(items[0].deleted_at, NOW + 5);
    assert_eq!(items[0].size, 4);
    let back = Backend::trash_restore(&mut fs, &items[0].id, NOW + 9).unwrap();
    assert_eq!(back, b"/a.txt");
    assert_eq!(fs.read_file("/a.txt").unwrap(), b"data");
}

#[test]
fn restore_to_a_taken_name_is_an_error_and_keeps_the_item() {
    let mut fs = fresh(4);
    fs.write_file("/a.txt", b"old", NOW).unwrap();
    remove(&mut fs, b"/a.txt", NOW).unwrap();
    fs.write_file("/a.txt", b"new", NOW).unwrap();
    let items = Backend::trash_list(&mut fs).unwrap();
    assert_eq!(
        Backend::trash_restore(&mut fs, &items[0].id, NOW),
        Err(VfsError::Exists)
    );
    assert_eq!(Backend::trash_list(&mut fs).unwrap().len(), 1);
}

#[test]
fn purge_and_empty_trash_free_space() {
    let mut fs = fresh(4);
    let before = Backend::usage(&mut fs).free;
    fs.write_file("/big", &pat(300_000, 1), NOW).unwrap();
    assert!(Backend::usage(&mut fs).free < before);
    purge(&mut fs, b"/big").unwrap();
    assert_eq!(Backend::usage(&mut fs).free, before);
    fs.write_file("/big2", &pat(300_000, 2), NOW).unwrap();
    remove(&mut fs, b"/big2", NOW).unwrap();
    assert!(Backend::usage(&mut fs).free < before);
    Backend::empty_trash(&mut fs).unwrap();
    assert_eq!(Backend::usage(&mut fs).free, before);
    assert!(Backend::trash_list(&mut fs).unwrap().is_empty());
}

#[test]
fn trash_purge_removes_one_item() {
    let mut fs = fresh(4);
    fs.write_file("/a", b"1", NOW).unwrap();
    fs.write_file("/b", b"2", NOW).unwrap();
    remove(&mut fs, b"/a", NOW).unwrap();
    remove(&mut fs, b"/b", NOW).unwrap();
    let items = Backend::trash_list(&mut fs).unwrap();
    let a = items.iter().find(|i| i.name == b"a").unwrap();
    Backend::trash_purge(&mut fs, &a.id).unwrap();
    let left = Backend::trash_list(&mut fs).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, b"b");
}

#[test]
fn the_root_cannot_be_removed() {
    let mut fs = fresh(4);
    assert_eq!(remove(&mut fs, b"/", NOW), Err(VfsError::Reserved));
    assert_eq!(purge(&mut fs, b"/"), Err(VfsError::Reserved));
}

#[test]
fn the_trash_folder_is_reserved_and_hidden() {
    let mut fs = fresh(4);
    assert!(!names(&list(&mut fs, b"/").unwrap()).contains(&String::from(".trash")));
    assert_eq!(
        new_file(&mut fs, b"/.trash", b"x", NOW),
        Err(VfsError::Reserved)
    );
}

#[test]
fn usage_reports_a_share() {
    let mut fs = fresh(4);
    let u = Backend::usage(&mut fs);
    assert!(u.total > 3 * 1024 * 1024);
    assert!(u.free <= u.total);
    assert!(u.used_permille() < 100);
    assert_eq!(Usage::default().used_permille(), 0);
    assert_eq!(
        Usage {
            total: 100,
            free: 0,
            ..Usage::default()
        }
        .used_permille(),
        1000
    );
}

#[test]
fn usage_counts_files_and_folders() {
    let mut fs = fresh(4);
    let before = Backend::usage(&mut fs);
    assert!(before.inodes_total > 0);
    assert!(before.inodes_used() < before.inodes_total);
    fs.mkdir(b"/contagem", 1).unwrap();
    fs.write_file(b"/contagem/a.txt", b"x", 1).unwrap();
    let after = Backend::usage(&mut fs);
    assert_eq!(after.inodes_total, before.inodes_total);
    assert_eq!(after.inodes_used(), before.inodes_used() + 2);
    assert_eq!(Usage::default().inodes_used(), 0);
}

#[test]
fn utf8_names_survive_a_round_trip() {
    let mut fs = fresh(4);
    let name = "relatório ção 日本.txt";
    let p = new_file(&mut fs, b"/", name.as_bytes(), NOW).unwrap();
    let l = list(&mut fs, b"/").unwrap();
    assert_eq!(l[0].name, name.as_bytes());
    assert_eq!(p, join(b"/", name.as_bytes()));
}
