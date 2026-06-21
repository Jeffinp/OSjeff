//! Text terminal / command shell. Fixed-capacity, allocation-free.
//!
//! Owns a scrollback ring, an editable input line with a caret, and a small
//! command interpreter. All effects are returned as [`Action`] so the kernel
//! decides how to react (e.g. opening the editor).

use crate::keymap::Key;

/// Visible columns per line.
pub const COLS: usize = 40;
/// Scrollback rows kept in the ring.
pub const ROWS: usize = 14;
/// Max characters typed on the input line.
pub const INPUT_MAX: usize = 32;
/// Command prompt prefix.
pub const PROMPT: &[u8] = b"OSJEFF> ";

/// A clock reading injected into time-dependent commands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Time {
    pub h: u8,
    pub m: u8,
    pub s: u8,
}

/// Maximum file-name length carried by a terminal action.
pub const FNAME_MAX: usize = 16;

/// A fixed-capacity file name parsed from a command argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileName {
    bytes: [u8; FNAME_MAX],
    len: usize,
}

impl FileName {
    /// Parse `arg` (already trimmed by the caller) into a name, or `None` if it
    /// is empty or too long.
    pub fn parse(arg: &[u8]) -> Option<Self> {
        let arg = trim(arg);
        if arg.is_empty() || arg.len() > FNAME_MAX {
            return None;
        }
        let mut bytes = [0u8; FNAME_MAX];
        bytes[..arg.len()].copy_from_slice(arg);
        Some(Self {
            bytes,
            len: arg.len(),
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Side effect requested by the terminal after handling input. Filesystem
/// actions are executed by the desktop (which owns the disk) and their output
/// is printed back via [`Terminal::println`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    OpenEditor,
    OpenTasks,
    OpenCalc,
    Reboot,
    Shutdown,
    List,
    Save(FileName),
    Load(FileName),
    Cat(FileName),
    Remove(FileName),
}

pub struct Terminal {
    lines: [[u8; COLS]; ROWS],
    line_len: [usize; ROWS],
    count: usize,
    input: [u8; INPUT_MAX],
    input_len: usize,
    caret: usize,
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal {
    pub fn new() -> Self {
        let mut t = Self {
            lines: [[0; COLS]; ROWS],
            line_len: [0; ROWS],
            count: 0,
            input: [0; INPUT_MAX],
            input_len: 0,
            caret: 0,
        };
        t.println(b"OSJEFF shell ready.");
        t.println(b"Type HELP for commands.");
        t
    }

    // ---- scrollback ----

    /// Number of populated scrollback rows.
    pub fn row_count(&self) -> usize {
        self.count
    }

    /// Borrow scrollback row `i` (`0..row_count`).
    pub fn row(&self, i: usize) -> &[u8] {
        &self.lines[i][..self.line_len[i]]
    }

    /// Current input text (without the prompt).
    pub fn input(&self) -> &[u8] {
        &self.input[..self.input_len]
    }

    /// Caret index within the input (`0..=input_len`).
    pub fn caret(&self) -> usize {
        self.caret
    }

    fn push_raw(&mut self, b: &[u8]) {
        let n = b.len().min(COLS);
        let idx = if self.count < ROWS {
            let i = self.count;
            self.count += 1;
            i
        } else {
            for i in 1..ROWS {
                self.lines[i - 1] = self.lines[i];
                self.line_len[i - 1] = self.line_len[i];
            }
            ROWS - 1
        };
        self.lines[idx] = [0; COLS];
        self.lines[idx][..n].copy_from_slice(&b[..n]);
        self.line_len[idx] = n;
    }

    /// Append a line, wrapping at [`COLS`]. Empty input yields one blank row.
    pub fn println(&mut self, text: &[u8]) {
        if text.is_empty() {
            self.push_raw(b"");
            return;
        }
        let mut i = 0;
        while i < text.len() {
            let n = (text.len() - i).min(COLS);
            self.push_raw(&text[i..i + n]);
            i += n;
        }
    }

    fn clear(&mut self) {
        self.count = 0;
        self.line_len = [0; ROWS];
    }

    // ---- input editing ----

    fn insert(&mut self, ch: u8) {
        if self.input_len >= INPUT_MAX {
            return;
        }
        let mut i = self.input_len;
        while i > self.caret {
            self.input[i] = self.input[i - 1];
            i -= 1;
        }
        self.input[self.caret] = ch;
        self.input_len += 1;
        self.caret += 1;
    }

    /// Feed a key. Returns the requested side effect.
    pub fn on_key(&mut self, key: Key, time: Time) -> Action {
        match key {
            Key::Char(c) => {
                self.insert(c);
                Action::None
            }
            Key::Backspace => {
                self.delete_before_caret();
                Action::None
            }
            Key::Delete => {
                self.delete_at_caret();
                Action::None
            }
            Key::Left => {
                if self.caret > 0 {
                    self.caret -= 1;
                }
                Action::None
            }
            Key::Right => {
                if self.caret < self.input_len {
                    self.caret += 1;
                }
                Action::None
            }
            Key::Home => {
                self.caret = 0;
                Action::None
            }
            Key::End => {
                self.caret = self.input_len;
                Action::None
            }
            Key::Enter => self.submit(time),
            _ => Action::None,
        }
    }

    fn delete_before_caret(&mut self) {
        if self.caret == 0 {
            return;
        }
        for i in (self.caret - 1)..(self.input_len - 1) {
            self.input[i] = self.input[i + 1];
        }
        self.input_len -= 1;
        self.caret -= 1;
    }

    fn delete_at_caret(&mut self) {
        if self.caret >= self.input_len {
            return;
        }
        for i in self.caret..(self.input_len - 1) {
            self.input[i] = self.input[i + 1];
        }
        self.input_len -= 1;
    }

    fn submit(&mut self, time: Time) -> Action {
        // Echo "PROMPT + input" to scrollback.
        let mut line = [0u8; PROMPT.len() + INPUT_MAX];
        line[..PROMPT.len()].copy_from_slice(PROMPT);
        line[PROMPT.len()..PROMPT.len() + self.input_len]
            .copy_from_slice(&self.input[..self.input_len]);
        self.println(&line[..PROMPT.len() + self.input_len]);

        // Copy the command out before mutating scrollback (avoids aliasing).
        let mut cmd = [0u8; INPUT_MAX];
        let len = self.input_len;
        cmd[..len].copy_from_slice(&self.input[..len]);

        self.input_len = 0;
        self.caret = 0;
        self.run(&cmd[..len], time)
    }

    fn run(&mut self, raw: &[u8], time: Time) -> Action {
        let cmd = trim(raw);
        if cmd.is_empty() {
            return Action::None;
        }

        let mut tok = [0u8; 8];
        let tn = upper_token(cmd, &mut tok);
        let token = &tok[..tn];
        let rest = arg_after_space(cmd);

        if token == b"HELP" {
            self.println(b"Commands:");
            self.println(b" HELP CLS TIME VER ECHO");
            self.println(b" EDIT CALC PS LS CAT");
            self.println(b" SAVE LOAD RM REBOOT SHUTDOWN");
            self.println(b"LS/CAT/SAVE/LOAD/RM = files");
        } else if token == b"LS" || token == b"DIR" {
            return Action::List;
        } else if token == b"SAVE" {
            return self.file_action(rest, Action::Save);
        } else if token == b"LOAD" || token == b"OPEN" {
            return self.file_action(rest, Action::Load);
        } else if token == b"CAT" || token == b"TYPE" {
            return self.file_action(rest, Action::Cat);
        } else if token == b"RM" || token == b"DEL" {
            return self.file_action(rest, Action::Remove);
        } else if token == b"PS" || token == b"TASK" || token == b"TASKS" {
            self.println(b"Opening task manager...");
            return Action::OpenTasks;
        } else if token == b"CALC" {
            self.println(b"Launching calculator...");
            return Action::OpenCalc;
        } else if token == b"REBOOT" || token == b"RESTART" {
            self.println(b"Rebooting...");
            return Action::Reboot;
        } else if token == b"SHUTDOWN" || token == b"POWEROFF" {
            self.println(b"Shutting down...");
            return Action::Shutdown;
        } else if token == b"CLS" || token == b"CLEAR" {
            self.clear();
        } else if token == b"VER" || token == b"ABOUT" {
            self.println(b"OSJEFF 0.2 - Rust bare metal");
        } else if token == b"TIME" {
            let mut out = [0u8; 12];
            out[..4].copy_from_slice(b"Now ");
            two(&mut out, 4, time.h);
            out[6] = b':';
            two(&mut out, 7, time.m);
            out[9] = b':';
            two(&mut out, 10, time.s);
            self.println(&out);
        } else if token == b"ECHO" {
            self.println(rest);
        } else if token == b"EDIT" || token == b"EDITOR" {
            self.println(b"Launching editor...");
            return Action::OpenEditor;
        } else {
            self.println(b"Unknown command. Type HELP.");
        }
        Action::None
    }

    /// Parse a single file-name argument into a filesystem action, printing a
    /// usage hint when it is missing or invalid.
    fn file_action(&mut self, arg: &[u8], make: fn(FileName) -> Action) -> Action {
        match FileName::parse(arg) {
            Some(f) => make(f),
            None => {
                self.println(b"usage: <cmd> <name>");
                Action::None
            }
        }
    }
}

// ---- byte helpers ----

fn two(buf: &mut [u8], idx: usize, val: u8) {
    buf[idx] = b'0' + (val / 10) % 10;
    buf[idx + 1] = b'0' + val % 10;
}

fn ascii_upper(c: u8) -> u8 {
    if c.is_ascii_lowercase() { c - 32 } else { c }
}

/// Copy the first whitespace-delimited token, uppercased, into `out`.
fn upper_token(cmd: &[u8], out: &mut [u8]) -> usize {
    let mut n = 0;
    while n < cmd.len() && cmd[n] != b' ' && n < out.len() {
        out[n] = ascii_upper(cmd[n]);
        n += 1;
    }
    n
}

/// Everything after the first space (the argument), or empty.
fn arg_after_space(cmd: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < cmd.len() && cmd[i] != b' ' {
        i += 1;
    }
    while i < cmd.len() && cmd[i] == b' ' {
        i += 1;
    }
    &cmd[i..]
}

/// Trim leading and trailing spaces.
fn trim(s: &[u8]) -> &[u8] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && s[a] == b' ' {
        a += 1;
    }
    while b > a && s[b - 1] == b' ' {
        b -= 1;
    }
    &s[a..b]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Time {
        Time { h: 5, m: 9, s: 7 }
    }

    fn type_str(term: &mut Terminal, s: &str) {
        for &b in s.as_bytes() {
            term.on_key(Key::Char(b), t0());
        }
    }

    fn last_row(term: &Terminal) -> &[u8] {
        term.row(term.row_count() - 1)
    }

    #[test]
    fn banner_on_start() {
        let t = Terminal::new();
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.row(0), b"OSJEFF shell ready.");
    }

    #[test]
    fn ls_returns_list_action() {
        let mut t = Terminal::new();
        type_str(&mut t, "ls");
        assert_eq!(t.on_key(Key::Enter, t0()), Action::List);
    }

    #[test]
    fn save_parses_filename() {
        let mut t = Terminal::new();
        type_str(&mut t, "save notes.txt");
        let a = t.on_key(Key::Enter, t0());
        match a {
            Action::Save(f) => assert_eq!(f.as_bytes(), b"notes.txt"),
            _ => panic!("expected Save"),
        }
    }

    #[test]
    fn file_command_without_name_prints_usage() {
        let mut t = Terminal::new();
        type_str(&mut t, "cat");
        assert_eq!(t.on_key(Key::Enter, t0()), Action::None);
        assert_eq!(last_row(&t), b"usage: <cmd> <name>");
    }

    #[test]
    fn load_and_rm_and_cat_actions() {
        for (cmd, want) in [
            ("load a", Action::Load(FileName::parse(b"a").unwrap())),
            ("rm b", Action::Remove(FileName::parse(b"b").unwrap())),
            ("cat c", Action::Cat(FileName::parse(b"c").unwrap())),
        ] {
            let mut t = Terminal::new();
            type_str(&mut t, cmd);
            assert_eq!(t.on_key(Key::Enter, t0()), want);
        }
    }

    #[test]
    fn app_and_power_commands_map_to_actions() {
        for (cmd, want) in [
            ("calc", Action::OpenCalc),
            ("edit", Action::OpenEditor),
            ("editor", Action::OpenEditor),
            ("tasks", Action::OpenTasks),
            ("ps", Action::OpenTasks),
            ("reboot", Action::Reboot),
            ("restart", Action::Reboot),
            ("shutdown", Action::Shutdown),
            ("poweroff", Action::Shutdown),
        ] {
            let mut t = Terminal::new();
            type_str(&mut t, cmd);
            assert_eq!(t.on_key(Key::Enter, t0()), want, "command: {cmd}");
        }
    }

    #[test]
    fn typing_builds_input_and_caret() {
        let mut t = Terminal::new();
        type_str(&mut t, "hi");
        assert_eq!(t.input(), b"hi");
        assert_eq!(t.caret(), 2);
    }

    #[test]
    fn input_capacity_is_capped() {
        let mut t = Terminal::new();
        for _ in 0..(INPUT_MAX + 10) {
            t.on_key(Key::Char(b'x'), t0());
        }
        assert_eq!(t.input().len(), INPUT_MAX);
    }

    #[test]
    fn caret_movement_and_mid_insert() {
        let mut t = Terminal::new();
        type_str(&mut t, "ac");
        t.on_key(Key::Left, t0()); // caret between a|c
        t.on_key(Key::Char(b'b'), t0());
        assert_eq!(t.input(), b"abc");
        assert_eq!(t.caret(), 2);
    }

    #[test]
    fn home_end_bounds() {
        let mut t = Terminal::new();
        type_str(&mut t, "abc");
        t.on_key(Key::Home, t0());
        assert_eq!(t.caret(), 0);
        t.on_key(Key::Left, t0()); // clamp
        assert_eq!(t.caret(), 0);
        t.on_key(Key::End, t0());
        assert_eq!(t.caret(), 3);
        t.on_key(Key::Right, t0()); // clamp
        assert_eq!(t.caret(), 3);
    }

    #[test]
    fn backspace_and_delete_mid_line() {
        let mut t = Terminal::new();
        type_str(&mut t, "abc");
        t.on_key(Key::Backspace, t0()); // -> "ab"
        assert_eq!(t.input(), b"ab");
        t.on_key(Key::Home, t0());
        t.on_key(Key::Delete, t0()); // remove 'a' -> "b"
        assert_eq!(t.input(), b"b");
        assert_eq!(t.caret(), 0);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut t = Terminal::new();
        type_str(&mut t, "a");
        t.on_key(Key::Home, t0());
        t.on_key(Key::Backspace, t0());
        assert_eq!(t.input(), b"a");
    }

    #[test]
    fn enter_echoes_prompt_and_clears_input() {
        let mut t = Terminal::new();
        type_str(&mut t, "ECHO hi");
        t.on_key(Key::Enter, t0());
        assert_eq!(t.input().len(), 0);
        assert_eq!(t.caret(), 0);
        // last row is the echo output "hi"; prompt line is before it.
        assert_eq!(last_row(&t), b"hi");
    }

    #[test]
    fn echo_with_no_arg_prints_blank() {
        let mut t = Terminal::new();
        type_str(&mut t, "ECHO");
        t.on_key(Key::Enter, t0());
        assert_eq!(last_row(&t), b"");
    }

    #[test]
    fn help_lists_commands() {
        let mut t = Terminal::new();
        type_str(&mut t, "help");
        t.on_key(Key::Enter, t0());
        // case-insensitive: lowercase still matches
        assert_eq!(t.row(t.row_count() - 1), b"LS/CAT/SAVE/LOAD/RM = files");
    }

    #[test]
    fn cls_clears_scrollback() {
        let mut t = Terminal::new();
        type_str(&mut t, "CLS");
        t.on_key(Key::Enter, t0());
        assert_eq!(t.row_count(), 0);
    }

    #[test]
    fn time_command_formats_utc() {
        let mut t = Terminal::new();
        type_str(&mut t, "TIME");
        t.on_key(Key::Enter, Time { h: 5, m: 9, s: 7 });
        assert_eq!(last_row(&t), b"Now 05:09:07");
    }

    #[test]
    fn ver_prints_version() {
        let mut t = Terminal::new();
        type_str(&mut t, "VER");
        t.on_key(Key::Enter, t0());
        assert_eq!(last_row(&t), b"OSJEFF 0.2 - Rust bare metal");
    }

    #[test]
    fn edit_returns_open_editor_action() {
        let mut t = Terminal::new();
        type_str(&mut t, "EDIT");
        let a = t.on_key(Key::Enter, t0());
        assert_eq!(a, Action::OpenEditor);
    }

    #[test]
    fn ps_and_task_return_open_tasks_action() {
        let mut t = Terminal::new();
        type_str(&mut t, "PS");
        assert_eq!(t.on_key(Key::Enter, t0()), Action::OpenTasks);
        type_str(&mut t, "task");
        assert_eq!(t.on_key(Key::Enter, t0()), Action::OpenTasks);
    }

    #[test]
    fn unknown_command() {
        let mut t = Terminal::new();
        type_str(&mut t, "frobnicate");
        t.on_key(Key::Enter, t0());
        assert_eq!(last_row(&t), b"Unknown command. Type HELP.");
    }

    #[test]
    fn blank_enter_is_noop_command() {
        let mut t = Terminal::new();
        let before = t.row_count();
        t.on_key(Key::Enter, t0()); // empty input -> only prompt echoed
        assert_eq!(t.row_count(), before + 1);
        assert_eq!(last_row(&t), PROMPT);
    }

    #[test]
    fn scrollback_ring_scrolls() {
        let mut t = Terminal::new();
        for i in 0..(ROWS + 5) {
            let mut buf = [b'L'; 1];
            buf[0] = b'0' + (i % 10) as u8;
            t.println(&buf);
        }
        assert_eq!(t.row_count(), ROWS);
    }

    #[test]
    fn long_line_wraps() {
        let mut t = Terminal::new();
        let long = [b'z'; COLS + 5];
        let before = t.row_count();
        t.println(&long);
        assert_eq!(t.row_count(), before + 2); // wrapped into two rows
    }

    #[test]
    fn leading_spaces_trimmed_for_command() {
        let mut t = Terminal::new();
        type_str(&mut t, "   VER");
        t.on_key(Key::Enter, t0());
        assert_eq!(last_row(&t), b"OSJEFF 0.2 - Rust bare metal");
    }

    #[test]
    fn non_text_keys_are_ignored() {
        let mut t = Terminal::new();
        assert_eq!(t.on_key(Key::Tab, t0()), Action::None);
        assert_eq!(t.on_key(Key::Esc, t0()), Action::None);
        assert_eq!(t.on_key(Key::Up, t0()), Action::None);
    }
}
