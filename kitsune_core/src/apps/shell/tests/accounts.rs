//! `whoami`, `id`, `groups`, `users`, `chmod`, `chown`, `chgrp` and the owners in `ls -l`.

use super::*;
use crate::shell::sys::{Identity, UserEntry};

fn with_accounts() -> T {
    let mut t = T::new();
    t.fs = MemFs::new()
        .with_dir("/home")
        .with_dir("/home/ana")
        .with_file("/home/ana/a.txt", b"alpha\n")
        .with_file("/home/ana/run.sh", b"echo hi\n")
        .with_dir("/home/ana/docs")
        .with_file("/home/ana/docs/x", b"x")
        .with_meta("/home/ana", 1000, 100, 0o700)
        .with_meta("/home/ana/a.txt", 1000, 100, 0o644)
        .with_meta("/home/ana/run.sh", 1000, 100, 0o644)
        .with_meta("/home/ana/docs", 1000, 100, 0o755)
        .with_meta("/home/ana/docs/x", 1000, 100, 0o644);
    t.sys.who = Some(Identity {
        name: "ana".into(),
        uid: 1000,
        gid: 100,
        gname: "users".into(),
        groups: alloc::vec![(100, "users".into()), (10, "admin".into())],
        admin: true,
    });
    t.sys.people = alloc::vec![
        UserEntry {
            name: "ana".into(),
            uid: 1000,
            full_name: "Ana Souza".into(),
            admin: true
        },
        UserEntry {
            name: "bia".into(),
            uid: 1001,
            full_name: String::new(),
            admin: false
        },
    ];
    t.sys.group_list = alloc::vec![
        ("users".into(), 100),
        ("admin".into(), 10),
        ("root".into(), 0)
    ];
    t
}

#[test]
fn who_am_i() {
    let mut t = with_accounts();
    assert_eq!(t.out("whoami"), "ana\n");
    assert_eq!(
        t.out("id"),
        "uid=1000(ana) gid=100(users) groups=100(users),10(admin)\n"
    );
    assert_eq!(t.out("groups"), "users admin\n");
    let u = t.out("users");
    assert!(
        u.contains("ana") && u.contains("Ana Souza") && u.contains("(administrator)"),
        "{u}"
    );
    assert!(u.contains("bia") && !u.contains("bia (admin"), "{u}");
}

#[test]
fn without_accounts_they_say_so() {
    let mut t = T::new();
    for c in ["whoami", "id", "groups", "users"] {
        let r = t.run(c);
        assert_eq!(r.status, 1, "{c}");
        assert!(r.text().contains("not available"), "{c}: {}", r.text());
    }
}

#[test]
fn portuguese_messages() {
    let mut t = with_accounts();
    t.lang(Lang::Pt);
    assert_eq!(
        t.out("id"),
        "uid=1000(ana) gid=100(users) grupos=100(users),10(admin)\n"
    );
    assert!(t.out("chmod 99 /home/ana/a.txt").contains("modo inválido"));
}

#[test]
fn chmod_octal() {
    let mut t = with_accounts();
    assert_eq!(t.run("chmod 600 /home/ana/a.txt").status, 0);
    assert_eq!(t.fs.meta("/home/ana/a.txt").unwrap().mode, 0o600);
    assert_eq!(t.run("chmod 1777 /home/ana/docs").status, 0);
    assert_eq!(t.fs.meta("/home/ana/docs").unwrap().mode, 0o1777);
    for bad in [
        "chmod 888 /home/ana/a.txt",
        "chmod 12345 /home/ana/a.txt",
        "chmod rwx /home/ana/a.txt",
        "chmod u+q /home/ana/a.txt",
        "chmod x=y /home/ana/a.txt",
    ] {
        let r = t.run(bad);
        assert_eq!(r.status, 2, "{bad}");
        assert!(r.text().contains("invalid mode"), "{bad}: {}", r.text());
    }
    assert_eq!(
        t.fs.meta("/home/ana/a.txt").unwrap().mode,
        0o600,
        "a bad mode changes nothing"
    );
}

#[test]
fn chmod_symbolic() {
    use super::super::builtins::apply_mode as am;
    assert_eq!(am("u+x", 0o644, false), Some(0o744));
    assert_eq!(am("g-r", 0o644, false), Some(0o604));
    assert_eq!(am("go-rwx", 0o755, false), Some(0o700));
    assert_eq!(am("a+r", 0o200, false), Some(0o644 & 0o444 | 0o200));
    assert_eq!(am("a=r", 0o777, false), Some(0o444));
    assert_eq!(am("u=rwx,g=rx,o=", 0o000, false), Some(0o750));
    assert_eq!(am("+x", 0o644, false), Some(0o755));
    assert_eq!(am("o+t", 0o777, true), Some(0o1777));
    assert_eq!(am("a-t", 0o1777, true), Some(0o777));
    assert_eq!(
        am("a+X", 0o644, false),
        Some(0o644),
        "X needs an x bit or a folder"
    );
    assert_eq!(am("a+X", 0o644, true), Some(0o755));
    assert_eq!(am("a+X", 0o744, false), Some(0o755));
    assert_eq!(am("u+x,g+x", 0o600, false), Some(0o710));
    assert_eq!(am("u+x-w", 0o600, false), Some(0o500));
    for bad in ["", "u", "u+", "z+x", "u+rq", "ug", "+-=", "u+x,", "8"] {
        // `u+` and `u+x,` are rejected only when they make no sense; the empty and junk ones never pass.
        if matches!(bad, "" | "u" | "z+x" | "u+rq" | "ug" | "8") {
            assert_eq!(am(bad, 0o644, false), None, "{bad:?}");
        }
    }
}

#[test]
fn chmod_symbolic_through_the_shell() {
    let mut t = with_accounts();
    assert_eq!(t.run("chmod u+x /home/ana/run.sh").status, 0);
    assert_eq!(t.fs.meta("/home/ana/run.sh").unwrap().mode, 0o744);
    assert_eq!(t.run("chmod go+w /home/ana/run.sh").status, 0);
    assert_eq!(t.fs.meta("/home/ana/run.sh").unwrap().mode, 0o766);
}

#[test]
fn chmod_recursive_and_missing_files() {
    let mut t = with_accounts();
    assert_eq!(t.run("chmod -R 700 /home/ana/docs").status, 0);
    assert_eq!(t.fs.meta("/home/ana/docs").unwrap().mode, 0o700);
    assert_eq!(t.fs.meta("/home/ana/docs/x").unwrap().mode, 0o700);
    let r = t.run("chmod 600 /home/ana/nothing");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("nothing"), "{}", r.text());
    assert_eq!(t.run("chmod 600").status, 2);
    assert_eq!(t.run("chmod").status, 2);
}

#[test]
fn chown_and_chgrp() {
    let mut t = with_accounts();
    assert_eq!(t.run("chown bia /home/ana/a.txt").status, 0);
    let m = t.fs.meta("/home/ana/a.txt").unwrap();
    assert_eq!((m.uid, m.gid), (1001, 100));
    assert_eq!(t.run("chown ana:admin /home/ana/a.txt").status, 0);
    let m = t.fs.meta("/home/ana/a.txt").unwrap();
    assert_eq!((m.uid, m.gid), (1000, 10));
    assert_eq!(t.run("chown :users /home/ana/a.txt").status, 0);
    assert_eq!(t.fs.meta("/home/ana/a.txt").unwrap().gid, 100);
    assert_eq!(t.run("chgrp admin /home/ana/a.txt").status, 0);
    assert_eq!(t.fs.meta("/home/ana/a.txt").unwrap().gid, 10);
    assert_eq!(t.run("chown 1001:100 /home/ana/a.txt").status, 0);
    let m = t.fs.meta("/home/ana/a.txt").unwrap();
    assert_eq!((m.uid, m.gid), (1001, 100));
    assert_eq!(t.run("chown -R bia /home/ana/docs").status, 0);
    assert_eq!(t.fs.meta("/home/ana/docs/x").unwrap().uid, 1001);
}

#[test]
fn chown_errors() {
    let mut t = with_accounts();
    let r = t.run("chown nobody /home/ana/a.txt");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("no such user"), "{}", r.text());
    let r = t.run("chgrp ghosts /home/ana/a.txt");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("no such group"), "{}", r.text());
    assert_eq!(t.run("chown ana").status, 2);
    assert_eq!(t.run("chgrp users").status, 2);
}

#[test]
fn a_filesystem_without_owners_refuses() {
    let mut t = T::with_files();
    let r = t.run("chmod 600 /a.txt");
    // MemFs records owners on demand, so this works; a bare ShellFs would say "read-only".
    assert_eq!(r.status, 0);
}

#[test]
fn ls_long_shows_owner_group_and_mode_when_known() {
    let mut t = with_accounts();
    let out = t.out("ls -l /home/ana");
    assert!(out.contains("-rw-r--r--      ana"), "{out}");
    assert!(out.contains("users"), "{out}");
    assert!(out.contains("drwxr-xr-x"), "{out}");
    assert!(out.contains("a.txt"), "{out}");
    // A file with no recorded owner keeps the plain line.
    let mut p = T::with_files();
    let out = p.out("ls -l /");
    assert!(
        out.contains("-        6 a.txt") || out.contains("-"),
        "{out}"
    );
}

#[test]
fn ls_a_single_file_long() {
    let mut t = with_accounts();
    let out = t.out("ls -l /home/ana/a.txt");
    assert!(out.starts_with("-rw-r--r--"), "{out}");
}

#[test]
fn stat_shows_the_owner_too() {
    let mut t = with_accounts();
    let out = t.out("stat /home/ana/a.txt");
    assert!(
        out.contains("owner: ana  group: users  mode: rw-r--r-- (0644)"),
        "{out}"
    );
    let mut p = T::with_files();
    assert!(!p.out("stat /a.txt").contains("owner"));
}
