use super::*;

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
