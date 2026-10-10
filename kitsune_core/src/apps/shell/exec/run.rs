//! run (split out of `exec.rs`).

use super::*;

impl Shell {
    /// Run one command line typed by the user. Records it in the history.
    pub fn run_line(&mut self, line: &str, h: &mut Host<'_>) -> RunResult {
        if line.len() > self.limits.max_line {
            self.status = 1;
            return RunResult {
                status: 1,
                output: alloc::format!("sh: {}\n", t!("sh.exec.line_long")).into_bytes(),
                ..RunResult::default()
            };
        }
        self.meta.history.add(line);
        if line.trim().is_empty() {
            return RunResult {
                status: self.status,
                ..RunResult::default()
            };
        }
        self.run_source(line, "sh", &[], h)
    }

    /// Run a script's text with positional arguments (`$1..`, `$@`, `$#`).
    pub fn run_script(&mut self, src: &str, args: &[String], h: &mut Host<'_>) -> RunResult {
        if src.len() > self.limits.max_script {
            return RunResult {
                status: 1,
                output: alloc::format!("sh: {}\n", t!("sh.exec.script_big_short")).into_bytes(),
                ..RunResult::default()
            };
        }
        self.run_source(src, "script", args, h)
    }

    pub(super) fn run_source(
        &mut self,
        src: &str,
        name: &str,
        args: &[String],
        h: &mut Host<'_>,
    ) -> RunResult {
        self.begin_run();
        let parsed = parse::parse(src);
        let status = match parsed {
            Err(e) => {
                self.syntax_error(name, src, e);
                2
            }
            Ok(list) => {
                self.frames.push(Frame {
                    name: name.to_string(),
                    args: args.to_vec(),
                });
                let s = self.exec_list(&list, &[], &mut Out::Display, h);
                self.frames.pop();
                // A wait inside the last command may have been cut short by Ctrl+C
                // with no later step to notice it.
                if !self.interrupted && h.sys.interrupted() {
                    self.interrupted = true;
                    self.say(t!("sh.sys.interrupted"));
                }
                self.finish_status(s)
            }
        };
        self.status = status;
        self.end_run(status)
    }

    pub(super) fn finish_status(&mut self, s: i32) -> i32 {
        if self.interrupted {
            return 130;
        }
        match self.ctl {
            Ctl::Exit(c) | Ctl::Return(c) => c,
            _ => s,
        }
    }

    pub(super) fn begin_run(&mut self) {
        self.steps = 0;
        self.depth = 0;
        self.sub_depth = 0;
        self.ctl = Ctl::None;
        self.clear = false;
        self.pipe_truncated = false;
        self.notified_limit = false;
        self.interrupted = false;
        self.display = OutBuf::new(self.limits.max_output);
    }

    pub(super) fn end_run(&mut self, status: i32) -> RunResult {
        let exit = match self.ctl {
            Ctl::Exit(c) => Some(c),
            _ => None,
        };
        self.ctl = Ctl::None;
        let mut out = core::mem::take(&mut self.display);
        let truncated = out.truncated || self.pipe_truncated;
        if out.truncated {
            out.truncated = false;
            out.data.extend_from_slice(b"\n");
            out.data
                .extend_from_slice(t!("sh.exec.output_cut").as_bytes());
            out.data.extend_from_slice(b"\n");
        }
        RunResult {
            status,
            output: out.data,
            exit,
            clear: self.clear,
            truncated,
        }
    }

    pub(super) fn syntax_error(&mut self, name: &str, src: &str, e: ParseError) {
        let (l, c) = e.line_col(src);
        let msg = alloc::format!(
            "{}\n",
            t!(
                "sh.exec.syntax",
                name = name,
                l = l,
                c = c,
                msg = e.message().as_str()
            )
        );
        self.display.push(msg.as_bytes());
    }

    /// Write `sh: msg` to the display.
    pub(super) fn say(&mut self, msg: &str) {
        self.display.push(b"sh: ");
        self.display.push(msg.as_bytes());
        self.display.push(b"\n");
    }

    pub(super) fn tick(&mut self, sys: &dyn SysInfo) -> bool {
        if self.ctl == Ctl::Abort {
            return false;
        }
        if sys.interrupted() {
            self.interrupted = true;
            self.say(t!("sh.sys.interrupted"));
            self.ctl = Ctl::Abort;
            return false;
        }
        self.steps += 1;
        if self.steps > self.limits.max_steps {
            if !self.notified_limit {
                self.notified_limit = true;
                self.say(t!("sh.exec.step_limit"));
            }
            self.ctl = Ctl::Abort;
            return false;
        }
        if self.display.is_full() {
            self.ctl = Ctl::Abort;
            return false;
        }
        true
    }

    pub(super) fn emit(&mut self, out: &mut Out<'_>, bytes: &[u8]) {
        match out {
            Out::Display => self.display.push(bytes),
            Out::Buf(b) => {
                b.push(bytes);
                if b.truncated && !self.pipe_truncated {
                    self.pipe_truncated = true;
                    self.say(t!("sh.exec.pipe_limit"));
                }
            }
        }
    }

    // ---- lists, pipelines, compound commands --------------------------------
}
