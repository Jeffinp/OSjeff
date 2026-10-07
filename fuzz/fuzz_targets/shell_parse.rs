//! Fuzz target: the shell engine (`osjeff_core::shell`).
//!
//! Arbitrary bytes (decoded lossily, so invalid UTF-8 becomes U+FFFD) go through
//! four layers:
//!
//! 1. the lexer/parser alone (`parse::parse`), which must return a typed error
//!    and never panic or recurse without bound;
//! 2. the executor: the input runs as one script and each NUL-separated chunk
//!    runs as a command line, in one shell with persistent state, against an
//!    in-memory filesystem with tight limits and a mock `SysInfo` (with a DNS
//!    answer, a web page and sometimes a Ctrl+C after a few polls). Output,
//!    pipes, steps, loops and recursion are capped, so every input must finish
//!    quickly;
//! 3. the line editor with Tab completion, driven by keys derived from the
//!    bytes;
//! 4. a whole terminal session (`Term`: scrollback, history, Ctrl+C, paste, Tab)
//!    driven by the same keys at a random window size, checking that what it would
//!    draw always fits the window; and `Screen::print` on the raw bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use osjeff_core::input::{KeyCode, KeyEvent, Mods};
use osjeff_core::shell::fs::{MemFs, ShellFs};
use osjeff_core::shell::line::{LineEditor, ShellCompleter};
use osjeff_core::shell::sys::HttpResponse;
use osjeff_core::shell::{Host, Limits, MockSys, Screen, Shell, Term, TermAction};

const MAX_INPUT: usize = 4096;

fn small_limits() -> Limits {
    Limits {
        max_output: 4096,
        max_pipe: 4096,
        max_steps: 600,
        max_loop: 100,
        max_call_depth: 8,
        max_sub_depth: 4,
        max_glob: 64,
        max_line: 2048,
        max_script: 4096,
        max_sleep_ms: 5,
    }
}

fn fresh_fs() -> MemFs {
    let mut fs = MemFs::small();
    let _ = fs.mkdir("/bin");
    let _ = fs.mkdir("/d");
    let _ = fs.write("/a.txt", b"alpha\nbeta\ngamma\n");
    let _ = fs.write("/d/x.md", b"# x\n");
    let _ = fs.write("/bin/tool.sh", b"echo tool $1\nexit 3\n");
    fs
}

fn fresh_sys(data: &[u8]) -> MockSys {
    let mut sys = MockSys::default();
    sys.dns.push(("a.org".to_string(), [10, 0, 0, 1]));
    sys.web.push((
        "http://a.org/".to_string(),
        HttpResponse {
            status: 200,
            head: b"HTTP/1.1 200 OK\r\n\r\n".to_vec(),
            body: data.to_vec(),
            truncated: false,
        },
    ));
    // Some inputs get a Ctrl+C after a few polls of the interrupt flag.
    sys.interrupt_after = data
        .first()
        .filter(|b| **b % 5 == 0)
        .map(|b| u32::from(*b));
    sys
}

/// A key from two bytes.
fn key_of(pair: &[u8]) -> KeyEvent {
    let b = pair[0];
    let m = pair.get(1).copied().unwrap_or(0);
    let code = match b % 28 {
        0 => KeyCode::Enter,
        1 => KeyCode::Backspace,
        2 => KeyCode::Delete,
        3 => KeyCode::Tab,
        4 => KeyCode::Esc,
        5 => KeyCode::Left,
        6 => KeyCode::Right,
        7 => KeyCode::Up,
        8 => KeyCode::Down,
        9 => KeyCode::Home,
        10 => KeyCode::End,
        11 => KeyCode::PageUp,
        12 => KeyCode::PageDown,
        _ => KeyCode::Char(char::from(b)),
    };
    let mods = Mods {
        ctrl: m & 1 != 0,
        shift: m & 2 != 0,
        alt: m & 4 != 0 && m & 8 == 0,
    };
    KeyEvent::new(code, mods)
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let text = String::from_utf8_lossy(data).into_owned();

    // 1. Parser only.
    if let Err(e) = osjeff_core::shell::parse::parse(&text) {
        // The error must point inside the input and describe itself.
        assert!(e.pos <= text.len());
        assert!(!e.message().is_empty());
        let _ = e.line_col(&text);
    }

    // 2. Executor.
    let mut sh = Shell::with_limits(small_limits());
    let mut fs = fresh_fs();
    let mut sys = fresh_sys(data);
    {
        let mut host = Host {
            fs: &mut fs,
            sys: &mut sys,
        };
        let r = sh.run_script(&text, &["a".to_string(), "b c".to_string()], &mut host);
        assert!(r.output.len() <= 4096 + 64);
        for chunk in text.split('\0').take(16) {
            let r = sh.run_line(chunk, &mut host);
            assert!(r.output.len() <= 4096 + 64);
        }
    }
    // The cwd must still be a real directory whatever the script did.
    assert!(fs.stat(&fs.cwd()).is_ok());

    // 3. Line editor + completion.
    let mut ed = LineEditor::new(&sh.prompt(&fs, &sys));
    for pair in data.chunks(2).take(512) {
        let comp = ShellCompleter {
            shell: &sh,
            fs: &fs,
        };
        let _ = ed.handle_key(key_of(pair), sh.history(), &comp);
        assert!(ed.cursor() <= ed.text().chars().count());
        let _ = ed.display();
    }

    // 4. A terminal session at a random window size.
    let cols = 1 + usize::from(data.first().copied().unwrap_or(0)) % 60;
    let rows = 1 + usize::from(data.get(1).copied().unwrap_or(0)) % 20;
    let mut sh = Shell::with_limits(small_limits());
    let mut fs = fresh_fs();
    let mut sys = fresh_sys(data);
    let mut term = Term::new(&sh.prompt(&fs, &sys));
    term.resize(cols, rows);
    for pair in data.chunks(2).take(256) {
        let action = term.key(key_of(pair), Some((&sh, &fs as &dyn ShellFs)));
        if let TermAction::Run(line) = action {
            let r = {
                let mut host = Host {
                    fs: &mut fs,
                    sys: &mut sys,
                };
                sh.run_line(&line, &mut host)
            };
            let prompt = sh.prompt(&fs, &sys);
            let _ = term.finish(&r, &prompt);
        }
        let v = term.view();
        assert!(v.rows.len() <= rows);
        for row in &v.rows {
            assert!(row.chars().count() <= cols);
        }
        if let Some((r, c)) = v.cursor {
            assert!(r < v.rows.len() && c < cols);
        }
        if data.len() % 3 == 0 {
            term.paste(&text);
        }
    }
    let mut screen = Screen::new();
    for chunk in data.chunks(7) {
        screen.print(chunk);
    }
    let v = screen.view(cols, rows, "$ ", 2);
    assert!(v.rows.len() <= rows);
    screen.scroll(isize::MAX / 2, cols, rows, 3);
    let _ = screen.view(cols, rows, "$ ", 2);
});
