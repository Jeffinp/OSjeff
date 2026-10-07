//! The builtin commands.
//!
//! Every command reads its arguments from [`CmdCtx::args`], writes with
//! [`CmdCtx::print`]/[`CmdCtx::error`] and returns an exit status (0 success,
//! 1 failure, 2 usage error). Commands that depend on the system (`date`,
//! `uptime`, `free`, `df`, `ps`, `kill`, `ping`, `sleep`, `clear`) go through
//! [`super::sys::SysInfo`].

use super::exec::{BuiltinFn, Builtins, CmdCtx, Registry};
use super::fs::{FsErr, Kind, basename, join};
use super::regex::Regex;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Register every standard builtin into `r`.
pub fn register_all(r: &mut Registry) {
    let table: [(&str, &'static str, BuiltinFn); 48] = [
        ("help", "help [CMD]: list commands or describe one", help),
        ("ls", "ls [-aF1l] [PATH...]: list directory contents", ls),
        ("cd", "cd [DIR|-]: change directory", cd),
        ("pwd", "pwd: print the working directory", pwd),
        ("cat", "cat [-n] [FILE...]: print files or stdin", cat),
        ("echo", "echo [-ne] [ARG...]: print arguments", echo),
        ("mkdir", "mkdir [-p] DIR...: create directories", mkdir),
        ("rm", "rm [-rf] PATH...: remove files or directories", rm),
        ("rmdir", "rmdir DIR...: remove empty directories", rmdir),
        ("mv", "mv SRC... DEST: move or rename", mv),
        ("cp", "cp [-r] SRC... DEST: copy files or directories", cp),
        ("touch", "touch FILE...: create empty files", touch),
        ("head", "head [-n N] [FILE...]: first lines", head),
        ("tail", "tail [-n N] [FILE...]: last lines", tail),
        ("wc", "wc [-lwcm] [FILE...]: count lines, words, bytes", wc),
        (
            "grep",
            "grep [-ivncFlqHh] PATTERN [FILE...]: search text",
            grep,
        ),
        ("sort", "sort [-rnu] [FILE...]: sort lines", sort),
        (
            "uniq",
            "uniq [-cdui]: collapse adjacent duplicate lines",
            uniq,
        ),
        (
            "tee",
            "tee [-a] FILE...: copy stdin to stdout and files",
            tee,
        ),
        ("clear", "clear: clear the terminal", clear),
        ("env", "env: list exported variables", env),
        (
            "export",
            "export [NAME[=VALUE]...]: mark variables exported",
            export,
        ),
        ("unset", "unset NAME...: remove variables", unset),
        (
            "history",
            "history [N|-c]: show or clear the command history",
            history,
        ),
        (
            "alias",
            "alias [NAME=VALUE...]: define or list aliases",
            alias,
        ),
        ("unalias", "unalias [-a] NAME...: remove aliases", unalias),
        ("which", "which CMD...: show how a command resolves", which),
        ("date", "date [+FORMAT]: print the date and time", date),
        ("uptime", "uptime: time since boot", uptime),
        ("free", "free [-bkm]: memory usage", free),
        ("df", "df: disk usage", df),
        ("ps", "ps: list processes", ps),
        ("kill", "kill [-SIGNAL] PID...: signal processes", kill),
        ("ping", "ping [-c N] HOST: test network reachability", ping),
        ("true", "true: succeed", true_cmd),
        ("false", "false: fail", false_cmd),
        ("test", "test EXPR: evaluate a condition", test),
        ("[", "[ EXPR ]: evaluate a condition", bracket),
        ("sleep", "sleep SECONDS: wait", sleep),
        ("seq", "seq [FIRST] LAST: print a sequence of numbers", seq),
        (
            "basename",
            "basename PATH [SUFFIX]: last path component",
            basename_cmd,
        ),
        (
            "dirname",
            "dirname PATH: directory part of a path",
            dirname_cmd,
        ),
        ("stat", "stat PATH...: show kind and size", stat),
        ("rev", "rev: reverse each line", rev),
        ("tr", "tr SET1 SET2: translate characters (stdin)", tr),
        (
            "cut",
            "cut -d C -f N[,M]: select fields (stdin or files)",
            cut,
        ),
        ("nl", "nl: number lines (stdin or files)", nl),
        (
            "yes",
            "yes [WORD]: repeat a word (bounded by the output limit)",
            yes,
        ),
    ];
    for (n, h, f) in table {
        r.register(n, h, f);
    }
}

// ---- helpers ---------------------------------------------------------------

/// Parsed option letters and operands.
pub(super) struct Opts {
    pub(super) flags: Vec<char>,
    pub(super) values: Vec<(char, String)>,
    pub(super) operands: Vec<String>,
}

impl Opts {
    pub(super) fn has(&self, c: char) -> bool {
        self.flags.contains(&c)
    }

    pub(super) fn value(&self, c: char) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(k, _)| *k == c)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse `-abc`, `-n 5`, `-n5`, `--`. `valued` lists letters taking a value;
/// `numeric` maps `-5` to that letter's value. Unknown letters are an error.
pub(super) fn parse_opts(
    cx: &mut CmdCtx<'_>,
    known: &str,
    valued: &str,
    numeric: Option<char>,
) -> Option<Opts> {
    let mut o = Opts {
        flags: Vec::new(),
        values: Vec::new(),
        operands: Vec::new(),
    };
    let args: Vec<String> = cx.args[1..].to_vec();
    let mut i = 0;
    let mut done = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if done || !a.starts_with('-') || a.len() == 1 {
            o.operands.push(a.clone());
            continue;
        }
        if a == "--" {
            done = true;
            continue;
        }
        let body = &a[1..];
        if let Some(nc) = numeric
            && body.chars().all(|c| c.is_ascii_digit())
        {
            o.values.push((nc, body.to_string()));
            continue;
        }
        let chars = body.char_indices();
        for (idx, c) in chars {
            if valued.contains(c) {
                let rest = &body[idx + c.len_utf8()..];
                if !rest.is_empty() {
                    o.values.push((c, rest.to_string()));
                } else if i < args.len() {
                    o.values.push((c, args[i].clone()));
                    i += 1;
                } else {
                    cx.error(&format!("option requires an argument -- '{c}'"));
                    return None;
                }
                break;
            } else if known.contains(c) {
                o.flags.push(c);
            } else {
                cx.error(&format!("invalid option -- '{c}'"));
                return None;
            }
        }
    }
    Some(o)
}

pub(super) fn fs_err(cx: &mut CmdCtx<'_>, path: &str, e: FsErr) {
    let msg = format!("{path}: {}", e.message());
    cx.error(&msg);
}

/// Read each operand (or stdin when there are none or for `-`).
fn inputs(cx: &mut CmdCtx<'_>, files: &[String]) -> (Vec<(String, Vec<u8>)>, bool) {
    let mut ok = true;
    let mut out = Vec::new();
    if files.is_empty() {
        out.push((String::new(), cx.stdin.to_vec()));
        return (out, ok);
    }
    for f in files {
        if f == "-" {
            out.push((String::from("-"), cx.stdin.to_vec()));
            continue;
        }
        match cx.fs.read(f) {
            Ok(d) => out.push((f.clone(), d)),
            Err(e) => {
                fs_err(cx, f, e);
                ok = false;
            }
        }
    }
    (out, ok)
}

fn lines_of(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    let mut v: Vec<String> = text.split('\n').map(String::from).collect();
    if v.last().is_some_and(String::is_empty) {
        v.pop();
    }
    v
}

fn parse_num(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

// ---- commands -----------------------------------------------------------------

fn help(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() > 1 {
        let mut st = 0;
        for name in &cx.args[1..] {
            match cx.registry.help_of(name) {
                Some(h) => cx.println(h),
                None => {
                    cx.error(&format!("no help for '{name}'"));
                    st = 1;
                }
            }
        }
        return st;
    }
    let list = cx.registry.list();
    cx.println("Commands (help NAME for one):");
    for (_, h) in list {
        cx.print("  ");
        cx.println(h);
    }
    0
}

fn ls(cx: &mut CmdCtx<'_>) -> i32 {
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
                let msg = format!("cannot access '{t}': {}", e.message());
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

fn set_pwd(cx: &mut CmdCtx<'_>, old: &str) {
    let now = cx.fs.cwd();
    cx.env.set_exported("OLDPWD", old);
    cx.env.set_exported("PWD", &now);
}

fn cd(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() > 2 {
        cx.error("too many arguments");
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
                cx.error("OLDPWD not set");
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

fn pwd(cx: &mut CmdCtx<'_>) -> i32 {
    let c = cx.fs.cwd();
    cx.println(&c);
    0
}

fn cat(cx: &mut CmdCtx<'_>) -> i32 {
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

fn echo(cx: &mut CmdCtx<'_>) -> i32 {
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

fn mkdir(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "p", "", None) else {
        return 2;
    };
    if o.operands.is_empty() {
        cx.error("missing operand");
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
            let msg = format!("cannot create directory '{d}': {}", e.message());
            cx.error(&msg);
            status = 1;
        }
    }
    status
}

fn remove_tree(cx: &mut CmdCtx<'_>, path: &str, depth: usize) -> Result<(), (String, FsErr)> {
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

fn rm(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "rRf", "", None) else {
        return 2;
    };
    if o.operands.is_empty() {
        if o.has('f') {
            return 0;
        }
        cx.error("missing operand");
        return 2;
    }
    let recursive = o.has('r') || o.has('R');
    let force = o.has('f');
    let mut status = 0;
    for p in &o.operands {
        let abs = cx.fs.resolve(p);
        let base = basename(p.trim_end_matches('/'));
        if abs == "/" || base == "." || base == ".." {
            cx.error(&format!("refusing to remove '{p}'"));
            status = 1;
            continue;
        }
        match cx.fs.stat(p) {
            Err(FsErr::NotFound) if force => {}
            Err(e) => {
                let msg = format!("cannot remove '{p}': {}", e.message());
                cx.error(&msg);
                status = 1;
            }
            Ok(s) if s.kind == Kind::Dir && !recursive => {
                cx.error(&format!("cannot remove '{p}': Is a directory"));
                status = 1;
            }
            Ok(_) => {
                if let Err((at, e)) = remove_tree(cx, p, 0) {
                    let msg = format!("cannot remove '{at}': {}", e.message());
                    cx.error(&msg);
                    status = 1;
                }
            }
        }
    }
    status
}

fn rmdir(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error("missing operand");
        return 2;
    }
    let mut status = 0;
    for d in &cx.args[1..] {
        match cx.fs.stat(d) {
            Ok(s) if s.kind == Kind::Dir => {
                if let Err(e) = cx.fs.remove(d) {
                    let msg = format!("failed to remove '{d}': {}", e.message());
                    cx.error(&msg);
                    status = 1;
                }
            }
            Ok(_) => {
                let msg = format!("failed to remove '{d}': Not a directory");
                cx.error(&msg);
                status = 1;
            }
            Err(e) => {
                let msg = format!("failed to remove '{d}': {}", e.message());
                cx.error(&msg);
                status = 1;
            }
        }
    }
    status
}

/// Destination path for `src` when `dst` may be a directory.
fn dest_for(cx: &CmdCtx<'_>, src: &str, dst: &str) -> String {
    match cx.fs.stat(dst) {
        Ok(s) if s.kind == Kind::Dir => {
            let abs = cx.fs.resolve(dst);
            join(&abs, basename(src.trim_end_matches('/')))
        }
        _ => dst.to_string(),
    }
}

fn mv(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 3 {
        cx.error("usage: mv SRC... DEST");
        return 2;
    }
    let dst = cx.args[cx.args.len() - 1].clone();
    let srcs: Vec<String> = cx.args[1..cx.args.len() - 1].to_vec();
    if srcs.len() > 1 && !matches!(cx.fs.stat(&dst), Ok(s) if s.kind == Kind::Dir) {
        cx.error(&format!("target '{dst}' is not a directory"));
        return 1;
    }
    let mut status = 0;
    for s in srcs {
        let to = dest_for(cx, &s, &dst);
        if let Err(e) = cx.fs.rename(&s, &to) {
            let msg = format!("cannot move '{s}' to '{to}': {}", e.message());
            cx.error(&msg);
            status = 1;
        }
    }
    status
}

fn copy_tree(
    cx: &mut CmdCtx<'_>,
    src: &str,
    dst: &str,
    recursive: bool,
    depth: usize,
) -> Result<(), String> {
    let st = cx
        .fs
        .stat(src)
        .map_err(|e| format!("cannot stat '{src}': {}", e.message()))?;
    if st.kind == Kind::File {
        let data = cx
            .fs
            .read(src)
            .map_err(|e| format!("cannot read '{src}': {}", e.message()))?;
        return cx
            .fs
            .write(dst, &data)
            .map_err(|e| format!("cannot create '{dst}': {}", e.message()));
    }
    if !recursive {
        return Err(format!("-r not specified; omitting directory '{src}'"));
    }
    if depth > 64 {
        return Err(format!("'{src}': nesting too deep"));
    }
    let abs_src = cx.fs.resolve(src);
    let abs_dst = cx.fs.resolve(dst);
    if abs_dst == abs_src || abs_dst.starts_with(&format!("{abs_src}/")) {
        return Err(format!("cannot copy '{src}' into itself"));
    }
    match cx.fs.mkdir(&abs_dst) {
        Ok(()) | Err(FsErr::AlreadyExists) => {}
        Err(e) => return Err(format!("cannot create directory '{dst}': {}", e.message())),
    }
    let entries = cx
        .fs
        .list(&abs_src)
        .map_err(|e| format!("cannot read '{src}': {}", e.message()))?;
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

fn cp(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "rR", "", None) else {
        return 2;
    };
    if o.operands.len() < 2 {
        cx.error("usage: cp [-r] SRC... DEST");
        return 2;
    }
    let dst = o.operands[o.operands.len() - 1].clone();
    let srcs = &o.operands[..o.operands.len() - 1];
    if srcs.len() > 1 && !matches!(cx.fs.stat(&dst), Ok(s) if s.kind == Kind::Dir) {
        cx.error(&format!("target '{dst}' is not a directory"));
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

fn touch(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error("missing file operand");
        return 2;
    }
    let mut status = 0;
    for f in &cx.args[1..] {
        if cx.fs.stat(f).is_err()
            && let Err(e) = cx.fs.write(f, b"")
        {
            let msg = format!("cannot touch '{f}': {}", e.message());
            cx.error(&msg);
            status = 1;
        }
    }
    status
}

fn head_tail(cx: &mut CmdCtx<'_>, tail: bool) -> i32 {
    let Some(o) = parse_opts(cx, "", "n", Some('n')) else {
        return 2;
    };
    let n = match o.value('n') {
        None => 10,
        Some(v) => match parse_num(v) {
            Some(n) if n >= 0 => n as usize,
            _ => {
                cx.error(&format!("invalid number of lines: '{v}'"));
                return 2;
            }
        },
    };
    let (ins, ok) = inputs(cx, &o.operands);
    let multi = ins.len() > 1;
    for (k, (name, data)) in ins.iter().enumerate() {
        if multi {
            if k > 0 {
                cx.print("\n");
            }
            cx.print(&format!("==> {name} <==\n"));
        }
        let lines = lines_of(data);
        let slice: &[String] = if tail {
            &lines[lines.len().saturating_sub(n)..]
        } else {
            &lines[..n.min(lines.len())]
        };
        for l in slice {
            cx.print(l);
            cx.print("\n");
        }
    }
    i32::from(!ok)
}

fn head(cx: &mut CmdCtx<'_>) -> i32 {
    head_tail(cx, false)
}

fn tail(cx: &mut CmdCtx<'_>) -> i32 {
    head_tail(cx, true)
}

fn wc(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "lwcm", "", None) else {
        return 2;
    };
    let any = o.has('l') || o.has('w') || o.has('c') || o.has('m');
    let (l, w, c, m) = if any {
        (o.has('l'), o.has('w'), o.has('c'), o.has('m'))
    } else {
        (true, true, true, false)
    };
    let (ins, ok) = inputs(cx, &o.operands);
    let mut total = [0usize; 4];
    let count = ins.len();
    for (name, data) in &ins {
        let text = String::from_utf8_lossy(data);
        let counts = [
            data.iter().filter(|&&b| b == b'\n').count(),
            text.split_whitespace().count(),
            data.len(),
            text.chars().count(),
        ];
        for i in 0..4 {
            total[i] += counts[i];
        }
        let mut parts: Vec<String> = Vec::new();
        for (flag, v) in [l, w, c, m].into_iter().zip(counts) {
            if flag {
                parts.push(v.to_string());
            }
        }
        if !name.is_empty() {
            parts.push(name.clone());
        }
        cx.println(&parts.join(" "));
    }
    if count > 1 {
        let mut parts: Vec<String> = Vec::new();
        for (flag, v) in [l, w, c, m].into_iter().zip(total) {
            if flag {
                parts.push(v.to_string());
            }
        }
        parts.push(String::from("total"));
        cx.println(&parts.join(" "));
    }
    i32::from(!ok)
}

fn grep(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "ivncFElqHh", "e", None) else {
        return 2;
    };
    let mut operands = o.operands.clone();
    let pattern = match o.value('e') {
        Some(p) => p.to_string(),
        None => {
            if operands.is_empty() {
                cx.error("usage: grep [OPTIONS] PATTERN [FILE...]");
                return 2;
            }
            operands.remove(0)
        }
    };
    let icase = o.has('i');
    let fixed = o.has('F');
    let re = if fixed {
        None
    } else {
        match Regex::new(&pattern, icase) {
            Ok(r) => Some(r),
            Err(e) => {
                cx.error(&format!("invalid pattern: {}", e.message()));
                return 2;
            }
        }
    };
    let needle = if icase {
        pattern.to_lowercase()
    } else {
        pattern.clone()
    };
    let (ins, ok) = inputs(cx, &operands);
    let with_name = (ins.len() > 1 || o.has('H')) && !o.has('h');
    let mut any = false;
    for (name, data) in &ins {
        let mut count = 0usize;
        for (idx, line) in lines_of(data).iter().enumerate() {
            let hit = match &re {
                Some(r) => r.is_match(line),
                None => {
                    if icase {
                        line.to_lowercase().contains(&needle)
                    } else {
                        line.contains(&needle)
                    }
                }
            };
            if hit == o.has('v') {
                continue;
            }
            count += 1;
            any = true;
            if o.has('q') {
                return 0;
            }
            if o.has('l') || o.has('c') {
                if o.has('l') {
                    break;
                }
                continue;
            }
            let mut s = String::new();
            if with_name {
                s.push_str(name);
                s.push(':');
            }
            if o.has('n') {
                s.push_str(&format!("{}:", idx + 1));
            }
            s.push_str(line);
            s.push('\n');
            cx.print(&s);
            if cx.full() {
                return 0;
            }
        }
        if o.has('l') {
            if count > 0 {
                let n = if name.is_empty() {
                    "(standard input)"
                } else {
                    name
                };
                cx.println(n);
            }
        } else if o.has('c') {
            if with_name {
                cx.println(&format!("{name}:{count}"));
            } else {
                cx.println(&count.to_string());
            }
        }
    }
    if !ok { 2 } else { i32::from(!any) }
}

fn leading_number(s: &str) -> i64 {
    let t = s.trim_start();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let v = digits.parse::<i64>().unwrap_or(0);
    if neg { -v } else { v }
}

fn sort(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "rnuf", "", None) else {
        return 2;
    };
    let (ins, ok) = inputs(cx, &o.operands);
    let mut lines: Vec<String> = Vec::new();
    for (_, d) in &ins {
        lines.extend(lines_of(d));
    }
    if o.has('n') {
        lines.sort_by(|a, b| {
            leading_number(a)
                .cmp(&leading_number(b))
                .then_with(|| a.cmp(b))
        });
    } else if o.has('f') {
        lines.sort_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b))
        });
    } else {
        lines.sort();
    }
    if o.has('r') {
        lines.reverse();
    }
    if o.has('u') {
        lines.dedup();
    }
    for l in lines {
        cx.print(&l);
        cx.print("\n");
        if cx.full() {
            break;
        }
    }
    i32::from(!ok)
}

fn uniq(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "cduiq", "", None) else {
        return 2;
    };
    let (ins, ok) = inputs(cx, &o.operands);
    let mut lines: Vec<String> = Vec::new();
    for (_, d) in &ins {
        lines.extend(lines_of(d));
    }
    let key = |s: &str| {
        if o.has('i') {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let mut groups: Vec<(String, usize)> = Vec::new();
    for l in lines {
        match groups.last_mut() {
            Some((g, n)) if key(g) == key(&l) => *n += 1,
            _ => groups.push((l, 1)),
        }
    }
    for (l, n) in groups {
        if o.has('d') && n < 2 {
            continue;
        }
        if o.has('u') && n > 1 {
            continue;
        }
        if o.has('c') {
            cx.print(&format!("{n:>7} {l}\n"));
        } else {
            cx.print(&l);
            cx.print("\n");
        }
    }
    i32::from(!ok)
}

fn tee(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "a", "", None) else {
        return 2;
    };
    let data = cx.stdin.to_vec();
    cx.out(&data);
    let mut status = 0;
    for f in &o.operands {
        let r = if o.has('a') {
            cx.fs.append(f, &data)
        } else {
            cx.fs.write(f, &data)
        };
        if let Err(e) = r {
            fs_err(cx, f, e);
            status = 1;
        }
    }
    status
}

fn clear(cx: &mut CmdCtx<'_>) -> i32 {
    cx.sys.clear_screen();
    cx.clear = true;
    0
}

fn env(cx: &mut CmdCtx<'_>) -> i32 {
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

fn export(cx: &mut CmdCtx<'_>) -> i32 {
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
        if !super::env::valid_name(&name) {
            cx.error(&format!("`{a}': not a valid identifier"));
            status = 1;
            continue;
        }
        let ok = match val {
            Some(v) => cx.env.set_exported(&name, &v),
            None => cx.env.export(&name),
        };
        if !ok {
            cx.error(&format!("cannot set `{name}'"));
            status = 1;
        }
    }
    status
}

fn unset(cx: &mut CmdCtx<'_>) -> i32 {
    for n in &cx.args[1..] {
        cx.env.unset(n);
    }
    0
}

fn history(cx: &mut CmdCtx<'_>) -> i32 {
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
                        cx.error(&format!("{a}: numeric argument required"));
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

fn alias(cx: &mut CmdCtx<'_>) -> i32 {
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
                    cx.error("too many aliases");
                    status = 1;
                } else {
                    cx.meta.aliases.insert(n.to_string(), v.to_string());
                }
            }
            Some(_) => {
                cx.error(&format!("invalid alias name in `{a}'"));
                status = 1;
            }
            None => match cx.meta.aliases.get(a).cloned() {
                Some(v) => cx.println(&format!("alias {a}='{v}'")),
                None => {
                    cx.error(&format!("{a}: not found"));
                    status = 1;
                }
            },
        }
    }
    status
}

fn unalias(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.get(1).map(String::as_str) == Some("-a") {
        cx.meta.aliases.clear();
        return 0;
    }
    if cx.args.len() < 2 {
        cx.error("usage: unalias [-a] NAME...");
        return 2;
    }
    let mut status = 0;
    for a in &cx.args[1..] {
        if cx.meta.aliases.remove(a).is_none() {
            cx.error(&format!("{a}: not found"));
            status = 1;
        }
    }
    status
}

fn which(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error("usage: which CMD...");
        return 2;
    }
    let mut status = 0;
    for n in &cx.args[1..] {
        if let Some(v) = cx.meta.aliases.get(n).cloned() {
            cx.println(&format!("{n}: aliased to '{v}'"));
        } else if cx.funcs.contains_key(n) {
            cx.println(&format!("{n}: shell function"));
        } else if cx.registry.contains(n) {
            cx.println(&format!("{n}: shell builtin"));
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
                    cx.err_bytes(format!("{n} not found\n").as_bytes());
                    status = 1;
                }
            }
        }
    }
    status
}

fn pad2(n: u8) -> String {
    format!("{n:02}")
}

fn date(cx: &mut CmdCtx<'_>) -> i32 {
    let t = cx.sys.now();
    let fmt = match cx.args.get(1) {
        None => "%Y-%m-%d %H:%M:%S".to_string(),
        Some(f) => match f.strip_prefix('+') {
            Some(r) => r.to_string(),
            None => {
                cx.error(&format!("invalid date '{f}'"));
                return 1;
            }
        },
    };
    let mut out = String::new();
    let mut it = fmt.chars();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('Y') => out.push_str(&format!("{:04}", t.year)),
            Some('y') => out.push_str(&format!("{:02}", t.year % 100)),
            Some('m') => out.push_str(&pad2(t.month)),
            Some('d') => out.push_str(&pad2(t.day)),
            Some('H') => out.push_str(&pad2(t.hour)),
            Some('M') => out.push_str(&pad2(t.minute)),
            Some('S') => out.push_str(&pad2(t.second)),
            Some('F') => out.push_str(&format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)),
            Some('T') => out.push_str(&format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)),
            Some('%') => out.push('%'),
            Some(o) => {
                out.push('%');
                out.push(o);
            }
            None => out.push('%'),
        }
    }
    cx.println(&out);
    0
}

fn uptime(cx: &mut CmdCtx<'_>) -> i32 {
    let total = cx.sys.uptime_ms() / 1000;
    let (d, h, m, s) = (
        total / 86_400,
        (total / 3600) % 24,
        (total / 60) % 60,
        total % 60,
    );
    let line = if d > 0 {
        let unit = if d == 1 { "day" } else { "days" };
        format!("up {d} {unit}, {h:02}:{m:02}:{s:02}")
    } else {
        format!("up {h:02}:{m:02}:{s:02}")
    };
    cx.println(&line);
    0
}

fn free(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "bkm", "", None) else {
        return 2;
    };
    let (div, unit) = if o.has('b') {
        (1u64, "bytes")
    } else if o.has('m') {
        (1024 * 1024, "MiB")
    } else {
        (1024, "KiB")
    };
    let m = cx.sys.mem();
    cx.println(&format!(
        "{:<6}{:>12}{:>12}{:>12}",
        unit, "total", "used", "free"
    ));
    cx.println(&format!(
        "{:<6}{:>12}{:>12}{:>12}",
        "Mem:",
        m.total / div,
        m.used / div,
        m.free() / div
    ));
    0
}

fn df(cx: &mut CmdCtx<'_>) -> i32 {
    let mut disks = cx.sys.disks();
    if disks.is_empty() {
        let u = cx.fs.usage();
        disks.push(super::sys::DiskInfo {
            name: String::from("rootfs"),
            mount: String::from("/"),
            total: u.total_bytes,
            used: u.used_bytes,
        });
    }
    cx.println(&format!(
        "{:<12}{:>10}{:>10}{:>10}{:>6}  Mounted on",
        "Filesystem", "KiB", "Used", "Avail", "Use%"
    ));
    for d in disks {
        let pct = match d.used.saturating_mul(100).checked_div(d.total) {
            Some(p) => format!("{p}%"),
            None => String::from("-"),
        };
        cx.println(&format!(
            "{:<12}{:>10}{:>10}{:>10}{:>6}  {}",
            d.name,
            d.total / 1024,
            d.used / 1024,
            d.total.saturating_sub(d.used) / 1024,
            pct,
            d.mount
        ));
    }
    0
}

fn ps(cx: &mut CmdCtx<'_>) -> i32 {
    cx.println(&format!(
        "{:>5}  {:<8}{:>8}  NAME",
        "PID", "STATE", "MEM(K)"
    ));
    for p in cx.sys.procs() {
        cx.println(&format!(
            "{:>5}  {:<8}{:>8}  {}",
            p.pid,
            p.state,
            p.mem_bytes / 1024,
            p.name
        ));
    }
    0
}

fn signal_number(s: &str) -> Option<i32> {
    if let Ok(n) = s.parse::<i32>() {
        return (0..=64).contains(&n).then_some(n);
    }
    let up = s.to_ascii_uppercase();
    let name = up.strip_prefix("SIG").unwrap_or(&up);
    Some(match name {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "KILL" => 9,
        "TERM" => 15,
        "CONT" => 18,
        "STOP" => 19,
        _ => return None,
    })
}

fn kill(cx: &mut CmdCtx<'_>) -> i32 {
    let mut sig = 15;
    let mut pids: Vec<String> = Vec::new();
    for a in &cx.args[1..] {
        if let Some(s) = a.strip_prefix('-')
            && pids.is_empty()
            && !s.is_empty()
        {
            match signal_number(s) {
                Some(n) => sig = n,
                None => {
                    cx.error(&format!("invalid signal '{s}'"));
                    return 2;
                }
            }
        } else {
            pids.push(a.clone());
        }
    }
    if pids.is_empty() {
        cx.error("usage: kill [-SIGNAL] PID...");
        return 2;
    }
    let mut status = 0;
    for p in pids {
        match p.parse::<u32>() {
            Ok(pid) => {
                if let Err(e) = cx.sys.kill(pid, sig) {
                    cx.error(&format!("({pid}) - {}", e.message()));
                    status = 1;
                }
            }
            Err(_) => {
                cx.error(&format!("invalid pid '{p}'"));
                status = 1;
            }
        }
    }
    status
}

fn ping(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "", "c", None) else {
        return 2;
    };
    let count = match o.value('c') {
        None => 4,
        Some(v) => match v.parse::<u32>() {
            Ok(n) if (1..=100).contains(&n) => n,
            _ => {
                cx.error(&format!("invalid count '{v}'"));
                return 2;
            }
        },
    };
    let Some(host) = o.operands.first().cloned() else {
        cx.error("usage: ping [-c N] HOST");
        return 2;
    };
    cx.println(&format!("PING {host}"));
    match cx.sys.ping(&host, count) {
        Ok(s) => {
            let loss = ((s.sent - s.received.min(s.sent)) * 100)
                .checked_div(s.sent)
                .unwrap_or(0);
            cx.println(&format!(
                "{} packets transmitted, {} received, {}% packet loss",
                s.sent, s.received, loss
            ));
            if s.received > 0 {
                cx.println(&format!(
                    "avg rtt = {}.{:03} ms",
                    s.avg_rtt_us / 1000,
                    s.avg_rtt_us % 1000
                ));
                0
            } else {
                1
            }
        }
        Err(e) => {
            cx.error(&format!("{host}: {}", e.message()));
            1
        }
    }
}

fn true_cmd(_: &mut CmdCtx<'_>) -> i32 {
    0
}

fn false_cmd(_: &mut CmdCtx<'_>) -> i32 {
    1
}

// ---- test / [ -----------------------------------------------------------------

struct TestEval<'a, 'b> {
    t: &'a [String],
    i: usize,
    cx: &'a mut CmdCtx<'b>,
}

fn is_unary(s: &str) -> bool {
    matches!(
        s,
        "-e" | "-f" | "-d" | "-s" | "-z" | "-n" | "-r" | "-w" | "-x"
    )
}

fn is_binary(s: &str) -> bool {
    matches!(
        s,
        "=" | "==" | "!=" | "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" | "<" | ">"
    )
}

impl TestEval<'_, '_> {
    fn peek(&self) -> Option<&str> {
        self.t.get(self.i).map(String::as_str)
    }

    fn or(&mut self) -> Result<bool, &'static str> {
        let mut v = self.and()?;
        while self.peek() == Some("-o") {
            self.i += 1;
            let r = self.and()?;
            v = v || r;
        }
        Ok(v)
    }

    fn and(&mut self) -> Result<bool, &'static str> {
        let mut v = self.not()?;
        while self.peek() == Some("-a") {
            self.i += 1;
            let r = self.not()?;
            v = v && r;
        }
        Ok(v)
    }

    fn not(&mut self) -> Result<bool, &'static str> {
        if self.peek() == Some("!") && self.i + 1 < self.t.len() {
            self.i += 1;
            return Ok(!self.not()?);
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<bool, &'static str> {
        let Some(tok) = self.peek().map(String::from) else {
            return Err("argument expected");
        };
        if tok == "(" {
            self.i += 1;
            let v = self.or()?;
            if self.peek() != Some(")") {
                return Err("missing ')'");
            }
            self.i += 1;
            return Ok(v);
        }
        let next_is_binary = self.t.get(self.i + 1).is_some_and(|s| is_binary(s));
        if is_unary(&tok)
            && self.i + 1 < self.t.len()
            && !(next_is_binary && self.i + 2 < self.t.len())
        {
            let arg = self.t[self.i + 1].clone();
            self.i += 2;
            return Ok(match tok.as_str() {
                "-z" => arg.is_empty(),
                "-n" => !arg.is_empty(),
                "-e" | "-r" | "-w" | "-x" => self.cx.fs.stat(&arg).is_ok(),
                "-f" => self.cx.fs.stat(&arg).is_ok_and(|s| s.kind == Kind::File),
                "-d" => self.cx.fs.stat(&arg).is_ok_and(|s| s.kind == Kind::Dir),
                _ => self.cx.fs.stat(&arg).is_ok_and(|s| s.size > 0),
            });
        }
        if next_is_binary && self.i + 2 < self.t.len() {
            let op = self.t[self.i + 1].clone();
            let rhs = self.t[self.i + 2].clone();
            self.i += 3;
            let ints = |a: &str, b: &str| -> Result<(i64, i64), &'static str> {
                match (parse_num(a), parse_num(b)) {
                    (Some(x), Some(y)) => Ok((x, y)),
                    _ => Err("integer expression expected"),
                }
            };
            return Ok(match op.as_str() {
                "=" | "==" => tok == rhs,
                "!=" => tok != rhs,
                "<" => tok < rhs,
                ">" => tok > rhs,
                "-eq" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a == b
                }
                "-ne" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a != b
                }
                "-lt" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a < b
                }
                "-le" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a <= b
                }
                "-gt" => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a > b
                }
                _ => {
                    let (a, b) = ints(&tok, &rhs)?;
                    a >= b
                }
            });
        }
        self.i += 1;
        Ok(!tok.is_empty())
    }
}

fn run_test(cx: &mut CmdCtx<'_>, tokens: &[String]) -> i32 {
    if tokens.is_empty() {
        return 1;
    }
    let mut ev = TestEval {
        t: tokens,
        i: 0,
        cx,
    };
    match ev.or() {
        Ok(v) if ev.i == tokens.len() => i32::from(!v),
        Ok(_) => {
            ev.cx.error("too many arguments");
            2
        }
        Err(m) => {
            ev.cx.error(m);
            2
        }
    }
}

fn test(cx: &mut CmdCtx<'_>) -> i32 {
    let t: Vec<String> = cx.args[1..].to_vec();
    run_test(cx, &t)
}

fn bracket(cx: &mut CmdCtx<'_>) -> i32 {
    let mut t: Vec<String> = cx.args[1..].to_vec();
    if t.last().map(String::as_str) != Some("]") {
        cx.error("missing ']'");
        return 2;
    }
    t.pop();
    run_test(cx, &t)
}

fn sleep(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(arg) = cx.args.get(1).cloned() else {
        cx.error("missing operand");
        return 2;
    };
    let (num, mult) = match arg.strip_suffix('s') {
        Some(n) => (n, 1000u64),
        None => match arg.strip_suffix('m') {
            Some(n) => (n, 60_000),
            None => (arg.as_str(), 1000),
        },
    };
    let (whole, frac) = num.split_once('.').unwrap_or((num, ""));
    let ok = !whole.is_empty() || !frac.is_empty();
    let w = whole
        .parse::<u64>()
        .ok()
        .or(if whole.is_empty() { Some(0) } else { None });
    let digits_ok = frac.chars().all(|c| c.is_ascii_digit());
    let (Some(w), true, true) = (w, digits_ok, ok) else {
        cx.error(&format!("invalid time interval '{arg}'"));
        return 1;
    };
    // Fraction: up to 3 digits of precision relative to the unit.
    let mut f3: u64 = 0;
    for (i, c) in frac.chars().take(3).enumerate() {
        f3 += u64::from(c as u8 - b'0') * 10u64.pow(2 - i as u32);
    }
    let ms = w
        .saturating_mul(mult)
        .saturating_add(f3.saturating_mul(mult) / 1000);
    let ms = ms.min(cx.limits.max_sleep_ms);
    cx.sys.sleep_ms(ms);
    0
}

fn seq(cx: &mut CmdCtx<'_>) -> i32 {
    let nums: Vec<Option<i64>> = cx.args[1..].iter().map(|s| parse_num(s)).collect();
    if nums.is_empty() || nums.len() > 3 || nums.iter().any(Option::is_none) {
        cx.error("usage: seq [FIRST [INCR]] LAST");
        return 2;
    }
    let n: Vec<i64> = nums.into_iter().flatten().collect();
    let (first, step, last) = match n.as_slice() {
        [l] => (1, 1, *l),
        [f, l] => (*f, 1, *l),
        [f, s, l] => (*f, *s, *l),
        _ => return 2,
    };
    if step == 0 {
        cx.error("increment must not be zero");
        return 2;
    }
    let mut v = first;
    let mut count = 0usize;
    while (step > 0 && v <= last) || (step < 0 && v >= last) {
        cx.print(&format!("{v}\n"));
        count += 1;
        if cx.full() || count >= cx.limits.max_loop * 100 {
            break;
        }
        match v.checked_add(step) {
            Some(x) => v = x,
            None => break,
        }
    }
    0
}

fn basename_cmd(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(p) = cx.args.get(1).cloned() else {
        cx.error("missing operand");
        return 2;
    };
    let trimmed = p.trim_end_matches('/');
    let mut b = if trimmed.is_empty() && p.starts_with('/') {
        "/"
    } else {
        basename(trimmed)
    }
    .to_string();
    if let Some(suf) = cx.args.get(2)
        && b.len() > suf.len()
        && b.ends_with(suf.as_str())
    {
        b.truncate(b.len() - suf.len());
    }
    cx.println(&b);
    0
}

fn dirname_cmd(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(p) = cx.args.get(1).cloned() else {
        cx.error("missing operand");
        return 2;
    };
    let trimmed = p.trim_end_matches('/');
    let d = match trimmed.rfind('/') {
        None => {
            if p.starts_with('/') {
                "/"
            } else {
                "."
            }
        }
        Some(0) => "/",
        Some(i) => &trimmed[..i],
    };
    cx.println(d);
    0
}

fn stat(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error("missing operand");
        return 2;
    }
    let mut status = 0;
    for p in &cx.args[1..] {
        match cx.fs.stat(p) {
            Ok(s) => {
                let k = if s.kind == Kind::Dir {
                    "directory"
                } else {
                    "file"
                };
                cx.println(&format!("{p}: {k}, {} bytes", s.size));
            }
            Err(e) => {
                let msg = format!("cannot stat '{p}': {}", e.message());
                cx.error(&msg);
                status = 1;
            }
        }
    }
    status
}

fn rev(cx: &mut CmdCtx<'_>) -> i32 {
    let files: Vec<String> = cx.args[1..].to_vec();
    let (ins, ok) = inputs(cx, &files);
    for (_, d) in ins {
        for l in lines_of(&d) {
            let r: String = l.chars().rev().collect();
            cx.print(&r);
            cx.print("\n");
        }
    }
    i32::from(!ok)
}

fn expand_set(s: &str) -> Vec<char> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if i + 2 < c.len() && c[i + 1] == '-' && c[i] <= c[i + 2] {
            let (lo, hi) = (c[i] as u32, c[i + 2] as u32);
            for u in lo..=hi.min(lo + 4096) {
                if let Some(ch) = char::from_u32(u) {
                    out.push(ch);
                }
            }
            i += 3;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

fn tr(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "d", "", None) else {
        return 2;
    };
    let delete = o.has('d');
    let ok_args = if delete {
        o.operands.len() == 1
    } else {
        o.operands.len() == 2
    };
    if !ok_args {
        cx.error("usage: tr SET1 SET2 | tr -d SET1");
        return 2;
    }
    let from = expand_set(&o.operands[0]);
    let to = if delete {
        Vec::new()
    } else {
        expand_set(&o.operands[1])
    };
    let text = String::from_utf8_lossy(cx.stdin).into_owned();
    let mut out = String::new();
    for ch in text.chars() {
        match from.iter().position(|&f| f == ch) {
            Some(_) if delete => {}
            Some(i) => out.push(*to.get(i).or(to.last()).unwrap_or(&ch)),
            None => out.push(ch),
        }
    }
    cx.print(&out);
    0
}

fn cut(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "", "df", None) else {
        return 2;
    };
    let delim = o.value('d').and_then(|d| d.chars().next()).unwrap_or('\t');
    let Some(fields) = o.value('f') else {
        cx.error("you must specify a list of fields (-f)");
        return 2;
    };
    let mut idx: Vec<usize> = Vec::new();
    for part in fields.split(',') {
        match part.parse::<usize>() {
            Ok(n) if n >= 1 => idx.push(n - 1),
            _ => {
                cx.error(&format!("invalid field '{part}'"));
                return 2;
            }
        }
    }
    let (ins, ok) = inputs(cx, &o.operands);
    for (_, d) in ins {
        for l in lines_of(&d) {
            let cols: Vec<&str> = l.split(delim).collect();
            let picked: Vec<&str> = idx.iter().filter_map(|&i| cols.get(i).copied()).collect();
            cx.print(&picked.join(&delim.to_string()));
            cx.print("\n");
        }
    }
    i32::from(!ok)
}

fn nl(cx: &mut CmdCtx<'_>) -> i32 {
    let files: Vec<String> = cx.args[1..].to_vec();
    let (ins, ok) = inputs(cx, &files);
    let mut n = 0;
    for (_, d) in ins {
        for l in lines_of(&d) {
            n += 1;
            cx.print(&format!("{n:>6}\t{l}\n"));
        }
    }
    i32::from(!ok)
}

fn yes(cx: &mut CmdCtx<'_>) -> i32 {
    let word = if cx.args.len() > 1 {
        cx.args[1..].join(" ")
    } else {
        String::from("y")
    };
    let line = format!("{word}\n");
    let mut n = 0usize;
    while !cx.full() && n < cx.limits.max_pipe {
        cx.print(&line);
        n += line.len();
    }
    0
}
