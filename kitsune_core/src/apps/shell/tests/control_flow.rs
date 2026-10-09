use super::*;

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
