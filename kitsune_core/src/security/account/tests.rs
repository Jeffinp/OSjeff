use super::*;
use crate::security::password::hash_with;

fn pw(p: &str) -> String {
    hash_with(p, &[5; 16], 1_000).unwrap()
}

fn db_with_ana() -> AccountDb {
    let mut db = AccountDb::system();
    db.add_user("ana", "Ana Souza", &pw("senha-ana"), true)
        .unwrap();
    db
}

#[test]
fn a_fresh_system_has_a_locked_root_and_the_standard_groups() {
    let db = AccountDb::system();
    assert_eq!(db.users().len(), 1);
    let root = db.user("root").unwrap();
    assert_eq!((root.uid, root.gid), (0, 0));
    assert!(root.is_locked());
    assert_eq!(db.group("admin").unwrap().gid, GID_ADMIN);
    assert_eq!(db.group("users").unwrap().gid, GID_USERS);
    assert_eq!(
        db.serialize(),
        AccountDb::parse(&db.serialize()).unwrap().serialize()
    );
}

#[test]
fn users_get_sequential_uids_the_users_group_and_a_home() {
    let mut db = db_with_ana();
    let bia = db.add_user("bia", "", "", false).unwrap();
    assert_eq!(db.user("ana").unwrap().uid, FIRST_UID);
    assert_eq!(bia, FIRST_UID + 1);
    let ana = db.user("ana").unwrap();
    assert_eq!(ana.home, "/home/ana");
    assert_eq!(ana.gid, GID_USERS);
    let c = db.cred(ana);
    assert!(c.in_group(GID_ADMIN) && c.in_group(GID_USERS));
    let c = db.cred(db.user("bia").unwrap());
    assert!(c.in_group(GID_USERS) && !c.in_group(GID_ADMIN));
    assert!(db.is_admin(db.user("ana").unwrap()));
    assert!(!db.is_admin(db.user("bia").unwrap()));
    assert!(db.is_admin(db.user("root").unwrap()));
}

#[test]
fn a_freed_uid_is_reused_but_never_a_taken_one() {
    let mut db = db_with_ana();
    db.add_user("bia", "", "", false).unwrap();
    db.remove_user("bia").unwrap();
    assert_eq!(db.add_user("caio", "", "", false).unwrap(), FIRST_UID + 1);
}

#[test]
fn names_are_validated() {
    let mut db = AccountDb::system();
    for bad in [
        "",
        "Ana",
        "1ana",
        "a b",
        "a:b",
        "root",
        "a/b",
        "ção",
        &"a".repeat(33),
        "-x",
    ] {
        assert_eq!(
            db.add_user(bad, "", "", false),
            Err(AccountError::BadName),
            "{bad:?}"
        );
    }
    for good in ["ana", "_svc", "a-b_c9", &"a".repeat(32)] {
        assert!(valid_name(good), "{good:?}");
    }
    assert_eq!(
        db.add_user("ok", "a:b", "", false),
        Err(AccountError::BadFullName)
    );
    assert_eq!(
        db.add_user("ok", "a\nb", "", false),
        Err(AccountError::BadFullName)
    );
    assert_eq!(
        db.add_user("ok", "", "plain", false),
        Err(AccountError::BadPassword)
    );
    assert!(
        db.add_user("ok", "Ação Ç", "", false).is_ok(),
        "accents are fine in full names"
    );
}

#[test]
fn duplicates_and_limits() {
    let mut db = db_with_ana();
    assert_eq!(
        db.add_user("ana", "", "", false),
        Err(AccountError::DuplicateName)
    );
    assert_eq!(
        db.add_user("admin", "", "", false),
        Err(AccountError::DuplicateName),
        "group name"
    );
    for i in db.users().len()..MAX_USERS {
        db.add_user(&alloc::format!("u{i}"), "", "", false).unwrap();
    }
    assert_eq!(
        db.add_user("one-more", "", "", false),
        Err(AccountError::TooManyUsers)
    );
}

#[test]
fn authentication() {
    let mut db = db_with_ana();
    db.add_user("bia", "", "", false).unwrap();
    db.add_user("caio", "", LOCKED, false).unwrap();
    assert!(db.authenticate("ana", "senha-ana").is_ok());
    assert_eq!(
        db.authenticate("ana", "outra").unwrap_err(),
        AuthError::BadPassword
    );
    assert_eq!(
        db.authenticate("ana", "").unwrap_err(),
        AuthError::BadPassword
    );
    assert_eq!(
        db.authenticate("nobody", "x").unwrap_err(),
        AuthError::UnknownUser
    );
    assert_eq!(db.authenticate("caio", "x").unwrap_err(), AuthError::Locked);
    assert_eq!(db.authenticate("root", "").unwrap_err(), AuthError::Locked);
    // No password: only the empty attempt gets in.
    assert!(db.authenticate("bia", "").is_ok());
    assert_eq!(
        db.authenticate("bia", "x").unwrap_err(),
        AuthError::BadPassword
    );
}

#[test]
fn changing_passwords() {
    let mut db = db_with_ana();
    db.set_password("ana", &pw("nova-senha")).unwrap();
    assert!(db.authenticate("ana", "nova-senha").is_ok());
    assert!(db.authenticate("ana", "senha-ana").is_err());
    db.set_password("ana", LOCKED).unwrap();
    assert_eq!(
        db.authenticate("ana", "nova-senha").unwrap_err(),
        AuthError::Locked
    );
    assert_eq!(
        db.set_password("ana", "junk"),
        Err(AccountError::BadPassword)
    );
    assert_eq!(db.set_password("who", ""), Err(AccountError::NoSuchUser));
}

#[test]
fn protected_accounts() {
    let mut db = db_with_ana();
    assert_eq!(db.remove_user("root"), Err(AccountError::Protected));
    assert_eq!(
        db.remove_user("ana"),
        Err(AccountError::Protected),
        "last administrator"
    );
    assert_eq!(
        db.remove_from_group("ana", GID_ADMIN),
        Err(AccountError::Protected)
    );
    db.add_user("bia", "", "", true).unwrap();
    assert_eq!(db.admin_count(), 2);
    db.remove_user("ana").unwrap();
    assert!(db.user("ana").is_none());
    assert!(
        db.group("users")
            .unwrap()
            .members
            .iter()
            .all(|m| m != "ana")
    );
    assert_eq!(db.remove_user("ana"), Err(AccountError::NoSuchUser));
}

#[test]
fn group_membership() {
    let mut db = db_with_ana();
    db.add_user("bia", "", "", false).unwrap();
    db.add_to_group("bia", GID_ADMIN).unwrap();
    assert!(db.is_admin(db.user("bia").unwrap()));
    db.remove_from_group("bia", GID_ADMIN).unwrap();
    assert!(!db.is_admin(db.user("bia").unwrap()));
    assert_eq!(
        db.add_to_group("who", GID_ADMIN),
        Err(AccountError::NoSuchUser)
    );
    assert_eq!(db.add_to_group("bia", 4242), Err(AccountError::NoSuchGroup));
    db.add_to_group("bia", GID_ADMIN).unwrap();
    db.add_to_group("bia", GID_ADMIN).unwrap();
    assert_eq!(
        db.group("admin")
            .unwrap()
            .members
            .iter()
            .filter(|m| *m == "bia")
            .count(),
        1
    );
}

#[test]
fn display_names_for_ids() {
    let db = db_with_ana();
    assert_eq!(db.user_name(0), "root");
    assert_eq!(db.user_name(FIRST_UID), "ana");
    assert_eq!(db.user_name(9999), "?");
    assert_eq!(db.group_name(GID_USERS), "users");
    assert_eq!(db.group_name(9999), "?");
}

#[test]
fn serialisation_round_trips() {
    let mut db = db_with_ana();
    db.add_user("bia", "Bia Ç. Lima", "", false).unwrap();
    db.add_user("caio", "", LOCKED, false).unwrap();
    let text = db.serialize();
    assert!(text.starts_with("# kitsune accounts v1\n"));
    let back = AccountDb::parse(&text).unwrap();
    assert_eq!(back, db);
    assert_eq!(back.serialize(), text);
}

#[test]
fn parse_rejects_damaged_files() {
    let good = db_with_ana().serialize();
    let bad: Vec<String> = alloc::vec![
        String::new(),
        "u:ana:1000:100::/home/ana:\n".to_string(),
        good.replace("# kitsune accounts v1", "# something else"),
        good.replace("u:ana:1000", "u:ana:1000:1"),
        good.replace("u:ana:1000:100", "u:ana:abc:100"),
        good.replace("u:ana:1000:100", "u:ana:99999999:100"),
        good.replace("u:ana:1000:100", "u:ana:70000:100"),
        good.replace("u:ana:1000:100", "u:ana:-1:100"),
        good.replace("u:ana:1000:100", "u:ana:1000:555"),
        good.replace("/home/ana", "home/ana"),
        good.replace("/home/ana", "/home/../etc"),
        good.replace("u:ana:", "u:Ana:"),
        good.replace("g:users:100:ana", "g:users:100:ghost"),
        good.replace("g:root:0:", "g:root:0:\ng:root:7:"),
        alloc::format!("{good}u:ana:1001:100::/home/x:\n"),
        alloc::format!("{good}u:bia:1000:100::/home/x:\n"),
        alloc::format!("{good}u:bia:1001:100::/home/x:plain\n"),
        alloc::format!("{good}x:what\n"),
        good.replace("u:root:0", "u:toor:0"),
        alloc::format!("{good}u:evil:0:0::/root:\n"),
        alloc::format!("{good}g:ana:200:\n"),
        "# kitsune accounts v1\ng:root:0:\n".to_string(),
        "x".repeat(MAX_FILE + 1),
    ];
    for b in &bad {
        assert!(AccountDb::parse(b).is_err(), "{b:?}");
    }
    assert!(AccountDb::parse(&good).is_ok());
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let good = db_with_ana().serialize();
    let text = alloc::format!(
        "{}\n# a comment\n\n{}",
        good.lines().next().unwrap(),
        good.split_once('\n').unwrap().1
    );
    assert_eq!(AccountDb::parse(&text).unwrap(), db_with_ana());
}

#[test]
fn arbitrary_text_never_panics() {
    // A cheap stand-in for the fuzz target: mangle a valid file every way a byte can go wrong.
    let good = db_with_ana().serialize();
    let bytes = good.as_bytes();
    for i in 0..bytes.len() {
        for repl in [b':', b'\n', b'0', b'z', b'$', 0xC3] {
            let mut m = bytes.to_vec();
            m[i] = repl;
            if let Ok(s) = core::str::from_utf8(&m) {
                let _ = AccountDb::parse(s);
            }
        }
        let cut = core::str::from_utf8(&bytes[..i]);
        if let Ok(s) = cut {
            let _ = AccountDb::parse(s);
        }
    }
}
