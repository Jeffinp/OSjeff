//! End-to-end tests of the executor and builtins against [`MemFs`] and
//! [`MockSys`].

use super::*;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

struct T {
    sh: Shell,
    fs: MemFs,
    sys: MockSys,
}

impl T {
    fn new() -> Self {
        Self {
            sh: Shell::new(),
            fs: MemFs::new(),
            sys: MockSys::default(),
        }
    }

    fn with_files() -> Self {
        let mut t = Self::new();
        t.fs = MemFs::new()
            .with_file("/a.txt", b"alpha\nbeta\n")
            .with_file("/b.txt", b"gamma\n")
            .with_file("/c.md", b"# title\n")
            .with_dir("/d")
            .with_file("/d/x.txt", b"x1\nx2\nx3\n");
        t
    }

    fn run(&mut self, line: &str) -> RunResult {
        let mut h = Host {
            fs: &mut self.fs,
            sys: &mut self.sys,
        };
        self.sh.run_line(line, &mut h)
    }

    fn out(&mut self, line: &str) -> String {
        self.run(line).text()
    }

    fn script(&mut self, src: &str, args: &[&str]) -> RunResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut h = Host {
            fs: &mut self.fs,
            sys: &mut self.sys,
        };
        self.sh.run_script(src, &args, &mut h)
    }
}

fn out(line: &str) -> String {
    T::with_files().out(line)
}

// ---- expansion and quoting ---------------------------------------------------

#[test]
fn echo_basics() {
    assert_eq!(out("echo hello world"), "hello world\n");
    assert_eq!(out("echo"), "\n");
    assert_eq!(out("echo -n hi"), "hi");
    assert_eq!(out("echo -e 'a\\tb\\\\'"), "a\tb\\\n");
    assert_eq!(out("echo -n -e 'x\\n'"), "x\n");
}

#[test]
fn quoting_preserves_spaces() {
    assert_eq!(out("echo 'a   b'"), "a   b\n");
    assert_eq!(out("echo \"a   b\""), "a   b\n");
    assert_eq!(out("echo a\\ \\ b"), "a  b\n");
    assert_eq!(out("echo a    b"), "a b\n");
    assert_eq!(out("echo ''"), "\n");
}

#[test]
fn variables_expand() {
    assert_eq!(out("X=5; echo $X ${X}y"), "5 5y\n");
    assert_eq!(out("echo \"[$NOPE]\""), "[]\n");
    assert_eq!(out("A=1 B=2; echo $A$B"), "12\n");
    assert_eq!(out("X='a b'; echo \"$X\""), "a b\n");
}

#[test]
fn unquoted_variables_are_split_on_whitespace() {
    assert_eq!(out("X='a  b   c'; echo $X | wc -w"), "3\n");
    assert_eq!(out("X='a  b'; echo \"$X\""), "a  b\n");
    assert_eq!(
        out("for w in 'x y' z; do echo \"<$w>\"; done"),
        "<x y>\n<z>\n"
    );
}

#[test]
fn single_quotes_block_expansion() {
    assert_eq!(out("X=1; echo '$X' \"$X\" \\$X"), "$X 1 $X\n");
}

#[test]
fn status_variable() {
    assert_eq!(out("false; echo $?"), "1\n");
    assert_eq!(out("true; echo $?"), "0\n");
    assert_eq!(
        out("nosuchcmd; echo $?"),
        "sh: nosuchcmd: command not found\n127\n"
    );
}

#[test]
fn command_substitution() {
    assert_eq!(out("echo $(echo hi)"), "hi\n");
    assert_eq!(out("echo $(echo $(echo deep))"), "deep\n");
    assert_eq!(out("X=$(echo a; echo b); echo \"$X\""), "a\nb\n");
    assert_eq!(out("echo \"[$(echo hi)]\""), "[hi]\n");
    assert_eq!(out("echo $(cat /b.txt) done"), "gamma done\n");
}

#[test]
fn command_substitution_status_and_exit_are_local() {
    assert_eq!(out("X=$(exit 3); echo $?"), "3\n");
    assert_eq!(out("echo $(exit 5) after"), " after\n".trim_start());
}

#[test]
fn arithmetic_expansion() {
    assert_eq!(out("echo $((2+3*4))"), "14\n");
    assert_eq!(out("X=5; echo $((X*2)) $(($X + 1))"), "10 6\n");
    assert_eq!(out("echo $((7 % 4)) $((-3 + 1))"), "3 -2\n");
    let r = T::new().run("echo $((1/0))");
    assert!(r.text().contains("division by zero"));
    assert!(r.text().ends_with("0\n"));
}

#[test]
fn tilde_is_home() {
    let mut t = T::new();
    t.sh.env.set_exported("HOME", "/home");
    assert_eq!(t.out("echo ~ ~/x a~"), "/home /home/x a~\n");
}

#[test]
fn glob_expansion() {
    assert_eq!(out("echo *.txt"), "a.txt b.txt\n");
    assert_eq!(out("echo /d/*"), "/d/x.txt\n");
    assert_eq!(out("echo *.zzz"), "*.zzz\n");
    assert_eq!(out("echo \"*.txt\" '*.txt'"), "*.txt *.txt\n");
    assert_eq!(out("echo ?.md"), "c.md\n");
    assert_eq!(out("echo */*.txt"), "d/x.txt\n");
    assert_eq!(out("echo *"), "a.txt b.txt c.md d\n");
}

#[test]
fn glob_in_for_and_commands() {
    assert_eq!(
        out("for f in *.txt; do echo f:$f; done"),
        "f:a.txt\nf:b.txt\n"
    );
    assert_eq!(out("cat *.txt | wc -l"), "3\n");
    assert_eq!(out("ls *.md"), "c.md\n");
}

#[test]
fn positional_parameters() {
    let mut t = T::new();
    let r = t.script("echo $# $1 $2 $@ $0", &["a", "b b"]);
    assert_eq!(r.text(), "2 a b b a b b script\n");
    let r = t.script("for x in \"$@\"; do echo \"<$x>\"; done", &["a b", "c"]);
    assert_eq!(r.text(), "<a b>\n<c>\n");
    let r = t.script("for x; do echo $x; done", &["p", "q"]);
    assert_eq!(r.text(), "p\nq\n");
    let r = t.script("echo \"$*\"", &["x", "y"]);
    assert_eq!(r.text(), "x y\n");
}

#[test]
fn shift_drops_arguments() {
    let mut t = T::new();
    let r = t.script("shift; echo $1 $#; shift 5; echo $?", &["a", "b", "c"]);
    assert_eq!(r.text(), "b 2\n1\n");
}

// ---- operators ---------------------------------------------------------------

#[test]
fn and_or_lists() {
    assert_eq!(out("true && echo y"), "y\n");
    assert_eq!(out("false && echo y; echo n"), "n\n");
    assert_eq!(out("false || echo z"), "z\n");
    assert_eq!(out("true || echo z; echo done"), "done\n");
    assert_eq!(out("false && echo a || echo b"), "b\n");
    assert_eq!(out("true && false || echo c"), "c\n");
}

#[test]
fn semicolons_newlines_and_comments() {
    assert_eq!(out("echo a; echo b ;echo c"), "a\nb\nc\n");
    assert_eq!(out("echo a # not b"), "a\n");
    assert_eq!(
        T::new().script("echo a\n\n# c\necho b\n", &[]).text(),
        "a\nb\n"
    );
}

#[test]
fn pipes_chain_stdout_to_stdin() {
    assert_eq!(out("echo -e 'b\\na\\nc' | sort"), "a\nb\nc\n");
    assert_eq!(out("cat a.txt b.txt | wc -l"), "3\n");
    assert_eq!(out("echo hello | cat | cat | cat"), "hello\n");
    assert_eq!(out("cat a.txt | grep beta | wc -c"), "5\n");
}

#[test]
fn pipe_status_is_the_last_stage() {
    assert_eq!(out("false | true; echo $?"), "0\n");
    assert_eq!(out("true | false; echo $?"), "1\n");
}

#[test]
fn redirect_out_and_append() {
    let mut t = T::new();
    assert_eq!(t.out("echo hi > f; cat f"), "hi\n");
    assert_eq!(t.out("echo again >> f; cat f"), "hi\nagain\n");
    assert_eq!(t.out("echo over > f; cat f"), "over\n");
    assert_eq!(t.fs.read("/f").unwrap(), b"over\n");
}

#[test]
fn redirect_in() {
    let mut t = T::with_files();
    assert_eq!(t.out("wc -l < a.txt"), "2\n");
    assert_eq!(t.out("cat < b.txt"), "gamma\n");
    let r = t.run("cat < nope");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("No such file"));
}

#[test]
fn redirect_errors() {
    let mut t = T::new();
    let r = t.run("echo x > /no/dir/f");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("No such file"));
    let r = t.run("echo x > ''");
    assert_eq!(r.status, 1);
}

#[test]
fn redirect_creates_file_even_for_failing_command() {
    let mut t = T::new();
    t.run("nosuch > made");
    assert!(t.fs.stat("/made").is_ok());
}

#[test]
fn redirect_target_is_expanded() {
    let mut t = T::new();
    t.run("N=out; echo data > $N.txt");
    assert_eq!(t.fs.read("/out.txt").unwrap(), b"data\n");
}

#[test]
fn pipe_into_redirect() {
    let mut t = T::with_files();
    t.run("cat a.txt | sort -r > sorted");
    assert_eq!(t.fs.read("/sorted").unwrap(), b"beta\nalpha\n");
}

#[test]
fn compound_command_as_pipeline_stage() {
    assert_eq!(out("for i in 3 1 2; do echo $i; done | sort"), "1\n2\n3\n");
    assert_eq!(out("{ echo b; echo a; } | sort"), "a\nb\n");
    assert_eq!(out("echo x | { cat; }"), "x\n");
}

#[test]
fn parse_errors_are_reported_with_position() {
    let mut t = T::new();
    let r = t.run("echo 'abc");
    assert_eq!(r.status, 2);
    assert!(r.text().contains("syntax error at line 1, column 6"));
    assert!(r.text().contains("single quote"));
    let r = t.run("sleep 1 &");
    assert_eq!(r.status, 2);
    assert!(r.text().contains("background"));
    let r = t.run("echo a |");
    assert_eq!(r.status, 2);
    assert_eq!(t.run("echo ok").text(), "ok\n");
}

#[test]
fn unknown_command_and_directory() {
    let mut t = T::with_files();
    let r = t.run("frobnicate now");
    assert_eq!(r.status, 127);
    assert!(r.text().contains("frobnicate: command not found"));
    let r = t.run("/d");
    assert_eq!(r.status, 126);
}

// ---- control flow ---------------------------------------------------------------

#[test]
fn if_then_else() {
    assert_eq!(out("if true; then echo y; else echo n; fi"), "y\n");
    assert_eq!(out("if false; then echo y; else echo n; fi"), "n\n");
    assert_eq!(
        out("if false; then echo a; elif true; then echo b; else echo c; fi"),
        "b\n"
    );
    assert_eq!(out("if false; then echo a; fi; echo $?"), "0\n");
    assert_eq!(out("if [ -f a.txt ]; then echo file; fi"), "file\n");
}

#[test]
fn for_loops() {
    assert_eq!(out("for x in a b c; do echo $x; done"), "a\nb\nc\n");
    assert_eq!(
        out("for i in $(seq 1 3); do echo n$i; done"),
        "n1\nn2\nn3\n"
    );
    assert_eq!(out("for x in; do echo never; done; echo after"), "after\n");
}

#[test]
fn while_and_until_loops() {
    assert_eq!(
        out("i=0; while [ $i -lt 3 ]; do echo $i; i=$((i+1)); done"),
        "0\n1\n2\n"
    );
    assert_eq!(
        out("i=0; until [ $i -ge 2 ]; do echo $i; i=$((i+1)); done"),
        "0\n1\n"
    );
}

#[test]
fn break_and_continue() {
    assert_eq!(
        out(
            "for i in 1 2 3 4; do if [ $i = 2 ]; then continue; fi; if [ $i = 4 ]; then break; fi; echo $i; done"
        ),
        "1\n3\n"
    );
    assert_eq!(
        out("for a in 1 2; do for b in x y; do echo $a$b; break 2; done; done"),
        "1x\n"
    );
}

#[test]
fn functions_and_return() {
    let mut t = T::new();
    t.run("greet() { echo hello $1; }");
    assert_eq!(t.out("greet world"), "hello world\n");
    t.run("function add { echo $(($1 + $2)); }");
    assert_eq!(t.out("add 2 3"), "5\n");
    t.run("check() { if [ $1 = ok ]; then return 0; fi; return 7; }");
    assert_eq!(t.out("check ok; echo $?; check bad; echo $?"), "0\n7\n");
}

#[test]
fn function_arguments_are_restored() {
    let mut t = T::new();
    let r = t.script("f() { echo $1; }\nf inner\necho $1", &["outer"]);
    assert_eq!(r.text(), "inner\nouter\n");
}

#[test]
fn recursion_works_within_the_limit() {
    let mut t = T::new();
    let r = t.script(
        "count() { if [ $1 -gt 0 ]; then echo $1; count $(($1 - 1)); fi; }\ncount 5",
        &[],
    );
    assert_eq!(r.text(), "5\n4\n3\n2\n1\n");
}

#[test]
fn exit_stops_the_script() {
    let mut t = T::new();
    let r = t.script("echo a\nexit 3\necho b", &[]);
    assert_eq!(r.text(), "a\n");
    assert_eq!(r.exit, Some(3));
    assert_eq!(r.status, 3);
    let r = t.run("echo x; exit; echo y");
    assert_eq!(r.text(), "x\n");
    assert_eq!(r.exit, Some(0));
    assert_eq!(t.run("echo after").exit, None);
}

#[test]
fn exit_inside_a_loop_and_function() {
    let mut t = T::new();
    let r = t.script(
        "f() { exit 9; }\nfor i in 1 2 3; do echo $i; f; done\necho no",
        &[],
    );
    assert_eq!(r.text(), "1\n");
    assert_eq!(r.exit, Some(9));
}

#[test]
fn assignment_prefix_is_temporary() {
    let mut t = T::new();
    assert_eq!(t.out("X=1 env | grep '^X='"), "X=1\n");
    assert_eq!(t.out("echo \"[$X]\""), "[]\n");
    t.run("Y=keep");
    t.run("Y=temp echo hi");
    assert_eq!(t.out("echo $Y"), "keep\n");
}

#[test]
fn source_runs_in_the_current_shell() {
    let mut t = T::new();
    t.fs.write("/lib.sh", b"VALUE=42\nhelper() { echo helped; }\n")
        .unwrap();
    assert_eq!(t.out("source /lib.sh; echo $VALUE; helper"), "42\nhelped\n");
    assert_eq!(t.out(". /lib.sh"), "");
    let r = t.run("source /missing.sh");
    assert_eq!(r.status, 126);
}

#[test]
fn scripts_run_from_path_and_by_path() {
    let mut t = T::new();
    t.fs.mkdir("/bin").unwrap();
    t.fs.write("/bin/hello.sh", b"echo hello from script $1\n")
        .unwrap();
    t.fs.write("/bin/tool", b"echo tool $#\n").unwrap();
    assert_eq!(t.out("hello a"), "hello from script a\n");
    assert_eq!(t.out("tool 1 2"), "tool 2\n");
    assert_eq!(t.out("/bin/hello.sh z"), "hello from script z\n");
    assert_eq!(t.out("sh /bin/tool x"), "tool 1\n");
    assert_eq!(t.out("which hello"), "/bin/hello.sh\n");
}

#[test]
fn script_syntax_error_names_the_line() {
    let mut t = T::new();
    t.fs.write("/bad.sh", b"echo ok\nif true; then\n").unwrap();
    let r = t.run("sh /bad.sh");
    assert_eq!(r.status, 2);
    assert!(r.text().contains("/bad.sh: syntax error at line"));
}

// ---- limits -------------------------------------------------------------------

#[test]
fn infinite_loop_is_stopped_by_the_step_limit() {
    let mut t = T::new();
    t.sh.limits.max_steps = 5_000;
    t.sh.limits.max_loop = 1_000_000;
    let r = t.run("while true; do :; done");
    assert!(r.text().contains("step limit exceeded"));
    // The shell still works afterwards.
    assert_eq!(t.out("echo alive"), "alive\n");
}

#[test]
fn loop_iteration_limit() {
    let mut t = T::new();
    t.sh.limits.max_loop = 50;
    let r = t.run("i=0; while [ 1 = 1 ]; do i=$((i+1)); done; echo $i");
    assert!(r.text().contains("loop iteration limit"));
}

#[test]
fn runaway_recursion_hits_the_depth_limit() {
    let mut t = T::new();
    let r = t.script("f() { f; }\nf\necho unreachable", &[]);
    assert!(r.text().contains("depth limit"));
    assert!(!r.text().contains("unreachable"));
    assert_eq!(t.out("echo ok"), "ok\n");
}

#[test]
fn output_is_capped_and_marked() {
    let mut t = T::new();
    t.sh.limits.max_output = 1000;
    let r = t.run("yes");
    assert!(r.truncated);
    assert!(r.output.len() < 1100);
    assert!(r.text().ends_with("[output truncated]\n"));
}

#[test]
fn big_output_through_pipes() {
    let mut t = T::new();
    t.fs.write("/big", &vec![b'x'; 200_000]).unwrap();
    assert_eq!(t.out("cat /big | wc -c"), "200000\n");
    assert_eq!(t.out("cat /big | cat | cat | wc -c"), "200000\n");
    t.sh.limits.max_pipe = 1000;
    let r = t.run("cat /big | wc -c");
    assert!(r.truncated);
    assert!(r.text().contains("pipe buffer limit"));
}

#[test]
fn nested_substitution_depth_is_limited() {
    let mut t = T::new();
    t.sh.limits.max_sub_depth = 3;
    let r = t.run("echo $(echo $(echo $(echo $(echo deep))))");
    assert!(r.text().contains("nested too deeply"));
}

#[test]
fn long_line_is_rejected() {
    let mut t = T::new();
    let long = "a".repeat(t.sh.limits.max_line + 1);
    let r = t.run(&long);
    assert_eq!(r.status, 1);
    assert!(r.text().contains("too long"));
}

#[test]
fn variable_doubling_is_bounded() {
    let mut t = T::new();
    let r = t.run("x=aaaaaaaaaa; for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do x=$x$x; done; echo done");
    assert!(r.status == 0 || r.status == 1);
    assert!(
        t.sh.env
            .get("x")
            .is_none_or(|v| v.len() <= super::env::MAX_VALUE)
    );
}

// ---- builtins -----------------------------------------------------------------

#[test]
fn pwd_and_cd() {
    let mut t = T::with_files();
    assert_eq!(t.out("pwd"), "/\n");
    assert_eq!(t.out("cd d; pwd"), "/d\n");
    assert_eq!(t.out("cd ..; pwd"), "/\n");
    assert_eq!(t.out("cd /d; cd -; pwd"), "/\n/\n");
    assert_eq!(t.out("cd /d && echo $PWD $OLDPWD"), "/d /\n");
    let r = t.run("cd nope");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("No such file"));
    assert_eq!(t.run("cd a.txt").status, 1);
    t.sh.env.set("HOME", "/d");
    assert_eq!(t.out("cd; pwd"), "/d\n");
}

#[test]
fn ls_variants() {
    let mut t = T::with_files();
    t.fs.write("/.hid", b"").unwrap();
    assert_eq!(t.out("ls"), "a.txt\nb.txt\nc.md\nd\n");
    assert_eq!(t.out("ls -F"), "a.txt\nb.txt\nc.md\nd/\n");
    assert!(t.out("ls -a").starts_with(".\n..\n.hid\n"));
    assert_eq!(t.out("ls d"), "x.txt\n");
    assert_eq!(t.out("ls a.txt"), "a.txt\n");
    assert_eq!(t.out("ls -l d"), "-        9 x.txt\n");
    assert_eq!(t.out("ls d /d"), "d:\nx.txt\n\n/d:\nx.txt\n");
    let r = t.run("ls /nope");
    assert_eq!(r.status, 2);
    assert!(r.text().contains("cannot access '/nope'"));
    assert_eq!(t.run("ls -Z").status, 2);
}

#[test]
fn cat_variants() {
    let mut t = T::with_files();
    assert_eq!(t.out("cat a.txt b.txt"), "alpha\nbeta\ngamma\n");
    assert_eq!(t.out("cat -n a.txt"), "     1\talpha\n     2\tbeta\n");
    assert_eq!(t.out("echo hi | cat - a.txt"), "hi\nalpha\nbeta\n");
    let r = t.run("cat nope a.txt");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("nope: No such file"));
    assert!(r.text().contains("alpha"));
    assert_eq!(t.run("cat d").status, 1);
}

#[test]
fn mkdir_rm_rmdir() {
    let mut t = T::new();
    assert_eq!(t.run("mkdir a").status, 0);
    assert_eq!(t.run("mkdir a").status, 1);
    assert_eq!(t.run("mkdir -p x/y/z").status, 0);
    assert!(t.fs.stat("/x/y/z").is_ok());
    assert_eq!(t.run("mkdir -p x/y/z").status, 0);
    t.run("touch x/f");
    assert_eq!(t.run("mkdir -p x/f/g").status, 1);
    assert_eq!(t.run("rmdir x").status, 1);
    assert_eq!(t.run("rm x").status, 1);
    assert_eq!(t.run("rm -r x").status, 0);
    assert!(t.fs.stat("/x").is_err());
    assert_eq!(t.run("rm nope").status, 1);
    assert_eq!(t.run("rm -f nope").status, 0);
    assert_eq!(t.run("rmdir a").status, 0);
    assert_eq!(t.run("rmdir nope").status, 1);
    assert_eq!(t.run("mkdir").status, 2);
}

#[test]
fn rm_refuses_dangerous_targets() {
    let mut t = T::with_files();
    assert_eq!(t.run("rm -rf /").status, 1);
    assert_eq!(t.run("rm -rf .").status, 1);
    assert_eq!(t.run("rm -rf ..").status, 1);
    assert!(t.fs.stat("/a.txt").is_ok());
}

#[test]
fn mv_and_cp() {
    let mut t = T::with_files();
    assert_eq!(t.run("mv a.txt z.txt").status, 0);
    assert!(t.fs.stat("/a.txt").is_err());
    assert_eq!(t.run("mv z.txt d").status, 0);
    assert!(t.fs.stat("/d/z.txt").is_ok());
    assert_eq!(t.run("mv b.txt c.md d").status, 0);
    assert!(t.fs.stat("/d/c.md").is_ok());
    assert_eq!(t.run("mv nope d").status, 1);
    assert_eq!(t.run("mv").status, 2);
    assert_eq!(t.run("cp d/x.txt copy.txt").status, 0);
    assert_eq!(t.fs.read("/copy.txt").unwrap(), b"x1\nx2\nx3\n");
    assert_eq!(t.run("cp d dd").status, 1);
    assert_eq!(t.run("cp -r d dd").status, 0);
    assert!(t.fs.stat("/dd/x.txt").is_ok());
    assert_eq!(t.run("cp copy.txt d").status, 0);
    assert!(t.fs.stat("/d/copy.txt").is_ok());
    assert_eq!(t.run("cp -r d d/inside").status, 1);
}

#[test]
fn touch_creates_but_keeps_content() {
    let mut t = T::with_files();
    assert_eq!(t.run("touch new a.txt").status, 0);
    assert_eq!(t.fs.read("/new").unwrap(), b"");
    assert_eq!(t.fs.read("/a.txt").unwrap(), b"alpha\nbeta\n");
    assert_eq!(t.run("touch /no/dir/f").status, 1);
}

#[test]
fn head_and_tail() {
    let mut t = T::with_files();
    assert_eq!(t.out("head -n 2 d/x.txt"), "x1\nx2\n");
    assert_eq!(t.out("head -1 d/x.txt"), "x1\n");
    assert_eq!(t.out("tail -n 1 d/x.txt"), "x3\n");
    assert_eq!(t.out("tail -2 d/x.txt"), "x2\nx3\n");
    assert_eq!(t.out("seq 20 | head -n 3"), "1\n2\n3\n");
    assert_eq!(t.out("seq 20 | tail -n 2"), "19\n20\n");
    assert_eq!(t.out("seq 20 | head | wc -l"), "10\n");
    assert_eq!(
        t.out("head -n 1 a.txt b.txt"),
        "==> a.txt <==\nalpha\n\n==> b.txt <==\ngamma\n"
    );
    assert_eq!(t.run("head -n x a.txt").status, 2);
    assert_eq!(t.out("head -n 0 a.txt"), "");
}

#[test]
fn wc_counts() {
    let mut t = T::with_files();
    assert_eq!(t.out("wc a.txt"), "2 2 11 a.txt\n");
    assert_eq!(t.out("wc -l a.txt"), "2 a.txt\n");
    assert_eq!(t.out("wc -w < a.txt"), "2\n");
    assert_eq!(t.out("echo héllo | wc -m"), "6\n");
    assert_eq!(t.out("echo héllo | wc -c"), "7\n");
    assert_eq!(t.out("wc -l a.txt b.txt"), "2 a.txt\n1 b.txt\n3 total\n");
    assert_eq!(t.run("wc nope").status, 1);
}

#[test]
fn grep_variants() {
    let mut t = T::with_files();
    assert_eq!(t.out("grep a a.txt"), "alpha\nbeta\n");
    assert_eq!(t.out("grep ^b a.txt"), "beta\n");
    assert_eq!(t.out("grep -n eta a.txt"), "2:beta\n");
    assert_eq!(t.out("grep -c a a.txt"), "2\n");
    assert_eq!(t.out("grep -v alpha a.txt"), "beta\n");
    assert_eq!(t.out("grep -i ALPHA a.txt"), "alpha\n");
    assert_eq!(t.out("grep -l a a.txt b.txt c.md"), "a.txt\nb.txt\n");
    assert_eq!(
        t.out("grep a a.txt b.txt"),
        "a.txt:alpha\na.txt:beta\nb.txt:gamma\n"
    );
    assert_eq!(t.out("grep -h a a.txt b.txt").lines().count(), 3);
    assert_eq!(t.out("echo 'a.b' | grep -F 'a.b'"), "a.b\n");
    assert_eq!(t.out("echo axb | grep -F 'a.b'"), "");
    assert_eq!(t.out("cat a.txt | grep -E 'alp|bet'"), "alpha\nbeta\n");
    assert_eq!(t.run("grep zzz a.txt").status, 1);
    assert_eq!(t.run("grep -q alpha a.txt").status, 0);
    assert_eq!(t.out("grep -q alpha a.txt"), "");
    assert_eq!(t.run("grep '(' a.txt").status, 2);
    assert_eq!(t.run("grep").status, 2);
    assert_eq!(t.run("grep a nope").status, 2);
}

#[test]
fn sort_variants() {
    let mut t = T::new();
    assert_eq!(t.out("echo -e 'b\\nc\\na' | sort"), "a\nb\nc\n");
    assert_eq!(t.out("echo -e 'b\\nc\\na' | sort -r"), "c\nb\na\n");
    assert_eq!(t.out("echo -e '10\\n9\\n100' | sort -n"), "9\n10\n100\n");
    assert_eq!(t.out("echo -e '10\\n9\\n100' | sort"), "10\n100\n9\n");
    assert_eq!(t.out("echo -e 'a\\na\\nb' | sort -u"), "a\nb\n");
    assert_eq!(t.out("echo -e 'B\\na' | sort -f"), "a\nB\n");
    assert_eq!(t.out("echo -e '-5\\n3\\n-10' | sort -n"), "-10\n-5\n3\n");
}

#[test]
fn uniq_variants() {
    let mut t = T::new();
    let data = "echo -e 'a\\na\\nb\\na'";
    assert_eq!(t.out(&format!("{data} | uniq")), "a\nb\na\n");
    assert_eq!(
        t.out(&format!("{data} | uniq -c")),
        "      2 a\n      1 b\n      1 a\n"
    );
    assert_eq!(t.out(&format!("{data} | uniq -d")), "a\n");
    assert_eq!(t.out(&format!("{data} | uniq -u")), "b\na\n");
    assert_eq!(
        t.out(&format!("{data} | sort | uniq -c")),
        "      3 a\n      1 b\n"
    );
}

#[test]
fn tee_copies() {
    let mut t = T::new();
    assert_eq!(t.out("echo hi | tee out.txt"), "hi\n");
    assert_eq!(t.fs.read("/out.txt").unwrap(), b"hi\n");
    t.run("echo more | tee -a out.txt > /dev/null");
    assert_eq!(t.fs.read("/out.txt").unwrap(), b"hi\nmore\n");
}

#[test]
fn env_export_set_unset() {
    let mut t = T::new();
    t.run("export A=1 B");
    assert!(t.sh.env.is_exported("A"));
    assert!(t.sh.env.is_exported("B"));
    assert!(t.out("env").contains("A=1\n"));
    assert!(t.out("export").contains("export A=\"1\"\n"));
    t.run("C=3");
    assert!(!t.out("env").contains("C=3"));
    assert!(t.out("set").contains("C=3\n"));
    t.run("unset A");
    assert_eq!(t.sh.env.get("A"), None);
    t.run("set D=4");
    assert_eq!(t.sh.env.get("D"), Some("4"));
    assert_eq!(t.run("export 1bad=2").status, 1);
    assert_eq!(t.run("set oops").status, 2);
}

#[test]
fn set_replaces_positional_parameters() {
    let mut t = T::new();
    assert_eq!(t.script("set -- x y z; echo $# $2", &[]).text(), "3 y\n");
}

#[test]
fn history_command() {
    let mut t = T::new();
    t.run("echo one");
    t.run("echo two");
    let h = t.out("history");
    assert!(h.contains("    1  echo one\n"));
    assert!(h.contains("    3  history\n"));
    assert_eq!(t.out("history 1"), "    4  history 1\n");
    t.run("history -c");
    assert_eq!(t.sh.history().len(), 0);
    assert_eq!(t.run("history x").status, 2);
}

#[test]
fn alias_and_unalias() {
    let mut t = T::with_files();
    t.run("alias ll='ls -F'");
    assert_eq!(t.out("ll"), "a.txt\nb.txt\nc.md\nd/\n");
    assert_eq!(t.out("alias ll"), "alias ll='ls -F'\n");
    assert!(t.out("alias").contains("alias ll='ls -F'"));
    assert_eq!(t.out("which ll"), "ll: aliased to 'ls -F'\n");
    t.run("unalias ll");
    assert_eq!(t.run("ll").status, 127);
    assert_eq!(t.run("unalias ll").status, 1);
    t.run("alias a1='echo hi'");
    t.run("unalias -a");
    assert!(t.sh.aliases().is_empty());
}

#[test]
fn alias_loops_terminate() {
    let mut t = T::new();
    t.run("alias echo='echo echo'");
    let r = t.run("echo x");
    assert_eq!(r.text(), "echo x\n");
    t.run("alias a='b'");
    t.run("alias b='a'");
    assert_eq!(t.run("a").status, 127);
}

#[test]
fn which_resolution() {
    let mut t = T::new();
    assert_eq!(t.out("which ls"), "ls: shell builtin\n");
    t.run("f() { :; }");
    assert_eq!(t.out("which f"), "f: shell function\n");
    let r = t.run("which nope");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("nope not found"));
}

#[test]
fn date_uptime_free_df() {
    let mut t = T::new();
    assert_eq!(t.out("date"), "2026-10-07 13:05:09\n");
    assert_eq!(t.out("date +%d/%m/%Y"), "07/10/2026\n");
    assert_eq!(t.out("date +%T"), "13:05:09\n");
    assert_eq!(t.run("date bad").status, 1);
    assert_eq!(t.out("uptime"), "up 1 day, 02:03:04\n");
    t.sys.uptime = 3_725_000;
    assert_eq!(t.out("uptime"), "up 01:02:05\n");
    let f = t.out("free");
    assert!(f.contains("KiB") && f.contains("65536") && f.contains("20480") && f.contains("45056"));
    assert!(t.out("free -m").contains("MiB"));
    let d = t.out("df");
    assert!(d.contains("Filesystem") && d.contains("rootfs"));
    t.sys.disk_list.push(super::sys::DiskInfo {
        name: "ojfs".into(),
        mount: "/".into(),
        total: 1 << 20,
        used: 1 << 19,
    });
    assert!(t.out("df").contains("50%"));
}

#[test]
fn ps_kill_ping_sleep_clear() {
    let mut t = T::new();
    let p = t.out("ps");
    assert!(p.contains("init") && p.contains("shell"));
    assert_eq!(t.run("kill 7").status, 0);
    assert_eq!(t.sys.killed, [(7, 15)]);
    assert_eq!(t.run("kill -9 1").status, 0);
    assert_eq!(t.sys.killed[1], (1, 9));
    assert_eq!(t.run("kill -KILL 99").status, 1);
    assert_eq!(t.run("kill -BOGUS 1").status, 2);
    assert_eq!(t.run("kill").status, 2);
    assert_eq!(t.run("kill abc").status, 1);
    let r = t.run("ping -c 2 example.org");
    assert!(
        r.text()
            .contains("2 packets transmitted, 2 received, 0% packet loss")
    );
    assert!(r.text().contains("avg rtt = 1.500 ms"));
    t.sys.ping_ok = false;
    assert_eq!(t.run("ping host").status, 1);
    assert_eq!(t.run("ping -c 0 host").status, 2);
    assert_eq!(t.run("ping").status, 2);
    t.run("sleep 2");
    t.run("sleep 0.25");
    t.run("sleep 500000");
    assert_eq!(t.sys.slept, [2000, 250, 10_000]);
    assert_eq!(t.run("sleep abc").status, 1);
    assert_eq!(t.run("sleep").status, 2);
    let r = t.run("echo before; clear");
    assert!(r.clear);
    assert_eq!(r.text(), "");
    assert_eq!(t.sys.cleared, 1);
}

#[test]
fn unsupported_system_calls_are_reported() {
    let mut sh = Shell::new();
    let mut fs = MemFs::new();
    let mut sys = super::sys::MinimalSys;
    let mut h = Host {
        fs: &mut fs,
        sys: &mut sys,
    };
    let r = sh.run_line("ping host", &mut h);
    assert_eq!(r.status, 1);
    assert!(r.text().contains("not supported"));
    let r = sh.run_line("kill 1", &mut h);
    assert_eq!(r.status, 1);
    assert!(sh.run_line("ps", &mut h).text().contains("PID"));
}

#[test]
fn test_builtin() {
    let mut t = T::with_files();
    let st = |t: &mut T, l: &str| t.run(l).status;
    assert_eq!(st(&mut t, "test -f a.txt"), 0);
    assert_eq!(st(&mut t, "test -d a.txt"), 1);
    assert_eq!(st(&mut t, "test -d d"), 0);
    assert_eq!(st(&mut t, "test -e nope"), 1);
    assert_eq!(st(&mut t, "test -s a.txt"), 0);
    assert_eq!(st(&mut t, "test -z ''"), 0);
    assert_eq!(st(&mut t, "test -n ''"), 1);
    assert_eq!(st(&mut t, "test a = a"), 0);
    assert_eq!(st(&mut t, "test a != a"), 1);
    assert_eq!(st(&mut t, "test 3 -lt 10"), 0);
    assert_eq!(st(&mut t, "test 3 -ge 10"), 1);
    assert_eq!(st(&mut t, "test 5 -eq 5 -a 1 -ne 2"), 0);
    assert_eq!(st(&mut t, "test 1 -eq 2 -o 2 -eq 2"), 0);
    assert_eq!(st(&mut t, "test ! -f nope"), 0);
    assert_eq!(st(&mut t, "test -f a.txt -a -d d"), 0);
    assert_eq!(st(&mut t, "test hello"), 0);
    assert_eq!(st(&mut t, "test"), 1);
    assert_eq!(st(&mut t, "test x -eq 1"), 2);
    assert_eq!(st(&mut t, "[ 1 -eq 1 ]"), 0);
    assert_eq!(st(&mut t, "[ 1 -eq 1"), 2);
    assert_eq!(st(&mut t, "[ \\( 1 -eq 1 \\) ]"), 0);
    assert_eq!(st(&mut t, "[ -n \"$UNSET\" ]"), 1);
}

#[test]
fn help_lists_every_command() {
    let mut t = T::new();
    let h = t.out("help");
    for cmd in [
        "help", "ls", "cd", "pwd", "cat", "echo", "mkdir", "rm", "rmdir", "mv", "cp", "touch",
        "head", "tail", "wc", "grep", "sort", "uniq", "tee", "clear", "env", "export", "set",
        "unset", "history", "alias", "which", "date", "uptime", "free", "df", "ps", "kill", "ping",
        "true", "false", "test", "sleep",
    ] {
        assert!(
            h.lines().any(|l| l.trim_start().starts_with(cmd)),
            "help is missing {cmd}"
        );
    }
    assert!(t.out("help ls").starts_with("ls "));
    assert_eq!(t.run("help nonexistent").status, 1);
}

#[test]
fn true_and_false() {
    let mut t = T::new();
    assert_eq!(t.run("true").status, 0);
    assert_eq!(t.run("false").status, 1);
    assert_eq!(t.run(":").status, 0);
}

#[test]
fn seq_basename_dirname_misc() {
    let mut t = T::new();
    assert_eq!(t.out("seq 3"), "1\n2\n3\n");
    assert_eq!(t.out("seq 2 4"), "2\n3\n4\n");
    assert_eq!(t.out("seq 10 -4 0"), "10\n6\n2\n");
    assert_eq!(t.run("seq 1 0 5").status, 2);
    assert_eq!(t.out("basename /a/b/c.txt"), "c.txt\n");
    assert_eq!(t.out("basename /a/b/c.txt .txt"), "c\n");
    assert_eq!(t.out("dirname /a/b/c.txt"), "/a/b\n");
    assert_eq!(t.out("dirname c.txt"), ".\n");
    assert_eq!(t.out("dirname /c"), "/\n");
    assert_eq!(t.out("echo abc | rev"), "cba\n");
    assert_eq!(t.out("echo hello | tr a-z A-Z"), "HELLO\n");
    assert_eq!(t.out("echo hello | tr -d l"), "heo\n");
    assert_eq!(t.out("echo a:b:c | cut -d : -f 2,3"), "b:c\n");
    assert_eq!(t.out("echo -e 'x\\ny' | nl"), "     1\tx\n     2\ty\n");
}

#[test]
fn stat_command() {
    let mut t = T::with_files();
    assert_eq!(
        t.out("stat a.txt d"),
        "a.txt: file, 11 bytes\nd: directory, 0 bytes\n"
    );
    assert_eq!(t.run("stat nope").status, 1);
}

#[test]
fn custom_commands_can_be_registered() {
    fn hello(cx: &mut CmdCtx<'_>) -> i32 {
        cx.println("custom!");
        7
    }
    let mut t = T::new();
    t.sh.register("hello", "hello: demo", hello);
    let r = t.run("hello");
    assert_eq!(r.text(), "custom!\n");
    assert_eq!(r.status, 7);
    assert_eq!(t.out("help hello"), "hello: demo\n");
}

#[test]
fn prompt_shows_cwd_and_home_tilde() {
    let mut t = T::with_files();
    assert_eq!(t.sh.prompt(&t.fs, &t.sys), "user@osjeff:/$ ");
    t.run("cd d");
    assert_eq!(t.sh.prompt(&t.fs, &t.sys), "user@osjeff:/d$ ");
    t.sh.env.set("HOME", "/d");
    assert_eq!(t.sh.prompt(&t.fs, &t.sys), "user@osjeff:~$ ");
    t.sh.env.set("PS1", "[\\W] \\\\ \\x ");
    assert_eq!(t.sh.prompt(&t.fs, &t.sys), "[d] \\ \\x ");
}

#[test]
fn history_is_recorded_by_run_line_only() {
    let mut t = T::new();
    t.run("echo a");
    t.run("echo a");
    t.run("   ");
    t.run(" hidden");
    t.script("echo scripted", &[]);
    assert_eq!(t.sh.history().len(), 1);
}

// ---- robustness -------------------------------------------------------------

fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

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
