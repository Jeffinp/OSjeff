use super::*;
use crate::apps::shell::{Host, MemFs, MockSys};
use crate::system::input::{KeyCode, KeyEvent, Mods};
use alloc::string::ToString;
use alloc::vec::Vec;

/// The kernel's loop in miniature: keys in, commands run inline.
struct Rig {
    term: Term,
    shell: Shell,
    fs: MemFs,
    sys: MockSys,
    exited: bool,
}

impl Rig {
    fn new() -> Self {
        let fs = MemFs::new()
            .with_file("/alpha.txt", b"a\n")
            .with_file("/alpine.md", b"b\n")
            .with_dir("/docs")
            .with_file("/docs/readme.txt", b"hi\n");
        let mut shell = Shell::new();
        shell.env.set("PS1", "\\w\\$ ");
        let mut term = Term::new("");
        term.set_prompt(&shell.prompt(&fs, &MockSys::default()));
        term.resize(40, 10);
        Self {
            term,
            shell,
            fs,
            sys: MockSys::default(),
            exited: false,
        }
    }

    fn press(&mut self, ev: KeyEvent) -> TermAction {
        let a = self
            .term
            .key(ev, Some((&self.shell, &self.fs as &dyn ShellFs)));
        if let TermAction::Run(line) = &a {
            let r = {
                let mut h = Host {
                    fs: &mut self.fs,
                    sys: &mut self.sys,
                };
                self.shell.run_line(line, &mut h)
            };
            let p = self.shell.prompt(&self.fs, &self.sys);
            if self.term.finish(&r, &p) == TermAction::Exit {
                self.exited = true;
            }
        }
        if a == TermAction::Exit {
            self.exited = true;
        }
        a
    }

    fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.press(KeyEvent::ch(c));
        }
    }

    fn enter(&mut self) -> TermAction {
        self.press(KeyEvent::plain(KeyCode::Enter))
    }

    fn run(&mut self, line: &str) {
        self.type_text(line);
        self.enter();
    }

    fn rows(&self) -> Vec<String> {
        self.term.view().rows
    }
}

#[test]
fn a_command_echoes_runs_and_shows_the_next_prompt() {
    let mut r = Rig::new();
    r.run("echo hello");
    assert_eq!(r.rows(), ["/$ echo hello", "hello", "/$ "]);
    r.run("cd docs");
    assert_eq!(r.rows().last().unwrap(), "/docs$ ");
    r.run("cat readme.txt");
    let rows = r.rows();
    assert_eq!(
        &rows[rows.len() - 3..],
        ["/docs$ cat readme.txt", "hi", "/docs$ "]
    );
}

#[test]
fn an_empty_line_only_moves_the_prompt() {
    let mut r = Rig::new();
    assert_eq!(r.enter(), TermAction::Redraw);
    assert_eq!(r.rows(), ["/$ ", "/$ "]);
    assert!(!r.term.is_running());
}

#[test]
fn up_and_down_walk_the_history() {
    let mut r = Rig::new();
    r.run("echo one");
    r.run("echo two");
    r.press(KeyEvent::plain(KeyCode::Up));
    assert_eq!(r.term.input(), "echo two");
    r.press(KeyEvent::plain(KeyCode::Up));
    assert_eq!(r.term.input(), "echo one");
    r.press(KeyEvent::plain(KeyCode::Down));
    assert_eq!(r.term.input(), "echo two");
    r.press(KeyEvent::plain(KeyCode::Down));
    assert_eq!(r.term.input(), "");
}

#[test]
fn tab_completes_commands_and_paths() {
    let mut r = Rig::new();
    r.type_text("hist");
    r.press(KeyEvent::plain(KeyCode::Tab));
    assert_eq!(r.term.input(), "history ");
    r.press(KeyEvent::ctrl('u'));
    r.type_text("cat docs/re");
    r.press(KeyEvent::plain(KeyCode::Tab));
    assert_eq!(r.term.input(), "cat docs/readme.txt ");
    // Several candidates: the common prefix first, then the list.
    r.press(KeyEvent::ctrl('u'));
    r.type_text("cat al");
    r.press(KeyEvent::plain(KeyCode::Tab));
    assert_eq!(r.term.input(), "cat alp");
    r.press(KeyEvent::plain(KeyCode::Tab));
    let rows = r.rows();
    assert!(
        rows.iter()
            .any(|l| l.contains("alpha.txt") && l.contains("alpine.md")),
        "{rows:?}"
    );
    // The typed line is echoed above the candidates and the line is intact.
    assert!(rows.iter().any(|l| l == "/$ cat alp"));
    assert_eq!(r.term.input(), "cat alp");
}

#[test]
fn ctrl_c_discards_the_line_and_shows_it_crossed_out() {
    let mut r = Rig::new();
    r.type_text("echo nope");
    r.press(KeyEvent::ctrl('c'));
    assert_eq!(r.term.input(), "");
    assert_eq!(r.rows(), ["/$ echo nope^C", "/$ "]);
    assert!(!r.term.is_running());
}

#[test]
fn ctrl_c_while_running_asks_to_cancel_and_other_keys_wait() {
    let mut r = Rig::new();
    r.type_text("sleep 5");
    let a = r.term.key(
        KeyEvent::plain(KeyCode::Enter),
        Some((&r.shell, &r.fs as &dyn ShellFs)),
    );
    assert_eq!(a, TermAction::Run("sleep 5".to_string()));
    assert!(r.term.is_running());
    // Keys are not queued; Ctrl+C asks the kernel to stop the command.
    assert_eq!(r.term.key(KeyEvent::ch('x'), None), TermAction::None);
    assert_eq!(r.term.key(KeyEvent::ctrl('c'), None), TermAction::Cancel);
    assert_eq!(r.term.input(), "");
    // While it runs there is no prompt on screen.
    assert_eq!(r.rows(), ["/$ sleep 5"]);
    let res = RunResult {
        status: 130,
        output: b"sh: interrupted\n".to_vec(),
        ..RunResult::default()
    };
    assert_eq!(r.term.finish(&res, "/$ "), TermAction::Redraw);
    assert!(!r.term.is_running());
    assert_eq!(r.rows(), ["/$ sleep 5", "sh: interrupted", "/$ "]);
}

#[test]
fn ctrl_l_and_clear_wipe_the_screen() {
    let mut r = Rig::new();
    r.run("echo a");
    r.type_text("par");
    r.press(KeyEvent::ctrl('l'));
    assert_eq!(r.rows(), ["/$ par"], "the typed text stays");
    r.run("echo b");
    r.run("clear");
    assert_eq!(r.rows(), ["/$ "]);
}

#[test]
fn exit_and_ctrl_d_close_the_window() {
    let mut r = Rig::new();
    r.run("exit");
    assert!(r.exited);
    let mut r = Rig::new();
    r.type_text("x");
    assert_eq!(
        r.press(KeyEvent::ctrl('d')),
        TermAction::Redraw,
        "not empty: delete"
    );
    let mut r = Rig::new();
    assert_eq!(r.press(KeyEvent::ctrl('d')), TermAction::Exit);
}

#[test]
fn page_keys_scroll_the_scrollback_and_typing_snaps_back() {
    let mut r = Rig::new();
    r.run("seq 100");
    let bottom = r.rows();
    assert_eq!(bottom.len(), 10);
    assert_eq!(bottom.last().unwrap(), "/$ ");
    r.press(KeyEvent::plain(KeyCode::PageUp));
    let up = r.rows();
    assert_ne!(up, bottom);
    assert!(r.term.screen.is_scrolled());
    assert_eq!(
        up.last().unwrap(),
        "92",
        "9 rows older than the bottom view"
    );
    r.press(KeyEvent::plain(KeyCode::PageDown));
    assert_eq!(r.rows(), bottom);
    // Ctrl+Home goes to the oldest output, Ctrl+End back; a typed key too.
    r.press(KeyEvent::new(KeyCode::Home, Mods::CTRL));
    assert_eq!(r.rows()[0], "/$ seq 100");
    r.press(KeyEvent::new(KeyCode::End, Mods::CTRL));
    assert_eq!(r.rows(), bottom);
    r.press(KeyEvent::plain(KeyCode::PageUp));
    r.type_text("x");
    assert!(!r.term.screen.is_scrolled());
    assert_eq!(r.rows().last().unwrap(), "/$ x");
    // The wheel path.
    r.term.scroll_rows(3);
    assert!(r.term.screen.is_scrolled());
}

#[test]
fn paste_never_submits() {
    let mut r = Rig::new();
    r.term.paste("echo a\necho b\r\n\x07x");
    assert!(!r.term.is_running());
    assert_eq!(r.term.input(), "echo a echo b   x");
    // Pasting while a command runs is ignored.
    let mut r = Rig::new();
    r.type_text("echo q");
    r.term.key(
        KeyEvent::plain(KeyCode::Enter),
        Some((&r.shell, &r.fs as &dyn ShellFs)),
    );
    r.term.paste("zzz");
    assert_eq!(r.term.input(), "");
}

#[test]
fn a_ten_thousand_line_output_stays_bounded_and_scrollable() {
    let mut r = Rig::new();
    r.run("seq 10000");
    assert!(r.term.screen.line_count() <= crate::apps::shell::screen::MAX_LINES);
    let rows = r.rows();
    assert_eq!(rows[rows.len() - 2], "10000");
    r.press(KeyEvent::new(KeyCode::Home, Mods::CTRL));
    assert!(
        r.rows().iter().any(|l| l.parse::<u32>().is_ok()),
        "oldest kept line is a number"
    );
}

#[test]
fn narrow_windows_wrap_the_live_line_and_keep_the_caret_visible() {
    let mut r = Rig::new();
    r.term.resize(8, 4);
    r.type_text("echo 0123456789");
    let v = r.term.view();
    assert_eq!(v.rows, ["/$ echo ", "01234567", "89"].map(String::from));
    assert_eq!(v.cursor, Some((2, 2)));
    r.term.resize(1, 1);
    let v = r.term.view();
    assert_eq!(v.rows.len(), 1);
}

#[test]
fn finish_with_stale_scroll_returns_to_the_bottom() {
    let mut r = Rig::new();
    r.run("seq 30");
    r.press(KeyEvent::plain(KeyCode::PageUp));
    r.run("echo done");
    assert!(!r.term.screen.is_scrolled());
    assert_eq!(r.rows()[r.rows().len() - 2], "done");
}

#[test]
fn the_prompt_can_be_read_back() {
    let mut t = Term::new("/home $ ");
    assert_eq!(t.prompt(), "/home $ ");
    t.set_prompt("/ $ ");
    assert_eq!(t.prompt(), "/ $ ");
}
