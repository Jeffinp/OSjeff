use super::*;

#[test]
fn path_clip_copy_and_cut() {
    let mut c = PathClip::new();
    assert!(c.is_empty());
    c.set(vec![b"/a".to_vec()], false);
    assert!(!c.is_cut_path(b"/a"));
    c.after_paste();
    assert!(!c.is_empty()); // a copy can be pasted again
    c.set(vec![b"/a".to_vec(), b"/b".to_vec()], true);
    assert!(c.is_cut_path(b"/b"));
    assert!(!c.is_cut_path(b"/c"));
    c.after_paste();
    assert!(c.is_empty() && !c.is_cut());
}
