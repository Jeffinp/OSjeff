//! accounts: `whoami`, `id`, `groups`, `users`, and the permission commands `chmod`, `chown`,
//! `chgrp`. They talk to the system through [`super::SysInfo`] (who is signed in, name lookups)
//! and to the files through [`super::ShellFs`] (`meta`, `set_owner`).

use super::*;
use crate::apps::shell::fs::FileMeta;
use crate::security::perm;

fn no_accounts(cx: &mut CmdCtx<'_>) -> i32 {
    cx.error(t!("sh.err.no_accounts"));
    1
}

pub(super) fn whoami(cx: &mut CmdCtx<'_>) -> i32 {
    match cx.sys.identity() {
        Some(id) => {
            cx.println(&id.name);
            0
        }
        None => no_accounts(cx),
    }
}

pub(super) fn id(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(me) = cx.sys.identity() else {
        return no_accounts(cx);
    };
    let groups: Vec<String> = me.groups.iter().map(|(g, n)| format!("{g}({n})")).collect();
    let line = t!(
        "sh.id.line",
        uid = me.uid,
        name = me.name.as_str(),
        gid = me.gid,
        gname = me.gname.as_str(),
        groups = groups.join(",").as_str()
    );
    cx.println(&line);
    0
}

pub(super) fn groups(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(me) = cx.sys.identity() else {
        return no_accounts(cx);
    };
    let names: Vec<&str> = me.groups.iter().map(|(_, n)| n.as_str()).collect();
    cx.println(&names.join(" "));
    0
}

pub(super) fn users(cx: &mut CmdCtx<'_>) -> i32 {
    let list = cx.sys.user_list();
    if list.is_empty() {
        return no_accounts(cx);
    }
    for u in list {
        let admin = if u.admin {
            format!("  {}", t!("sh.users.admin"))
        } else {
            String::new()
        };
        let full = if u.full_name.is_empty() {
            String::new()
        } else {
            format!("  {}", u.full_name)
        };
        cx.println(&format!("{:<12} {:>5}{}{}", u.name, u.uid, full, admin));
    }
    0
}

/// Apply one `chmod` mode argument to `cur`: an octal number (`644`, `1777`) or symbolic clauses
/// (`u+x`, `go-w`, `a=r`, `u+rwx,g=rx`, `+t`, `a+X`). `None` if it is neither.
pub(crate) fn apply_mode(spec: &str, cur: u16, is_dir: bool) -> Option<u16> {
    if spec.bytes().all(|b| b.is_ascii_digit()) && !spec.is_empty() {
        return perm::parse_mode(spec);
    }
    let mut mode = cur & perm::MODE_MASK;
    for clause in spec.split(',') {
        let b = clause.as_bytes();
        let ops_at = b.iter().position(|c| matches!(c, b'+' | b'-' | b'='))?;
        let who = &clause[..ops_at];
        let mut users: u16 = 0;
        if who.is_empty() {
            users = 0o777;
        }
        for c in who.chars() {
            users |= match c {
                'u' => 0o700,
                'g' => 0o070,
                'o' => 0o007,
                'a' => 0o777,
                _ => return None,
            };
        }
        let sticky_ok = who.is_empty() || who.contains('o') || who.contains('a');
        let mut rest = &clause[ops_at..];
        while !rest.is_empty() {
            let op = rest.as_bytes()[0];
            let end = rest[1..]
                .find(['+', '-', '='])
                .map_or(rest.len(), |i| i + 1);
            let perms = &rest[1..end];
            let mut bits: u16 = 0;
            for c in perms.chars() {
                match c {
                    'r' => bits |= 0o444,
                    'w' => bits |= 0o222,
                    'x' => bits |= 0o111,
                    'X' if is_dir || mode & 0o111 != 0 => bits |= 0o111,
                    'X' => {}
                    't' => {
                        if sticky_ok {
                            bits |= perm::STICKY;
                        }
                    }
                    _ => return None,
                }
            }
            let sticky = bits & perm::STICKY;
            let masked = (bits & 0o777) & users;
            match op {
                b'+' => mode |= masked | sticky,
                b'-' => {
                    mode &= !masked;
                    if sticky != 0 {
                        mode &= !perm::STICKY;
                    }
                }
                _ => {
                    mode = (mode & !users) | masked;
                    if sticky_ok {
                        mode = (mode & !perm::STICKY) | sticky;
                    }
                }
            }
            rest = &rest[end..];
        }
    }
    Some(mode & perm::MODE_MASK)
}

/// Every path `p` stands for: itself, and with `-R` everything under a directory.
fn targets(cx: &mut CmdCtx<'_>, p: &str, recursive: bool) -> Vec<String> {
    let root = cx.fs.resolve(p);
    let mut out = alloc::vec![root.clone()];
    if !recursive {
        return out;
    }
    let mut stack = alloc::vec![root];
    while let Some(d) = stack.pop() {
        if out.len() > 10_000 {
            break;
        }
        if cx.fs.stat(&d).map(|s| s.kind) != Ok(Kind::Dir) {
            continue;
        }
        if let Ok(list) = cx.fs.list(&d) {
            for e in list {
                let child = join(&d, &e.name);
                out.push(child.clone());
                stack.push(child);
            }
        }
    }
    out
}

pub(super) fn chmod(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "R", "", None) else {
        return 2;
    };
    if o.operands.len() < 2 {
        cx.error(t!("sh.chmod.usage"));
        return 2;
    }
    let spec = o.operands[0].clone();
    let mut status = 0;
    for p in &o.operands[1..] {
        for path in targets(cx, p, o.has('R')) {
            let (cur, is_dir) = match (cx.fs.meta(&path), cx.fs.stat(&path)) {
                (Some(m), Ok(s)) => (m.mode, s.kind == Kind::Dir),
                (None, Ok(s)) => (
                    if s.kind == Kind::Dir { 0o755 } else { 0o644 },
                    s.kind == Kind::Dir,
                ),
                (_, Err(e)) => {
                    let msg = t!(
                        "sh.err.cannot_access",
                        path = path.as_str(),
                        why = e.message()
                    );
                    cx.error(&msg);
                    status = 1;
                    continue;
                }
            };
            let Some(new) = apply_mode(&spec, cur, is_dir) else {
                cx.error(&t!("sh.chmod.bad_mode", mode = spec.as_str()));
                return 2;
            };
            if let Err(e) = cx.fs.set_owner(&path, None, None, Some(new)) {
                fs_err(cx, &path, e);
                status = 1;
            }
        }
    }
    status
}

/// `OWNER[:GROUP]`, `:GROUP` or `OWNER:` -> (uid, gid) to set; each is a name or a number.
fn parse_owner(cx: &mut CmdCtx<'_>, spec: &str) -> Option<(Option<u32>, Option<u32>)> {
    let (u, g) = match spec.split_once(':') {
        Some((u, g)) => (u, g),
        None => (spec, ""),
    };
    let uid = if u.is_empty() {
        None
    } else if let Ok(n) = u.parse::<u32>() {
        Some(n)
    } else if let Some(n) = cx.sys.lookup_user(u) {
        Some(n)
    } else {
        cx.error(&t!("sh.chown.no_user", name = u));
        return None;
    };
    let gid = if g.is_empty() {
        None
    } else if let Ok(n) = g.parse::<u32>() {
        Some(n)
    } else if let Some(n) = cx.sys.lookup_group(g) {
        Some(n)
    } else {
        cx.error(&t!("sh.chown.no_group", name = g));
        return None;
    };
    Some((uid, gid))
}

fn chown_files(
    cx: &mut CmdCtx<'_>,
    uid: Option<u32>,
    gid: Option<u32>,
    files: &[String],
    rec: bool,
) -> i32 {
    let mut status = 0;
    for p in files {
        for path in targets(cx, p, rec) {
            if let Err(e) = cx.fs.set_owner(&path, uid, gid, None) {
                fs_err(cx, &path, e);
                status = 1;
            }
        }
    }
    status
}

pub(super) fn chown(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "R", "", None) else {
        return 2;
    };
    if o.operands.len() < 2 {
        cx.error(t!("sh.chown.usage"));
        return 2;
    }
    let Some((uid, gid)) = parse_owner(cx, &o.operands[0].clone()) else {
        return 1;
    };
    chown_files(cx, uid, gid, &o.operands[1..], o.has('R'))
}

pub(super) fn chgrp(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "R", "", None) else {
        return 2;
    };
    if o.operands.len() < 2 {
        cx.error(t!("sh.chgrp.usage"));
        return 2;
    }
    let Some((_, gid)) = parse_owner(cx, &alloc::format!(":{}", o.operands[0])) else {
        return 1;
    };
    chown_files(cx, None, gid, &o.operands[1..], o.has('R'))
}

/// `drwxr-xr-x ana users 4096 name`: the long listing line when the filesystem has owners.
pub(super) fn long_line(
    cx: &CmdCtx<'_>,
    meta: FileMeta,
    kind: Kind,
    size: u64,
    name: &str,
    suffix: &str,
) -> String {
    let t = if kind == Kind::Dir { 'd' } else { '-' };
    let owner = cx
        .sys
        .user_name(meta.uid)
        .unwrap_or_else(|| meta.uid.to_string());
    let group = cx
        .sys
        .group_name(meta.gid)
        .unwrap_or_else(|| meta.gid.to_string());
    format!(
        "{t}{} {owner:>8} {group:>8} {size:>8} {name}{suffix}\n",
        perm::mode_string(meta.mode)
    )
}
