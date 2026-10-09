use super::*;

#[test]
fn normalize_absolute_and_relative() {
    assert_eq!(normalize("/a/b", "c"), "/a/b/c");
    assert_eq!(normalize("/a/b", "/x/y"), "/x/y");
    assert_eq!(normalize("/a/b", ".."), "/a");
    assert_eq!(normalize("/a/b", "../.."), "/");
    assert_eq!(normalize("/a/b", "../../../.."), "/");
    assert_eq!(normalize("/a", "./b//c/./d/.."), "/a/b/c");
    assert_eq!(normalize("/", ""), "/");
    assert_eq!(normalize("/a", "/"), "/");
}

#[test]
fn path_helpers() {
    assert_eq!(parent("/a/b"), "/a");
    assert_eq!(parent("/a"), "/");
    assert_eq!(parent("/"), "/");
    assert_eq!(basename("/a/b.txt"), "b.txt");
    assert_eq!(basename("/"), "");
    assert_eq!(join("/", "x"), "/x");
    assert_eq!(join("/a", "x"), "/a/x");
}

#[test]
fn memfs_write_read_append() {
    let mut f = MemFs::new();
    f.write("/a.txt", b"hi").unwrap();
    f.append("/a.txt", b" there").unwrap();
    assert_eq!(f.read("/a.txt").unwrap(), b"hi there");
    assert_eq!(f.stat("a.txt").unwrap().size, 8);
    f.append("/new", b"x").unwrap();
    assert_eq!(f.read("new").unwrap(), b"x");
}

#[test]
fn memfs_requires_parent_directory() {
    let mut f = MemFs::new();
    assert_eq!(f.write("/no/such/file", b"x"), Err(FsErr::NotFound));
    f.write("/file", b"x").unwrap();
    assert_eq!(f.write("/file/child", b"x"), Err(FsErr::NotADirectory));
    assert_eq!(f.mkdir("/file/d"), Err(FsErr::NotADirectory));
}

#[test]
fn memfs_dirs_and_list() {
    let mut f = MemFs::new();
    f.mkdir("/d").unwrap();
    f.mkdir("/d/e").unwrap();
    f.write("/d/f", b"abc").unwrap();
    f.write("/top", b"").unwrap();
    let mut names: Vec<_> = f.list("/d").unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["e", "f"]);
    let root: Vec<_> = f.list("/").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(root.len(), 2);
    assert_eq!(f.list("/top"), Err(FsErr::NotADirectory));
    assert_eq!(f.list("/zzz"), Err(FsErr::NotFound));
    assert_eq!(f.mkdir("/d"), Err(FsErr::AlreadyExists));
}

#[test]
fn memfs_cwd_and_relative_paths() {
    let mut f = MemFs::new().with_dir("/home").with_file("/home/x", b"1");
    f.set_cwd("/home").unwrap();
    assert_eq!(f.cwd(), "/home");
    assert_eq!(f.read("x").unwrap(), b"1");
    assert_eq!(f.read("../home/./x").unwrap(), b"1");
    assert_eq!(f.set_cwd("x"), Err(FsErr::NotADirectory));
    assert_eq!(f.set_cwd("nope"), Err(FsErr::NotFound));
    f.set_cwd("..").unwrap();
    assert_eq!(f.cwd(), "/");
}

#[test]
fn memfs_remove_rules() {
    let mut f = MemFs::new().with_dir("/d").with_file("/d/x", b"");
    assert_eq!(f.remove("/d"), Err(FsErr::NotEmpty));
    f.remove("/d/x").unwrap();
    f.remove("/d").unwrap();
    assert_eq!(f.remove("/d"), Err(FsErr::NotFound));
    assert_eq!(f.remove("/"), Err(FsErr::InvalidPath));
}

#[test]
fn memfs_cannot_remove_cwd() {
    let mut f = MemFs::new().with_dir("/d");
    f.set_cwd("/d").unwrap();
    assert_eq!(f.remove("/d"), Err(FsErr::InvalidPath));
}

#[test]
fn memfs_rename_file_and_dir_tree() {
    let mut f = MemFs::new()
        .with_dir("/a")
        .with_dir("/a/b")
        .with_file("/a/b/c", b"data");
    f.rename("/a", "/z").unwrap();
    assert_eq!(f.read("/z/b/c").unwrap(), b"data");
    assert_eq!(f.stat("/a"), Err(FsErr::NotFound));
    assert_eq!(f.rename("/z", "/z/b/inner"), Err(FsErr::InvalidPath));
    f.write("/x", b"1").unwrap();
    f.write("/y", b"2").unwrap();
    f.rename("/x", "/y").unwrap();
    assert_eq!(f.read("/y").unwrap(), b"1");
    assert_eq!(f.rename("/nope", "/q"), Err(FsErr::NotFound));
}

#[test]
fn memfs_limits() {
    let mut f = MemFs::small();
    assert_eq!(f.write("/big", &[0u8; 5000]), Err(FsErr::TooBig));
    let long = alloc::format!("/{}", "n".repeat(40));
    assert_eq!(f.write(&long, b"x"), Err(FsErr::NameTooLong));
    for i in 0..8 {
        f.write(&alloc::format!("/f{i}"), &[1u8; 4000]).unwrap();
    }
    assert_eq!(f.write("/over", &[1u8; 4000]), Err(FsErr::NoSpace));
}

#[test]
fn memfs_read_at_default() {
    let mut f = MemFs::new().with_file("/a", b"0123456789");
    assert_eq!(f.read_at("/a", 3, 4).unwrap(), b"3456");
    assert_eq!(f.read_at("/a", 8, 100).unwrap(), b"89");
    assert_eq!(f.read_at("/a", 100, 5).unwrap(), b"");
}

#[test]
fn memfs_usage_counts() {
    let f = MemFs::new().with_dir("/d").with_file("/d/a", b"123");
    let u = f.usage();
    assert_eq!((u.files, u.used_bytes), (1, 3));
    assert!(u.dirs >= 2);
}

#[test]
fn fserr_messages_are_nonempty() {
    for e in [
        FsErr::NotFound,
        FsErr::NotADirectory,
        FsErr::IsADirectory,
        FsErr::AlreadyExists,
        FsErr::NotEmpty,
        FsErr::NoSpace,
        FsErr::TooBig,
        FsErr::NameTooLong,
        FsErr::InvalidPath,
        FsErr::ReadOnly,
        FsErr::Io,
    ] {
        assert!(!e.message().is_empty());
    }
}
