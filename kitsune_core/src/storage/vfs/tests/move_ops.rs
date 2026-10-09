use super::*;

#[test]
fn move_renames_a_whole_folder() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let rep = move_to(&mut fs, &[b"/a".to_vec()], b"/b", NOW);
    assert_eq!(rep.error, None);
    assert_eq!(rep.moved, vec![b"/b/a".to_vec()]);
    assert_eq!(fs.read_file("/b/a/one.txt").unwrap(), b"one");
    assert!(!exists(&mut fs, b"/a"));
}

#[test]
fn move_into_itself_or_its_subtree_fails_and_changes_nothing() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let rep = move_to(&mut fs, &[b"/a".to_vec()], b"/a/sub", NOW);
    assert_eq!(rep.error, Some(VfsError::InvalidMove));
    let rep = move_to(&mut fs, &[b"/a".to_vec()], b"/a", NOW);
    assert_eq!(rep.error, Some(VfsError::InvalidMove));
    assert!(exists(&mut fs, b"/a/sub/two.txt"));
}

#[test]
fn move_to_the_same_folder_is_a_no_op() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let rep = move_to(&mut fs, &[b"/a/one.txt".to_vec()], b"/a", NOW);
    assert_eq!(rep.error, None);
    assert_eq!(rep.moved, vec![b"/a/one.txt".to_vec()]);
}

#[test]
fn move_collision_gets_a_numbered_name() {
    let mut fs = fresh(4);
    tree(&mut fs);
    fs.write_file("/b/one.txt", b"other", NOW).unwrap();
    let rep = move_to(&mut fs, &[b"/a/one.txt".to_vec()], b"/b", NOW);
    assert_eq!(rep.moved, vec![b"/b/one (2).txt".to_vec()]);
    assert_eq!(fs.read_file("/b/one.txt").unwrap(), b"other");
    assert_eq!(fs.read_file("/b/one (2).txt").unwrap(), b"one");
}

#[test]
fn move_several_sources_with_the_same_name() {
    let mut fs = fresh(4);
    tree(&mut fs);
    fs.write_file("/b/x", b"1", NOW).unwrap();
    fs.mkdir("/c", NOW).unwrap();
    fs.write_file("/c/x", b"2", NOW).unwrap();
    fs.mkdir("/d", NOW).unwrap();
    let rep = move_to(&mut fs, &[b"/b/x".to_vec(), b"/c/x".to_vec()], b"/d", NOW);
    assert_eq!(rep.error, None);
    assert_eq!(rep.moved, vec![b"/d/x".to_vec(), b"/d/x (2)".to_vec()]);
}

#[test]
fn move_to_a_file_or_missing_folder_fails() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let rep = move_to(&mut fs, &[b"/a".to_vec()], b"/a/one.txt", NOW);
    assert_eq!(rep.error, Some(VfsError::NotDir));
    let rep = move_to(&mut fs, &[b"/a".to_vec()], b"/zzz", NOW);
    assert_eq!(rep.error, Some(VfsError::NotFound));
}

#[test]
fn tree_size_counts_everything() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let t = tree_size(&mut fs, b"/a").unwrap();
    assert_eq!((t.files, t.dirs, t.bytes), (2, 2, 3 + 10_000));
    let f = tree_size(&mut fs, b"/a/one.txt").unwrap();
    assert_eq!((f.files, f.dirs, f.bytes), (1, 0, 3));
    assert_eq!(tree_size(&mut fs, b"/zzz"), Err(VfsError::NotFound));
}
