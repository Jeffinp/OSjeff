use super::*;

fn img() -> [u8; IMAGE_SIZE] {
    let mut img = [0u8; IMAGE_SIZE];
    format(&mut img);
    img
}

#[test]
fn fresh_image_is_formatted_and_empty() {
    let img = img();
    assert!(is_formatted(&img));
    assert_eq!(count(&img), 0);
}

#[test]
fn unformatted_image_detected() {
    let img = [0u8; IMAGE_SIZE];
    assert!(!is_formatted(&img));
    let mut img2 = img;
    assert_eq!(write(&mut img2, b"x", b"y"), Err(FsError::NotFormatted));
}

#[test]
fn write_then_read() {
    let mut img = img();
    write(&mut img, b"notes.txt", b"hello world").unwrap();
    assert_eq!(read(&img, b"notes.txt"), Some(&b"hello world"[..]));
    assert_eq!(count(&img), 1);
}

#[test]
fn overwrite_reuses_slot() {
    let mut img = img();
    write(&mut img, b"a", b"first").unwrap();
    write(&mut img, b"a", b"second longer").unwrap();
    assert_eq!(read(&img, b"a"), Some(&b"second longer"[..]));
    assert_eq!(count(&img), 1);
}

#[test]
fn trash_restore_purge_flow() {
    let mut img = img();
    write(&mut img, b"a.txt", b"hi").unwrap();
    write(&mut img, b"b.txt", b"yo").unwrap();
    assert_eq!(count_active(&img), 2);

    // Trash keeps the data, hides it from the active count and from read().
    trash(&mut img, b"a.txt").unwrap();
    assert_eq!(count_active(&img), 1);
    assert_eq!(count_trashed(&img), 1);
    assert_eq!(read(&img, b"a.txt"), None); // read() only sees live files now
    let ti = (0..MAX_FILES)
        .find(|&i| is_trashed(&img, i) && name_at(&img, i) == b"a.txt")
        .unwrap();
    assert_eq!(read_slot(&img, ti), Some(&b"hi"[..])); // data still there
    assert_eq!(trash(&mut img, b"a.txt"), Err(FsError::NotFound)); // already trashed

    // Restore brings it back.
    restore(&mut img, b"a.txt").unwrap();
    assert_eq!(count_active(&img), 2);
    assert_eq!(count_trashed(&img), 0);

    // Permanent delete frees the slot and the data is gone.
    purge(&mut img, b"a.txt").unwrap();
    assert_eq!(count_active(&img), 1);
    assert_eq!(read(&img, b"a.txt"), None);

    // Empty trash purges everything in it.
    trash(&mut img, b"b.txt").unwrap();
    empty_trash(&mut img);
    assert_eq!(count_trashed(&img), 0);
    assert_eq!(read(&img, b"b.txt"), None);
}

#[test]
fn directories_and_recursive_delete() {
    let mut img = img();
    let docs = mkdir(&mut img, ROOT, b"docs").unwrap();
    assert!(is_dir(&img, docs));
    write_in(&mut img, docs as u8, b"a.txt", b"hi").unwrap();
    write_in(&mut img, ROOT, b"root.txt", b"r").unwrap();

    // a.txt lives inside docs, not at the root.
    assert!(find_in(&img, docs as u8, b"a.txt").is_some());
    assert!(find_in(&img, ROOT, b"a.txt").is_none());
    let child = find_in(&img, docs as u8, b"a.txt").unwrap();
    assert_eq!(read_slot(&img, child), Some(&b"hi"[..]));
    assert_eq!(read_slot(&img, docs), None); // dirs have no payload

    // Trashing the directory takes its contents with it.
    trash_slot(&mut img, docs);
    assert!(is_trashed(&img, docs) && is_trashed(&img, child));
    assert_eq!(count_active(&img), 1); // only root.txt

    // Restore brings the whole subtree back.
    restore_slot(&mut img, docs);
    assert!(is_active(&img, docs) && is_active(&img, child));

    // Permanent delete frees the whole subtree.
    purge_slot(&mut img, docs);
    assert!(!is_used(&img, docs) && !is_used(&img, child));
    assert_eq!(count_active(&img), 1);
}

#[test]
fn multiple_files() {
    let mut img = img();
    write(&mut img, b"one", b"1").unwrap();
    write(&mut img, b"two", b"22").unwrap();
    write(&mut img, b"three", b"333").unwrap();
    assert_eq!(count(&img), 3);
    assert_eq!(read(&img, b"two"), Some(&b"22"[..]));
    assert_eq!(read(&img, b"missing"), None);
}

#[test]
fn remove_frees_slot() {
    let mut img = img();
    write(&mut img, b"gone", b"data").unwrap();
    assert!(remove(&mut img, b"gone").is_ok());
    assert_eq!(read(&img, b"gone"), None);
    assert_eq!(count(&img), 0);
    assert_eq!(remove(&mut img, b"gone"), Err(FsError::NotFound));
}

#[test]
fn rejects_bad_names_and_sizes() {
    let mut img = img();
    assert_eq!(write(&mut img, b"", b"x"), Err(FsError::EmptyName));
    let long = [b'a'; MAX_NAME + 1];
    assert_eq!(write(&mut img, &long, b"x"), Err(FsError::NameTooLong));
    let big = [0u8; MAX_FILE_SIZE + 1];
    assert_eq!(write(&mut img, b"f", &big), Err(FsError::TooBig));
}

#[test]
fn no_space_when_full() {
    let mut img = img();
    for i in 0..MAX_FILES {
        let name = [b'a' + i as u8];
        write(&mut img, &name, b"x").unwrap();
    }
    assert_eq!(write(&mut img, b"overflow", b"x"), Err(FsError::NoSpace));
    // Overwriting an existing file still works when full.
    assert!(write(&mut img, b"a", b"updated").is_ok());
}

#[test]
fn name_and_size_accessors() {
    let mut img = img();
    write(&mut img, b"hi", b"abcd").unwrap();
    let slot = find(&img, b"hi").unwrap();
    assert_eq!(name_at(&img, slot), b"hi");
    assert_eq!(size_at(&img, slot), 4);
}

/// Regression: a corrupted on-disk `size` (> MAX_FILE_SIZE) used to make
/// `read_slot` slice past the record (leaking the next records' bytes) and,
/// for the last slot, past the end of the image (panic in the kernel).
#[test]
fn corrupted_size_field_is_clamped() {
    for slot in [0, 1, MAX_FILES - 1] {
        let mut img = img();
        let o = rec_off(slot);
        img[o] = ST_ACTIVE;
        img[o + OFF_NAMELEN] = 1;
        img[o + OFF_NAME] = b'a';
        img[o + OFF_SIZE..o + OFF_SIZE + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
        assert_eq!(size_at(&img, slot), MAX_FILE_SIZE);
        assert_eq!(read_slot(&img, slot).map(|d| d.len()), Some(MAX_FILE_SIZE));
    }
}

/// Regression: a directory record whose `parent` points at itself made
/// `trash_slot`/`purge_slot` recurse forever (stack overflow in the kernel).
#[test]
fn parent_self_cycle_does_not_recurse_forever() {
    let mut img = img();
    let d = mkdir(&mut img, ROOT, b"d").unwrap();
    img[rec_off(d) + OFF_PARENT] = d as u8; // corrupt: parent == self
    trash_slot(&mut img, d);
    assert!(is_trashed(&img, d));
    restore_slot(&mut img, d);
    assert!(is_active(&img, d));
    purge_slot(&mut img, d);
    assert!(!is_used(&img, d));
}

/// Same, for a two-directory cycle (a -> b -> a) with a file inside.
#[test]
fn parent_two_cycle_does_not_recurse_forever() {
    let mut img = img();
    let a = mkdir(&mut img, ROOT, b"a").unwrap();
    let b = mkdir(&mut img, a as u8, b"b").unwrap();
    write_in(&mut img, b as u8, b"f", b"x").unwrap();
    img[rec_off(a) + OFF_PARENT] = b as u8; // corrupt: a's parent is its own child
    trash_slot(&mut img, a);
    assert_eq!(count_active(&img), 0);
    purge_slot(&mut img, a);
    assert_eq!(count(&img), 0);
}

/// Regression (found by the ojfs_parse fuzzer): every accessor indexed the
/// image directly, so any buffer shorter than IMAGE_SIZE (e.g. a failed or
/// partial disk read) panicked in `is_used` / `is_active` / `is_trashed`.
#[test]
fn short_image_never_panics() {
    for len in [0, 1, 4, 5, 1000, IMAGE_SIZE - 1] {
        let mut short = vec![0u8; len];
        short[..len.min(4)].copy_from_slice(&MAGIC[..len.min(4)]);
        assert!(!is_formatted(&short));
        assert_eq!(count(&short), 0);
        assert_eq!(count_active(&short), 0);
        assert_eq!(count_trashed(&short), 0);
        assert_eq!(find(&short, b"a"), None);
        assert_eq!(read(&short, b"a"), None);
        assert_eq!(name_at(&short, 0), b"");
        assert_eq!(size_at(&short, 0), 0);
        assert!(!is_dir(&short, 0));
        assert_eq!(parent_at(&short, 0), ROOT);
        assert_eq!(write(&mut short, b"a", b"b"), Err(FsError::NotFormatted));
        assert_eq!(mkdir(&mut short, ROOT, b"d"), Err(FsError::NotFormatted));
        assert_eq!(remove(&mut short, b"a"), Err(FsError::NotFound));
        trash_slot(&mut short, 0);
        restore_slot(&mut short, 0);
        purge_slot(&mut short, 0);
        empty_trash(&mut short);
    }
}

#[test]
fn same_name_in_folder_and_root_are_independent() {
    let mut img = img();
    let docs = mkdir(&mut img, ROOT, b"docs").unwrap() as u8;
    write_in(&mut img, docs, b"p.txt", b"inside").unwrap();
    write(&mut img, b"p.txt", b"root").unwrap();

    // Name-only (root) lookups never see the folder's file...
    assert_eq!(read(&img, b"p.txt"), Some(&b"root"[..]));
    // ...and the parent-aware read picks the right one.
    assert_eq!(read_in(&img, docs, b"p.txt"), Some(&b"inside"[..]));
    assert_eq!(read_in(&img, ROOT, b"p.txt"), Some(&b"root"[..]));
    assert_eq!(read_in(&img, docs, b"missing"), None);

    // Saving into the folder rewrites the folder's file only.
    write_in(&mut img, docs, b"p.txt", b"edited").unwrap();
    assert_eq!(read_in(&img, docs, b"p.txt"), Some(&b"edited"[..]));
    assert_eq!(read(&img, b"p.txt"), Some(&b"root"[..]));
    assert_eq!(count(&img), 3); // docs + two files, no stray copy
}

#[test]
fn live_dir_falls_back_to_root() {
    let mut img = img();
    let docs = mkdir(&mut img, ROOT, b"docs").unwrap();
    let file = {
        write_in(&mut img, docs as u8, b"f", b"x").unwrap();
        find_in(&img, docs as u8, b"f").unwrap()
    };
    assert_eq!(live_dir(&img, ROOT), ROOT);
    assert_eq!(live_dir(&img, docs as u8), docs as u8);
    assert_eq!(live_dir(&img, file as u8), ROOT); // a file is not a directory
    assert_eq!(live_dir(&img, 40), ROOT); // free slot
    assert_eq!(live_dir(&img, 200), ROOT); // out of range
    trash_slot(&mut img, docs);
    assert_eq!(live_dir(&img, docs as u8), ROOT); // trashed folder
    purge_slot(&mut img, docs);
    assert_eq!(live_dir(&img, docs as u8), ROOT); // deleted folder
    let short = [0u8; 8];
    assert_eq!(live_dir(&short, 0), ROOT); // unformatted / too small
}

#[test]
fn max_size_file_roundtrips() {
    let mut img = img();
    let data = [b'z'; MAX_FILE_SIZE];
    write(&mut img, b"big", &data).unwrap();
    assert_eq!(read(&img, b"big"), Some(&data[..]));
}
