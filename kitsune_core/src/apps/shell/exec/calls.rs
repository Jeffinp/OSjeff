//! calls (split out of `exec.rs`).

use super::*;

impl Shell {
    pub(super) fn find_script(&self, name: &str, fs: &dyn ShellFs) -> Option<String> {
        let is_file = |p: &str| fs.stat(p).is_ok_and(|s| s.kind == Kind::File);
        if name.contains('/') {
            return is_file(name).then(|| name.to_string());
        }
        for dir in self.env.path_dirs() {
            for cand in [
                super::super::fs::join(&dir, name),
                super::super::fs::join(&dir, &alloc::format!("{name}.sh")),
            ] {
                if is_file(&cand) {
                    return Some(cand);
                }
            }
        }
        None
    }

    pub(super) fn call_builtin(
        &mut self,
        f: BuiltinFn,
        args: &[String],
        stdin: &[u8],
        out: &mut Out<'_>,
        merged: bool,
        h: &mut Host<'_>,
    ) -> i32 {
        let mut cx = CmdCtx {
            args,
            stdin,
            io: Io::new(self.limits.max_pipe.max(self.limits.max_output), merged),
            env: &mut self.env,
            fs: &mut *h.fs,
            sys: &mut *h.sys,
            meta: &mut self.meta,
            registry: &self.registry,
            funcs: &self.funcs,
            limits: &self.limits,
            clear: false,
        };
        let status = f(&mut cx);
        let (o, e, clear) = (cx.io.out, cx.io.err, cx.clear);
        if clear {
            self.clear = true;
            // Output printed before `clear` is meaningless on a cleared view.
            self.display.data.clear();
        }
        self.emit(out, &o.data);
        if !e.data.is_empty() {
            self.display.push(&e.data);
        }
        status
    }

    pub(super) fn call_function(
        &mut self,
        body: &Arc<Vec<Stmt>>,
        args: &[String],
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        if self.depth >= self.limits.max_call_depth {
            self.say(t!("sh.exec.depth_limit"));
            self.ctl = Ctl::Abort;
            return 1;
        }
        self.depth += 1;
        self.frames.push(Frame {
            name: args[0].clone(),
            args: args[1..].to_vec(),
        });
        let mut st = self.exec_list(body, stdin, out, h);
        self.frames.pop();
        self.depth -= 1;
        if let Ctl::Return(c) = self.ctl {
            self.ctl = Ctl::None;
            st = c;
        }
        st
    }

    pub(super) fn run_file(
        &mut self,
        path: &str,
        args: &[String],
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        let data = match h.fs.read(path) {
            Ok(d) => d,
            Err(e) => {
                self.say(&alloc::format!("{path}: {}", e.message()));
                return 126;
            }
        };
        if data.len() > self.limits.max_script {
            self.say(&t!("sh.exec.script_big", path = path));
            return 126;
        }
        let src = String::from_utf8_lossy(&data).into_owned();
        let list = match parse::parse(&src) {
            Ok(l) => l,
            Err(e) => {
                self.syntax_error(path, &src, e);
                return 2;
            }
        };
        if self.depth >= self.limits.max_call_depth {
            self.say(t!("sh.exec.nest_limit"));
            self.ctl = Ctl::Abort;
            return 1;
        }
        self.depth += 1;
        self.frames.push(Frame {
            name: path.to_string(),
            args: args.to_vec(),
        });
        let mut st = self.exec_list(&list, stdin, out, h);
        self.frames.pop();
        self.depth -= 1;
        if let Ctl::Return(c) = self.ctl {
            self.ctl = Ctl::None;
            st = c;
        }
        st
    }

    pub(super) fn run_special(
        &mut self,
        args: &[String],
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        let num = |i: usize, default: i32| -> Option<i32> {
            match args.get(i) {
                None => Some(default),
                Some(s) => s.parse::<i32>().ok(),
            }
        };
        match args[0].as_str() {
            ":" => 0,
            "exit" => match num(1, self.status) {
                Some(c) => {
                    self.ctl = Ctl::Exit(c & 0xFF);
                    c & 0xFF
                }
                None => {
                    self.say(t!("sh.exec.exit_numeric"));
                    self.ctl = Ctl::Exit(2);
                    2
                }
            },
            "return" => match num(1, self.status) {
                Some(c) => {
                    if self.frames.len() <= 1 && self.depth == 0 {
                        self.say(t!("sh.exec.return_outside"));
                        1
                    } else {
                        self.ctl = Ctl::Return(c & 0xFF);
                        c & 0xFF
                    }
                }
                None => {
                    self.say(t!("sh.exec.return_numeric"));
                    2
                }
            },
            "break" | "continue" => match num(1, 1) {
                Some(n) if n >= 1 => {
                    let n = n as u32;
                    self.ctl = if args[0] == "break" {
                        Ctl::Break(n)
                    } else {
                        Ctl::Continue(n)
                    };
                    0
                }
                _ => {
                    self.say(&t!("sh.exec.bad_loop_count", cmd = args[0].as_str()));
                    1
                }
            },
            "shift" => match num(1, 1) {
                Some(n) if n >= 0 => {
                    let n = n as usize;
                    match self.frames.last_mut() {
                        Some(f) if n <= f.args.len() => {
                            f.args.drain(..n);
                            0
                        }
                        _ => 1,
                    }
                }
                _ => {
                    self.say(t!("sh.exec.bad_shift"));
                    1
                }
            },
            "source" | "." | "sh" => {
                let Some(file) = args.get(1) else {
                    self.say(&t!("sh.exec.file_required", cmd = args[0].as_str()));
                    return 2;
                };
                let file = file.clone();
                self.run_file(&file, &args[2..], stdin, out, h)
            }
            "set" => self.run_set(args, out),
            _ => 0,
        }
    }

    pub(super) fn run_set(&mut self, args: &[String], out: &mut Out<'_>) -> i32 {
        if args.len() == 1 {
            let mut text = String::new();
            for (n, v, _) in self.env.iter() {
                text.push_str(&alloc::format!("{n}={v}\n"));
            }
            self.emit(out, text.as_bytes());
            return 0;
        }
        if args[1] == "--" {
            if let Some(f) = self.frames.last_mut() {
                f.args = args[2..].to_vec();
            }
            return 0;
        }
        let mut status = 0;
        for a in &args[1..] {
            match a.split_once('=') {
                Some((n, v)) if super::super::env::valid_name(n) => {
                    if !self.env.set(n, v) {
                        status = 1;
                    }
                }
                _ => {
                    self.say(&t!("sh.exec.set_invalid", arg = a.as_str()));
                    status = 2;
                }
            }
        }
        status
    }

    // ---- expansion --------------------------------------------------------------
}
