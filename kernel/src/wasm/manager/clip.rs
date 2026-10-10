//! The app clipboard, shared with the desktop's clipboard (one small buffer and a generation
//! counter so the desktop notices an app's copy).

use super::*;

static CLIP: RacyCell<([u8; 256], usize)> = RacyCell::new(([0; 256], 0));
static CLIP_GEN: AtomicU64 = AtomicU64::new(0);

/// Copy the shared clipboard into `out`; returns the length.
pub fn clip_get(out: &mut [u8]) -> usize {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the closure.
        let (buf, len) = unsafe { &*CLIP.get() };
        let n = (*len).min(out.len());
        out[..n].copy_from_slice(&buf[..n]);
        n
    })
}

/// Load the desktop's clipboard into the shared buffer (no generation bump).
pub fn clip_load(data: &[u8]) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the closure.
        let (buf, len) = unsafe { &mut *CLIP.get() };
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        *len = n;
    });
}

/// Replace the shared clipboard (app side) and bump its generation.
pub fn clip_set(data: &[u8]) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core with interrupts masked for the closure.
        let (buf, len) = unsafe { &mut *CLIP.get() };
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        *len = n;
    });
    CLIP_GEN.fetch_add(1, Ordering::Release);
}

/// Generation counter of app-side clipboard writes (the desktop mirrors changes).
pub fn clip_generation() -> u64 {
    CLIP_GEN.load(Ordering::Acquire)
}
