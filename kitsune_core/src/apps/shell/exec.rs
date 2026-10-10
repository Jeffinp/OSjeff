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
use crate::{t, tk};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

mod calls;
mod expand;
mod pipeline;
mod run;

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
    /// Sorted `(name, one-line help)`, the help in the language in effect.
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

    /// Add a command. `help` is the catalog key of its one-line help (synopsis and description),
    /// looked up in the language in effect whenever the help is shown.
    pub fn register(&mut self, name: &str, help: &'static str, f: BuiltinFn) {
        self.map.insert(name.to_string(), (f, help));
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    pub fn help_of(&self, name: &str) -> Option<&'static str> {
        self.map.get(name).map(|e| crate::i18n::tr(e.1))
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
        self.map
            .iter()
            .map(|(k, v)| (k.clone(), crate::i18n::tr(v.1)))
            .collect()
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
/// The help is a catalog key; the text follows the language when `help` runs.
pub const SPECIAL_HELP: [(&str, &str); 10] = [
    ("exit", tk!("sh.exit.help")),
    ("return", tk!("sh.return.help")),
    ("break", tk!("sh.break.help")),
    ("continue", tk!("sh.continue.help")),
    ("shift", tk!("sh.shift.help")),
    ("source", tk!("sh.source.help")),
    (".", tk!("sh.dot.help")),
    ("sh", tk!("sh.sh.help")),
    ("set", tk!("sh.set.help")),
    (":", tk!("sh.colon.help")),
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
