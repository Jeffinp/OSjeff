use super::*;

#[test]
fn copy_a_file_into_a_folder() {
    let mut fs = fresh(8);
    tree(&mut fs);
    let data = pat(200_000, 3);
    fs.write_file("/a/big.bin", &data, NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/a/big.bin".to_vec()], b"/b").unwrap();
    assert_eq!(job.total_bytes(), 200_000);
    assert_eq!(job.files(), (0, 1));
    let steps = run(&mut job, &mut fs, 64 * 1024);
    assert!(steps >= 3, "{steps}");
    assert_eq!(job.permille(), 1000);
    assert_eq!(job.files(), (1, 1));
    assert_eq!(fs.read_file("/b/big.bin").unwrap(), data);
    assert_eq!(fs.read_file("/a/big.bin").unwrap(), data);
    assert_eq!(job.results(), &[b"/b/big.bin".to_vec()]);
}

#[test]
fn copy_into_the_same_folder_duplicates_with_a_suffix() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let mut job = CopyJob::plan(&mut fs, &[b"/a/one.txt".to_vec()], b"/a").unwrap();
    run(&mut job, &mut fs, COPY_CHUNK);
    assert_eq!(fs.read_file("/a/one (2).txt").unwrap(), b"one");
    let mut job = CopyJob::plan(&mut fs, &[b"/a/one (2).txt".to_vec()], b"/a").unwrap();
    run(&mut job, &mut fs, COPY_CHUNK);
    assert_eq!(fs.read_file("/a/one (3).txt").unwrap(), b"one");
}

#[test]
fn copy_a_folder_recursively() {
    let mut fs = fresh(4);
    tree(&mut fs);
    let mut job = CopyJob::plan(&mut fs, &[b"/a".to_vec()], b"/b").unwrap();
    assert_eq!(job.files(), (0, 2));
    run(&mut job, &mut fs, 4096);
    assert_eq!(fs.read_file("/b/a/one.txt").unwrap(), b"one");
    assert_eq!(fs.read_file("/b/a/sub/two.txt").unwrap(), pat(10_000, 7));
    // The source is untouched.
    assert_eq!(fs.read_file("/a/sub/two.txt").unwrap(), pat(10_000, 7));
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "{r:?}");
}

#[test]
fn copying_a_folder_into_itself_is_refused() {
    let mut fs = fresh(4);
    tree(&mut fs);
    assert_eq!(
        CopyJob::plan(&mut fs, &[b"/a".to_vec()], b"/a/sub").err(),
        Some(VfsError::InvalidMove)
    );
    assert_eq!(
        CopyJob::plan(&mut fs, &[b"/a".to_vec()], b"/a").err(),
        Some(VfsError::InvalidMove)
    );
}

#[test]
fn copy_to_a_folder_with_the_same_name_gets_a_suffix() {
    let mut fs = fresh(4);
    tree(&mut fs);
    fs.mkdir("/b/a", NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/a".to_vec()], b"/b").unwrap();
    run(&mut job, &mut fs, COPY_CHUNK);
    assert_eq!(job.results(), &[b"/b/a (2)".to_vec()]);
    assert_eq!(fs.read_file("/b/a (2)/one.txt").unwrap(), b"one");
}

#[test]
fn copy_several_sources_with_clashing_names() {
    let mut fs = fresh(4);
    fs.mkdir("/p", NOW).unwrap();
    fs.mkdir("/q", NOW).unwrap();
    fs.mkdir("/dest", NOW).unwrap();
    fs.write_file("/p/n.txt", b"P", NOW).unwrap();
    fs.write_file("/q/n.txt", b"Q", NOW).unwrap();
    let mut job = CopyJob::plan(
        &mut fs,
        &[b"/p/n.txt".to_vec(), b"/q/n.txt".to_vec()],
        b"/dest",
    )
    .unwrap();
    run(&mut job, &mut fs, COPY_CHUNK);
    assert_eq!(fs.read_file("/dest/n.txt").unwrap(), b"P");
    assert_eq!(fs.read_file("/dest/n (2).txt").unwrap(), b"Q");
}

#[test]
fn copy_empty_files_and_empty_folders() {
    let mut fs = fresh(4);
    fs.mkdir("/src", NOW).unwrap();
    fs.mkdir("/src/empty", NOW).unwrap();
    fs.write_file("/src/zero", b"", NOW).unwrap();
    fs.mkdir("/dst", NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/src".to_vec()], b"/dst").unwrap();
    assert_eq!(job.total_bytes(), 0);
    let steps = run(&mut job, &mut fs, COPY_CHUNK);
    assert_eq!(steps, 1);
    assert_eq!(fs.stat("/dst/src/zero").unwrap().size, 0);
    assert!(fs.stat("/dst/src/empty").unwrap().kind == Kind::Dir);
}

#[test]
fn copy_progress_grows_monotonically_and_a_step_is_bounded() {
    let mut fs = fresh(8);
    fs.mkdir("/dst", NOW).unwrap();
    fs.write_file("/f", &pat(1_000_000, 9), NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/f".to_vec()], b"/dst").unwrap();
    let mut last = 0;
    let mut steps = 0;
    while job.step(&mut fs, 100_000, NOW).unwrap() == Progress::Running {
        let d = job.done_bytes();
        assert!(d > last && d - last <= 100_000);
        assert!(job.permille() <= 1000);
        last = d;
        steps += 1;
    }
    assert_eq!(steps, 9);
    assert_eq!(job.done_bytes(), 1_000_000);
}

#[test]
fn cancelling_a_copy_removes_the_partial_file_only() {
    let mut fs = fresh(8);
    fs.mkdir("/dst", NOW).unwrap();
    fs.write_file("/one", b"1", NOW).unwrap();
    fs.write_file("/two", &pat(500_000, 4), NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/one".to_vec(), b"/two".to_vec()], b"/dst").unwrap();
    // First step finishes /one and starts /two.
    assert_eq!(job.step(&mut fs, 100_000, NOW).unwrap(), Progress::Running);
    assert!(exists(&mut fs, b"/dst/two"));
    job.abort(&mut fs);
    assert!(!exists(&mut fs, b"/dst/two"));
    assert_eq!(fs.read_file("/dst/one").unwrap(), b"1");
    assert_eq!(job.step(&mut fs, 10, NOW).unwrap(), Progress::Done);
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "{r:?}");
}

#[test]
fn a_full_disk_fails_the_copy_and_cleans_up() {
    let mut fs = fresh(2); // ~1.9 MiB of data space
    fs.mkdir("/dst", NOW).unwrap();
    fs.write_file("/big", &pat(1_200_000, 5), NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/big".to_vec()], b"/dst").unwrap();
    let mut err = None;
    for _ in 0..100 {
        match job.step(&mut fs, 64 * 1024, NOW) {
            Ok(Progress::Running) => {}
            Ok(Progress::Done) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    assert_eq!(err, Some(VfsError::NoSpace));
    assert!(!exists(&mut fs, b"/dst/big"));
    assert_eq!(fs.read_file("/big").unwrap().len(), 1_200_000);
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "{r:?}");
}

#[test]
fn copy_of_a_missing_source_fails_to_plan() {
    let mut fs = fresh(4);
    fs.mkdir("/d", NOW).unwrap();
    assert_eq!(
        CopyJob::plan(&mut fs, &[b"/nope".to_vec()], b"/d").err(),
        Some(VfsError::NotFound)
    );
    fs.write_file("/f", b"", NOW).unwrap();
    assert_eq!(
        CopyJob::plan(&mut fs, &[b"/f".to_vec()], b"/f").err(),
        Some(VfsError::NotDir)
    );
}

#[test]
fn copy_a_three_megabyte_file_end_to_end() {
    let mut fs = fresh(16);
    let data = pat(3 * 1024 * 1024 + 17, 11);
    fs.write_file("/foto.bin", &data, NOW).unwrap();
    fs.mkdir("/Documentos", NOW).unwrap();
    let mut job = CopyJob::plan(&mut fs, &[b"/foto.bin".to_vec()], b"/Documentos").unwrap();
    run(&mut job, &mut fs, 256 * 1024);
    assert_eq!(fs.read_file("/Documentos/foto.bin").unwrap(), data);
    let rep = move_to(&mut fs, &[b"/foto.bin".to_vec()], b"/Documentos", NOW);
    assert_eq!(rep.moved, vec![b"/Documentos/foto (2).bin".to_vec()]);
    let new = rename_in(&mut fs, &rep.moved[0], b"renomeada.bin", NOW).unwrap();
    remove(&mut fs, &new, NOW).unwrap();
    let items = Backend::trash_list(&mut fs).unwrap();
    assert_eq!(items[0].name, b"renomeada.bin");
    Backend::trash_restore(&mut fs, &items[0].id, NOW).unwrap();
    assert_eq!(fs.read_file("/Documentos/renomeada.bin").unwrap(), data);
    Backend::empty_trash(&mut fs).unwrap();
    let r = fs.fsck().unwrap();
    assert!(r.is_clean(), "{r:?}");
}

#[test]
fn append_and_write_at_through_the_backend() {
    let mut fs = fresh(4);
    Backend::create(&mut fs, b"/log", NOW).unwrap();
    Backend::append(&mut fs, b"/log", b"ab", NOW).unwrap();
    Backend::append(&mut fs, b"/log", b"cd", NOW).unwrap();
    Backend::write_at(&mut fs, b"/log", 1, b"XY", NOW).unwrap();
    assert_eq!(fs.read_file("/log").unwrap(), b"aXYd");
    let mut buf = [0u8; 8];
    assert_eq!(Backend::read_at(&mut fs, b"/log", 2, &mut buf).unwrap(), 2);
    assert_eq!(&buf[..2], b"Yd");
    assert_eq!(
        Backend::read_at(&mut fs, b"/nope", 0, &mut buf),
        Err(VfsError::NotFound)
    );
}

#[test]
fn works_through_a_trait_object() {
    let mut fs = fresh(4);
    let b: &mut dyn Backend = &mut fs;
    new_folder(b, b"/", b"x", NOW).unwrap();
    new_file(b, b"/x", b"y.txt", NOW).unwrap();
    assert_eq!(list(b, b"/x").unwrap().len(), 1);
    remove(b, b"/x", NOW).unwrap();
    assert!(!exists(b, b"/x"));
}
