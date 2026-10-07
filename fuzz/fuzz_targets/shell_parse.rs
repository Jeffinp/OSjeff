//! Fuzz target: the shell engine (`osjeff_core::shell`).
//!
//! Arbitrary bytes (decoded lossily, so invalid UTF-8 becomes U+FFFD) go through
//! three layers:
//!
//! 1. the lexer/parser alone (`parse::parse`), which must return a typed error
//!    and never panic or recurse without bound;
//! 2. the executor: the input runs as one script and each NUL-separated chunk
//!    runs as a command line, in one shell with persistent state, against an
//!    in-memory filesystem with tight limits and a mock `SysInfo`. Output, pipes,
//!    steps, loops and recursion are capped, so every input must finish quickly;
//! 3. the line editor with Tab completion, driven by keys derived from the
//!    bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use osjeff_core::input::{KeyCode, KeyEvent, Mods};
use osjeff_core::shell::fs::{MemFs, ShellFs};
use osjeff_core::shell::line::{LineEditor, ShellCompleter};
use osjeff_core::shell::{Host, Limits, MockSys, Shell};

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
    let mut sys = MockSys::default();
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
        let b = pair[0];
        let m = pair.get(1).copied().unwrap_or(0);
        let code = match b % 24 {
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
            _ => KeyCode::Char(char::from(b)),
        };
        let mods = Mods {
            ctrl: m & 1 != 0,
            shift: m & 2 != 0,
            alt: m & 4 != 0 && m & 8 == 0,
        };
        let comp = ShellCompleter {
            shell: &sh,
            fs: &fs,
        };
        let _ = ed.handle_key(KeyEvent::new(code, mods), sh.history(), &comp);
        assert!(ed.cursor() <= ed.text().chars().count());
        let _ = ed.display();
    }
});
