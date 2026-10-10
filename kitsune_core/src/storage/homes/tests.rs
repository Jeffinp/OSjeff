use super::*;
use crate::security::account::AccountDb;
use crate::storage::blockdev::RamDisk;
use crate::storage::fs3::{FormatOptions, Fs3};
use crate::storage::secured::Secured;

const NOW: u64 = 1_700_000_000;

fn volume() -> Fs3<RamDisk> {
    Fs3::format(
        RamDisk::new(8 * 2048),
        &FormatOptions::new(*b"0123456789abcdef", NOW),
    )
    .unwrap()
}

fn user(db: &mut AccountDb, name: &str) -> User {
    db.add_user(name, "", "", true).unwrap();
    db.user(name).unwrap().clone()
}

#[test]
fn the_system_folders_belong_to_root() {
    let mut fs = volume();
    ensure_system(&mut fs, NOW).unwrap();
    for d in ["/", "/etc", "/var", "/apps", "/data", "/home"] {
        let i = fs.stat(d.as_bytes()).unwrap();
        assert_eq!((i.uid, i.gid, i.mode), (0, 0, 0o755), "{d}");
    }
    // Idempotent.
    ensure_system(&mut fs, NOW).unwrap();
}

#[test]
fn a_new_home_is_private_and_has_its_folders() {
    let mut fs = volume();
    ensure_system(&mut fs, NOW).unwrap();
    let mut db = AccountDb::system();
    let ana = user(&mut db, "ana");
    let h = create_home(&mut fs, &ana, NOW).unwrap();
    assert_eq!(h, b"/home/ana");
    let i = fs.stat(b"/home/ana").unwrap();
    assert_eq!((i.uid, i.gid, i.mode), (ana.uid, ana.gid, 0o700));
    for f in ["Documentos", "Imagens"] {
        let i = fs.stat(alloc::format!("/home/ana/{f}").as_bytes()).unwrap();
        assert_eq!((i.uid, i.mode), (ana.uid, 0o755), "{f}");
    }
    // A second call changes nothing.
    create_home(&mut fs, &ana, NOW).unwrap();
}

#[test]
fn another_user_cannot_enter_a_home() {
    let mut fs = volume();
    ensure_system(&mut fs, NOW).unwrap();
    let mut db = AccountDb::system();
    let ana = user(&mut db, "ana");
    let bia = user(&mut db, "bia");
    create_home(&mut fs, &ana, NOW).unwrap();
    create_home(&mut fs, &bia, NOW).unwrap();
    let mut a = Secured::new(&mut fs, db.cred(&ana), 0o022);
    a.write_file(b"/home/ana/Documentos/a.txt", b"hi", NOW)
        .unwrap();
    let mut b = Secured::new(&mut fs, db.cred(&bia), 0o022);
    assert_eq!(
        b.read_file(b"/home/ana/Documentos/a.txt"),
        Err(VfsError::PermissionDenied)
    );
    assert_eq!(b.readdir(b"/home/ana"), Err(VfsError::PermissionDenied));
    assert!(b.readdir(b"/home/bia").is_ok());
    // Nobody but root creates things at the top level or among the homes.
    assert_eq!(b.mkdir(b"/loose", NOW), Err(VfsError::PermissionDenied));
    assert_eq!(b.mkdir(b"/home/zed", NOW), Err(VfsError::PermissionDenied));
}

fn legacy_volume() -> Fs3<RamDisk> {
    let mut fs = volume();
    fs.mkdir(b"/Documentos", NOW).unwrap();
    fs.write_file(b"/Documentos/projeto.txt", b"p", NOW)
        .unwrap();
    fs.mkdir(b"/Imagens", NOW).unwrap();
    fs.write_file(b"/leiame.txt", b"read me", NOW).unwrap();
    fs.write_file(b"/notas.txt", b"notes", NOW).unwrap();
    fs.mkdir(b"/home", NOW).unwrap();
    fs.write_file(b"/home/old.txt", b"old", NOW).unwrap();
    fs.mkdir(b"/apps", NOW).unwrap();
    fs.write_file(b"/apps/clock.wasm", b"\0asm", NOW).unwrap();
    fs.mkdir(b"/var", NOW).unwrap();
    fs
}

#[test]
fn the_first_account_adopts_everything_a_pre_accounts_volume_held() {
    let mut fs = legacy_volume();
    ensure_system(&mut fs, NOW).unwrap();
    let mut db = AccountDb::system();
    let ana = user(&mut db, "ana");
    let r = adopt_legacy(&mut fs, &ana, &[], NOW).unwrap();
    assert_eq!(
        r,
        Adopted {
            moved: 4,
            from_home: 1
        }
    );
    // User content moved into the home, owned by the user.
    for p in [
        "/home/ana/Documentos/projeto.txt",
        "/home/ana/leiame.txt",
        "/home/ana/notas.txt",
        "/home/ana/old.txt",
        "/home/ana/Imagens",
    ] {
        let i = fs
            .stat(p.as_bytes())
            .unwrap_or_else(|e| panic!("{p}: {e:?}"));
        assert_eq!(i.uid, ana.uid, "{p}");
    }
    // The top level is just the system folders now.
    let mut names: Vec<String> = fs
        .names(b"/")
        .unwrap()
        .into_iter()
        .map(|(n, _)| String::from_utf8(n).unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["apps", "data", "etc", "home", "var"].map(String::from)
    );
    // System content was not touched.
    assert_eq!(fs.stat(b"/apps/clock.wasm").unwrap().uid, 0);
}

#[test]
fn adoption_keeps_other_homes_and_renames_collisions() {
    let mut fs = legacy_volume();
    fs.mkdir(b"/home/bia", NOW).unwrap();
    fs.write_file(b"/home/bia/secret", b"s", NOW).unwrap();
    // A legacy file named like one of the standard home folders.
    fs.write_file(b"/home/Documentos", b"a file called Documentos", NOW)
        .unwrap();
    ensure_system(&mut fs, NOW).unwrap();
    let mut db = AccountDb::system();
    let ana = user(&mut db, "ana");
    let r = adopt_legacy(&mut fs, &ana, &["bia"], NOW).unwrap();
    assert!(r.from_home >= 2);
    // bia's folder is not swallowed.
    assert_eq!(fs.read_file(b"/home/bia/secret").unwrap(), b"s");
    // Both Documentos survive: the folder from the top level and the file from /home.
    assert!(fs.stat(b"/home/ana/Documentos").is_ok());
    let docs = fs.names(b"/home/ana").unwrap();
    assert!(
        docs.iter()
            .any(|(n, k)| n.starts_with(b"Documentos (2)") && *k == EntryKind::File),
        "{docs:?}"
    );
}

#[test]
fn adoption_on_an_empty_volume_just_makes_the_home() {
    let mut fs = volume();
    ensure_system(&mut fs, NOW).unwrap();
    let mut db = AccountDb::system();
    let ana = user(&mut db, "ana");
    let r = adopt_legacy(&mut fs, &ana, &[], NOW).unwrap();
    assert_eq!(r, Adopted::default());
    assert!(fs.stat(b"/home/ana/Documentos").is_ok());
}

#[test]
fn own_tree_reaches_every_level() {
    let mut fs = volume();
    fs.mkdir(b"/a", NOW).unwrap();
    fs.mkdir(b"/a/b", NOW).unwrap();
    fs.write_file(b"/a/b/c", b"x", NOW).unwrap();
    own_tree(&mut fs, b"/a", 1234, 55).unwrap();
    for p in ["/a", "/a/b", "/a/b/c"] {
        let i = fs.stat(p.as_bytes()).unwrap();
        assert_eq!((i.uid, i.gid), (1234, 55), "{p}");
    }
}
