use super::*;

fn user(uid: u32, gid: u32) -> Cred {
    Cred::new(uid, gid, Vec::new())
}

#[test]
fn owner_group_other_classes() {
    let f = Owner::new(1000, 100, 0o640);
    let owner = user(1000, 5);
    let member = user(1001, 100);
    let stranger = user(1002, 200);
    assert!(allowed(&owner, &f, R | W, false));
    assert!(!allowed(&owner, &f, X, false));
    assert!(allowed(&member, &f, R, false));
    assert!(!allowed(&member, &f, W, false));
    assert!(!allowed(&stranger, &f, R, false));
}

#[test]
fn the_owner_class_wins_even_when_it_is_stricter() {
    // Classic rule: the owner is denied here although group and others may read.
    let f = Owner::new(1000, 100, 0o044);
    assert!(!allowed(&user(1000, 100), &f, R, false));
    assert!(allowed(&user(1001, 100), &f, R, false));
    assert!(allowed(&user(1002, 7), &f, R, false));
}

#[test]
fn supplementary_groups_count() {
    let f = Owner::new(1000, 50, 0o060);
    let c = Cred::new(1001, 7, alloc::vec![50]);
    assert!(allowed(&c, &f, R | W, false));
    assert!(c.in_group(7) && c.in_group(50) && !c.in_group(8));
}

#[test]
fn root_reads_and_writes_everything_but_needs_an_x_bit_to_execute() {
    let locked = Owner::new(1000, 100, 0o000);
    let root = Cred::root();
    assert!(allowed(&root, &locked, R | W, false));
    assert!(!allowed(&root, &locked, X, false));
    assert!(
        allowed(&root, &locked, X, true),
        "any directory can be searched"
    );
    assert!(allowed(&root, &Owner::new(1000, 100, 0o100), X, false));
}

#[test]
fn asking_for_nothing_is_always_allowed() {
    assert!(allowed(&user(5, 5), &Owner::new(1, 1, 0), 0, false));
}

#[test]
fn chmod_is_for_the_owner_and_root() {
    let f = Owner::new(1000, 100, 0o644);
    assert!(may_chmod(&user(1000, 1), &f));
    assert!(may_chmod(&Cred::root(), &f));
    assert!(!may_chmod(&user(1001, 100), &f));
}

#[test]
fn chown_rules() {
    let f = Owner::new(1000, 100, 0o644);
    let me = Cred::new(1000, 100, alloc::vec![200]);
    assert!(may_chown(&me, &f, None, Some(200)), "to a group I am in");
    assert!(may_chown(&me, &f, Some(1000), None), "to myself is a no-op");
    assert!(
        !may_chown(&me, &f, None, Some(300)),
        "not to a group I am not in"
    );
    assert!(
        !may_chown(&me, &f, Some(1001), None),
        "no giving files away"
    );
    assert!(
        !may_chown(&user(1001, 100), &f, None, Some(100)),
        "not my file"
    );
    assert!(may_chown(&Cred::root(), &f, Some(1001), Some(300)));
}

#[test]
fn removing_needs_write_and_search_on_the_folder() {
    let dir = Owner::new(1000, 100, 0o755);
    let file = Owner::new(1001, 100, 0o644);
    assert!(may_remove(&user(1000, 100), &dir, &file));
    assert!(
        !may_remove(&user(1001, 100), &dir, &file),
        "no write on the folder"
    );
    let open = Owner::new(1000, 100, 0o777);
    assert!(
        may_remove(&user(1002, 7), &open, &file),
        "shared folder, no sticky bit"
    );
}

#[test]
fn the_sticky_bit_protects_other_peoples_files() {
    let tmp = Owner::new(0, 0, 0o1777);
    let mine = Owner::new(1000, 100, 0o600);
    let theirs = Owner::new(1001, 100, 0o666);
    let me = user(1000, 100);
    assert!(may_remove(&me, &tmp, &mine));
    assert!(
        !may_remove(&me, &tmp, &theirs),
        "even though the file is world-writable"
    );
    assert!(may_remove(&Cred::root(), &tmp, &theirs));
    // The owner of the folder may remove anything in it.
    let own = Owner::new(1000, 100, 0o1777);
    assert!(may_remove(&me, &own, &theirs));
}

#[test]
fn umask_clears_bits() {
    assert_eq!(apply_umask(0o666, 0o022), 0o644);
    assert_eq!(apply_umask(0o777, 0o077), 0o700);
    assert_eq!(apply_umask(0o1777, 0), 0o1777);
    assert_eq!(apply_umask(0o666, 0), 0o666);
}

#[test]
fn mode_text() {
    assert_eq!(mode_string(0o755), "rwxr-xr-x");
    assert_eq!(mode_string(0o640), "rw-r-----");
    assert_eq!(mode_string(0), "---------");
    assert_eq!(mode_string(0o1777), "rwxrwxrwt");
    assert_eq!(mode_string(0o1776), "rwxrwxrwT");
}

#[test]
fn octal_parsing() {
    assert_eq!(parse_mode("644"), Some(0o644));
    assert_eq!(parse_mode("0755"), Some(0o755));
    assert_eq!(parse_mode("1777"), Some(0o1777));
    for bad in ["", "8", "abc", "-1", "7777", "2755", "123456", "6 4"] {
        assert_eq!(parse_mode(bad), None, "{bad:?}");
    }
}
