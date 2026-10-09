use super::*;

#[test]
fn random_command_lines_never_panic_or_hang() {
    let atoms = [
        "echo", "ls", "cat", "cd", "rm", "-r", "mkdir", "mv", "cp", "touch", "head", "tail", "wc",
        "grep", "sort", "uniq", "tee", "seq", "yes", "sleep", "kill", "test", "[", "]", "if",
        "then", "else", "fi", "for", "in", "do", "done", "while", "until", "x", "y", "$x", "$(",
        ")", "${x}", "$((1+1))", "|", "||", "&&", ";", "\n", ">", ">>", "<", "'", "\"", "*", "?",
        "/", "..", ".", "a", "b.txt", "f() {", "}", "f", "break", "continue", "return", "exit 0",
        "shift", "set", "alias", "unalias", "export", "unset", "history", "which", "date", "free",
        "df", "ps", "ping", "-n", "-c", "1", "2", "=", "x=1", "$?", "$@", "$#", "~", "\\",
    ];
    let mut x: u64 = 0xC0FFEE;
    for _ in 0..600 {
        let mut line = String::new();
        for _ in 0..(1 + xorshift(&mut x) % 14) {
            line.push_str(atoms[(xorshift(&mut x) % atoms.len() as u64) as usize]);
            line.push(' ');
        }
        let mut sh = Shell::new();
        sh.limits.max_steps = 2000;
        sh.limits.max_output = 4096;
        sh.limits.max_pipe = 4096;
        let mut fs = MemFs::small();
        fs.write("/a.txt", b"one\ntwo\n").unwrap();
        fs.write("/b.txt", b"three\n").unwrap();
        let mut sys = MockSys::default();
        let mut h = Host {
            fs: &mut fs,
            sys: &mut sys,
        };
        let r = sh.run_line(&line, &mut h);
        assert!(r.output.len() <= 4096 + 64, "{line:?}");
    }
}

#[test]
fn random_scripts_with_loops_stay_bounded() {
    let parts = [
        "while true; do echo x; done",
        "for i in $(seq 1 100000); do echo $i; done",
        "f() { f; f; }; f",
        "until false; do :; done",
        "while [ 1 -eq 1 ]; do x=$x$x; done",
        "yes | head -n 1",
        "yes | wc -l",
        "echo $(yes)",
        "cat /dev/zero",
    ];
    for p in parts {
        let mut sh = Shell::new();
        sh.limits.max_steps = 3000;
        sh.limits.max_output = 2000;
        sh.limits.max_pipe = 2000;
        let mut fs = MemFs::small();
        let mut sys = MockSys::default();
        let mut h = Host {
            fs: &mut fs,
            sys: &mut sys,
        };
        let r = sh.run_line(p, &mut h);
        assert!(r.output.len() < 2200, "{p}: {}", r.output.len());
    }
}

#[test]
fn worst_case_nesting_fits_a_256_kib_stack() {
    on_small_stack(256, || {
        let mut t = T::new();
        // Deepest recursion the limits allow, then the deepest parse nesting.
        let r = t.script("f() { f; }\nf", &[]);
        assert!(r.text().contains("depth limit"));
        let mut deep = String::new();
        for _ in 0..22 {
            deep.push_str("if true; then ");
        }
        deep.push_str("echo deep");
        for _ in 0..22 {
            deep.push_str("; fi");
        }
        assert_eq!(t.out(&deep), "deep\n");
        let r = t.script(
            "g() { if true; then for i in 1; do while true; do echo $(echo $(echo ok)); break; done; done; fi; g; }\ng",
            &[],
        );
        assert!(r.text().starts_with("ok\n"));
    });
}

#[test]
fn arbitrary_bytes_never_panic_the_whole_pipeline() {
    let mut x: u64 = 7777;
    for _ in 0..500 {
        let mut bytes = Vec::new();
        for _ in 0..(xorshift(&mut x) % 50) {
            bytes.push((xorshift(&mut x) >> 3) as u8);
        }
        let line = String::from_utf8_lossy(&bytes).into_owned();
        let mut sh = Shell::new();
        sh.limits.max_steps = 500;
        let mut fs = MemFs::small();
        let mut sys = MockSys::default();
        let mut h = Host {
            fs: &mut fs,
            sys: &mut sys,
        };
        let _ = sh.run_line(&line, &mut h);
    }
}
