//! paths (split out of `fileman.rs`).

use super::*;

/// One clickable part of the address bar.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Crumb {
    pub label: Vec<u8>,
    pub path: Vec<u8>,
}

/// The crumbs of `path`: the disk (`Disco` / `Disk`), then one per folder (the trash for the
/// trash). The special labels follow the language in effect.
pub fn breadcrumbs(path: &[u8]) -> Vec<Crumb> {
    let mut out = alloc::vec![Crumb {
        label: crate::t!("files.place.disk").as_bytes().to_vec(),
        path: b"/".to_vec(),
    }];
    if path == TRASH_PATH {
        out.push(Crumb {
            label: crate::t!("files.place.trash").as_bytes().to_vec(),
            path: TRASH_PATH.to_vec(),
        });
        return out;
    }
    if path == APPS_PATH {
        out.push(Crumb {
            label: b"Apps".to_vec(),
            path: APPS_PATH.to_vec(),
        });
        return out;
    }
    let mut acc: Vec<u8> = Vec::new();
    for c in vfs::components(path) {
        acc.push(b'/');
        acc.extend_from_slice(c);
        out.push(Crumb {
            label: c.to_vec(),
            path: acc.clone(),
        });
    }
    out
}

/// Back/forward history of locations.
#[derive(Clone, Debug)]
pub struct History {
    stack: Vec<Vec<u8>>,
    pos: usize,
}

/// Most locations remembered.
pub(super) const HISTORY_MAX: usize = 64;

impl History {
    /// History starting at `start`.
    pub fn new(start: &[u8]) -> Self {
        History {
            stack: alloc::vec![start.to_vec()],
            pos: 0,
        }
    }

    /// Visit `path`: drops the forward part; visiting the current place again does nothing.
    pub fn push(&mut self, path: &[u8]) {
        if self.stack[self.pos] == path {
            return;
        }
        self.stack.truncate(self.pos + 1);
        self.stack.push(path.to_vec());
        if self.stack.len() > HISTORY_MAX {
            self.stack.remove(0);
        }
        self.pos = self.stack.len() - 1;
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.stack.len()
    }

    /// Step back; the new current place.
    pub fn back(&mut self) -> Option<&[u8]> {
        if self.can_back() {
            self.pos -= 1;
            Some(&self.stack[self.pos])
        } else {
            None
        }
    }

    /// Step forward; the new current place.
    pub fn forward(&mut self) -> Option<&[u8]> {
        if self.can_forward() {
            self.pos += 1;
            Some(&self.stack[self.pos])
        } else {
            None
        }
    }

    /// The current place.
    pub fn current(&self) -> &[u8] {
        &self.stack[self.pos]
    }

    /// Replace the current place (it moved or vanished).
    pub fn replace_current(&mut self, path: &[u8]) {
        self.stack[self.pos] = path.to_vec();
    }
}
