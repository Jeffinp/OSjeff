//! The executor: expansion, pipelines, redirections, control flow, functions
//! and scripts, all bounded by [`Limits`].

use super::env::Env;
use super::fs::{Kind, ShellFs};
use super::glob::{self, PatChar};
use super::history::History;
use super::parse::{
    self, AndOr, Command, Connector, ParseError, Part, Pipeline, RedirKind, Simple, Stmt, Word,
};
use super::sys::SysInfo;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Resource limits that keep a runaway script from hanging the kernel.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Bytes of output returned by one `run_line`/`run_script`.
    pub max_output: usize,
    /// Bytes buffered in one pipe or `$( )` capture.
    pub max_pipe: usize,
    /// Commands plus loop iterations executed by one run.
    pub max_steps: usize,
    /// Iterations of a single loop.
    pub max_loop: usize,
    /// Nested function calls / `source`s.
    pub max_call_depth: usize,
    /// Nested `$( )` executions.
    pub max_sub_depth: usize,
    /// Paths one glob may produce.
    pub max_glob: usize,
    /// Bytes of one input line.
    pub max_line: usize,
    /// Bytes of one script.
    pub max_script: usize,
    /// Longest single `sleep`.
    pub max_sleep_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_output: 1 << 20,
            max_pipe: 1 << 20,
            max_steps: 100_000,
            max_loop: 10_000,
            max_call_depth: 32,
            max_sub_depth: 8,
            max_glob: 4096,
            max_line: 8192,
            max_script: 256 * 1024,
            max_sleep_ms: 10_000,
        }
    }
}

/// A byte buffer that stops growing at its limit.
#[derive(Clone, Debug, Default)]
pub struct OutBuf {
    pub data: Vec<u8>,
    limit: usize,
    pub truncated: bool,
}

impl OutBuf {
    pub fn new(limit: usize) -> Self {
        Self {
            data: Vec::new(),
            limit,
            truncated: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        let room = self.limit.saturating_sub(self.data.len());
        if bytes.len() > room {
            self.data.extend_from_slice(&bytes[..room]);
            self.truncated = true;
        } else {
            self.data.extend_from_slice(bytes);
        }
    }

    pub fn is_full(&self) -> bool {
        self.data.len() >= self.limit
    }
}

/// The streams of one command.
pub struct Io {
    pub out: OutBuf,
    pub err: OutBuf,
    /// stdout goes to the terminal: stderr is interleaved into `out`.
    merged: bool,
}

impl Io {
    fn new(limit: usize, merged: bool) -> Self {
        Self {
            out: OutBuf::new(limit),
            err: OutBuf::new(limit),
            merged,
        }
    }
}

/// Everything a builtin may touch.
pub struct CmdCtx<'a> {
    /// `args[0]` is the command name.
    pub args: &'a [String],
    pub stdin: &'a [u8],
    pub io: Io,
    pub env: &'a mut Env,
    pub fs: &'a mut dyn ShellFs,
    pub sys: &'a mut dyn SysInfo,
    pub meta: &'a mut Meta,
    pub registry: &'a Registry,
    pub funcs: &'a BTreeMap<String, Arc<Vec<Stmt>>>,
    pub limits: &'a Limits,
    /// Set by `clear`: the terminal view should be wiped.
    pub clear: bool,
}

impl CmdCtx<'_> {
    /// Write to standard output.
    pub fn out(&mut self, bytes: &[u8]) {
        self.io.out.push(bytes);
    }

    /// Write a string to standard output.
    pub fn print(&mut self, s: &str) {
        self.io.out.push(s.as_bytes());
    }

    /// Write a line to standard output.
    pub fn println(&mut self, s: &str) {
        self.io.out.push(s.as_bytes());
        self.io.out.push(b"\n");
    }

    /// Write raw bytes to standard error.
    pub fn err_bytes(&mut self, bytes: &[u8]) {
        if self.io.merged {
            self.io.out.push(bytes);
        } else {
            self.io.err.push(bytes);
        }
    }

    /// Write `name: msg` plus a newline to standard error.
    pub fn error(&mut self, msg: &str) {
        let name = self.args.first().cloned().unwrap_or_default();
        self.err_bytes(name.as_bytes());
        self.err_bytes(b": ");
        self.err_bytes(msg.as_bytes());
        self.err_bytes(b"\n");
    }

    /// True once the output limit is hit (stop producing output).
    pub fn full(&self) -> bool {
        self.io.out.is_full()
    }

    /// The command name.
    pub fn name(&self) -> &str {
        self.args.first().map_or("", String::as_str)
    }
}

/// A builtin: returns the exit status.
pub type BuiltinFn = fn(&mut CmdCtx<'_>) -> i32;

/// Lookup of commands by name. Implemented by [`Registry`]; the trait lets
/// callers describe additional command sets.
pub trait Builtins {
    fn lookup(&self, name: &str) -> Option<BuiltinFn>;
    /// Sorted `(name, one-line help)`.
    fn list(&self) -> Vec<(String, &'static str)>;
}

/// The command table. [`Registry::standard`] has every builtin; the kernel can
/// [`Registry::register`] more (for example `ping` or app launchers).
#[derive(Clone, Default)]
pub struct Registry {
    map: BTreeMap<String, (BuiltinFn, &'static str)>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// All standard builtins.
    pub fn standard() -> Self {
        let mut r = Self::new();
        super::builtins::register_all(&mut r);
        super::netcmds::register_all(&mut r);
        r
    }

    pub fn register(&mut self, name: &str, help: &'static str, f: BuiltinFn) {
        self.map.insert(name.to_string(), (f, help));
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    pub fn help_of(&self, name: &str) -> Option<&'static str> {
        self.map.get(name).map(|e| e.1)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }
}

impl Builtins for Registry {
    fn lookup(&self, name: &str) -> Option<BuiltinFn> {
        self.map.get(name).map(|e| e.0)
    }

    fn list(&self) -> Vec<(String, &'static str)> {
        self.map.iter().map(|(k, v)| (k.clone(), v.1)).collect()
    }
}

/// Shell state builtins may edit.
#[derive(Default)]
pub struct Meta {
    pub aliases: BTreeMap<String, String>,
    pub history: History,
}

/// The services a run needs from the kernel.
pub struct Host<'a> {
    pub fs: &'a mut dyn ShellFs,
    pub sys: &'a mut dyn SysInfo,
}

/// What a `run_line` / `run_script` produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunResult {
    /// Exit status of the last command (`$?`).
    pub status: i32,
    /// Everything to print (stdout and stderr interleaved).
    pub output: Vec<u8>,
    /// The script or line called `exit N`.
    pub exit: Option<i32>,
    /// The terminal should be cleared before printing `output`.
    pub clear: bool,
    /// Output or a pipe hit its size limit.
    pub truncated: bool,
}

impl RunResult {
    /// The output as text (invalid UTF-8 replaced).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ctl {
    None,
    Exit(i32),
    Return(i32),
    Break(u32),
    Continue(u32),
    Abort,
}

/// Where a command's standard output goes.
pub(crate) enum Out<'a> {
    Display,
    Buf(&'a mut OutBuf),
}

struct Frame {
    name: String,
    args: Vec<String>,
}

type Field = Vec<PatChar>;

/// A shell instance: variables, aliases, functions, history, limits.
pub struct Shell {
    pub env: Env,
    pub limits: Limits,
    registry: Registry,
    meta: Meta,
    funcs: BTreeMap<String, Arc<Vec<Stmt>>>,
    frames: Vec<Frame>,
    status: i32,
    display: OutBuf,
    pipe_truncated: bool,
    steps: usize,
    depth: usize,
    sub_depth: usize,
    ctl: Ctl,
    clear: bool,
    notified_limit: bool,
    /// The last run was stopped by `SysInfo::interrupted` (status 130).
    interrupted: bool,
    /// Status of the last `$( )` run while expanding the current command.
    last_sub: Option<i32>,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

fn is_special(name: &str) -> bool {
    matches!(
        name,
        "exit" | "return" | "break" | "continue" | "shift" | "source" | "." | "sh" | "set" | ":"
    )
}

/// Names and help of builtins that the executor handles itself.
pub const SPECIAL_HELP: [(&str, &str); 10] = [
    ("exit", "exit [N]: leave the shell or script with status N"),
    ("return", "return [N]: leave a function with status N"),
    ("break", "break [N]: leave N enclosing loops"),
    ("continue", "continue [N]: next iteration of the Nth loop"),
    ("shift", "shift [N]: drop the first N positional arguments"),
    ("source", "source FILE [ARGS]: run a script in this shell"),
    (".", ". FILE [ARGS]: same as source"),
    ("sh", "sh FILE [ARGS]: run a script file"),
    (
        "set",
        "set [NAME=VALUE | -- ARGS]: list variables or set them",
    ),
    (":", ":: do nothing, successfully"),
];

impl Shell {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        let mut registry = Registry::standard();
        fn special_stub(_: &mut CmdCtx<'_>) -> i32 {
            0
        }
        for (n, h) in SPECIAL_HELP {
            registry.register(n, h, special_stub);
        }
        Self {
            env: Env::new(),
            limits,
            registry,
            meta: Meta::default(),
            funcs: BTreeMap::new(),
            frames: alloc::vec![Frame {
                name: "sh".to_string(),
                args: Vec::new(),
            }],
            status: 0,
            display: OutBuf::new(limits.max_output),
            pipe_truncated: false,
            steps: 0,
            depth: 0,
            sub_depth: 0,
            ctl: Ctl::None,
            clear: false,
            notified_limit: false,
            interrupted: false,
            last_sub: None,
        }
    }

    /// Add or replace a command (for kernel-specific tools).
    pub fn register(&mut self, name: &str, help: &'static str, f: BuiltinFn) {
        self.registry.register(name, help, f);
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn history(&self) -> &History {
        &self.meta.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.meta.history
    }

    pub fn aliases(&self) -> &BTreeMap<String, String> {
        &self.meta.aliases
    }

    pub fn function_names(&self) -> impl Iterator<Item = &str> {
        self.funcs.keys().map(String::as_str)
    }

    /// `$?` of the last command.
    pub fn last_status(&self) -> i32 {
        self.status
    }

    /// The prompt: `PS1` with `\u` (user), `\h` (host), `\w` (cwd, `~` for
    /// `$HOME`), `\W` (last component), `\$` and `\\` expanded.
    pub fn prompt(&self, fs: &dyn ShellFs, sys: &dyn SysInfo) -> String {
        let ps1 = self.env.get("PS1").unwrap_or("\\w\\$ ");
        let cwd = fs.cwd();
        let home = self.env.get("HOME").unwrap_or("/");
        let shown = if home != "/" && (cwd == home || cwd.starts_with(&alloc::format!("{home}/"))) {
            alloc::format!("~{}", &cwd[home.len()..])
        } else {
            cwd.clone()
        };
        let mut out = String::new();
        let mut it = ps1.chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('u') => out.push_str(self.env.get("USER").unwrap_or("user")),
                Some('h') => out.push_str(&sys.hostname()),
                Some('w') => out.push_str(&shown),
                Some('W') => {
                    let b = super::fs::basename(&cwd);
                    out.push_str(if b.is_empty() { "/" } else { b });
                }
                Some('$') => out.push('$'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        }
        out
    }

    // ---- entry points ----------------------------------------------------

    /// Run one command line typed by the user. Records it in the history.
    pub fn run_line(&mut self, line: &str, h: &mut Host<'_>) -> RunResult {
        if line.len() > self.limits.max_line {
            self.status = 1;
            return RunResult {
                status: 1,
                output: b"sh: line too long\n".to_vec(),
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
                output: b"sh: script too large\n".to_vec(),
                ..RunResult::default()
            };
        }
        self.run_source(src, "script", args, h)
    }

    fn run_source(
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
                    self.say("interrupted");
                }
                self.finish_status(s)
            }
        };
        self.status = status;
        self.end_run(status)
    }

    fn finish_status(&mut self, s: i32) -> i32 {
        if self.interrupted {
            return 130;
        }
        match self.ctl {
            Ctl::Exit(c) | Ctl::Return(c) => c,
            _ => s,
        }
    }

    fn begin_run(&mut self) {
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

    fn end_run(&mut self, status: i32) -> RunResult {
        let exit = match self.ctl {
            Ctl::Exit(c) => Some(c),
            _ => None,
        };
        self.ctl = Ctl::None;
        let mut out = core::mem::take(&mut self.display);
        let truncated = out.truncated || self.pipe_truncated;
        if out.truncated {
            out.truncated = false;
            out.data.extend_from_slice(b"\n[output truncated]\n");
        }
        RunResult {
            status,
            output: out.data,
            exit,
            clear: self.clear,
            truncated,
        }
    }

    fn syntax_error(&mut self, name: &str, src: &str, e: ParseError) {
        let (l, c) = e.line_col(src);
        let msg = alloc::format!(
            "{name}: syntax error at line {l}, column {c}: {}\n",
            e.message()
        );
        self.display.push(msg.as_bytes());
    }

    /// Write `sh: msg` to the display.
    fn say(&mut self, msg: &str) {
        self.display.push(b"sh: ");
        self.display.push(msg.as_bytes());
        self.display.push(b"\n");
    }

    fn tick(&mut self, sys: &dyn SysInfo) -> bool {
        if self.ctl == Ctl::Abort {
            return false;
        }
        if sys.interrupted() {
            self.interrupted = true;
            self.say("interrupted");
            self.ctl = Ctl::Abort;
            return false;
        }
        self.steps += 1;
        if self.steps > self.limits.max_steps {
            if !self.notified_limit {
                self.notified_limit = true;
                self.say("step limit exceeded, aborting");
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

    fn emit(&mut self, out: &mut Out<'_>, bytes: &[u8]) {
        match out {
            Out::Display => self.display.push(bytes),
            Out::Buf(b) => {
                b.push(bytes);
                if b.truncated && !self.pipe_truncated {
                    self.pipe_truncated = true;
                    self.say("pipe buffer limit reached, data dropped");
                }
            }
        }
    }

    // ---- lists, pipelines, compound commands --------------------------------

    fn exec_list(
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

    fn exec_and_or(
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

    fn exec_pipeline(
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
                    self.say("pipe buffer limit reached, data dropped");
                }
                input = buf.data;
            }
            self.status = status;
        }
        status
    }

    fn exec_command(
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
                        self.say("loop iteration limit reached");
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
                        self.say("loop iteration limit reached");
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
                    self.say("too many functions");
                    return 1;
                }
                self.funcs.insert(name.clone(), body.clone());
                0
            }
        }
    }

    /// After a loop body: true when the loop must stop.
    fn loop_control(&mut self) -> bool {
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

    fn exec_simple(
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
                self.say("ambiguous redirect");
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
                    self.say("cannot set variable");
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
    fn dispatch(
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
            self.say(&alloc::format!("{name}: Is a directory"));
            return 126;
        }
        self.say(&alloc::format!("{name}: command not found"));
        127
    }

    fn find_script(&self, name: &str, fs: &dyn ShellFs) -> Option<String> {
        let is_file = |p: &str| fs.stat(p).is_ok_and(|s| s.kind == Kind::File);
        if name.contains('/') {
            return is_file(name).then(|| name.to_string());
        }
        for dir in self.env.path_dirs() {
            for cand in [
                super::fs::join(&dir, name),
                super::fs::join(&dir, &alloc::format!("{name}.sh")),
            ] {
                if is_file(&cand) {
                    return Some(cand);
                }
            }
        }
        None
    }

    fn call_builtin(
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

    fn call_function(
        &mut self,
        body: &Arc<Vec<Stmt>>,
        args: &[String],
        stdin: &[u8],
        out: &mut Out<'_>,
        h: &mut Host<'_>,
    ) -> i32 {
        if self.depth >= self.limits.max_call_depth {
            self.say("function call depth limit exceeded");
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

    fn run_file(
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
            self.say(&alloc::format!("{path}: script too large"));
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
            self.say("script nesting limit exceeded");
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

    fn run_special(
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
                    self.say("exit: numeric argument required");
                    self.ctl = Ctl::Exit(2);
                    2
                }
            },
            "return" => match num(1, self.status) {
                Some(c) => {
                    if self.frames.len() <= 1 && self.depth == 0 {
                        self.say("return: only valid in a function or sourced script");
                        1
                    } else {
                        self.ctl = Ctl::Return(c & 0xFF);
                        c & 0xFF
                    }
                }
                None => {
                    self.say("return: numeric argument required");
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
                    self.say(&alloc::format!("{}: bad loop count", args[0]));
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
                    self.say("shift: bad count");
                    1
                }
            },
            "source" | "." | "sh" => {
                let Some(file) = args.get(1) else {
                    self.say(&alloc::format!("{}: file name required", args[0]));
                    return 2;
                };
                let file = file.clone();
                self.run_file(&file, &args[2..], stdin, out, h)
            }
            "set" => self.run_set(args, out),
            _ => 0,
        }
    }

    fn run_set(&mut self, args: &[String], out: &mut Out<'_>) -> i32 {
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
                Some((n, v)) if super::env::valid_name(n) => {
                    if !self.env.set(n, v) {
                        status = 1;
                    }
                }
                _ => {
                    self.say(&alloc::format!("set: invalid argument `{a}`"));
                    status = 2;
                }
            }
        }
        status
    }

    // ---- expansion --------------------------------------------------------------

    fn lookup(&self, name: &str) -> String {
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

    fn positional(&self) -> Vec<String> {
        self.frames
            .last()
            .map(|f| f.args.clone())
            .unwrap_or_default()
    }

    fn run_sub(&mut self, body: &[Stmt], h: &mut Host<'_>) -> String {
        if self.sub_depth >= self.limits.max_sub_depth {
            self.say("command substitution nested too deeply");
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
            self.say("command substitution output truncated");
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

    fn arith(&mut self, expr: &Word, h: &mut Host<'_>) -> String {
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
    fn part_value(&mut self, p: &Part, h: &mut Host<'_>) -> Option<(String, bool)> {
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
    fn expand_fields(&mut self, w: &Word, h: &mut Host<'_>, globbing: bool) -> Vec<String> {
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
    fn expand_string(&mut self, w: &Word, h: &mut Host<'_>) -> String {
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

fn add_value(
    value: &str,
    quoted: bool,
    fields: &mut Vec<Field>,
    cur: &mut Field,
    started: &mut bool,
) {
    if quoted {
        cur.extend(value.chars().map(|c| (c, false)));
        *started = true;
        return;
    }
    for c in value.chars() {
        if matches!(c, ' ' | '\t' | '\n') {
            if *started || !cur.is_empty() {
                fields.push(core::mem::take(cur));
                *started = false;
            }
        } else {
            cur.push((c, false));
            *started = true;
        }
    }
}
