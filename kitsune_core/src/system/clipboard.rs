//! A tiny fixed-capacity text clipboard shared across apps.
//!
//! Allocation-free: copies are bounded by [`CAP`] and truncated past it. The
//! kernel owns one instance and the desktop orchestrates copy/paste between the
//! focused app and this buffer (the apps themselves never reference each other).

/// Maximum clipboard payload in bytes.
pub const CAP: usize = 256;

pub struct Clipboard {
    buf: [u8; CAP],
    len: usize,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard {
    pub const fn new() -> Self {
        Self {
            buf: [0; CAP],
            len: 0,
        }
    }

    /// Replace the contents with `data` (truncated to [`CAP`]).
    pub fn set(&mut self, data: &[u8]) {
        let n = data.len().min(CAP);
        self.buf[..n].copy_from_slice(&data[..n]);
        self.len = n;
    }

    /// Current clipboard text.
    pub fn get(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests;
