//! misc (split out of `builtins.rs`).

use super::*;

pub(super) fn sleep(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(arg) = cx.args.get(1).cloned() else {
        cx.error(t!("sh.err.missing_operand"));
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
        cx.error(&t!("sh.sleep.bad_interval", arg = arg.as_str()));
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

pub(super) fn seq(cx: &mut CmdCtx<'_>) -> i32 {
    let nums: Vec<Option<i64>> = cx.args[1..].iter().map(|s| parse_num(s)).collect();
    if nums.is_empty() || nums.len() > 3 || nums.iter().any(Option::is_none) {
        cx.error(t!("sh.seq.usage"));
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
        cx.error(t!("sh.seq.zero_step"));
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

pub(super) fn basename_cmd(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(p) = cx.args.get(1).cloned() else {
        cx.error(t!("sh.err.missing_operand"));
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

pub(super) fn dirname_cmd(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(p) = cx.args.get(1).cloned() else {
        cx.error(t!("sh.err.missing_operand"));
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

pub(super) fn stat(cx: &mut CmdCtx<'_>) -> i32 {
    if cx.args.len() < 2 {
        cx.error(t!("sh.err.missing_operand"));
        return 2;
    }
    let mut status = 0;
    for p in &cx.args[1..] {
        match cx.fs.stat(p) {
            Ok(s) => {
                let key = if s.kind == Kind::Dir {
                    tk!("sh.stat.dir")
                } else {
                    tk!("sh.stat.file")
                };
                let size = i64::try_from(s.size).unwrap_or(i64::MAX);
                let line = i18n::tr_fmt(key, &[("path", Arg::Str(p)), ("size", Arg::Int(size))]);
                cx.println(&line);
            }
            Err(e) => {
                let msg = t!("sh.err.cannot_stat", path = p.as_str(), why = e.message());
                cx.error(&msg);
                status = 1;
            }
        }
    }
    status
}

pub(super) fn rev(cx: &mut CmdCtx<'_>) -> i32 {
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

/// The characters of a `tr` set: ranges (`a-z`) and the escapes `\n`, `\t`, `\r`, `\\`.
pub(super) fn expand_set(s: &str) -> Vec<char> {
    let mut c: Vec<char> = Vec::new();
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        if ch != '\\' {
            c.push(ch);
            continue;
        }
        match it.next() {
            Some('n') => c.push('\n'),
            Some('t') => c.push('\t'),
            Some('r') => c.push('\r'),
            Some('\\') => c.push('\\'),
            Some(o) => {
                c.push('\\');
                c.push(o);
            }
            None => c.push('\\'),
        }
    }
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

pub(super) fn tr(cx: &mut CmdCtx<'_>) -> i32 {
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
        cx.error(t!("sh.tr.usage"));
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

pub(super) fn cut(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "", "df", None) else {
        return 2;
    };
    let delim = o.value('d').and_then(|d| d.chars().next()).unwrap_or('\t');
    let Some(fields) = o.value('f') else {
        cx.error(t!("sh.cut.no_fields"));
        return 2;
    };
    let mut idx: Vec<usize> = Vec::new();
    for part in fields.split(',') {
        match part.parse::<usize>() {
            Ok(n) if n >= 1 => idx.push(n - 1),
            _ => {
                cx.error(&t!("sh.cut.bad_field", part = part));
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

pub(super) fn nl(cx: &mut CmdCtx<'_>) -> i32 {
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

pub(super) fn yes(cx: &mut CmdCtx<'_>) -> i32 {
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
