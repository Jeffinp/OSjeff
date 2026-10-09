//! Tests of the desktop VFS layer, on an OJFS v3 volume in RAM.

use super::*;
use crate::storage::blockdev::RamDisk;
use crate::storage::fs3::FormatOptions;
use alloc::string::String;
use alloc::vec;

const NOW: u64 = 1_700_000_000;

fn fresh(mib: u64) -> Fs3<RamDisk> {
    Fs3::format(
        RamDisk::new(mib * 2048),
        &FormatOptions::new(*b"0123456789abcdef", NOW),
    )
    .unwrap()
}

fn pat(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[2] ^ seed)
        .collect()
}

fn run(job: &mut CopyJob, fs: &mut Fs3<RamDisk>, budget: usize) -> usize {
    let mut steps = 0;
    loop {
        steps += 1;
        match job.step(fs, budget, NOW + 1).unwrap() {
            Progress::Done => return steps,
            Progress::Running => assert!(steps < 100_000, "copy never ends"),
        }
    }
}

fn names(es: &[Entry]) -> Vec<String> {
    let mut v: Vec<String> = es
        .iter()
        .map(|e| String::from_utf8_lossy(&e.name).into_owned())
        .collect();
    v.sort();
    v
}

// ---- paths ----

#[test]
fn join_handles_the_root() {
    assert_eq!(join(b"/", b"a"), b"/a");
    assert_eq!(join(b"/a", b"b"), b"/a/b");
}

#[test]
fn parent_and_base_name() {
    assert_eq!(parent(b"/a/b"), b"/a");
    assert_eq!(parent(b"/a"), b"/");
    assert_eq!(parent(b"/"), b"/");
    assert_eq!(base_name(b"/a/b.txt"), b"b.txt");
    assert_eq!(base_name(b"/a"), b"a");
    assert_eq!(base_name(b"/"), b"");
}

#[test]
fn is_inside_is_component_wise() {
    assert!(is_inside(b"/a/b", b"/a"));
    assert!(is_inside(b"/a", b"/a"));
    assert!(!is_inside(b"/ab", b"/a"));
    assert!(is_inside(b"/x", b"/"));
    assert!(!is_inside(b"/a", b"/a/b"));
}

#[test]
fn components_skip_empty_parts() {
    assert_eq!(components(b"/"), Vec::<&[u8]>::new());
    assert_eq!(components(b"/a/b"), vec![&b"a"[..], &b"b"[..]]);
}

// ---- names ----

#[test]
fn names_are_validated() {
    assert_eq!(validate_name(b"ok.txt"), Ok(()));
    assert_eq!(validate_name(b""), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"."), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b".."), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"a/b"), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"a\0b"), Err(VfsError::InvalidName));
    assert_eq!(validate_name(b"a\nb"), Err(VfsError::InvalidName));
    assert_eq!(validate_name(&[0xFF, 0xFE]), Err(VfsError::InvalidName));
    assert_eq!(validate_name(&[b'a'; 256]), Err(VfsError::NameTooLong));
    assert_eq!(validate_name(&[b'a'; 255]), Ok(()));
}

#[test]
fn utf8_names_are_valid() {
    assert_eq!(validate_name("relatório ção.txt".as_bytes()), Ok(()));
    assert_eq!(validate_name("日本語.png".as_bytes()), Ok(()));
}

#[test]
fn trim_name_strips_spaces_only_at_the_ends() {
    assert_eq!(trim_name(b"  a b  "), b"a b");
    assert_eq!(trim_name(b"   "), b"");
}

#[test]
fn split_ext_cases() {
    assert_eq!(split_ext(b"a.txt"), (&b"a"[..], &b".txt"[..]));
    assert_eq!(split_ext(b"a.tar.gz"), (&b"a.tar"[..], &b".gz"[..]));
    assert_eq!(split_ext(b".profile"), (&b".profile"[..], &b""[..]));
    assert_eq!(split_ext(b"end."), (&b"end."[..], &b""[..]));
    assert_eq!(split_ext(b"plain"), (&b"plain"[..], &b""[..]));
}

#[test]
fn unique_name_keeps_a_free_name() {
    assert_eq!(unique_name(b"a.txt", |_| false), b"a.txt");
}

#[test]
fn unique_name_adds_a_counter_before_the_extension() {
    assert_eq!(unique_name(b"a.txt", |n| n == b"a.txt"), b"a (2).txt");
    assert_eq!(
        unique_name(b"a.txt", |n| n == b"a.txt" || n == b"a (2).txt"),
        b"a (3).txt"
    );
}

#[test]
fn unique_name_replaces_an_existing_counter() {
    assert_eq!(
        unique_name(b"a (2).txt", |n| n == b"a (2).txt"),
        b"a (3).txt".to_vec()
    );
    // A non-numeric suffix is part of the name.
    assert_eq!(
        unique_name(b"a (x).txt", |n| n == b"a (x).txt"),
        b"a (x) (2).txt".to_vec()
    );
}

#[test]
fn unique_name_for_a_folder_and_a_dotfile() {
    assert_eq!(unique_name(b"Docs", |n| n == b"Docs"), b"Docs (2)");
    assert_eq!(unique_name(b".rc", |n| n == b".rc"), b".rc (2)");
}

#[test]
fn unique_name_never_exceeds_255_bytes_and_stays_utf8() {
    let long: String = "é".repeat(127); // 254 bytes
    let name = long.as_bytes().to_vec();
    let got = unique_name(&name, |n| n == &name[..]);
    assert!(got.len() <= 255);
    assert!(core::str::from_utf8(&got).is_ok());
    assert!(got.ends_with(b" (2)"));
    let mut full = vec![b'a'; 255];
    full[250..].copy_from_slice(b".abcd");
    let got = unique_name(&full, |n| n == &full[..]);
    assert!(got.len() <= 255);
    assert!(got.ends_with(b" (2).abcd"));
}

// ---- errors ----

#[test]
fn every_error_has_a_message_in_both_languages() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let all = [
        VfsError::NotFound,
        VfsError::Exists,
        VfsError::NotDir,
        VfsError::IsDir,
        VfsError::NotEmpty,
        VfsError::InvalidName,
        VfsError::NameTooLong,
        VfsError::InvalidPath,
        VfsError::Reserved,
        VfsError::InvalidMove,
        VfsError::NoSpace,
        VfsError::NoInodes,
        VfsError::TooBig,
        VfsError::Busy,
        VfsError::Unavailable,
        VfsError::Io,
        VfsError::Corrupt,
        VfsError::Cancelled,
    ];
    for l in Lang::ALL {
        let _g = LangGuard::new(l);
        for e in all {
            assert!(!e.message().is_empty());
            assert_ne!(e.message(), "files.err.", "{e:?}");
            assert!(!e.message().starts_with("files."), "{e:?}: key shown");
        }
    }
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(VfsError::NotFound.message(), "Item não encontrado");
    assert_eq!(VfsError::Cancelled.message(), "Operação cancelada");
    let _g = LangGuard::new(Lang::En);
    assert_eq!(VfsError::NotFound.message(), "Item not found");
    assert_eq!(VfsError::NoSpace.message(), "Disk full");
}

#[test]
fn fs_errors_map() {
    assert_eq!(VfsError::from(FsError::NotFound), VfsError::NotFound);
    assert_eq!(VfsError::from(FsError::NoSpace), VfsError::NoSpace);
    assert_eq!(
        VfsError::from(FsError::Io(crate::storage::blockdev::IoError::Read)),
        VfsError::Io
    );
    assert_eq!(VfsError::from(FsError::Corrupt("x")), VfsError::Corrupt);
    assert_eq!(VfsError::from(FsError::Poisoned), VfsError::Io);
}

// ---- backend basics ----

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

// ---- move ----

fn tree(fs: &mut Fs3<RamDisk>) {
    fs.mkdir("/a", NOW).unwrap();
    fs.mkdir("/a/sub", NOW).unwrap();
    fs.write_file("/a/one.txt", b"one", NOW).unwrap();
    fs.write_file("/a/sub/two.txt", &pat(10_000, 7), NOW)
        .unwrap();
    fs.mkdir("/b", NOW).unwrap();
}

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

// ---- copy ----

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
