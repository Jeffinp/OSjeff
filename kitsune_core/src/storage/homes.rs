//! The folders accounts rely on, and moving a pre-accounts volume into them.
//!
//! A volume made before accounts existed keeps everything at the top level (`/Documentos`,
//! `/Imagens`, `/leiame.txt`, ...) and in a flat `/home`. When the first account is created,
//! [`adopt_legacy`] moves all of that into that user's own home, so nothing is lost and nothing
//! is left owned by nobody; later users get an empty home from [`create_home`].
//!
//! Every function takes a plain [`Backend`] and acts as the superuser (it is called by the
//! system, not on behalf of a user).

use super::vfs::{Backend, EntryKind, Result, VfsError, join};
use crate::security::account::User;
use alloc::vec::Vec;

/// System folders: owner root, mode `rwxr-xr-x`, never moved into a home.
pub const SYSTEM_DIRS: [&[u8]; 6] = [b"/etc", b"/var", b"/apps", b"/data", b"/home", b"/.trash"];

/// Folders created inside every home.
pub const HOME_FOLDERS: [&[u8]; 2] = [b"Documentos", b"Imagens"];

#[cfg(not(test))]
mod cur {
    /// Longest home path the registry keeps (`/home/` + a 32-character name fits easily).
    const CUR_MAX: usize = 96;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering::Relaxed};

    static CUR: [AtomicU8; CUR_MAX] = [const { AtomicU8::new(0) }; CUR_MAX];
    static CUR_LEN: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn set(path: &[u8]) {
        let n = path.len().min(CUR_MAX);
        CUR_LEN.store(0, Relaxed);
        for (i, b) in path[..n].iter().enumerate() {
            CUR[i].store(*b, Relaxed);
        }
        CUR_LEN.store(n, Relaxed);
    }

    pub(super) fn get() -> Vec<u8> {
        let n = CUR_LEN.load(Relaxed);
        (0..n).map(|i| CUR[i].load(Relaxed)).collect()
    }
}

/// Under `cargo test` each test thread has its own, so tests that sign somebody in cannot
/// disturb the ones that expect the default.
#[cfg(test)]
mod cur {
    use alloc::vec::Vec;
    std::thread_local! {
        static CUR: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    pub(super) fn set(path: &[u8]) {
        CUR.with(|c| *c.borrow_mut() = path.to_vec());
    }
    pub(super) fn get() -> Vec<u8> {
        CUR.with(|c| c.borrow().clone())
    }
}

/// Tell the file manager and the editor's dialogs whose home the sidebar shows. The kernel calls
/// this when somebody signs in; the writer is the only thread that changes it.
pub fn set_current_home(path: &[u8]) {
    cur::set(path);
}

/// The home folder to show (`/home` before anyone has signed in).
pub fn current_home() -> Vec<u8> {
    let h = cur::get();
    if h.is_empty() { b"/home".to_vec() } else { h }
}

/// `/home/<name>`.
pub fn home_path(name: &str) -> Vec<u8> {
    let mut p = b"/home/".to_vec();
    p.extend_from_slice(name.as_bytes());
    p
}

/// Set owner, group and mode only when they differ (boot runs this every time; each write is a
/// disk transaction).
fn fix(be: &mut dyn Backend, path: &[u8], uid: u32, gid: u32, mode: u16) -> Result<()> {
    if let Ok(i) = be.stat(path)
        && (i.uid, i.gid, i.mode) == (uid, gid, mode)
    {
        return Ok(());
    }
    be.set_owner(path, Some(uid), Some(gid), Some(mode))
}

/// Make sure the system folders exist, belong to root and are `rwxr-xr-x`, and the root folder
/// is too. (`/.trash` is managed by the volume itself and left alone.)
pub fn ensure_system(be: &mut dyn Backend, now: u64) -> Result<()> {
    fix(be, b"/", 0, 0, 0o755)?;
    for d in SYSTEM_DIRS {
        if d == b"/.trash" {
            continue;
        }
        if be.stat(d).is_err() {
            be.mkdir(d, now)?;
        }
        fix(be, d, 0, 0, 0o755)?;
    }
    Ok(())
}

/// Create `/home/<user>` (private: `rwx------`) with its standard folders, all owned by the user.
/// An existing home is left as it is (its owner and mode are put right).
pub fn create_home(be: &mut dyn Backend, user: &User, now: u64) -> Result<Vec<u8>> {
    let home = make_home_dir(be, user, now)?;
    ensure_home_folders(be, user, &home, now)?;
    Ok(home)
}

fn make_home_dir(be: &mut dyn Backend, user: &User, now: u64) -> Result<Vec<u8>> {
    let home = user.home.as_bytes().to_vec();
    if be.stat(&home).is_err() {
        be.mkdir(&home, now)?;
    }
    fix(be, &home, user.uid, user.gid, 0o700)?;
    Ok(home)
}

fn ensure_home_folders(be: &mut dyn Backend, user: &User, home: &[u8], now: u64) -> Result<()> {
    for f in HOME_FOLDERS {
        let p = join(home, f);
        if be.stat(&p).is_err() {
            be.mkdir(&p, now)?;
        }
        fix(be, &p, user.uid, user.gid, 0o755)?;
    }
    Ok(())
}

/// What [`adopt_legacy`] did.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Adopted {
    /// Top-level entries moved into the home.
    pub moved: usize,
    /// Entries of the old flat `/home` moved into the user's home.
    pub from_home: usize,
}

fn unique_in(be: &mut dyn Backend, dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut n = 1u32;
    let mut cand = name.to_vec();
    while be.stat(&join(dir, &cand)).is_ok() {
        n += 1;
        cand = name.to_vec();
        cand.extend_from_slice(alloc::format!(" ({n})").as_bytes());
        if n > 999 {
            break;
        }
    }
    cand
}

/// Move the pre-accounts content of the volume into `user`'s home and hand it to them: every
/// top-level entry that is not a system folder, and every entry of the old flat `/home` (except
/// the folders of other accounts, which are `homes`). Names that collide inside the home get a
/// ` (2)` suffix.
pub fn adopt_legacy(
    be: &mut dyn Backend,
    user: &User,
    homes: &[&str],
    now: u64,
) -> Result<Adopted> {
    let home = make_home_dir(be, user, now)?;
    let mut out = Adopted::default();

    let top = be.names(b"/")?;
    for (name, _kind) in top {
        let full = join(b"/", &name);
        if SYSTEM_DIRS.contains(&full.as_slice()) || name.first() == Some(&b'.') {
            continue;
        }
        let dest_name = unique_in(be, &home, &name);
        be.rename(&full, &join(&home, &dest_name), now)?;
        out.moved += 1;
    }

    let flat = be.names(b"/home")?;
    for (name, kind) in flat {
        let is_home_dir = kind == EntryKind::Dir
            && core::str::from_utf8(&name).is_ok_and(|n| homes.contains(&n) || n == user.name);
        if is_home_dir {
            continue;
        }
        let full = join(b"/home", &name);
        let dest_name = unique_in(be, &home, &name);
        be.rename(&full, &join(&home, &dest_name), now)?;
        out.from_home += 1;
    }

    // The standard folders only now, so a legacy `/Documentos` becomes the home's `Documentos`
    // instead of colliding with an empty one.
    ensure_home_folders(be, user, &home, now)?;
    own_tree(be, &home, user.uid, user.gid)?;
    Ok(out)
}

/// Give `path` and everything under it to `uid:gid` (modes are kept).
pub fn own_tree(be: &mut dyn Backend, path: &[u8], uid: u32, gid: u32) -> Result<()> {
    let mut stack: Vec<Vec<u8>> = alloc::vec![path.to_vec()];
    let mut budget = 100_000usize;
    while let Some(p) = stack.pop() {
        if budget == 0 {
            return Err(VfsError::TooBig);
        }
        budget -= 1;
        be.set_owner(&p, Some(uid), Some(gid), None)?;
        let info = be.stat(&p)?;
        if info.kind == EntryKind::Dir {
            for (n, _) in be.names(&p)? {
                stack.push(join(&p, &n));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
