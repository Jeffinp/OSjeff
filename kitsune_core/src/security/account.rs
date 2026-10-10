//! The account database: users, groups, membership, and the text it is stored as.
//!
//! One file (`/etc/accounts` on the volume) holds both tables:
//!
//! ```text
//! # kitsune accounts v1
//! g:root:0:
//! g:admin:10:ana
//! g:users:100:ana,bia
//! u:ana:1000:100:Ana Souza:/home/ana:$kpw1$20000$...$...
//! ```
//!
//! A user's password field is a hash from [`super::password`], `!` for a locked account (nobody
//! can log in) or empty for an account without a password. Parsing is total: any bytes either
//! give a valid database or an [`AccountError`], never a panic, and a database that parses always
//! satisfies the invariants below (unique names and ids, a root user, bounded sizes).

use super::password::{self, DEFAULT_ITERATIONS};
use super::perm::Cred;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Most users a database holds.
pub const MAX_USERS: usize = 32;
/// Most groups a database holds.
pub const MAX_GROUPS: usize = 32;
/// Longest user or group name.
pub const MAX_NAME: usize = 32;
/// Longest full name.
pub const MAX_FULL_NAME: usize = 64;
/// Largest serialised database [`AccountDb::parse`] looks at.
pub const MAX_FILE: usize = 16 * 1024;
/// First uid given to a normal user.
pub const FIRST_UID: u32 = 1000;
/// Last uid (and gid) a database may use; keeps ids well away from "nobody"-style sentinels.
pub const MAX_ID: u32 = 60_000;

/// Group ids every database has.
pub const GID_ROOT: u32 = 0;
/// Members may administer the system (accounts, system files).
pub const GID_ADMIN: u32 = 10;
/// Everybody's primary group.
pub const GID_USERS: u32 = 100;

/// Marker in the password field for a locked account.
pub const LOCKED: &str = "!";

const HEADER: &str = "# kitsune accounts v1";

/// A user account.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct User {
    pub name: String,
    pub uid: u32,
    /// Primary group.
    pub gid: u32,
    pub full_name: String,
    pub home: String,
    /// A [`password`] hash, [`LOCKED`], or empty (no password).
    pub password: String,
}

impl User {
    /// Can this user log in without typing a password?
    pub fn has_no_password(&self) -> bool {
        self.password.is_empty()
    }

    /// Is the account locked?
    pub fn is_locked(&self) -> bool {
        self.password == LOCKED
    }
}

/// A group and its supplementary members (by user name).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Group {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

/// What can be wrong with a database or a change to it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AccountError {
    /// Not a valid name (see [`valid_name`]).
    BadName,
    /// Full name too long or containing `:` or control characters.
    BadFullName,
    BadHome,
    /// The password field is not a hash, `!` or empty.
    BadPassword,
    /// A line that is not `u:` or `g:` with the right number of fields, or a bad number.
    BadLine(usize),
    /// First line is not the header.
    BadHeader,
    TooBig,
    TooManyUsers,
    TooManyGroups,
    DuplicateName,
    DuplicateId,
    /// A user's primary group does not exist.
    NoSuchGroup,
    /// A group lists a member that is not a user.
    NoSuchMember,
    /// There is no uid 0 user named `root` (or `root` has another uid).
    NoRoot,
    NoSuchUser,
    /// `root`, the last administrator, or the group a user needs cannot be removed.
    Protected,
}

/// Why a login failed. The text shown to the user should not tell the first two apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthError {
    UnknownUser,
    BadPassword,
    Locked,
}

/// `[a-z_][a-z0-9_-]{0,31}`.
pub fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && (b[0].is_ascii_lowercase() || b[0] == b'_')
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

fn valid_full_name(n: &str) -> bool {
    n.chars().count() <= MAX_FULL_NAME && !n.chars().any(|c| c.is_control() || c == ':')
}

fn valid_home(h: &str) -> bool {
    h.starts_with('/')
        && h.len() <= 128
        && !h.chars().any(|c| c.is_control() || c == ':')
        && !h.split('/').any(|p| p == "..")
}

fn valid_password_field(p: &str) -> bool {
    p.is_empty() || p == LOCKED || password::is_valid_hash(p)
}

/// The users and groups of one machine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AccountDb {
    users: Vec<User>,
    groups: Vec<Group>,
}

impl Default for AccountDb {
    fn default() -> Self {
        AccountDb::system()
    }
}

impl AccountDb {
    /// The database of a fresh install: `root` (locked) and the groups `root`, `admin`, `users`.
    pub fn system() -> AccountDb {
        AccountDb {
            users: alloc::vec![User {
                name: "root".to_string(),
                uid: 0,
                gid: GID_ROOT,
                full_name: String::new(),
                home: "/root".to_string(),
                password: LOCKED.to_string(),
            }],
            groups: alloc::vec![
                Group {
                    name: "root".to_string(),
                    gid: GID_ROOT,
                    members: Vec::new()
                },
                Group {
                    name: "admin".to_string(),
                    gid: GID_ADMIN,
                    members: Vec::new()
                },
                Group {
                    name: "users".to_string(),
                    gid: GID_USERS,
                    members: Vec::new()
                },
            ],
        }
    }

    pub fn users(&self) -> &[User] {
        &self.users
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn user(&self, name: &str) -> Option<&User> {
        self.users.iter().find(|u| u.name == name)
    }

    pub fn user_by_uid(&self, uid: u32) -> Option<&User> {
        self.users.iter().find(|u| u.uid == uid)
    }

    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    pub fn group_by_gid(&self, gid: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.gid == gid)
    }

    /// Name of uid `uid` for display (`"?"` when unknown).
    pub fn user_name(&self, uid: u32) -> &str {
        self.user_by_uid(uid).map_or("?", |u| u.name.as_str())
    }

    /// Name of gid `gid` for display (`"?"` when unknown).
    pub fn group_name(&self, gid: u32) -> &str {
        self.group_by_gid(gid).map_or("?", |g| g.name.as_str())
    }

    /// The credentials of `user`: its uid, primary group and every group that lists it.
    pub fn cred(&self, user: &User) -> Cred {
        let groups = self
            .groups
            .iter()
            .filter(|g| g.members.contains(&user.name))
            .map(|g| g.gid)
            .collect();
        Cred::new(user.uid, user.gid, groups)
    }

    /// Is `user` allowed to administer the system (root, or a member of `admin`)?
    pub fn is_admin(&self, user: &User) -> bool {
        user.uid == 0
            || self
                .group_by_gid(GID_ADMIN)
                .is_some_and(|g| g.members.contains(&user.name))
    }

    /// How many administrators (members of `admin`, root excluded) there are.
    pub fn admin_count(&self) -> usize {
        self.group_by_gid(GID_ADMIN).map_or(0, |g| {
            g.members
                .iter()
                .filter(|m| self.user(m).is_some_and(|u| u.uid != 0))
                .count()
        })
    }

    fn next_uid(&self) -> Option<u32> {
        (FIRST_UID..=MAX_ID).find(|id| !self.users.iter().any(|u| u.uid == *id))
    }

    /// Add a user. `password_hash` is a [`password`] hash, [`LOCKED`] or empty. The user gets the
    /// next free uid from [`FIRST_UID`], the group `users`, and `/home/<name>`; `admin` also puts
    /// it in the `admin` group. Returns the new uid.
    pub fn add_user(
        &mut self,
        name: &str,
        full_name: &str,
        password_hash: &str,
        admin: bool,
    ) -> Result<u32, AccountError> {
        if !valid_name(name) || name == "root" {
            return Err(AccountError::BadName);
        }
        if !valid_full_name(full_name) {
            return Err(AccountError::BadFullName);
        }
        if !valid_password_field(password_hash) {
            return Err(AccountError::BadPassword);
        }
        if self.users.len() >= MAX_USERS {
            return Err(AccountError::TooManyUsers);
        }
        if self.user(name).is_some() || self.group(name).is_some() {
            return Err(AccountError::DuplicateName);
        }
        let uid = self.next_uid().ok_or(AccountError::TooManyUsers)?;
        self.users.push(User {
            name: name.to_string(),
            uid,
            gid: GID_USERS,
            full_name: full_name.to_string(),
            home: alloc::format!("/home/{name}"),
            password: password_hash.to_string(),
        });
        self.join(name, GID_USERS);
        if admin {
            self.join(name, GID_ADMIN);
        }
        Ok(uid)
    }

    fn join(&mut self, user: &str, gid: u32) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.gid == gid)
            && !g.members.iter().any(|m| m == user)
        {
            g.members.push(user.to_string());
        }
    }

    /// Put `user` in group `gid` (both must exist).
    pub fn add_to_group(&mut self, user: &str, gid: u32) -> Result<(), AccountError> {
        if self.user(user).is_none() {
            return Err(AccountError::NoSuchUser);
        }
        if self.group_by_gid(gid).is_none() {
            return Err(AccountError::NoSuchGroup);
        }
        self.join(user, gid);
        Ok(())
    }

    /// Take `user` out of group `gid`. The last administrator cannot leave `admin`.
    pub fn remove_from_group(&mut self, user: &str, gid: u32) -> Result<(), AccountError> {
        if gid == GID_ADMIN && self.admin_count() <= 1 && self.is_admin_member(user) {
            return Err(AccountError::Protected);
        }
        if let Some(g) = self.groups.iter_mut().find(|g| g.gid == gid) {
            g.members.retain(|m| m != user);
        }
        Ok(())
    }

    fn is_admin_member(&self, user: &str) -> bool {
        self.group_by_gid(GID_ADMIN)
            .is_some_and(|g| g.members.iter().any(|m| m == user))
    }

    /// Delete a user and its memberships. `root` and the last administrator are protected.
    pub fn remove_user(&mut self, name: &str) -> Result<(), AccountError> {
        let Some(u) = self.user(name) else {
            return Err(AccountError::NoSuchUser);
        };
        if u.uid == 0 || (self.is_admin_member(name) && self.admin_count() <= 1) {
            return Err(AccountError::Protected);
        }
        self.users.retain(|u| u.name != name);
        for g in &mut self.groups {
            g.members.retain(|m| m != name);
        }
        Ok(())
    }

    /// Replace `name`'s password field (a hash, [`LOCKED`] or empty).
    pub fn set_password(&mut self, name: &str, password_hash: &str) -> Result<(), AccountError> {
        if !valid_password_field(password_hash) {
            return Err(AccountError::BadPassword);
        }
        let u = self
            .users
            .iter_mut()
            .find(|u| u.name == name)
            .ok_or(AccountError::NoSuchUser)?;
        u.password = password_hash.to_string();
        Ok(())
    }

    /// Change a user's full name.
    pub fn set_full_name(&mut self, name: &str, full_name: &str) -> Result<(), AccountError> {
        if !valid_full_name(full_name) {
            return Err(AccountError::BadFullName);
        }
        let u = self
            .users
            .iter_mut()
            .find(|u| u.name == name)
            .ok_or(AccountError::NoSuchUser)?;
        u.full_name = full_name.to_string();
        Ok(())
    }

    /// Check a login. An unknown user still costs a full hash computation, so the time taken does
    /// not reveal which names exist.
    pub fn authenticate(&self, name: &str, password: &str) -> Result<&User, AuthError> {
        let Some(u) = self.user(name) else {
            let _ = password::pbkdf2(
                password.as_bytes(),
                b"kitsune-no-such-user",
                DEFAULT_ITERATIONS,
            );
            return Err(AuthError::UnknownUser);
        };
        if u.is_locked() {
            return Err(AuthError::Locked);
        }
        if u.has_no_password() {
            // Only an empty attempt matches an account without a password.
            return if password.is_empty() {
                Ok(u)
            } else {
                Err(AuthError::BadPassword)
            };
        }
        if password::verify(password, &u.password) {
            Ok(u)
        } else {
            Err(AuthError::BadPassword)
        }
    }

    /// Serialise to the on-disk text (always parses back to an equal database).
    pub fn serialize(&self) -> String {
        let mut s = String::new();
        s.push_str(HEADER);
        s.push('\n');
        for g in &self.groups {
            s.push_str(&alloc::format!(
                "g:{}:{}:{}\n",
                g.name,
                g.gid,
                g.members.join(",")
            ));
        }
        for u in &self.users {
            s.push_str(&alloc::format!(
                "u:{}:{}:{}:{}:{}:{}\n",
                u.name,
                u.uid,
                u.gid,
                u.full_name,
                u.home,
                u.password
            ));
        }
        s
    }

    /// Parse the on-disk text, validating everything.
    pub fn parse(text: &str) -> Result<AccountDb, AccountError> {
        if text.len() > MAX_FILE {
            return Err(AccountError::TooBig);
        }
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(AccountError::BadHeader);
        }
        let mut db = AccountDb {
            users: Vec::new(),
            groups: Vec::new(),
        };
        for (i, line) in lines.enumerate() {
            let n = i + 2;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("g:") {
                let f: Vec<&str> = rest.split(':').collect();
                if f.len() != 3 {
                    return Err(AccountError::BadLine(n));
                }
                if !valid_name(f[0]) {
                    return Err(AccountError::BadName);
                }
                let gid = parse_id(f[1]).ok_or(AccountError::BadLine(n))?;
                let members: Vec<String> = if f[2].is_empty() {
                    Vec::new()
                } else {
                    f[2].split(',').map(|m| m.to_string()).collect()
                };
                if members.iter().any(|m| !valid_name(m)) {
                    return Err(AccountError::BadName);
                }
                if db.groups.len() >= MAX_GROUPS {
                    return Err(AccountError::TooManyGroups);
                }
                db.groups.push(Group {
                    name: f[0].to_string(),
                    gid,
                    members,
                });
            } else if let Some(rest) = line.strip_prefix("u:") {
                let f: Vec<&str> = rest.split(':').collect();
                if f.len() != 6 {
                    return Err(AccountError::BadLine(n));
                }
                if !valid_name(f[0]) {
                    return Err(AccountError::BadName);
                }
                let uid = parse_id(f[1]).ok_or(AccountError::BadLine(n))?;
                let gid = parse_id(f[2]).ok_or(AccountError::BadLine(n))?;
                if !valid_full_name(f[3]) {
                    return Err(AccountError::BadFullName);
                }
                if !valid_home(f[4]) {
                    return Err(AccountError::BadHome);
                }
                if !valid_password_field(f[5]) {
                    return Err(AccountError::BadPassword);
                }
                if db.users.len() >= MAX_USERS {
                    return Err(AccountError::TooManyUsers);
                }
                db.users.push(User {
                    name: f[0].to_string(),
                    uid,
                    gid,
                    full_name: f[3].to_string(),
                    home: f[4].to_string(),
                    password: f[5].to_string(),
                });
            } else {
                return Err(AccountError::BadLine(n));
            }
        }
        db.validate()?;
        Ok(db)
    }

    fn validate(&self) -> Result<(), AccountError> {
        for (i, u) in self.users.iter().enumerate() {
            if self.users[..i]
                .iter()
                .any(|o| o.name == u.name || o.uid == u.uid)
            {
                return Err(if self.users[..i].iter().any(|o| o.name == u.name) {
                    AccountError::DuplicateName
                } else {
                    AccountError::DuplicateId
                });
            }
            if self.group_by_gid(u.gid).is_none() {
                return Err(AccountError::NoSuchGroup);
            }
        }
        for (i, g) in self.groups.iter().enumerate() {
            if self.groups[..i]
                .iter()
                .any(|o| o.name == g.name || o.gid == g.gid)
            {
                return Err(if self.groups[..i].iter().any(|o| o.name == g.name) {
                    AccountError::DuplicateName
                } else {
                    AccountError::DuplicateId
                });
            }
            if g.members.iter().any(|m| self.user(m).is_none()) {
                return Err(AccountError::NoSuchMember);
            }
        }
        // A user and a group may not share a name unless it is the user's own private group name
        // (we do not create those, so keep the rule simple: names are unique across both tables
        // except `root`, which is both).
        for u in &self.users {
            if u.name != "root" && self.group(&u.name).is_some() {
                return Err(AccountError::DuplicateName);
            }
        }
        match self.user("root") {
            Some(r) if r.uid == 0 => {}
            _ => return Err(AccountError::NoRoot),
        }
        if self.users.iter().any(|u| u.uid == 0 && u.name != "root") {
            return Err(AccountError::NoRoot);
        }
        Ok(())
    }
}

fn parse_id(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: u32 = s.parse().ok()?;
    (v <= MAX_ID).then_some(v)
}

#[cfg(test)]
mod tests;
