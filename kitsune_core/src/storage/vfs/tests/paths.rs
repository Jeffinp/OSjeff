use super::*;

#[test]
fn join_handles_the_root() {
    assert_eq!(join(b"/", b"a"), b"/a");
    assert_eq!(join(b"/a", b"b"), b"/a/b");
}

#[test]
fn parent_and_base_name() {
    assert_eq!(parent(b"/a/b"), b"/a");
    assert_eq!(parent(b"/a"), b"/");
    assert_eq!(parent(b"/"), b"/");
    assert_eq!(base_name(b"/a/b.txt"), b"b.txt");
    assert_eq!(base_name(b"/a"), b"a");
    assert_eq!(base_name(b"/"), b"");
}

#[test]
fn is_inside_is_component_wise() {
    assert!(is_inside(b"/a/b", b"/a"));
    assert!(is_inside(b"/a", b"/a"));
    assert!(!is_inside(b"/ab", b"/a"));
    assert!(is_inside(b"/x", b"/"));
    assert!(!is_inside(b"/a", b"/a/b"));
}

#[test]
fn components_skip_empty_parts() {
    assert_eq!(components(b"/"), Vec::<&[u8]>::new());
    assert_eq!(components(b"/a/b"), vec![&b"a"[..], &b"b"[..]]);
}
