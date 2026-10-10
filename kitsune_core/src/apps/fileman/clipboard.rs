//! clipboard (split out of `fileman.rs`).

use super::*;

/// What Ctrl+C / Ctrl+X put aside for Ctrl+V; shared by all file-manager windows.
#[derive(Clone, Debug, Default)]
pub struct PathClip {
    paths: Vec<Vec<u8>>,
    cut: bool,
}

impl PathClip {
    pub const fn new() -> Self {
        PathClip {
            paths: Vec::new(),
            cut: false,
        }
    }

    pub fn set(&mut self, paths: Vec<Vec<u8>>, cut: bool) {
        self.paths = paths;
        self.cut = cut;
    }

    pub fn paths(&self) -> &[Vec<u8>] {
        &self.paths
    }

    pub fn is_cut(&self) -> bool {
        self.cut
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// After a successful move the clipboard is spent; a copy can be pasted again.
    pub fn after_paste(&mut self) {
        if self.cut {
            self.paths.clear();
            self.cut = false;
        }
    }

    /// Whether `path` is waiting to be moved (drawn dimmed).
    pub fn is_cut_path(&self, path: &[u8]) -> bool {
        self.cut && self.paths.iter().any(|p| p == path)
    }
}
