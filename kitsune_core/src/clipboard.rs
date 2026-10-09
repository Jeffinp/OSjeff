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
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let c = Clipboard::new();
        assert!(c.is_empty());
        assert_eq!(c.get(), b"");
    }

    #[test]
    fn set_then_get() {
        let mut c = Clipboard::new();
        c.set(b"hello");
        assert_eq!(c.get(), b"hello");
        assert!(!c.is_empty());
    }

    #[test]
    fn set_overwrites() {
        let mut c = Clipboard::new();
        c.set(b"first");
        c.set(b"2nd");
        assert_eq!(c.get(), b"2nd");
    }

    #[test]
    fn truncates_past_capacity() {
        let mut c = Clipboard::new();
        let big = [b'x'; CAP + 50];
        c.set(&big);
        assert_eq!(c.get().len(), CAP);
    }

    #[test]
    fn clear_empties() {
        let mut c = Clipboard::new();
        c.set(b"data");
        c.clear();
        assert!(c.is_empty());
    }
}
