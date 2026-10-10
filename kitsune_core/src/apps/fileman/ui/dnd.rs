//! dnd (split out of `ui.rs`).

use super::*;

/// What a drop would do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropOp {
    Move,
    Copy,
    Trash,
}

/// Where the pointer is while dragging items.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget<'a> {
    /// A folder (absolute path).
    Folder(&'a [u8]),
    /// The bin.
    Trash,
    /// Nothing that accepts a drop.
    None,
}

/// The place a sidebar entry stands for as a drop target.
pub fn place_target(p: Place) -> DropTarget<'static> {
    match p {
        Place::Trash => DropTarget::Trash,
        Place::Apps => DropTarget::None,
        other => DropTarget::Folder(other.path()),
    }
}

pub(super) fn parent_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) | None => b"/",
        Some(i) => &path[..i],
    }
}

pub(super) fn is_inside(path: &[u8], folder: &[u8]) -> bool {
    if folder == b"/" {
        return true;
    }
    path == folder || (path.starts_with(folder) && path.get(folder.len()) == Some(&b'/'))
}

/// Whether dropping `sources` into the folder `dest` is meaningful: it is not a source itself or
/// inside one, and at least one source would actually change folders.
pub fn can_drop_into(sources: &[Vec<u8>], dest: &[u8]) -> bool {
    if sources.is_empty() || dest == TRASH_PATH || dest == APPS_PATH {
        return false;
    }
    if sources.iter().any(|s| is_inside(dest, s)) {
        return false;
    }
    sources.iter().any(|s| parent_of(s) != dest)
}

/// The operation a drop of `sources` onto `target` performs, or `None` when it would do nothing
/// useful. `copy` is the Ctrl key: a copy instead of a move. The bin always moves to the bin.
pub fn plan_drop(sources: &[Vec<u8>], target: DropTarget<'_>, copy: bool) -> Option<DropOp> {
    match target {
        DropTarget::Trash => (!sources.is_empty()).then_some(DropOp::Trash),
        DropTarget::Folder(dest) => {
            // A copy into the folder the items already live in would duplicate them, which
            // the paste command does; a drop is for going somewhere else.
            can_drop_into(sources, dest).then_some(if copy { DropOp::Copy } else { DropOp::Move })
        }
        DropTarget::None => None,
    }
}
