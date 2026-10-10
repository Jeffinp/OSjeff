//! File permissions: the `rwx` triplets for owner, group and others, plus the sticky bit.
//!
//! A [`Cred`] is who is asking; an [`Owner`] is who a file belongs to and its mode. The rules are
//! the classic ones: the owner class applies to the owner only, the group class to a member of the
//! file's group who is not the owner, the others class to everybody else; `root` (uid 0) may read
//! and write anything and execute what has at least one execute bit (or any directory).

use alloc::vec::Vec;

/// Read bit of one class.
pub const R: u8 = 4;
/// Write bit of one class.
pub const W: u8 = 2;
/// Execute (files) or search (directories) bit of one class.
pub const X: u8 = 1;

/// Sticky bit: in a shared folder only the owner of a file (or of the folder) may remove it.
pub const STICKY: u16 = 0o1000;
/// All the permission bits this module knows (`rwxrwxrwx` and sticky).
pub const MODE_MASK: u16 = 0o1777;

/// The superuser.
pub const ROOT_UID: u32 = 0;

/// Who is asking.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cred {
    pub uid: u32,
    pub gid: u32,
    /// Supplementary groups.
    pub groups: Vec<u32>,
}

impl Cred {
    pub fn new(uid: u32, gid: u32, groups: Vec<u32>) -> Cred {
        Cred { uid, gid, groups }
    }

    /// The superuser (uid 0, gid 0).
    pub fn root() -> Cred {
        Cred::new(ROOT_UID, 0, Vec::new())
    }

    pub fn is_root(&self) -> bool {
        self.uid == ROOT_UID
    }

    /// Is the user in group `gid` (primary or supplementary)?
    pub fn in_group(&self, gid: u32) -> bool {
        self.gid == gid || self.groups.contains(&gid)
    }
}

/// Who a file belongs to and who may do what to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Owner {
    pub uid: u32,
    pub gid: u32,
    pub mode: u16,
}

impl Owner {
    pub const fn new(uid: u32, gid: u32, mode: u16) -> Owner {
        Owner { uid, gid, mode }
    }

    fn class_bits(&self, who: &Cred) -> u8 {
        let shift = if who.uid == self.uid {
            6
        } else if who.in_group(self.gid) {
            3
        } else {
            0
        };
        ((self.mode >> shift) & 7) as u8
    }
}

/// May `who` do all of `want` (a mix of [`R`], [`W`], [`X`]) to something owned by `owner`?
/// `is_dir` matters for the superuser only (it may search any directory).
pub fn allowed(who: &Cred, owner: &Owner, want: u8, is_dir: bool) -> bool {
    let want = want & 7;
    if who.is_root() {
        let x_ok = want & X == 0 || is_dir || owner.mode & 0o111 != 0;
        return x_ok;
    }
    owner.class_bits(who) & want == want
}

/// May `who` change the mode of the file? Its owner and the superuser may.
pub fn may_chmod(who: &Cred, owner: &Owner) -> bool {
    who.is_root() || who.uid == owner.uid
}

/// May `who` give the file to `new_uid` / `new_gid` (`None` = leave as is)?
/// Only the superuser changes the owner; the owner may move the file to a group it belongs to.
pub fn may_chown(who: &Cred, owner: &Owner, new_uid: Option<u32>, new_gid: Option<u32>) -> bool {
    if who.is_root() {
        return true;
    }
    if who.uid != owner.uid {
        return false;
    }
    if new_uid.is_some_and(|u| u != owner.uid) {
        return false;
    }
    new_gid.is_none_or(|g| who.in_group(g))
}

/// May `who` remove or rename `child` out of `dir`? Needs write+search on the folder and, when the
/// folder is sticky, to own the file or the folder (the superuser always may).
pub fn may_remove(who: &Cred, dir: &Owner, child: &Owner) -> bool {
    if !allowed(who, dir, W | X, true) {
        return false;
    }
    if dir.mode & STICKY != 0 && !who.is_root() {
        return who.uid == child.uid || who.uid == dir.uid;
    }
    true
}

/// The mode a new file gets: the requested `mode` without the bits in `umask`.
pub fn apply_umask(mode: u16, umask: u16) -> u16 {
    mode & !umask & MODE_MASK
}

/// `rwxr-xr-x` style text for a mode (nine characters, `t`/`T` for the sticky bit).
pub fn mode_string(mode: u16) -> alloc::string::String {
    let mut s = alloc::string::String::with_capacity(9);
    for (i, c) in "rwxrwxrwx".chars().enumerate() {
        let bit = 0o400 >> i;
        s.push(if mode & bit != 0 { c } else { '-' });
    }
    if mode & STICKY != 0 {
        let last = s.pop().unwrap_or('-');
        s.push(if last == 'x' { 't' } else { 'T' });
    }
    s
}

/// Parse an octal mode (`644`, `0755`, `1777`); `None` if it is not octal or has unknown bits.
pub fn parse_mode(s: &str) -> Option<u16> {
    if s.is_empty() || s.len() > 5 {
        return None;
    }
    let v = u16::from_str_radix(s, 8).ok()?;
    (v & !MODE_MASK == 0).then_some(v)
}

#[cfg(test)]
mod tests;
