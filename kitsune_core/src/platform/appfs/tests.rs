use super::path::{MAX_COMPONENT, MAX_DEPTH, MAX_PATH, join, normalize, split, within};
use super::*;
use crate::platform::appabi::*;
use crate::platform::appmanifest::FsPerm;
use alloc::string::ToString;
use alloc::vec::Vec;

const MIB: u64 = 1 << 20;

fn norm(s: &str) -> Result<String, PathError> {
    normalize(s.as_bytes())
}

// ------------------------------------------------------------ path normalization

#[test]
fn canonical_forms() {
    for (raw, want) in [
        ("/", "/"),
        ("//", "/"),
        ("///", "/"),
        (".", "/"),
        ("./", "/"),
        ("/.", "/"),
        ("a", "/a"),
        ("/a", "/a"),
        ("a/", "/a"),
        ("//a//b///c//", "/a/b/c"),
        ("/./a/./b/.", "/a/b"),
        ("a/../b", "/b"),
        ("/a/b/..", "/a"),
        ("/a/b/../..", "/"),
        ("a/b/../../c", "/c"),
        ("/a/./../a/b", "/a/b"),
        ("notes/2024/jan.txt", "/notes/2024/jan.txt"),
        ("my file.txt", "/my file.txt"),
        (".hidden", "/.hidden"),
        ("a.b.c", "/a.b.c"),
        ("%2e%2e/%2e%2e/etc", "/%2e%2e/%2e%2e/etc"),
        ("~", "/~"),
        ("~/x", "/~/x"),
        ("a b/c d", "/a b/c d"),
    ] {
        assert_eq!(norm(raw).as_deref(), Ok(want), "{raw:?}");
    }
}

#[test]
fn climbing_above_the_root_is_an_error_not_a_clamp() {
    for raw in [
        "..",
        "../",
        "/..",
        "/../",
        "../..",
        "../../etc/x",
        "/../../etc/x",
        "a/../..",
        "a/../../b",
        "a/b/../../..",
        "./../x",
        "/a/../../x",
        "x/../../../../../../etc/passwd",
        "..//..//etc",
        "/./../.",
    ] {
        assert_eq!(norm(raw), Err(PathError::Escapes), "{raw:?}");
    }
}

#[test]
fn hostile_names_are_refused() {
    for raw in [
        "",
        "...",
        "....",
        "a/.../b",
        "a/....",
        "a\\b",
        "..\\..\\x",
        "C:\\x",
        "c:x",
        "a:b",
        "a*b",
        "a?b",
        "a\"b",
        "a<b",
        "a>b",
        "a|b",
        "a\0b",
        "a\nb",
        "a\rb",
        "a\tb",
        "\x7f",
        "\x1b[2J",
        "caf\u{e9}",
        "\u{202e}txt.exe",
        "a\u{0}",
        " lead",
        "trail ",
        "a/ lead",
        "a/trail ",
        "trailing.",
        "a/trailing./b",
        "/a/ /b",
    ] {
        let r = norm(raw);
        assert!(
            matches!(r, Err(PathError::BadName) | Err(PathError::Empty)),
            "{raw:?} -> {r:?}"
        );
    }
}

#[test]
fn raw_bytes_that_are_not_text() {
    for raw in [
        &[0xFFu8, 0xFE][..],
        &[b'a', 0x80][..],
        &[0xC0, 0xAF][..],
        &[0u8][..],
    ] {
        assert_eq!(normalize(raw), Err(PathError::BadName));
    }
    // overlong "../" encodings are just invalid bytes, never a traversal
    assert_eq!(
        normalize(&[0xC0, 0xAE, 0xC0, 0xAE, b'/', b'x']),
        Err(PathError::BadName)
    );
}

#[test]
fn length_and_depth_limits() {
    let comp = "a".repeat(MAX_COMPONENT);
    assert!(norm(&comp).is_ok());
    assert_eq!(
        norm(&"a".repeat(MAX_COMPONENT + 1)),
        Err(PathError::BadName)
    );
    let deep = ["d"; MAX_DEPTH].join("/");
    assert!(norm(&deep).is_ok());
    let deeper = ["d"; MAX_DEPTH + 1].join("/");
    assert_eq!(norm(&deeper), Err(PathError::TooDeep));
    // depth counts after normalization: going up and down is fine
    let wiggle = "a/../".repeat(100) + "b";
    assert_eq!(norm(&wiggle), Err(PathError::TooLong)); // 100*5+1 > 256
    let wiggle = "a/../".repeat(40) + "b";
    assert_eq!(norm(&wiggle).as_deref(), Ok("/b"));
    assert_eq!(norm(&"/".repeat(MAX_PATH)).as_deref(), Ok("/"));
    assert_eq!(norm(&"/".repeat(MAX_PATH + 1)), Err(PathError::TooLong));
}

#[test]
fn normalize_is_idempotent() {
    for raw in ["/a/b", "a//b/./c/../d", "/", "x y/z"] {
        let n = norm(raw).unwrap();
        assert_eq!(norm(&n).unwrap(), n);
    }
}

#[test]
fn path_helpers() {
    assert_eq!(join("/data/x", "/"), "/data/x");
    assert_eq!(join("/data/x", "/a/b"), "/data/x/a/b");
    assert_eq!(join("", "/"), "/");
    assert_eq!(join("", "/a"), "/a");
    assert_eq!(split("/a/b"), Some(("/a", "b")));
    assert_eq!(split("/a"), Some(("/", "a")));
    assert_eq!(split("/"), None);
    assert!(within("/a/b", "/a"));
    assert!(within("/a", "/a"));
    assert!(!within("/ab", "/a"));
    assert!(within("/anything", "/"));
}

/// A tiny deterministic generator for the property test below.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn property_no_generated_path_resolves_outside_the_prefix() {
    // Hostile alphabet: everything that matters to traversal.
    let atoms: [&str; 16] = [
        "..", ".", "/", "//", "a", "b", "...", "\\", "%2e", "~", ":", " ", "/..", "../", "x.y",
        "\0",
    ];
    let mut rng = Lcg(0xC0FFEE);
    let mut accepted = 0;
    for _ in 0..30_000 {
        let n = 1 + (rng.next() % 9) as usize;
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(atoms[(rng.next() % atoms.len() as u64) as usize]);
        }
        if let Ok(rel) = normalize(s.as_bytes()) {
            accepted += 1;
            assert!(rel.starts_with('/'));
            assert!(!rel.contains("//"));
            assert!(
                !rel.split('/').any(|c| c == ".." || c == "."),
                "{s:?} -> {rel}"
            );
            let real = join("/data/app", &rel);
            assert!(within(&real, "/data/app"), "{s:?} -> {real}");
            // re-normalizing the real path from the root must not climb either
            assert_eq!(normalize(real.as_bytes()).as_deref(), Ok(real.as_str()));
        }
    }
    assert!(
        accepted > 1000,
        "generator is too hostile to be useful: {accepted}"
    );
}

// ------------------------------------------------------------ MemFs

#[test]
fn memfs_basic_tree() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir("/a").unwrap();
    fs.create("/a/f").unwrap();
    assert_eq!(fs.write_at("/a/f", 0, b"hello").unwrap(), 5);
    assert_eq!(
        fs.stat("/a/f").unwrap(),
        Stat {
            kind: Kind::File,
            size: 5
        }
    );
    let mut b = [0u8; 16];
    assert_eq!(fs.read_at("/a/f", 1, &mut b).unwrap(), 4);
    assert_eq!(&b[..4], b"ello");
    assert_eq!(fs.read_at("/a/f", 99, &mut b).unwrap(), 0);
    assert_eq!(fs.stat("/a").unwrap().kind, Kind::Dir);
    assert_eq!(fs.create("/a/f"), Err(FsError::Exists));
    assert_eq!(fs.mkdir("/a"), Err(FsError::Exists));
    assert_eq!(fs.create("/nope/f"), Err(FsError::NotFound));
    assert_eq!(fs.create("/a/f/g"), Err(FsError::NotDir));
    assert_eq!(fs.remove("/a"), Err(FsError::NotEmpty));
    fs.remove("/a/f").unwrap();
    fs.remove("/a").unwrap();
    assert_eq!(fs.stat("/a"), Err(FsError::NotFound));
    assert_eq!(fs.remove("/"), Err(FsError::Invalid));
    assert!(fs.is_empty());
}

#[test]
fn memfs_zero_fills_holes_and_truncates() {
    let mut fs = MemFs::new(MIB);
    fs.create("/f").unwrap();
    fs.write_at("/f", 4, b"xy").unwrap();
    let mut b = [9u8; 6];
    assert_eq!(fs.read_at("/f", 0, &mut b).unwrap(), 6);
    assert_eq!(&b, &[0, 0, 0, 0, b'x', b'y']);
    fs.set_len("/f", 3).unwrap();
    assert_eq!(fs.stat("/f").unwrap().size, 3);
    fs.set_len("/f", 5).unwrap();
    assert_eq!(fs.used_bytes(), 5);
    assert_eq!(fs.set_len("/f", MIB + 1), Err(FsError::NoSpace));
}

#[test]
fn memfs_readdir_lists_direct_children_in_order() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir("/d").unwrap();
    fs.mkdir("/d/sub").unwrap();
    fs.create("/d/b").unwrap();
    fs.create("/d/a").unwrap();
    fs.create("/d/sub/deep").unwrap();
    fs.create("/dd").unwrap();
    let names: Vec<_> = (0..)
        .map_while(|i| fs.read_dir("/d", i).unwrap())
        .map(|e| (e.name, e.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("a".to_string(), Kind::File),
            ("b".to_string(), Kind::File),
            ("sub".to_string(), Kind::Dir)
        ]
    );
    assert_eq!(fs.read_dir("/d/a", 0), Err(FsError::NotDir));
    assert_eq!(fs.read_dir("/zz", 0), Err(FsError::NotFound));
    assert_eq!(fs.read_dir("/", 0).unwrap().unwrap().name, "d");
}

#[test]
fn memfs_rename_moves_subtrees() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir("/a").unwrap();
    fs.create("/a/f").unwrap();
    fs.write_at("/a/f", 0, b"data").unwrap();
    fs.mkdir("/z").unwrap();
    fs.rename("/a", "/z/b").unwrap();
    assert_eq!(fs.stat("/a"), Err(FsError::NotFound));
    assert_eq!(fs.stat("/z/b/f").unwrap().size, 4);
    assert_eq!(fs.rename("/z", "/z/b/inside"), Err(FsError::Invalid));
    assert_eq!(fs.rename("/z/b", "/z"), Err(FsError::Exists));
    assert_eq!(fs.rename("/nope", "/x"), Err(FsError::NotFound));
    assert_eq!(fs.rename("/", "/x"), Err(FsError::Invalid));
    assert_eq!(fs.rename("/z", "/"), Err(FsError::Invalid));
    assert_eq!(fs.rename("/z", "/q/r"), Err(FsError::NotFound));
    assert_eq!(fs.used_bytes(), 4);
}

#[test]
fn memfs_rejects_non_canonical_paths_itself() {
    let mut fs = MemFs::new(MIB);
    for p in [
        "a", "/a/", "//a", "/a/../b", "/a/./b", "/..", "", "/a\0b", "/a\\b",
    ] {
        assert_eq!(fs.stat(p), Err(FsError::Invalid), "{p:?}");
        assert_eq!(fs.create(p), Err(FsError::Invalid), "{p:?}");
        assert_eq!(fs.mkdir(p), Err(FsError::Invalid), "{p:?}");
    }
}

#[test]
fn memfs_capacity_limits() {
    let mut fs = MemFs::new(100);
    fs.create("/f").unwrap();
    assert_eq!(fs.write_at("/f", 0, &[0; 101]), Err(FsError::NoSpace));
    assert_eq!(fs.write_at("/f", 0, &[0; 100]).unwrap(), 100);
    fs.create("/g").unwrap();
    assert_eq!(fs.write_at("/g", 0, b"x"), Err(FsError::NoSpace));
    assert_eq!(
        fs.write_at("/f", u64::MAX - 1, b"abc"),
        Err(FsError::Invalid)
    );
    let mut fs = MemFs::new(u64::MAX);
    fs.create("/f").unwrap();
    assert_eq!(
        fs.write_at("/f", super::memfs::MAX_FILE, b"x"),
        Err(FsError::NoSpace)
    );
}

#[test]
fn memfs_tree_size() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir("/d").unwrap();
    fs.create("/d/f").unwrap();
    fs.write_at("/d/f", 0, &[1; 10]).unwrap();
    fs.mkdir("/d/s").unwrap();
    assert_eq!(fs.tree_size("/d").unwrap(), 2 * ENTRY_OVERHEAD + 10);
    assert_eq!(fs.tree_size("/").unwrap(), 3 * ENTRY_OVERHEAD + 10);
    assert_eq!(fs.tree_size("/nope"), Err(FsError::NotFound));
}

#[test]
fn mkdir_all_creates_parents() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir_all("/data/app").unwrap();
    fs.mkdir_all("/data/app").unwrap();
    assert_eq!(fs.stat("/data/app").unwrap().kind, Kind::Dir);
}

// ------------------------------------------------------------ Sandbox

fn sb(perm: FsPerm, id: &str) -> Sandbox {
    Sandbox::new(perm, id, 64 * 1024, 8).unwrap()
}

fn rw() -> u32 {
    O_READ | O_WRITE | O_CREATE
}

fn put(s: &mut Sandbox, fs: &mut MemFs, path: &str, data: &[u8]) {
    let fd = s
        .open(fs, path.as_bytes(), O_WRITE | O_CREATE | O_TRUNC)
        .unwrap();
    assert_eq!(s.write(fs, fd, data).unwrap(), data.len());
    s.close(fd).unwrap();
}

fn get(s: &mut Sandbox, fs: &mut MemFs, path: &str) -> Vec<u8> {
    let fd = s.open(fs, path.as_bytes(), O_READ).unwrap();
    let mut out = Vec::new();
    let mut b = [0u8; 100];
    loop {
        let n = s.read(fs, fd, &mut b).unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&b[..n]);
    }
    s.close(fd).unwrap();
    out
}

#[test]
fn none_permission_denies_everything() {
    let mut fs = MemFs::new(MIB);
    let mut s = Sandbox::new(FsPerm::None, "app", 0, 4).unwrap();
    assert_eq!(s.open(&mut fs, b"/a", rw()), Err(FsError::Perm));
    assert_eq!(s.stat(&mut fs, b"/"), Err(FsError::Perm));
    assert_eq!(s.read_dir(&mut fs, b"/", 0), Err(FsError::Perm));
    assert_eq!(s.mkdir(&mut fs, b"/d"), Err(FsError::Perm));
    assert_eq!(s.unlink(&mut fs, b"/d"), Err(FsError::Perm));
    assert_eq!(s.rename(&mut fs, b"/a", b"/b"), Err(FsError::Perm));
    assert!(fs.is_empty(), "a denied app must not even create its root");
    assert_eq!(FsError::Perm.code(), ERR_PERM);
}

#[test]
fn invalid_ids_cannot_make_a_sandbox() {
    for id in ["", "../x", "A", "a/b", "a b", "a..b"] {
        assert!(Sandbox::new(FsPerm::Own, id, 1, 1).is_none(), "{id:?}");
    }
}

#[test]
fn own_root_is_data_id_and_invisible_to_the_guest() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "notes");
    put(&mut s, &mut fs, "/hello.txt", b"hi");
    assert_eq!(fs.stat("/data/notes/hello.txt").unwrap().size, 2);
    assert_eq!(fs.stat("/hello.txt"), Err(FsError::NotFound));
    assert_eq!(get(&mut s, &mut fs, "hello.txt"), b"hi");
    assert_eq!(get(&mut s, &mut fs, "//./hello.txt"), b"hi");
    // the guest's "/" listing shows only its own files
    let e = s.read_dir(&mut fs, b"/", 0).unwrap().unwrap();
    assert_eq!(e.name, "hello.txt");
    assert_eq!(s.read_dir(&mut fs, b"/", 1).unwrap(), None);
}

#[test]
fn home_root_is_home() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir_all("/home").unwrap();
    fs.create("/home/existing").unwrap();
    let mut s = sb(FsPerm::Home, "viewer");
    assert_eq!(s.stat(&mut fs, b"/existing").unwrap().kind, Kind::File);
    put(&mut s, &mut fs, "/new.txt", b"x");
    assert_eq!(fs.stat("/home/new.txt").unwrap().size, 1);
    assert_eq!(fs.stat("/data"), Err(FsError::NotFound));
}

#[test]
fn escapes_are_refused_counted_and_touch_nothing() {
    let mut fs = MemFs::new(MIB);
    fs.mkdir_all("/etc").unwrap();
    fs.create("/etc/x").unwrap();
    fs.write_at("/etc/x", 0, b"secret").unwrap();
    let mut s = sb(FsPerm::Own, "evil");
    for p in [
        "../../etc/x",
        "/../../etc/x",
        "..",
        "a/../../etc/x",
        "../evil/../../etc/x",
        "/data/evil/../../../etc/x",
        "x/../../..",
    ] {
        assert_eq!(
            s.open(&mut fs, p.as_bytes(), O_READ),
            Err(FsError::Perm),
            "{p}"
        );
        assert_eq!(
            s.open(&mut fs, p.as_bytes(), rw()),
            Err(FsError::Perm),
            "{p}"
        );
        assert_eq!(s.stat(&mut fs, p.as_bytes()), Err(FsError::Perm), "{p}");
        assert_eq!(s.mkdir(&mut fs, p.as_bytes()), Err(FsError::Perm), "{p}");
        assert_eq!(s.unlink(&mut fs, p.as_bytes()), Err(FsError::Perm), "{p}");
        assert_eq!(
            s.rename(&mut fs, p.as_bytes(), b"/y"),
            Err(FsError::Perm),
            "{p}"
        );
        assert_eq!(
            s.rename(&mut fs, b"/y", p.as_bytes()),
            Err(FsError::Perm),
            "{p}"
        );
        assert_eq!(
            s.read_dir(&mut fs, p.as_bytes(), 0),
            Err(FsError::Perm),
            "{p}"
        );
    }
    assert!(s.escapes >= 7 * 8);
    assert_eq!(fs.stat("/etc/x").unwrap().size, 6);
    let mut b = [0u8; 8];
    assert_eq!(fs.read_at("/etc/x", 0, &mut b).unwrap(), 6);
    // the absolute-looking guest path `/data/evil/x` is *inside* the sandbox
    s.mkdir(&mut fs, b"/data").unwrap();
    s.mkdir(&mut fs, b"/data/evil").unwrap();
    put(&mut s, &mut fs, "/data/evil/x", b"inner");
    assert_eq!(fs.stat("/data/evil/data/evil/x").unwrap().size, 5);
}

#[test]
fn guest_cannot_name_another_apps_directory() {
    let mut fs = MemFs::new(MIB);
    let mut a = sb(FsPerm::Own, "alpha");
    let mut b = sb(FsPerm::Own, "beta");
    put(&mut a, &mut fs, "/secret.txt", b"alpha-secret");
    for p in [
        "/data/alpha/secret.txt",
        "../alpha/secret.txt",
        "/../alpha/secret.txt",
        "../../data/alpha/secret.txt",
        "/alpha/secret.txt",
        "secret.txt",
    ] {
        let r = b.open(&mut fs, p.as_bytes(), O_READ);
        assert!(r.is_err(), "{p} leaked alpha's file to beta");
    }
    put(&mut b, &mut fs, "/secret.txt", b"beta");
    assert_eq!(get(&mut a, &mut fs, "/secret.txt"), b"alpha-secret");
    assert_eq!(get(&mut b, &mut fs, "/secret.txt"), b"beta");
}

#[test]
fn open_flag_validation() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "f");
    assert_eq!(s.open(&mut fs, b"/a", 0), Err(FsError::Invalid));
    assert_eq!(s.open(&mut fs, b"/a", O_CREATE), Err(FsError::Invalid));
    assert_eq!(
        s.open(&mut fs, b"/a", O_READ | O_CREATE),
        Err(FsError::Invalid)
    );
    assert_eq!(
        s.open(&mut fs, b"/a", O_READ | O_TRUNC),
        Err(FsError::Invalid)
    );
    assert_eq!(
        s.open(&mut fs, b"/a", O_READ | O_APPEND),
        Err(FsError::Invalid)
    );
    assert_eq!(s.open(&mut fs, b"/a", 32 | O_READ), Err(FsError::Invalid));
    assert_eq!(s.open(&mut fs, b"/a", u32::MAX), Err(FsError::Invalid));
    assert_eq!(s.open(&mut fs, b"/a", O_READ), Err(FsError::NotFound));
    assert_eq!(s.open(&mut fs, b"/", O_READ), Err(FsError::IsDir));
    assert_eq!(s.open(&mut fs, b"/", rw()), Err(FsError::IsDir));
}

#[test]
fn descriptor_table_limit_and_reuse() {
    let mut fs = MemFs::new(MIB);
    let mut s = Sandbox::new(FsPerm::Own, "fdtest", 64 * 1024, 4).unwrap();
    let fds: Vec<i32> = (0..4)
        .map(|_| s.open(&mut fs, b"/f", rw()).unwrap())
        .collect();
    assert_eq!(fds, [1, 2, 3, 4]);
    assert_eq!(s.open_count(), 4);
    assert_eq!(s.open(&mut fs, b"/f", rw()), Err(FsError::TooManyFds));
    assert_eq!(FsError::TooManyFds.code(), ERR_MFILE);
    s.close(2).unwrap();
    assert_eq!(s.open(&mut fs, b"/f", rw()).unwrap(), 2);
    assert_eq!(s.close(2), Ok(()));
    assert_eq!(s.close(2), Err(FsError::BadFd));
    s.close_all();
    assert_eq!(s.open_count(), 0);
    assert_eq!(s.open(&mut fs, b"/f", rw()).unwrap(), 1);
}

#[test]
fn bad_descriptors() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "bad");
    let mut b = [0u8; 4];
    for fd in [0, -1, 9, 100, i32::MIN, i32::MAX] {
        assert_eq!(s.read(&mut fs, fd, &mut b), Err(FsError::BadFd), "{fd}");
        assert_eq!(s.write(&mut fs, fd, b"x"), Err(FsError::BadFd), "{fd}");
        assert_eq!(
            s.seek(&mut fs, fd, 0, SEEK_SET),
            Err(FsError::BadFd),
            "{fd}"
        );
        assert_eq!(s.close(fd), Err(FsError::BadFd), "{fd}");
    }
}

#[test]
fn read_and_write_modes() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "modes");
    put(&mut s, &mut fs, "/f", b"abc");
    let ro = s.open(&mut fs, b"/f", O_READ).unwrap();
    assert_eq!(s.write(&mut fs, ro, b"x"), Err(FsError::BadFd));
    let wo = s.open(&mut fs, b"/f", O_WRITE).unwrap();
    let mut b = [0u8; 4];
    assert_eq!(s.read(&mut fs, wo, &mut b), Err(FsError::BadFd));
    assert_eq!(s.write(&mut fs, wo, b"Z").unwrap(), 1);
    assert_eq!(get(&mut s, &mut fs, "/f"), b"Zbc");
}

#[test]
fn seek_semantics() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "seek");
    put(&mut s, &mut fs, "/f", b"0123456789");
    let fd = s.open(&mut fs, b"/f", O_READ | O_WRITE).unwrap();
    assert_eq!(s.seek(&mut fs, fd, 4, SEEK_SET), Ok(4));
    let mut b = [0u8; 3];
    assert_eq!(s.read(&mut fs, fd, &mut b), Ok(3));
    assert_eq!(&b, b"456");
    assert_eq!(s.seek(&mut fs, fd, -2, SEEK_CUR), Ok(5));
    assert_eq!(s.seek(&mut fs, fd, -1, SEEK_END), Ok(9));
    assert_eq!(s.seek(&mut fs, fd, 0, SEEK_END), Ok(10));
    assert_eq!(s.seek(&mut fs, fd, -1, SEEK_SET), Err(FsError::Invalid));
    assert_eq!(s.seek(&mut fs, fd, -11, SEEK_END), Err(FsError::Invalid));
    assert_eq!(
        s.seek(&mut fs, fd, i64::MAX, SEEK_CUR),
        Err(FsError::Invalid)
    );
    assert_eq!(s.seek(&mut fs, fd, 0, 7), Err(FsError::Invalid));
    assert_eq!(
        s.seek(&mut fs, fd, 1 << 31, SEEK_SET),
        Err(FsError::Invalid)
    );
    // seek past the end and write: the hole is zero-filled
    assert_eq!(s.seek(&mut fs, fd, 12, SEEK_SET), Ok(12));
    assert_eq!(s.write(&mut fs, fd, b"!").unwrap(), 1);
    assert_eq!(get(&mut s, &mut fs, "/f"), b"0123456789\0\0!");
}

#[test]
fn append_and_truncate() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "app");
    put(&mut s, &mut fs, "/log", b"one");
    let fd = s.open(&mut fs, b"/log", O_WRITE | O_APPEND).unwrap();
    s.seek(&mut fs, fd, 0, SEEK_SET).unwrap();
    assert_eq!(s.write(&mut fs, fd, b"two").unwrap(), 3);
    assert_eq!(get(&mut s, &mut fs, "/log"), b"onetwo");
    let used = s.used();
    let fd2 = s.open(&mut fs, b"/log", O_WRITE | O_TRUNC).unwrap();
    assert!(s.used() < used, "truncation refunds quota");
    assert_eq!(get(&mut s, &mut fs, "/log"), b"");
    s.close(fd).unwrap();
    s.close(fd2).unwrap();
}

#[test]
fn directories_through_the_sandbox() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "dirs");
    s.mkdir(&mut fs, b"/docs").unwrap();
    assert_eq!(s.mkdir(&mut fs, b"/docs"), Err(FsError::Exists));
    assert_eq!(s.mkdir(&mut fs, b"/"), Err(FsError::Exists));
    assert_eq!(s.mkdir(&mut fs, b"/a/b"), Err(FsError::NotFound));
    put(&mut s, &mut fs, "/docs/a.txt", b"A");
    assert_eq!(s.unlink(&mut fs, b"/docs"), Err(FsError::NotEmpty));
    assert_eq!(s.open(&mut fs, b"/docs", O_READ), Err(FsError::IsDir));
    s.unlink(&mut fs, b"/docs/a.txt").unwrap();
    s.unlink(&mut fs, b"/docs").unwrap();
    assert_eq!(s.unlink(&mut fs, b"/docs"), Err(FsError::NotFound));
    assert_eq!(
        s.unlink(&mut fs, b"/"),
        Err(FsError::Perm),
        "cannot remove own root"
    );
    assert_eq!(s.unlink(&mut fs, b"."), Err(FsError::Perm));
    assert_eq!(s.unlink(&mut fs, b"a/.."), Err(FsError::Perm));
}

#[test]
fn rename_inside_the_sandbox_and_open_fds_follow() {
    let mut fs = MemFs::new(MIB);
    let mut s = sb(FsPerm::Own, "ren");
    put(&mut s, &mut fs, "/a.txt", b"payload");
    s.mkdir(&mut fs, b"/d").unwrap();
    let fd = s.open(&mut fs, b"/a.txt", O_READ).unwrap();
    s.rename(&mut fs, b"/a.txt", b"/d/b.txt").unwrap();
    let mut b = [0u8; 16];
    assert_eq!(
        s.read(&mut fs, fd, &mut b),
        Ok(7),
        "descriptor follows the rename"
    );
    assert_eq!(s.stat(&mut fs, b"/a.txt"), Err(FsError::NotFound));
    assert_eq!(
        s.rename(&mut fs, b"/d/b.txt", b"/d/b.txt"),
        Err(FsError::Exists)
    );
    assert_eq!(s.rename(&mut fs, b"/", b"/x"), Err(FsError::Perm));
    assert_eq!(s.rename(&mut fs, b"/d", b"/"), Err(FsError::Perm));
    // moving a folder moves open files below it
    s.rename(&mut fs, b"/d", b"/e").unwrap();
    s.seek(&mut fs, fd, 0, SEEK_SET).unwrap();
    assert_eq!(s.read(&mut fs, fd, &mut b), Ok(7));
}

#[test]
fn quota_is_enforced_with_partial_writes() {
    let mut fs = MemFs::new(10 * MIB);
    let quota = 1000 + ENTRY_OVERHEAD;
    let mut s = Sandbox::new(FsPerm::Own, "quota", quota, 4).unwrap();
    let fd = s.open(&mut fs, b"/big", rw()).unwrap();
    assert_eq!(s.write(&mut fs, fd, &[1; 600]).unwrap(), 600);
    assert_eq!(
        s.write(&mut fs, fd, &[1; 600]).unwrap(),
        400,
        "partial up to the limit"
    );
    assert_eq!(s.write(&mut fs, fd, &[1; 1]), Err(FsError::NoSpace));
    assert_eq!(FsError::NoSpace.code(), ERR_NOSPC);
    assert_eq!(s.used(), quota);
    assert_eq!(fs.stat("/data/quota/big").unwrap().size, 1000);
    // freeing space makes room again
    s.unlink(&mut fs, b"/big").unwrap();
    assert_eq!(s.used(), 0);
    assert!(s.open(&mut fs, b"/small", rw()).is_ok());
}

#[test]
fn quota_counts_entries_and_folders() {
    let mut fs = MemFs::new(10 * MIB);
    let mut s = Sandbox::new(FsPerm::Own, "entries", ENTRY_OVERHEAD * 3, 4).unwrap();
    s.mkdir(&mut fs, b"/a").unwrap();
    s.mkdir(&mut fs, b"/b").unwrap();
    s.mkdir(&mut fs, b"/c").unwrap();
    assert_eq!(s.mkdir(&mut fs, b"/d"), Err(FsError::NoSpace));
    assert_eq!(s.open(&mut fs, b"/f", rw()), Err(FsError::NoSpace));
    assert!(fs.stat("/data/entries/d").is_err() && fs.stat("/data/entries/f").is_err());
    assert_eq!(s.used(), ENTRY_OVERHEAD * 3);
}

#[test]
fn quota_charges_the_hole_of_a_far_write() {
    let mut fs = MemFs::new(10 * MIB);
    let mut s = Sandbox::new(FsPerm::Own, "hole", 4096 + ENTRY_OVERHEAD, 4).unwrap();
    let fd = s.open(&mut fs, b"/f", rw()).unwrap();
    s.seek(&mut fs, fd, 1 << 20, SEEK_SET).unwrap();
    assert_eq!(s.write(&mut fs, fd, b"x"), Err(FsError::NoSpace));
    assert_eq!(
        fs.stat("/data/hole/f").unwrap().size,
        0,
        "nothing was written"
    );
    s.seek(&mut fs, fd, 4000, SEEK_SET).unwrap();
    assert_eq!(s.write(&mut fs, fd, &[1; 200]).unwrap(), 96);
}

#[test]
fn quota_starts_from_what_is_already_stored() {
    let mut fs = MemFs::new(10 * MIB);
    {
        let mut s = Sandbox::new(FsPerm::Own, "persist", 10_000, 4).unwrap();
        put(&mut s, &mut fs, "/f", &[7; 3000]);
    }
    let mut again = Sandbox::new(FsPerm::Own, "persist", 10_000, 4).unwrap();
    again.stat(&mut fs, b"/").unwrap();
    assert_eq!(again.used(), 3000 + ENTRY_OVERHEAD);
    let fd = again.open(&mut fs, b"/g", rw()).unwrap();
    assert_eq!(
        again.write(&mut fs, fd, &[0; 9000]).unwrap(),
        10_000 - 3000 - 2 * ENTRY_OVERHEAD as usize
    );
}

#[test]
fn home_quota_counts_only_this_run() {
    let mut fs = MemFs::new(10 * MIB);
    fs.mkdir_all("/home").unwrap();
    fs.create("/home/old").unwrap();
    fs.write_at("/home/old", 0, &[0; 5000]).unwrap();
    let mut s = Sandbox::new(FsPerm::Home, "viewer", 1000 + ENTRY_OVERHEAD, 4).unwrap();
    assert_eq!(s.stat(&mut fs, b"/old").unwrap().size, 5000);
    assert_eq!(s.used(), 0);
    let fd = s.open(&mut fs, b"/new", rw()).unwrap();
    assert_eq!(s.write(&mut fs, fd, &[1; 2000]).unwrap(), 1000);
}

#[test]
fn per_call_io_is_capped() {
    let mut fs = MemFs::new(10 * MIB);
    let mut s = Sandbox::new(FsPerm::Own, "io", 2 * MIB, 4).unwrap();
    let fd = s.open(&mut fs, b"/f", rw()).unwrap();
    let data = alloc::vec![5u8; MAX_IO * 2];
    assert_eq!(s.write(&mut fs, fd, &data).unwrap(), MAX_IO);
    s.seek(&mut fs, fd, 0, SEEK_SET).unwrap();
    let mut big = alloc::vec![0u8; MAX_IO * 2];
    assert_eq!(s.read(&mut fs, fd, &mut big).unwrap(), MAX_IO);
    assert_eq!(s.write(&mut fs, fd, b"").unwrap(), 0);
}

#[test]
fn backend_errors_do_not_leak_quota() {
    let mut fs = MemFs::new(10);
    let mut s = Sandbox::new(FsPerm::Own, "leak", 1 << 20, 4).unwrap();
    let fd = s.open(&mut fs, b"/f", rw()).unwrap();
    let before = s.used();
    // backend capacity (10 bytes) is smaller than the app's quota
    assert!(s.write(&mut fs, fd, &[1; 100]).is_err());
    assert_eq!(s.used(), before);
}

#[test]
fn write_at_the_offset_cap() {
    let mut fs = MemFs::new(u64::MAX);
    let mut s = Sandbox::new(FsPerm::Own, "cap", u64::MAX / 2, 4).unwrap();
    let fd = s.open(&mut fs, b"/f", rw()).unwrap();
    s.seek(&mut fs, fd, sandbox::MAX_OFFSET as i64, SEEK_SET)
        .unwrap();
    assert_eq!(s.write(&mut fs, fd, b"x"), Err(FsError::NoSpace));
}

use super::sandbox;

#[test]
fn error_codes_and_display() {
    assert_eq!(FsError::NotFound.code(), ERR_NOENT);
    assert_eq!(FsError::Exists.code(), ERR_EXIST);
    assert_eq!(FsError::BadFd.code(), ERR_BADF);
    assert_eq!(FsError::Invalid.code(), ERR_INVAL);
    assert_eq!(FsError::NotDir.code(), ERR_NOTDIR);
    assert_eq!(FsError::IsDir.code(), ERR_ISDIR);
    assert_eq!(FsError::NotEmpty.code(), ERR_NOTEMPTY);
    assert_eq!(FsError::from(PathError::Escapes), FsError::Perm);
    assert_eq!(FsError::from(PathError::BadName), FsError::Invalid);
    for e in [
        FsError::NotFound,
        FsError::Perm,
        FsError::NoSpace,
        FsError::Io,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn fuzz_like_sequence_never_escapes_or_panics() {
    // A random but deterministic mix of operations with hostile paths on two
    // apps sharing one backend: each app's files stay in its own tree.
    let mut fs = MemFs::new(4 * MIB);
    fs.mkdir_all("/etc").unwrap();
    fs.create("/etc/passwd").unwrap();
    let mut a = Sandbox::new(FsPerm::Own, "aa", 64 * 1024, 6).unwrap();
    let mut b = Sandbox::new(FsPerm::Own, "bb", 64 * 1024, 6).unwrap();
    let atoms: [&str; 10] = [
        "..", ".", "x", "y", "/", "dir", "f.txt", "../bb", "../aa", "etc",
    ];
    let mut rng = Lcg(42);
    let mut fds_a: Vec<i32> = Vec::new();
    for step in 0..20_000u32 {
        let n = 1 + (rng.next() % 4) as usize;
        let mut p = String::new();
        for _ in 0..n {
            p.push_str(atoms[(rng.next() % 10) as usize]);
            if rng.next().is_multiple_of(2) {
                p.push('/');
            }
        }
        let (me, other) = if step % 2 == 0 {
            (&mut a, "bb")
        } else {
            (&mut b, "aa")
        };
        let _ = other;
        match rng.next() % 7 {
            0 => {
                if let Ok(fd) = me.open(&mut fs, p.as_bytes(), rw()) {
                    if step % 2 == 0 {
                        fds_a.push(fd);
                    } else {
                        let _ = me.close(fd);
                    }
                }
            }
            1 => {
                let _ = me.mkdir(&mut fs, p.as_bytes());
            }
            2 => {
                let _ = me.unlink(&mut fs, p.as_bytes());
            }
            3 => {
                let _ = me.stat(&mut fs, p.as_bytes());
            }
            4 => {
                let _ = me.rename(&mut fs, p.as_bytes(), b"/x");
            }
            5 => {
                if let Some(&fd) = fds_a.last() {
                    let _ = a.write(&mut fs, fd, b"data");
                }
            }
            _ => {
                if step % 2 == 0 {
                    for fd in fds_a.drain(..) {
                        let _ = a.close(fd);
                    }
                }
            }
        }
    }
    // Nothing outside /data/aa and /data/bb (and the seeded /etc) exists.
    let mut top = Vec::new();
    for i in 0.. {
        match fs.read_dir("/", i).unwrap() {
            Some(e) => top.push(e.name),
            None => break,
        }
    }
    assert_eq!(top, ["data", "etc"]);
    let mut under_data = Vec::new();
    for i in 0.. {
        match fs.read_dir("/data", i).unwrap() {
            Some(e) => under_data.push(e.name),
            None => break,
        }
    }
    assert_eq!(under_data, ["aa", "bb"]);
    assert_eq!(fs.stat("/etc/passwd").unwrap().size, 0);
    assert!(fs.read_dir("/etc", 1).unwrap().is_none());
}
