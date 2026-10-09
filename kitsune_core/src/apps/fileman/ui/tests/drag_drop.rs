use super::*;

#[test]
fn a_folder_cannot_be_dropped_into_itself_or_below() {
    let src = paths(&["/a/docs"]);
    assert!(!can_drop_into(&src, b"/a/docs"));
    assert!(!can_drop_into(&src, b"/a/docs/inner"));
    assert!(!can_drop_into(&src, b"/a/docs/inner/deeper"));
    // A sibling with a similar name is fine.
    assert!(can_drop_into(&src, b"/a/docs2"));
    assert!(can_drop_into(&src, b"/b"));
    assert!(can_drop_into(&src, b"/"));
}

#[test]
fn dropping_where_the_items_already_are_does_nothing() {
    assert!(!can_drop_into(&paths(&["/a/x", "/a/y"]), b"/a"));
    assert!(can_drop_into(&paths(&["/a/x", "/b/y"]), b"/a")); // one of them moves
    assert!(!can_drop_into(&paths(&["/x"]), b"/"));
    assert!(can_drop_into(&paths(&["/a/x"]), b"/"));
    assert!(!can_drop_into(&[], b"/a"));
    assert!(!can_drop_into(&paths(&["/a/x"]), TRASH_PATH));
    assert!(!can_drop_into(&paths(&["/a/x"]), APPS_PATH));
}

#[test]
fn plan_drop_picks_the_operation() {
    let src = paths(&["/a/x"]);
    assert_eq!(
        plan_drop(&src, DropTarget::Folder(b"/b"), false),
        Some(DropOp::Move)
    );
    assert_eq!(
        plan_drop(&src, DropTarget::Folder(b"/b"), true),
        Some(DropOp::Copy)
    );
    assert_eq!(plan_drop(&src, DropTarget::Folder(b"/a"), false), None);
    assert_eq!(
        plan_drop(&src, DropTarget::Trash, false),
        Some(DropOp::Trash)
    );
    assert_eq!(
        plan_drop(&src, DropTarget::Trash, true),
        Some(DropOp::Trash)
    );
    assert_eq!(plan_drop(&src, DropTarget::None, false), None);
    assert_eq!(plan_drop(&[], DropTarget::Trash, false), None);
    assert_eq!(
        plan_drop(&paths(&["/a"]), DropTarget::Folder(b"/a/b"), false),
        None
    );
}

#[test]
fn sidebar_places_are_drop_targets() {
    assert_eq!(place_target(Place::Trash), DropTarget::Trash);
    assert_eq!(place_target(Place::Apps), DropTarget::None);
    assert_eq!(
        place_target(Place::Documents),
        DropTarget::Folder(b"/Documentos")
    );
    assert_eq!(place_target(Place::Disk), DropTarget::Folder(b"/"));
    assert_eq!(place_target(Place::Home), DropTarget::Folder(b"/home"));
    assert_eq!(place_target(Place::Images), DropTarget::Folder(b"/Imagens"));
}
