use super::*;

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
