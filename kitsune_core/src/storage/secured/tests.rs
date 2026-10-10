use super::*;
use crate::storage::blockdev::RamDisk;
use crate::storage::fs3::{FormatOptions, Fs3};

const NOW: u64 = 1_700_000_000;
const ANA: u32 = 1000;
const BIA: u32 = 1001;
const USERS: u32 = 100;

fn ana() -> Cred {
    Cred::new(ANA, USERS, Vec::new())
}

fn bia() -> Cred {
    Cred::new(BIA, USERS, Vec::new())
}

/// A volume with `/home/ana` (700, ana), `/home/bia` (700, bia), `/pub` (755, root) and `/tmp`
/// (1777, root), built through the root view.
fn volume() -> Fs3<RamDisk> {
    let mut fs = Fs3::format(
        RamDisk::new(8 * 2048),
        &FormatOptions::new(*b"0123456789abcdef", NOW),
    )
    .unwrap();
    {
        let mut r = Secured::new(&mut fs, Cred::root(), 0);
        for d in ["/home", "/pub", "/tmp"] {
            r.mkdir(d.as_bytes(), NOW).unwrap();
        }
        r.mkdir(b"/home/ana", NOW).unwrap();
        r.mkdir(b"/home/bia", NOW).unwrap();
        r.set_owner(b"/home/ana", Some(ANA), Some(USERS), Some(0o700))
            .unwrap();
        r.set_owner(b"/home/bia", Some(BIA), Some(USERS), Some(0o700))
            .unwrap();
        r.set_owner(b"/tmp", None, None, Some(0o1777)).unwrap();
        for d in ["/home", "/pub"] {
            r.set_owner(d.as_bytes(), None, None, Some(0o755)).unwrap();
        }
    }
    fs
}

fn as_user<'a>(fs: &'a mut Fs3<RamDisk>, c: Cred) -> Secured<'a, Fs3<RamDisk>> {
    Secured::new(fs, c, DEFAULT_UMASK)
}

#[test]
fn a_user_works_in_their_own_home() {
    let mut fs = volume();
    let mut a = as_user(&mut fs, ana());
    a.write_file(b"/home/ana/notes.txt", b"hello", NOW).unwrap();
    assert_eq!(a.read_file(b"/home/ana/notes.txt").unwrap(), b"hello");
    a.mkdir(b"/home/ana/docs", NOW).unwrap();
    a.create(b"/home/ana/docs/a", NOW).unwrap();
    a.append(b"/home/ana/docs/a", b"xy", NOW).unwrap();
    a.write_at(b"/home/ana/docs/a", 0, b"Z", NOW).unwrap();
    a.truncate(b"/home/ana/docs/a", 1, NOW).unwrap();
    assert_eq!(a.read_file(b"/home/ana/docs/a").unwrap(), b"Z");
    assert_eq!(a.readdir(b"/home/ana").unwrap().len(), 2);
    a.rename(b"/home/ana/notes.txt", b"/home/ana/docs/notes.txt", NOW)
        .unwrap();
    a.remove_all(b"/home/ana/docs").unwrap();
    assert_eq!(a.readdir(b"/home/ana").unwrap().len(), 0);
}

#[test]
fn another_user_cannot_look_inside() {
    let mut fs = volume();
    as_user(&mut fs, ana())
        .write_file(b"/home/ana/secret.txt", b"s3cret", NOW)
        .unwrap();
    let mut b = as_user(&mut fs, bia());
    assert_eq!(
        b.stat(b"/home/ana/secret.txt"),
        Err(DENIED),
        "no search on /home/ana"
    );
    assert_eq!(b.read_file(b"/home/ana/secret.txt"), Err(DENIED));
    assert_eq!(b.readdir(b"/home/ana"), Err(DENIED));
    assert_eq!(b.names(b"/home/ana"), Err(DENIED));
    assert_eq!(b.write_file(b"/home/ana/x", b"1", NOW), Err(DENIED));
    assert_eq!(b.create(b"/home/ana/y", NOW), Err(DENIED));
    assert_eq!(b.mkdir(b"/home/ana/z", NOW), Err(DENIED));
    assert_eq!(b.remove_all(b"/home/ana/secret.txt"), Err(DENIED));
    assert_eq!(b.trash(b"/home/ana/secret.txt", NOW), Err(DENIED));
    assert_eq!(
        b.rename(b"/home/ana/secret.txt", b"/home/bia/s", NOW),
        Err(DENIED)
    );
    // The folder itself is visible (its parent is searchable) but not its contents.
    assert!(b.stat(b"/home/ana").is_ok());
    assert_eq!(
        b.read_at(b"/home/ana/secret.txt", 0, &mut [0u8; 4]),
        Err(DENIED)
    );
}

#[test]
fn denied_comes_before_not_found_so_names_do_not_leak() {
    let mut fs = volume();
    let mut b = as_user(&mut fs, bia());
    assert_eq!(b.stat(b"/home/ana/exists-or-not"), Err(DENIED));
    assert_eq!(b.stat(b"/pub/nope"), Err(VfsError::NotFound));
}

#[test]
fn new_items_belong_to_the_creator_and_respect_the_umask() {
    let mut fs = volume();
    {
        let mut a = as_user(&mut fs, ana());
        a.create(b"/home/ana/f", NOW).unwrap();
        a.mkdir(b"/home/ana/d", NOW).unwrap();
        a.write_file(b"/home/ana/w", b"x", NOW).unwrap();
    }
    let mut r = as_user(&mut fs, Cred::root());
    let f = r.stat(b"/home/ana/f").unwrap();
    assert_eq!((f.uid, f.gid, f.mode), (ANA, USERS, 0o644));
    let d = r.stat(b"/home/ana/d").unwrap();
    assert_eq!((d.uid, d.gid, d.mode), (ANA, USERS, 0o755));
    assert_eq!(r.stat(b"/home/ana/w").unwrap().uid, ANA);
    drop(r);
    let mut strict = Secured::new(&mut fs, ana(), 0o077);
    strict.create(b"/home/ana/g", NOW).unwrap();
    strict.mkdir(b"/home/ana/e", NOW).unwrap();
    assert_eq!(strict.stat(b"/home/ana/g").unwrap().mode, 0o600);
    assert_eq!(strict.stat(b"/home/ana/e").unwrap().mode, 0o700);
}

#[test]
fn public_folders_are_readable_but_not_writable_by_others() {
    let mut fs = volume();
    as_user(&mut fs, Cred::root())
        .write_file(b"/pub/readme", b"hi", NOW)
        .unwrap();
    let mut a = as_user(&mut fs, ana());
    assert_eq!(a.read_file(b"/pub/readme").unwrap(), b"hi");
    assert_eq!(a.write_file(b"/pub/readme", b"x", NOW), Err(DENIED));
    assert_eq!(a.write_file(b"/pub/new", b"x", NOW), Err(DENIED));
    assert_eq!(a.remove_all(b"/pub/readme"), Err(DENIED));
    assert_eq!(a.readdir(b"/pub").unwrap().len(), 1);
}

#[test]
fn a_read_only_file_stops_its_own_owner() {
    let mut fs = volume();
    let mut a = as_user(&mut fs, ana());
    a.write_file(b"/home/ana/ro", b"data", NOW).unwrap();
    a.set_owner(b"/home/ana/ro", None, None, Some(0o444))
        .unwrap();
    assert_eq!(a.write_file(b"/home/ana/ro", b"x", NOW), Err(DENIED));
    assert_eq!(a.append(b"/home/ana/ro", b"x", NOW), Err(DENIED));
    assert_eq!(a.truncate(b"/home/ana/ro", 0, NOW), Err(DENIED));
    assert_eq!(a.read_file(b"/home/ana/ro").unwrap(), b"data");
    // Removing needs the folder, not the file: still possible.
    a.remove_all(b"/home/ana/ro").unwrap();
}

#[test]
fn the_sticky_folder_keeps_users_out_of_each_others_files() {
    let mut fs = volume();
    as_user(&mut fs, ana())
        .write_file(b"/tmp/a", b"A", NOW)
        .unwrap();
    as_user(&mut fs, ana())
        .set_owner(b"/tmp/a", None, None, Some(0o666))
        .unwrap();
    let mut b = as_user(&mut fs, bia());
    // World-writable file in a world-writable folder, yet not removable by someone else.
    b.write_file(b"/tmp/a", b"B", NOW).unwrap();
    assert_eq!(b.remove_all(b"/tmp/a"), Err(DENIED));
    assert_eq!(b.rename(b"/tmp/a", b"/tmp/b", NOW), Err(DENIED));
    assert_eq!(b.trash(b"/tmp/a", NOW), Err(DENIED));
    b.create(b"/tmp/mine", NOW).unwrap();
    b.remove_all(b"/tmp/mine").unwrap();
    as_user(&mut fs, ana()).remove_all(b"/tmp/a").unwrap();
}

#[test]
fn removing_a_tree_needs_every_entry_to_be_removable() {
    let mut fs = volume();
    {
        let mut a = as_user(&mut fs, ana());
        a.mkdir(b"/home/ana/share", NOW).unwrap();
        a.set_owner(b"/home/ana/share", None, None, Some(0o777))
            .unwrap();
        a.set_owner(b"/home/ana", None, None, Some(0o711)).unwrap();
    }
    as_user(&mut fs, bia())
        .write_file(b"/home/ana/share/bia.txt", b"b", NOW)
        .unwrap();
    let mut a = as_user(&mut fs, ana());
    // Ana owns the folder: she may remove bia's file inside it (no sticky bit).
    a.remove_all(b"/home/ana/share").unwrap();
    // But a folder holding something she cannot remove blocks the whole removal.
    a.mkdir(b"/home/ana/vault", NOW).unwrap();
    a.write_file(b"/home/ana/vault/f", b"1", NOW).unwrap();
    a.set_owner(b"/home/ana/vault", None, None, Some(0o500))
        .unwrap();
    assert_eq!(
        a.remove_all(b"/home/ana/vault"),
        Err(DENIED),
        "no write on vault"
    );
    a.set_owner(b"/home/ana/vault", None, None, Some(0o700))
        .unwrap();
    a.remove_all(b"/home/ana/vault").unwrap();
}

#[test]
fn chmod_and_chown() {
    let mut fs = volume();
    let mut a = as_user(&mut fs, ana());
    a.create(b"/home/ana/f", NOW).unwrap();
    a.set_owner(b"/home/ana/f", None, None, Some(0o600))
        .unwrap();
    assert_eq!(a.stat(b"/home/ana/f").unwrap().mode, 0o600);
    // Not to someone else, not to a group she is not in.
    assert_eq!(
        a.set_owner(b"/home/ana/f", Some(BIA), None, None),
        Err(DENIED)
    );
    assert_eq!(
        a.set_owner(b"/home/ana/f", None, Some(7), None),
        Err(DENIED)
    );
    assert!(a.set_owner(b"/home/ana/f", None, Some(USERS), None).is_ok());
    // Not other people's files.
    let mut b = as_user(&mut fs, bia());
    assert_eq!(b.set_owner(b"/pub", None, None, Some(0o777)), Err(DENIED));
    assert_eq!(b.set_owner(b"/home/bia", Some(0), None, None), Err(DENIED));
    // Root may.
    let mut r = as_user(&mut fs, Cred::root());
    r.set_owner(b"/home/ana/f", Some(BIA), Some(7), Some(0o640))
        .unwrap();
    let i = r.stat(b"/home/ana/f").unwrap();
    assert_eq!((i.uid, i.gid, i.mode), (BIA, 7, 0o640));
}

#[test]
fn group_access_through_a_supplementary_group() {
    let mut fs = volume();
    {
        let mut r = as_user(&mut fs, Cred::root());
        r.mkdir(b"/team", NOW).unwrap();
        r.set_owner(b"/team", Some(0), Some(50), Some(0o770))
            .unwrap();
        r.write_file(b"/team/plan", b"p", NOW).unwrap();
        r.set_owner(b"/team/plan", Some(0), Some(50), Some(0o660))
            .unwrap();
    }
    let member = Cred::new(ANA, USERS, alloc::vec![50]);
    let mut m = as_user(&mut fs, member);
    assert_eq!(m.read_file(b"/team/plan").unwrap(), b"p");
    m.append(b"/team/plan", b"q", NOW).unwrap();
    let mut o = as_user(&mut fs, bia());
    assert_eq!(o.read_file(b"/team/plan"), Err(DENIED));
    assert_eq!(o.readdir(b"/team"), Err(DENIED));
}

#[test]
fn the_trash_is_shared_but_each_user_sees_only_their_own() {
    let mut fs = volume();
    as_user(&mut fs, ana())
        .write_file(b"/home/ana/a.txt", b"A", NOW)
        .unwrap();
    as_user(&mut fs, bia())
        .write_file(b"/home/bia/b.txt", b"B", NOW)
        .unwrap();
    as_user(&mut fs, ana())
        .trash(b"/home/ana/a.txt", NOW)
        .unwrap();
    as_user(&mut fs, bia())
        .trash(b"/home/bia/b.txt", NOW)
        .unwrap();
    let mut a = as_user(&mut fs, ana());
    let mine = a.trash_list().unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].name, b"a.txt");
    let id = mine[0].id.clone();
    let mut b = as_user(&mut fs, bia());
    let theirs = b.trash_list().unwrap();
    assert_eq!(theirs.len(), 1);
    assert_eq!(b.trash_restore(&id, NOW), Err(DENIED));
    assert_eq!(b.trash_purge(&id), Err(DENIED));
    let bid = theirs[0].id.clone();
    let mut a = as_user(&mut fs, ana());
    assert_eq!(a.trash_purge(&bid), Err(DENIED));
    // Emptying the trash only empties your own.
    a.empty_trash().unwrap();
    assert_eq!(a.trash_list().unwrap().len(), 0);
    let mut r = as_user(&mut fs, Cred::root());
    assert_eq!(
        r.trash_list().unwrap().len(),
        1,
        "bia's item survived ana's empty"
    );
    // Restoring brings the file back with its owner.
    let mut b = as_user(&mut fs, bia());
    assert_eq!(b.trash_restore(&bid, NOW).unwrap(), b"/home/bia/b.txt");
    assert_eq!(b.read_file(b"/home/bia/b.txt").unwrap(), b"B");
}

#[test]
fn root_reads_and_writes_everywhere() {
    let mut fs = volume();
    as_user(&mut fs, ana())
        .write_file(b"/home/ana/p", b"x", NOW)
        .unwrap();
    let mut r = as_user(&mut fs, Cred::root());
    assert_eq!(r.read_file(b"/home/ana/p").unwrap(), b"x");
    r.write_file(b"/home/ana/p", b"y", NOW).unwrap();
    r.remove_all(b"/home/ana/p").unwrap();
    r.write_file(b"/pub/r", b"r", NOW).unwrap();
    assert_eq!(r.stat(b"/pub/r").unwrap().uid, 0);
}

#[test]
fn errors_from_the_volume_still_come_through() {
    let mut fs = volume();
    let mut a = as_user(&mut fs, ana());
    assert_eq!(a.create(b"/home/ana/f", NOW), Ok(()));
    assert_eq!(a.create(b"/home/ana/f", NOW), Err(VfsError::Exists));
    assert_eq!(a.mkdir(b"/home/ana/f/x", NOW), Err(VfsError::NotDir));
    assert_eq!(a.read_file(b"/home/ana/none"), Err(VfsError::NotFound));
    assert_eq!(a.write_file(b"/home/ana", b"x", NOW), Err(VfsError::IsDir));
    assert_eq!(a.remove_all(b"/"), Err(VfsError::InvalidPath));
}

#[test]
fn a_failed_claim_leaves_nothing_behind() {
    // A backend whose set_owner always fails: the new file must be removed again.
    struct NoOwner<'a>(&'a mut Fs3<RamDisk>);
    impl Backend for NoOwner<'_> {
        fn stat(&mut self, p: &[u8]) -> Result<Info> {
            Backend::stat(&mut *self.0, p)
        }
        fn readdir(&mut self, p: &[u8]) -> Result<Vec<Entry>> {
            Backend::readdir(&mut *self.0, p)
        }
        fn read_file(&mut self, p: &[u8]) -> Result<Vec<u8>> {
            Backend::read_file(&mut *self.0, p)
        }
        fn read_at(&mut self, p: &[u8], o: u64, b: &mut [u8]) -> Result<usize> {
            Backend::read_at(&mut *self.0, p, o, b)
        }
        fn write_file(&mut self, p: &[u8], d: &[u8], n: u64) -> Result<()> {
            Backend::write_file(&mut *self.0, p, d, n)
        }
        fn create(&mut self, p: &[u8], n: u64) -> Result<()> {
            Backend::create(&mut *self.0, p, n)
        }
        fn write_at(&mut self, p: &[u8], o: u64, d: &[u8], n: u64) -> Result<()> {
            Backend::write_at(&mut *self.0, p, o, d, n)
        }
        fn append(&mut self, p: &[u8], d: &[u8], n: u64) -> Result<()> {
            Backend::append(&mut *self.0, p, d, n)
        }
        fn truncate(&mut self, p: &[u8], s: u64, n: u64) -> Result<()> {
            Backend::truncate(&mut *self.0, p, s, n)
        }
        fn names(&mut self, p: &[u8]) -> Result<Vec<(Vec<u8>, EntryKind)>> {
            Backend::names(&mut *self.0, p)
        }
        fn mkdir(&mut self, p: &[u8], n: u64) -> Result<()> {
            Backend::mkdir(&mut *self.0, p, n)
        }
        fn rename(&mut self, a: &[u8], b: &[u8], n: u64) -> Result<()> {
            Backend::rename(&mut *self.0, a, b, n)
        }
        fn remove_all(&mut self, p: &[u8]) -> Result<()> {
            Backend::remove_all(&mut *self.0, p)
        }
        fn trash(&mut self, p: &[u8], n: u64) -> Result<()> {
            Backend::trash(&mut *self.0, p, n)
        }
        fn trash_list(&mut self) -> Result<Vec<TrashItem>> {
            Backend::trash_list(&mut *self.0)
        }
        fn trash_restore(&mut self, i: &[u8], n: u64) -> Result<Vec<u8>> {
            Backend::trash_restore(&mut *self.0, i, n)
        }
        fn trash_purge(&mut self, i: &[u8]) -> Result<()> {
            Backend::trash_purge(&mut *self.0, i)
        }
        fn empty_trash(&mut self) -> Result<()> {
            Backend::empty_trash(&mut *self.0)
        }
        fn usage(&mut self) -> Usage {
            Backend::usage(&mut *self.0)
        }
        fn set_owner(
            &mut self,
            _: &[u8],
            _: Option<u32>,
            _: Option<u32>,
            _: Option<u16>,
        ) -> Result<()> {
            Err(VfsError::Io)
        }
    }
    let mut fs = volume();
    // Make /home/ana writable for the test, then try to create through the failing backend.
    as_user(&mut fs, Cred::root())
        .set_owner(b"/home/ana", None, None, Some(0o777))
        .unwrap();
    {
        let mut be = NoOwner(&mut fs);
        let mut s = Secured::new(&mut be, ana(), DEFAULT_UMASK);
        assert_eq!(s.create(b"/home/ana/f", NOW), Err(VfsError::Io));
        assert_eq!(s.mkdir(b"/home/ana/d", NOW), Err(VfsError::Io));
        assert_eq!(s.write_file(b"/home/ana/w", b"x", NOW), Err(VfsError::Io));
    }
    let mut r = as_user(&mut fs, Cred::root());
    assert_eq!(r.names(b"/home/ana").unwrap().len(), 0);
}

#[test]
fn the_backend_trait_object_works() {
    let mut fs = volume();
    let mut s = Secured::new(&mut fs, ana(), DEFAULT_UMASK);
    let b: &mut dyn Backend = &mut s;
    b.write_file(b"/home/ana/x", b"1", NOW).unwrap();
    assert_eq!(b.read_file(b"/home/ana/x").unwrap(), b"1");
    assert_eq!(b.stat(b"/home/bia/anything"), Err(DENIED));
}

#[test]
fn rewriting_a_file_keeps_its_owner_group_and_mode() {
    let mut fs = volume();
    {
        let mut a = as_user(&mut fs, ana());
        a.write_file(b"/home/ana/doc", b"one", NOW).unwrap();
        a.set_owner(b"/home/ana/doc", None, None, Some(0o600))
            .unwrap();
        a.write_file(b"/home/ana/doc", b"two", NOW + 1).unwrap();
    }
    // Root rewriting someone's file must not take it over either.
    as_user(&mut fs, Cred::root())
        .write_file(b"/home/ana/doc", b"three", NOW + 2)
        .unwrap();
    let mut r = as_user(&mut fs, Cred::root());
    let i = r.stat(b"/home/ana/doc").unwrap();
    assert_eq!((i.uid, i.gid, i.mode), (ANA, USERS, 0o600));
    assert_eq!(r.read_file(b"/home/ana/doc").unwrap(), b"three");
}
