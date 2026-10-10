//! text (split out of `builtins.rs`).

use super::*;

pub(super) fn head_tail(cx: &mut CmdCtx<'_>, tail: bool) -> i32 {
    let Some(o) = parse_opts(cx, "", "n", Some('n')) else {
        return 2;
    };
    let n = match o.value('n') {
        None => 10,
        Some(v) => match parse_num(v) {
            Some(n) if n >= 0 => n as usize,
            _ => {
                cx.error(&t!("sh.err.bad_lines", v = v));
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

pub(super) fn head(cx: &mut CmdCtx<'_>) -> i32 {
    head_tail(cx, false)
}

pub(super) fn tail(cx: &mut CmdCtx<'_>) -> i32 {
    head_tail(cx, true)
}

pub(super) fn wc(cx: &mut CmdCtx<'_>) -> i32 {
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

pub(super) fn grep(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "ivncFElqHh", "e", None) else {
        return 2;
    };
    let mut operands = o.operands.clone();
    let pattern = match o.value('e') {
        Some(p) => p.to_string(),
        None => {
            if operands.is_empty() {
                cx.error(t!("sh.grep.usage"));
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
                cx.error(&t!("sh.grep.bad_pattern", why = e.message()));
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
                if name.is_empty() {
                    cx.println(t!("sh.grep.stdin"));
                } else {
                    cx.println(name);
                }
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

pub(super) fn leading_number(s: &str) -> i64 {
    let t = s.trim_start();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let v = digits.parse::<i64>().unwrap_or(0);
    if neg { -v } else { v }
}

pub(super) fn sort(cx: &mut CmdCtx<'_>) -> i32 {
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

pub(super) fn uniq(cx: &mut CmdCtx<'_>) -> i32 {
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

pub(super) fn tee(cx: &mut CmdCtx<'_>) -> i32 {
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
