use super::*;

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
