//! files (split out of `builtins.rs`).

use super::*;

pub(super) fn ls(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "aFl1", "", None) else {
        return 2;
    };
    let long = o.has('l');
    let all = o.has('a');
    let classify = o.has('F');
    let mut targets = o.operands.clone();
    if targets.is_empty() {
        targets.push(String::from("."));
    }
    let many = targets.len() > 1;
    let mut status = 0;
    let mut first = true;
    let render = |name: &str, kind: Kind, size: u64| -> String {
        let suffix = if classify && kind == Kind::Dir {
            "/"
        } else {
            ""
        };
        if long {
            let t = if kind == Kind::Dir { 'd' } else { '-' };
            format!("{t} {size:>8} {name}{suffix}\n")
        } else {
            format!("{name}{suffix}\n")
        }
    };
    for t in &targets {
        let st = match cx.fs.stat(t) {
            Ok(s) => s,
            Err(e) => {
                let msg = t!("sh.err.cannot_access", path = t.as_str(), why = e.message());
                cx.error(&msg);
                status = 2;
                continue;
            }
        };
        if st.kind == Kind::File {
            let s = render(t, Kind::File, st.size);
            cx.print(&s);
            continue;
        }
        let mut entries = match cx.fs.list(t) {
            Ok(e) => e,
            Err(e) => {
                fs_err(cx, t, e);
                status = 2;
                continue;
            }
        };
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        if many {
            if !first {
                cx.print("\n");
            }
            cx.print(&format!("{t}:\n"));
        }
        first = false;
        if all {
            cx.print(&render(".", Kind::Dir, 0));
            cx.print(&render("..", Kind::Dir, 0));
        }
        for e in entries {
            if !all && e.name.starts_with('.') {
                continue;
            }
            if cx.full() {
                break;
            }
            let s = render(&e.name, e.kind, e.size);
            cx.print(&s);
        }
    }
    status
}

pub(super) fn set_pwd(cx: &mut CmdCtx<'_>, old: &str) {
    let now = cx.fs.cwd();
    cx.env.set_exported("OLDPWD", old);
    cx.env.set_exported("PWD", &now);
}

pub(super) fn cd(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() > 2 {
        cx.error(t!("sh.err.too_many_args"));
        return 1;
    }
    let old = cx.fs.cwd();
    let target = match cx.args.get(1).map(String::as_str) {
        None => cx.env.get("HOME").unwrap_or("/").to_string(),
        Some("-") => match cx.env.get("OLDPWD") {
            Some(p) if !p.is_empty() => {
                let p = p.to_string();
                cx.println(&p);
                p
            }
            _ => {
                cx.error(t!("sh.cd.oldpwd_unset"));
                return 1;
            }
        },
        Some(p) => p.to_string(),
    };
    match cx.fs.set_cwd(&target) {
        Ok(()) => {
            set_pwd(cx, &old);
            0
        }
        Err(e) => {
            fs_err(cx, &target, e);
            1
        }
    }
}

pub(super) fn pwd(cx: &mut CmdCtx<'_>) -> i32 {
    let c = cx.fs.cwd();
    cx.println(&c);
    0
}

pub(super) fn cat(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "n", "", None) else {
        return 2;
    };
    let (ins, ok) = inputs(cx, &o.operands);
    let mut n = 0usize;
    for (_, data) in ins {
        if o.has('n') {
            for l in lines_of(&data) {
                n += 1;
                cx.print(&format!("{n:>6}\t{l}\n"));
            }
        } else {
            cx.out(&data);
        }
        if cx.full() {
            break;
        }
    }
    i32::from(!ok)
}

pub(super) fn echo(cx: &mut CmdCtx<'_>) -> i32 {
    let mut newline = true;
    let mut escapes = false;
    let mut i = 1;
    while i < cx.args.len() {
        let a = &cx.args[i];
        if a.len() > 1 && a.starts_with('-') && a[1..].chars().all(|c| c == 'n' || c == 'e') {
            for c in a[1..].chars() {
                if c == 'n' {
                    newline = false;
                } else {
                    escapes = true;
                }
            }
            i += 1;
        } else {
            break;
        }
    }
    let text = cx.args[i..].join(" ");
    if escapes {
        let mut out = String::new();
        let mut it = text.chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('a') => out.push('\u{7}'),
                Some('\\') => out.push('\\'),
                Some('c') => {
                    newline = false;
                    break;
                }
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        }
        cx.print(&out);
    } else {
        cx.print(&text);
    }
    if newline {
        cx.print("\n");
    }
    0
}

pub(super) fn mkdir(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "p", "", None) else {
        return 2;
    };
    if o.operands.is_empty() {
        cx.error(t!("sh.err.missing_operand"));
        return 2;
    }
    let mut status = 0;
    for d in &o.operands {
        if o.has('p') {
            let abs = cx.fs.resolve(d);
            let mut cur = String::new();
            for comp in abs.split('/').filter(|c| !c.is_empty()) {
                cur.push('/');
                cur.push_str(comp);
                match cx.fs.stat(&cur) {
                    Ok(s) if s.kind == Kind::Dir => {}
                    Ok(_) => {
                        fs_err(cx, &cur, FsErr::NotADirectory);
                        status = 1;
                        break;
                    }
                    Err(_) => {
                        if let Err(e) = cx.fs.mkdir(&cur) {
                            fs_err(cx, &cur, e);
                            status = 1;
                            break;
                        }
                    }
                }
            }
        } else if let Err(e) = cx.fs.mkdir(d) {
            let msg = t!(
                "sh.err.cannot_create_dir",
                path = d.as_str(),
                why = e.message()
            );
            cx.error(&msg);
            status = 1;
        }
    }
    status
}

pub(super) fn remove_tree(
    cx: &mut CmdCtx<'_>,
    path: &str,
    depth: usize,
) -> Result<(), (String, FsErr)> {
    let st = cx.fs.stat(path).map_err(|e| (path.to_string(), e))?;
    if st.kind == Kind::Dir {
        if depth > 64 {
            return Err((path.to_string(), FsErr::InvalidPath));
        }
        let abs = cx.fs.resolve(path);
        let entries = cx.fs.list(&abs).map_err(|e| (path.to_string(), e))?;
        for e in entries {
            remove_tree(cx, &join(&abs, &e.name), depth + 1)?;
        }
    }
    cx.fs.remove(path).map_err(|e| (path.to_string(), e))
}

pub(super) fn rm(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "rRf", "", None) else {
        return 2;
    };
    if o.operands.is_empty() {
        if o.has('f') {
            return 0;
        }
        cx.error(t!("sh.err.missing_operand"));
        return 2;
    }
    let recursive = o.has('r') || o.has('R');
    let force = o.has('f');
    let mut status = 0;
    for p in &o.operands {
        let abs = cx.fs.resolve(p);
        let base = basename(p.trim_end_matches('/'));
        if abs == "/" || base == "." || base == ".." {
            cx.error(&t!("sh.err.refuse_remove", path = p.as_str()));
            status = 1;
            continue;
        }
        match cx.fs.stat(p) {
            Err(FsErr::NotFound) if force => {}
            Err(e) => {
                let msg = t!("sh.err.cannot_remove", path = p.as_str(), why = e.message());
                cx.error(&msg);
                status = 1;
            }
            Ok(s) if s.kind == Kind::Dir && !recursive => {
                cx.error(&t!(
                    "sh.err.cannot_remove",
                    path = p.as_str(),
                    why = FsErr::IsADirectory.message()
                ));
                status = 1;
            }
            Ok(_) => {
                if let Err((at, e)) = remove_tree(cx, p, 0) {
                    let msg = t!(
                        "sh.err.cannot_remove",
                        path = at.as_str(),
                        why = e.message()
                    );
                    cx.error(&msg);
                    status = 1;
                }
            }
        }
    }
    status
}

pub(super) fn rmdir(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error(t!("sh.err.missing_operand"));
        return 2;
    }
    let mut status = 0;
    for d in &cx.args[1..] {
        match cx.fs.stat(d) {
            Ok(s) if s.kind == Kind::Dir => {
                if let Err(e) = cx.fs.remove(d) {
                    let msg = t!("sh.err.rmdir_failed", path = d.as_str(), why = e.message());
                    cx.error(&msg);
                    status = 1;
                }
            }
            Ok(_) => {
                let msg = t!(
                    "sh.err.rmdir_failed",
                    path = d.as_str(),
                    why = FsErr::NotADirectory.message()
                );
                cx.error(&msg);
                status = 1;
            }
            Err(e) => {
                let msg = t!("sh.err.rmdir_failed", path = d.as_str(), why = e.message());
                cx.error(&msg);
                status = 1;
            }
        }
    }
    status
}

/// Destination path for `src` when `dst` may be a directory.
pub(super) fn dest_for(cx: &CmdCtx<'_>, src: &str, dst: &str) -> String {
    match cx.fs.stat(dst) {
        Ok(s) if s.kind == Kind::Dir => {
            let abs = cx.fs.resolve(dst);
            join(&abs, basename(src.trim_end_matches('/')))
        }
        _ => dst.to_string(),
    }
}

pub(super) fn mv(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 3 {
        cx.error(t!("sh.mv.usage"));
        return 2;
    }
    let dst = cx.args[cx.args.len() - 1].clone();
    let srcs: Vec<String> = cx.args[1..cx.args.len() - 1].to_vec();
    if srcs.len() > 1 && !matches!(cx.fs.stat(&dst), Ok(s) if s.kind == Kind::Dir) {
        cx.error(&t!("sh.err.target_not_dir", path = dst.as_str()));
        return 1;
    }
    let mut status = 0;
    for s in srcs {
        let to = dest_for(cx, &s, &dst);
        if let Err(e) = cx.fs.rename(&s, &to) {
            let msg = t!(
                "sh.err.cannot_move",
                from = s.as_str(),
                to = to.as_str(),
                why = e.message()
            );
            cx.error(&msg);
            status = 1;
        }
    }
    status
}

pub(super) fn copy_tree(
    cx: &mut CmdCtx<'_>,
    src: &str,
    dst: &str,
    recursive: bool,
    depth: usize,
) -> Result<(), String> {
    let st = cx
        .fs
        .stat(src)
        .map_err(|e| t!("sh.err.cannot_stat", path = src, why = e.message()))?;
    if st.kind == Kind::File {
        let data = cx
            .fs
            .read(src)
            .map_err(|e| t!("sh.err.cannot_read", path = src, why = e.message()))?;
        return cx
            .fs
            .write(dst, &data)
            .map_err(|e| t!("sh.err.cannot_create", path = dst, why = e.message()));
    }
    if !recursive {
        return Err(t!("sh.cp.omit_dir", path = src));
    }
    if depth > 64 {
        return Err(t!("sh.cp.too_deep", path = src));
    }
    let abs_src = cx.fs.resolve(src);
    let abs_dst = cx.fs.resolve(dst);
    if abs_dst == abs_src || abs_dst.starts_with(&format!("{abs_src}/")) {
        return Err(t!("sh.cp.into_itself", path = src));
    }
    match cx.fs.mkdir(&abs_dst) {
        Ok(()) | Err(FsErr::AlreadyExists) => {}
        Err(e) => {
            return Err(t!(
                "sh.err.cannot_create_dir",
                path = dst,
                why = e.message()
            ));
        }
    }
    let entries = cx
        .fs
        .list(&abs_src)
        .map_err(|e| t!("sh.err.cannot_read", path = src, why = e.message()))?;
    for e in entries {
        copy_tree(
            cx,
            &join(&abs_src, &e.name),
            &join(&abs_dst, &e.name),
            true,
            depth + 1,
        )?;
    }
    Ok(())
}

pub(super) fn cp(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "rR", "", None) else {
        return 2;
    };
    if o.operands.len() < 2 {
        cx.error(t!("sh.cp.usage"));
        return 2;
    }
    let dst = o.operands[o.operands.len() - 1].clone();
    let srcs = &o.operands[..o.operands.len() - 1];
    if srcs.len() > 1 && !matches!(cx.fs.stat(&dst), Ok(s) if s.kind == Kind::Dir) {
        cx.error(&t!("sh.err.target_not_dir", path = dst.as_str()));
        return 1;
    }
    let recursive = o.has('r') || o.has('R');
    let mut status = 0;
    for s in srcs {
        let to = dest_for(cx, s, &dst);
        if let Err(msg) = copy_tree(cx, s, &to, recursive, 0) {
            cx.error(&msg);
            status = 1;
        }
    }
    status
}

pub(super) fn touch(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error(t!("sh.err.missing_file_operand"));
        return 2;
    }
    let mut status = 0;
    for f in &cx.args[1..] {
        if cx.fs.stat(f).is_err()
            && let Err(e) = cx.fs.write(f, b"")
        {
            let msg = t!("sh.err.cannot_create", path = f.as_str(), why = e.message());
            cx.error(&msg);
            status = 1;
        }
    }
    status
}
