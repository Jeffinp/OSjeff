//! session (split out of `builtins.rs`).

use super::*;

pub(super) fn clear(cx: &mut CmdCtx<'_>) -> i32 {
    cx.sys.clear_screen();
    cx.clear = true;
    0
}

pub(super) fn env(cx: &mut CmdCtx<'_>) -> i32 {
    let lines: Vec<String> = cx
        .env
        .iter()
        .filter(|(_, _, e)| *e)
        .map(|(n, v, _)| format!("{n}={v}\n"))
        .collect();
    for l in lines {
        cx.print(&l);
    }
    0
}

pub(super) fn export(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() == 1 {
        let lines: Vec<String> = cx
            .env
            .iter()
            .filter(|(_, _, e)| *e)
            .map(|(n, v, _)| format!("export {n}=\"{v}\"\n"))
            .collect();
        for l in lines {
            cx.print(&l);
        }
        return 0;
    }
    let mut status = 0;
    for a in &cx.args[1..] {
        let (name, val) = match a.split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (a.clone(), None),
        };
        if !super::super::env::valid_name(&name) {
            cx.error(&t!("sh.export.bad_name", name = a.as_str()));
            status = 1;
            continue;
        }
        let ok = match val {
            Some(v) => cx.env.set_exported(&name, &v),
            None => cx.env.export(&name),
        };
        if !ok {
            cx.error(&t!("sh.export.cannot_set", name = name.as_str()));
            status = 1;
        }
    }
    status
}

pub(super) fn unset(cx: &mut CmdCtx<'_>) -> i32 {
    for n in &cx.args[1..] {
        cx.env.unset(n);
    }
    0
}

pub(super) fn history(cx: &mut CmdCtx<'_>) -> i32 {
    match cx.args.get(1).map(String::as_str) {
        Some("-c") => {
            cx.meta.history.clear();
            0
        }
        arg => {
            let total = cx.meta.history.len();
            let n = match arg {
                None => total,
                Some(a) => match a.parse::<usize>() {
                    Ok(n) => n.min(total),
                    Err(_) => {
                        cx.error(&t!("sh.err.numeric_required", name = a));
                        return 2;
                    }
                },
            };
            let lines: Vec<String> = (total - n..total)
                .map(|i| format!("{:>5}  {}\n", i + 1, cx.meta.history.get(i).unwrap_or("")))
                .collect();
            for l in lines {
                cx.print(&l);
            }
            0
        }
    }
}

pub(super) fn alias(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() == 1 {
        let lines: Vec<String> = cx
            .meta
            .aliases
            .iter()
            .map(|(k, v)| format!("alias {k}='{v}'\n"))
            .collect();
        for l in lines {
            cx.print(&l);
        }
        return 0;
    }
    let mut status = 0;
    for a in &cx.args[1..] {
        match a.split_once('=') {
            Some((n, v)) if !n.is_empty() && !n.contains(char::is_whitespace) => {
                if cx.meta.aliases.len() >= 256 && !cx.meta.aliases.contains_key(n) {
                    cx.error(t!("sh.alias.too_many"));
                    status = 1;
                } else {
                    cx.meta.aliases.insert(n.to_string(), v.to_string());
                }
            }
            Some(_) => {
                cx.error(&t!("sh.alias.bad_name", arg = a.as_str()));
                status = 1;
            }
            None => match cx.meta.aliases.get(a).cloned() {
                Some(v) => cx.println(&format!("alias {a}='{v}'")),
                None => {
                    cx.error(&t!("sh.err.not_found", name = a.as_str()));
                    status = 1;
                }
            },
        }
    }
    status
}

pub(super) fn unalias(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.get(1).map(String::as_str) == Some("-a") {
        cx.meta.aliases.clear();
        return 0;
    }
    if cx.args.len() < 2 {
        cx.error(t!("sh.unalias.usage"));
        return 2;
    }
    let mut status = 0;
    for a in &cx.args[1..] {
        if cx.meta.aliases.remove(a).is_none() {
            cx.error(&t!("sh.err.not_found", name = a.as_str()));
            status = 1;
        }
    }
    status
}

pub(super) fn which(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error(t!("sh.which.usage"));
        return 2;
    }
    let mut status = 0;
    for n in &cx.args[1..] {
        if let Some(v) = cx.meta.aliases.get(n).cloned() {
            cx.println(&t!("sh.which.alias", name = n.as_str(), value = v.as_str()));
        } else if cx.funcs.contains_key(n) {
            cx.println(&t!("sh.which.function", name = n.as_str()));
        } else if cx.registry.contains(n) {
            cx.println(&t!("sh.which.builtin", name = n.as_str()));
        } else {
            let mut found = None;
            let cands: Vec<String> = if n.contains('/') {
                alloc::vec![n.clone()]
            } else {
                let mut v = Vec::new();
                for d in cx.env.path_dirs() {
                    v.push(join(&d, n));
                    v.push(join(&d, &format!("{n}.sh")));
                }
                v
            };
            for c in cands {
                if cx.fs.stat(&c).is_ok_and(|s| s.kind == Kind::File) {
                    found = Some(cx.fs.resolve(&c));
                    break;
                }
            }
            match found {
                Some(p) => cx.println(&p),
                None => {
                    let msg = t!("sh.which.missing", name = n.as_str());
                    cx.err_bytes(msg.as_bytes());
                    cx.err_bytes(b"\n");
                    status = 1;
                }
            }
        }
    }
    status
}
