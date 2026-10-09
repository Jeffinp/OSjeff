use super::*;

fn ins(pos: usize, s: &[u8]) -> Edit {
    Edit {
        pos,
        removed: Vec::new(),
        inserted: s.to_vec(),
    }
}

#[test]
fn starts_clean_and_empty() {
    let h = History::new();
    assert!(!h.modified());
    assert!(!h.can_undo() && !h.can_redo());
}

#[test]
fn typing_merges_into_one_group() {
    let mut h = History::new();
    for (i, c) in b"abc".iter().enumerate() {
        h.begin(Kind::Typing, i, false);
        h.push(ins(i, &[*c]));
        h.finish(i + 1, Kind::Typing);
    }
    assert_eq!(h.undo_depth(), 1);
    let g = h.undo_with(Clone::clone).unwrap();
    assert_eq!(g.edits.len(), 1);
    assert_eq!(g.edits[0].inserted, b"abc");
}

#[test]
fn other_kind_never_merges() {
    let mut h = History::new();
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Other);
    h.begin(Kind::Other, 1, false);
    h.push(ins(1, b"b"));
    h.finish(2, Kind::Other);
    assert_eq!(h.undo_depth(), 2);
}

#[test]
fn break_group_splits_typing() {
    let mut h = History::new();
    h.begin(Kind::Typing, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Typing);
    h.break_group();
    h.begin(Kind::Typing, 1, false);
    h.push(ins(1, b"b"));
    h.finish(2, Kind::Typing);
    assert_eq!(h.undo_depth(), 2);
}

#[test]
fn word_boundary_splits_typing() {
    let mut h = History::new();
    for (i, c) in b"ab cd".iter().enumerate() {
        h.begin(Kind::Typing, i, *c == b' ');
        h.push(ins(i, &[*c]));
        h.finish(i + 1, Kind::Typing);
    }
    // "ab " then "cd".
    assert_eq!(h.undo_depth(), 2);
}

#[test]
fn new_edit_clears_redo() {
    let mut h = History::new();
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Other);
    h.undo_with(|_| ());
    assert!(h.can_redo());
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"b"));
    h.finish(1, Kind::Other);
    assert!(!h.can_redo());
}

#[test]
fn modified_tracks_save_point() {
    let mut h = History::new();
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Other);
    assert!(h.modified());
    h.mark_saved();
    assert!(!h.modified());
    h.undo_with(|_| ());
    assert!(h.modified());
    h.redo_with(|_| ());
    assert!(!h.modified());
}

#[test]
fn save_point_lost_when_redo_discarded() {
    let mut h = History::new();
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Other);
    h.mark_saved();
    h.undo_with(|_| ());
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"b"));
    h.finish(1, Kind::Other);
    // Depth equals the saved depth (1) but the content differs.
    assert!(h.modified());
    h.undo_with(|_| ());
    assert!(h.modified());
}

#[test]
fn typing_after_save_is_a_new_group() {
    let mut h = History::new();
    h.begin(Kind::Typing, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Typing);
    h.mark_saved();
    h.begin(Kind::Typing, 1, false);
    h.push(ins(1, b"b"));
    h.finish(2, Kind::Typing);
    assert!(h.modified());
    assert_eq!(h.undo_depth(), 2);
}

#[test]
fn backspace_merges_in_reverse() {
    let mut h = History::new();
    // Text "abc": backspace at 3, then 2.
    h.begin(Kind::Backspacing, 3, false);
    h.push(Edit {
        pos: 2,
        removed: b"c".to_vec(),
        inserted: Vec::new(),
    });
    h.finish(2, Kind::Backspacing);
    h.begin(Kind::Backspacing, 2, false);
    h.push(Edit {
        pos: 1,
        removed: b"b".to_vec(),
        inserted: Vec::new(),
    });
    h.finish(1, Kind::Backspacing);
    let g = h.undo_with(Clone::clone).unwrap();
    assert_eq!(g.edits.len(), 1);
    assert_eq!(g.edits[0].pos, 1);
    assert_eq!(g.edits[0].removed, b"bc");
}

#[test]
fn delete_merges_forward() {
    let mut h = History::new();
    for _ in 0..2 {
        h.begin(Kind::Deleting, 1, false);
        h.push(Edit {
            pos: 1,
            removed: b"x".to_vec(),
            inserted: Vec::new(),
        });
        h.finish(1, Kind::Deleting);
    }
    let g = h.undo_with(Clone::clone).unwrap();
    assert_eq!(g.edits[0].removed, b"xx");
}

#[test]
fn empty_group_is_dropped() {
    let mut h = History::new();
    h.begin(Kind::Other, 0, false);
    h.finish(0, Kind::Other);
    assert_eq!(h.undo_depth(), 0);
    assert!(!h.modified());
}

#[test]
fn limit_discards_oldest() {
    let mut h = History::new();
    for i in 0..10 {
        h.begin(Kind::Other, i, false);
        h.push(ins(i, &[b'x'; 100]));
        h.finish(i + 1, Kind::Other);
    }
    h.set_limit(500);
    assert!(h.undo_depth() < 10);
    assert!(h.undo_depth() >= 1);
    assert!(h.modified());
}

#[test]
fn clear_resets() {
    let mut h = History::new();
    h.begin(Kind::Other, 0, false);
    h.push(ins(0, b"a"));
    h.finish(1, Kind::Other);
    h.clear();
    assert!(!h.modified());
    assert!(!h.can_undo());
}
