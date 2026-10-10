//! pipeline (split out of `exec.rs`).

use super::*;

impl Shell {
    pub(super) fn exec_list(
        &mut self,
        list: &[Stmt],
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        let mut status = 0;
        for st in list {
            if self.ctl != Ctl::None {
                break;
            }
            status = self.exec_and_or(&st.and_or, stdin, out, h);
        }
        status
    }

    pub(super) fn exec_and_or(
        &mut self,
        ao: &AndOr,
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        let mut status = self.exec_pipeline(&ao.first, stdin, out, h);
        for (c, p) in &ao.rest {
            if self.ctl != Ctl::None {
                break;
            }
            let run = match c {
                Connector::And => status == 0,
                Connector::Or => status != 0,
            };
            if run {
                status = self.exec_pipeline(p, stdin, out, h);
            }
        }
        status
    }

    pub(super) fn exec_pipeline(
        &mut self,
        p: &Pipeline,
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        let n = p.cmds.len();
        let mut input: Vec<u8> = stdin.to_vec();
        let mut status = 0;
        for (i, cmd) in p.cmds.iter().enumerate() {
            if self.ctl != Ctl::None {
                break;
            }
            if i + 1 == n {
                status = self.exec_command(cmd, &input, out, h);
            } else {
                let mut buf = OutBuf::new(self.limits.max_pipe);
                status = self.exec_command(cmd, &input, &mut Out::Buf(&mut buf), h);
                if buf.truncated && !self.pipe_truncated {
                    self.pipe_truncated = true;
                    self.say(t!("sh.exec.pipe_limit"));
                }
                input = buf.data;
            }
            self.status = status;
        }
        status
    }

    pub(super) fn exec_command(
        &mut self,
        cmd: &Command,
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        match cmd {
            Command::Simple(s) => self.exec_simple(s, stdin, out, h),
            Command::Group(body) => self.exec_list(body, stdin, out, h),
            Command::If { arms, else_body } => {
                for (cond, body) in arms {
                    let c = self.exec_list(cond, stdin, out, h);
                    if self.ctl != Ctl::None {
                        return c;
                    }
                    if c == 0 {
                        return self.exec_list(body, stdin, out, h);
                    }
                }
                match else_body {
                    Some(b) => self.exec_list(b, stdin, out, h),
                    None => 0,
                }
            }
            Command::For { var, words, body } => {
                let mut items: Vec<String> = Vec::new();
                for w in words {
                    items.extend(self.expand_fields(w, h, true));
                }
                let mut status = 0;
                for (i, item) in items.into_iter().enumerate() {
                    if i >= self.limits.max_loop {
                        self.say(t!("sh.exec.loop_limit"));
                        status = 1;
                        break;
                    }
                    if !self.tick(h.sys) {
                        break;
                    }
                    self.env.set(var, &item);
                    status = self.exec_list(body, stdin, out, h);
                    if self.loop_control() {
                        break;
                    }
                }
                status
            }
            Command::While { cond, body, until } => {
                let mut status = 0;
                let mut iters = 0usize;
                loop {
                    if iters >= self.limits.max_loop {
                        self.say(t!("sh.exec.loop_limit"));
                        status = 1;
                        break;
                    }
                    iters += 1;
                    if !self.tick(h.sys) {
                        break;
                    }
                    let c = self.exec_list(cond, stdin, out, h);
                    if self.ctl != Ctl::None {
                        break;
                    }
                    if (c == 0) == *until {
                        break;
                    }
                    status = self.exec_list(body, stdin, out, h);
                    if self.loop_control() {
                        break;
                    }
                }
                status
            }
            Command::Func { name, body } => {
                if self.funcs.len() >= 256 && !self.funcs.contains_key(name) {
                    self.say(t!("sh.exec.too_many_funcs"));
                    return 1;
                }
                self.funcs.insert(name.clone(), body.clone());
                0
            }
        }
    }

    /// After a loop body: true when the loop must stop.
    pub(super) fn loop_control(&mut self) -> bool {
        match self.ctl {
            Ctl::None => false,
            Ctl::Break(n) => {
                self.ctl = if n > 1 { Ctl::Break(n - 1) } else { Ctl::None };
                true
            }
            Ctl::Continue(n) => {
                if n > 1 {
                    self.ctl = Ctl::Continue(n - 1);
                    true
                } else {
                    self.ctl = Ctl::None;
                    false
                }
            }
            _ => true,
        }
    }

    // ---- simple commands ------------------------------------------------------

    pub(super) fn exec_simple(
        &mut self,
        s: &Simple,
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        if !self.tick(h.sys) {
            return 1;
        }
        // Expand words (command substitutions run here).
        self.last_sub = None;
        let mut args: Vec<String> = Vec::new();
        for w in &s.words {
            args.extend(self.expand_fields(w, h, true));
        }
        let mut assigns: Vec<(String, String)> = Vec::new();
        for (name, w) in &s.assigns {
            let v = self.expand_string(w, h);
            assigns.push((name.clone(), v));
        }
        if self.ctl != Ctl::None {
            return self.status;
        }

        // Redirections.
        let mut input: Option<Vec<u8>> = None;
        let mut sink: Option<(String, bool)> = None;
        for r in &s.redirs {
            let target = self.expand_string(&r.target, h);
            if target.is_empty() {
                self.say(t!("sh.exec.ambiguous"));
                return 1;
            }
            if target == "/dev/null" {
                match r.kind {
                    RedirKind::In => input = Some(Vec::new()),
                    _ => sink = Some((target, false)),
                }
                continue;
            }
            match r.kind {
                RedirKind::In => match h.fs.read(&target) {
                    Ok(d) => input = Some(d),
                    Err(e) => {
                        self.say(&alloc::format!("{target}: {}", e.message()));
                        return 1;
                    }
                },
                RedirKind::Out | RedirKind::Append => {
                    let append = r.kind == RedirKind::Append;
                    // Fail early (and create/truncate) like a real shell.
                    let res = if append {
                        h.fs.append(&target, b"")
                    } else {
                        h.fs.write(&target, b"")
                    };
                    if let Err(e) = res {
                        self.say(&alloc::format!("{target}: {}", e.message()));
                        return 1;
                    }
                    sink = Some((target, true));
                }
            }
        }

        if args.is_empty() {
            // Assignments only (or just redirections).
            for (n, v) in &assigns {
                if !self.env.set(n, v) {
                    self.say(t!("sh.exec.cannot_set_var"));
                    return 1;
                }
            }
            return self.last_sub.unwrap_or(0);
        }

        // Alias expansion on the command name.
        let mut seen: Vec<String> = Vec::new();
        while let Some(rep) = self.meta.aliases.get(&args[0]).cloned() {
            if seen.contains(&args[0]) || seen.len() >= 8 {
                break;
            }
            seen.push(args[0].clone());
            let mut new: Vec<String> = rep.split_whitespace().map(String::from).collect();
            if new.is_empty() {
                break;
            }
            new.extend(args.drain(1..));
            args = new;
        }

        // Temporary assignments for this command only.
        let mut saved: Vec<(String, Option<String>)> = Vec::new();
        for (n, v) in &assigns {
            saved.push((n.clone(), self.env.get(n).map(String::from)));
            self.env.set_exported(n, v);
        }

        let input_ref: &[u8] = match &input {
            Some(d) => d,
            None => stdin,
        };
        let status = if let Some((path, real)) = sink {
            // Output goes to a file: capture, then append.
            let mut buf = OutBuf::new(self.limits.max_pipe);
            let st = self.dispatch(&args, input_ref, &mut Out::Buf(&mut buf), false, h);
            if !real {
                st
            } else if let Err(e) = h.fs.append(&path, &buf.data) {
                self.say(&alloc::format!("{path}: {}", e.message()));
                1
            } else {
                st
            }
        } else {
            let merged = matches!(out, Out::Display);
            self.dispatch(&args, input_ref, out, merged, h)
        };

        for (n, old) in saved {
            match old {
                Some(v) => {
                    self.env.set(&n, &v);
                }
                None => {
                    self.env.unset(&n);
                }
            }
        }
        status
    }

    /// Run `args` as a command: special builtin, function, builtin or script.
    pub(super) fn dispatch(
        &mut self,
        args: &[String],
        stdin: &[u8],
        out: &mut Out<'_>,
        merged: bool,
        h: &mut Host<'_>,
    ) -> i32 {
        let name = args[0].as_str();
        if is_special(name) {
            return self.run_special(args, stdin, out, h);
        }
        if let Some(body) = self.funcs.get(name).cloned() {
            return self.call_function(&body, args, stdin, out, h);
        }
        if let Some(f) = self.registry.lookup(name) {
            return self.call_builtin(f, args, stdin, out, merged, h);
        }
        // Script on PATH or by path.
        if let Some(path) = self.find_script(name, h.fs) {
            return self.run_file(&path, &args[1..], stdin, out, h);
        }
        if name.contains('/')
            && let Ok(st) = h.fs.stat(name)
            && st.kind == Kind::Dir
        {
            self.say(&alloc::format!(
                "{name}: {}",
                super::super::fs::FsErr::IsADirectory.message()
            ));
            return 126;
        }
        self.say(&t!("sh.exec.not_found", name = name));
        127
    }
}
