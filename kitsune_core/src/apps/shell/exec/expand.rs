//! expand (split out of `exec.rs`).

use super::*;

impl Shell {
    pub(super) fn lookup(&self, name: &str) -> String {
        if name.chars().all(|c| c.is_ascii_digit()) {
            let idx: usize = name.parse().unwrap_or(usize::MAX);
            let f = self.frames.last();
            return match (idx, f) {
                (0, Some(f)) => f.name.clone(),
                (i, Some(f)) => f.args.get(i - 1).cloned().unwrap_or_default(),
                _ => String::new(),
            };
        }
        self.env.get(name).unwrap_or("").to_string()
    }

    pub(super) fn positional(&self) -> Vec<String> {
        self.frames
            .last()
            .map(|f| f.args.clone())
            .unwrap_or_default()
    }

    pub(super) fn run_sub(&mut self, body: &[Stmt], h: &mut Host<'_>) -> String {
        if self.sub_depth >= self.limits.max_sub_depth {
            self.say(t!("sh.exec.subst_deep"));
            self.ctl = Ctl::Abort;
            return String::new();
        }
        self.sub_depth += 1;
        let mut buf = OutBuf::new(self.limits.max_pipe);
        let saved_status = self.status;
        let st = self.exec_list(body, &[], &mut Out::Buf(&mut buf), h);
        self.sub_depth -= 1;
        // A substitution is a subshell: `exit`/`return` inside stay inside.
        if matches!(
            self.ctl,
            Ctl::Exit(_) | Ctl::Return(_) | Ctl::Break(_) | Ctl::Continue(_)
        ) {
            self.ctl = Ctl::None;
        }
        if buf.truncated && !self.pipe_truncated {
            self.pipe_truncated = true;
            self.say(t!("sh.exec.subst_cut"));
        }
        self.status = st;
        self.last_sub = Some(st);
        let _ = saved_status;
        let mut s = String::from_utf8_lossy(&buf.data).into_owned();
        while s.ends_with('\n') {
            s.pop();
        }
        s
    }

    pub(super) fn arith(&mut self, expr: &Word, h: &mut Host<'_>) -> String {
        let text = self.expand_string(expr, h);
        let env = &self.env;
        let frames = &self.frames;
        let var = |n: &str| -> i64 {
            let v = if n.chars().all(|c| c.is_ascii_digit()) {
                let i: usize = n.parse().unwrap_or(usize::MAX);
                frames
                    .last()
                    .and_then(|f| i.checked_sub(1).and_then(|k| f.args.get(k)))
                    .cloned()
                    .unwrap_or_default()
            } else {
                env.get(n).unwrap_or("").to_string()
            };
            v.trim().parse::<i64>().unwrap_or(0)
        };
        match glob::eval_arith(&text, &var) {
            Ok(v) => v.to_string(),
            Err(e) => {
                self.say(e.message());
                self.status = 1;
                "0".to_string()
            }
        }
    }

    /// The text a part contributes when it is a substitution result.
    pub(super) fn part_value(&mut self, p: &Part, h: &mut Host<'_>) -> Option<(String, bool)> {
        match p {
            Part::Var { name, quoted } => Some((self.lookup(name), *quoted)),
            Part::Status { quoted } => Some((self.status.to_string(), *quoted)),
            Part::Count { quoted } => Some((self.positional().len().to_string(), *quoted)),
            Part::Cmd { body, quoted } => {
                let body = body.clone();
                Some((self.run_sub(&body, h), *quoted))
            }
            Part::Arith { expr, quoted } => Some((self.arith(expr, h), *quoted)),
            Part::Lit { .. } | Part::Args { .. } => None,
        }
    }

    /// Expand a word to fields (word splitting on unquoted substitutions) and,
    /// when `globbing`, wildcards.
    pub(super) fn expand_fields(
        &mut self,
        w: &Word,
        h: &mut Host<'_>,
        globbing: bool,
    ) -> Vec<String> {
        let mut fields: Vec<Field> = Vec::new();
        let mut cur: Field = Vec::new();
        let mut started = false;
        for part in w {
            if self.ctl == Ctl::Abort {
                break;
            }
            match part {
                Part::Lit { text, quoted } => {
                    for c in text.chars() {
                        cur.push((c, !quoted && (c == '*' || c == '?')));
                    }
                    started = true;
                }
                Part::Args { at, quoted } => {
                    let args = self.positional();
                    if *quoted && *at {
                        for (i, a) in args.iter().enumerate() {
                            if i > 0 {
                                fields.push(core::mem::take(&mut cur));
                            }
                            cur.extend(a.chars().map(|c| (c, false)));
                            started = true;
                        }
                    } else {
                        let joined = args.join(" ");
                        add_value(&joined, *quoted, &mut fields, &mut cur, &mut started);
                    }
                }
                other => {
                    if let Some((v, q)) = self.part_value(other, h) {
                        add_value(&v, q, &mut fields, &mut cur, &mut started);
                    }
                }
            }
        }
        if started || !cur.is_empty() {
            fields.push(cur);
        }
        let mut out = Vec::new();
        for f in fields {
            if globbing && f.iter().any(|c| c.1) {
                let m = glob::expand(&*h.fs, &f, self.limits.max_glob);
                if !m.is_empty() {
                    out.extend(m);
                    continue;
                }
            }
            out.push(f.iter().map(|c| c.0).collect());
        }
        out
    }

    /// Expand a word to one string: no splitting, no globbing.
    pub(super) fn expand_string(&mut self, w: &Word, h: &mut Host<'_>) -> String {
        let mut s = String::new();
        for part in w {
            if self.ctl == Ctl::Abort {
                break;
            }
            match part {
                Part::Lit { text, .. } => s.push_str(text),
                Part::Args { .. } => s.push_str(&self.positional().join(" ")),
                other => {
                    if let Some((v, _)) = self.part_value(other, h) {
                        s.push_str(&v);
                    }
                }
            }
        }
        s
    }
}
