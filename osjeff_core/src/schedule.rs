//! Pure round-robin choice for the kernel scheduler.
//!
//! The kernel keeps per-thread state in atomics (wake tick, dead flag); this
//! module only decides *which slot runs next* given a "may this slot run?"
//! predicate, so the ordering rules (and the "dead threads are never picked"
//! guarantee) are unit-tested on the host.

/// The next thread to run after `cur` out of `n` slots, round-robin.
///
/// `runnable(i)` says whether slot `i` may be scheduled (ready and not dead).
/// Slots are tried in the order `cur + 1, cur + 2, ..., cur` — the current
/// thread is the *last* candidate, so it keeps the CPU when nobody else can
/// run, but only if it is still runnable itself. Returns `None` when no slot at
/// all is runnable (for example the current thread has just died and every
/// other slot is blocked); the caller must then have another way to make
/// progress (the kernel never lets the compositor, slot 0, die).
pub fn next_runnable(cur: usize, n: usize, runnable: impl Fn(usize) -> bool) -> Option<usize> {
    if n == 0 {
        return None;
    }
    (1..=n).map(|step| (cur + step) % n).find(|&c| runnable(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_order() {
        assert_eq!(next_runnable(0, 3, |_| true), Some(1));
        assert_eq!(next_runnable(1, 3, |_| true), Some(2));
        assert_eq!(next_runnable(2, 3, |_| true), Some(0));
    }

    #[test]
    fn skips_blocked_threads() {
        assert_eq!(next_runnable(0, 3, |i| i != 1), Some(2));
        assert_eq!(next_runnable(2, 3, |i| i != 0), Some(1));
    }

    #[test]
    fn current_is_the_last_candidate() {
        // Only the current thread can run: it keeps the CPU.
        assert_eq!(next_runnable(1, 3, |i| i == 1), Some(1));
    }

    #[test]
    fn dead_current_is_never_picked() {
        // The current thread is not runnable (just died): another one is chosen.
        assert_eq!(next_runnable(2, 3, |i| i != 2), Some(0));
        assert_eq!(next_runnable(1, 3, |i| i == 0 || i == 2), Some(2));
        // Nobody else is runnable: no answer instead of resuming the dead thread.
        assert_eq!(next_runnable(2, 3, |_| false), None);
    }

    #[test]
    fn single_slot_and_empty() {
        assert_eq!(next_runnable(0, 1, |_| true), Some(0));
        assert_eq!(next_runnable(0, 1, |_| false), None);
        assert_eq!(next_runnable(0, 0, |_| true), None);
    }

    #[test]
    fn wraps_with_a_cur_outside_the_range() {
        // `cur` is a slot index; a stale value larger than n still terminates.
        assert_eq!(next_runnable(5, 3, |_| true), Some(0));
    }
}
