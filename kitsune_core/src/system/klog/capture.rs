//! capture (split out of `klog.rs`).

use super::*;

/// Rebuilds lines from the byte stream that goes to the serial port, so every
/// `serial_println!` in the kernel also reaches the ring (see
/// `kernel/src/klog.rs`). Pure: the kernel feeds it and stores what it emits.
pub struct LineAsm {
    pub(super) buf: [u8; MAX_MSG],
    pub(super) len: usize,
}

impl Default for LineAsm {
    fn default() -> Self {
        Self::new()
    }
}

impl LineAsm {
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_MSG],
            len: 0,
        }
    }

    /// Feed bytes; `emit` is called once per finished line (at `\n`, or when
    /// the line reaches [`MAX_MSG`]). `\r` is dropped and empty lines skipped.
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(&[u8])) {
        for &b in bytes {
            match b {
                b'\n' => {
                    if self.len > 0 {
                        emit(&self.buf[..self.len]);
                        self.len = 0;
                    }
                }
                b'\r' => {}
                _ => {
                    self.buf[self.len] = b;
                    self.len += 1;
                    if self.len == MAX_MSG {
                        emit(&self.buf[..self.len]);
                        self.len = 0;
                    }
                }
            }
        }
    }
}

/// Severity guessed for a plain serial line, or `None` for lines that must not
/// enter the log (the `[trace]` statistics of perf-trace builds, a per-second
/// flood that would push everything else out of the ring).
pub fn classify(line: &[u8]) -> Option<Level> {
    if line.starts_with(b"[trace]") {
        return None;
    }
    if contains_ci(line, b"FATAL") || contains_ci(line, b"KERNEL PANIC") {
        return Some(Level::Fatal);
    }
    if contains_ci(line, b" died") || contains_ci(line, b"stack overflow") {
        return Some(Level::Error);
    }
    if contains_ci(line, b"failed")
        || contains_ci(line, b"refus")
        || contains_ci(line, b"fallback")
        || contains_ci(line, b"truncated")
        || contains_ci(line, b"expired")
        || contains_ci(line, b"unavailable")
    {
        return Some(Level::Warn);
    }
    Some(Level::Info)
}
