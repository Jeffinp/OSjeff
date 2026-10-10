//! Login throttling and the idle lock.
//!
//! [`LoginGuard`] slows down password guessing: the first few wrong attempts for a name cost
//! nothing, then each further one doubles the wait, up to a ceiling; a success, or a quiet
//! period, clears the count. [`IdleLock`] says when the screen should lock after inactivity.
//! Neither reads a clock: callers pass `now_ms` (monotonic milliseconds).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Wrong attempts allowed with no delay.
pub const FREE_ATTEMPTS: u32 = 3;
/// First delay after the free attempts, in ms; it doubles with each further failure.
pub const BASE_DELAY_MS: u64 = 1_000;
/// Longest delay.
pub const MAX_DELAY_MS: u64 = 15 * 60 * 1000;
/// A name with no failures for this long starts over.
pub const FORGET_AFTER_MS: u64 = 30 * 60 * 1000;
/// Names tracked at once (an attacker trying many names cannot grow this without bound).
pub const MAX_TRACKED: usize = 16;

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    fails: u32,
    last_fail_ms: u64,
}

/// Why a login attempt is refused before the password is even looked at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Throttled {
    /// How long to wait before trying again.
    pub retry_in_ms: u64,
}

/// Failed-login bookkeeping, per user name.
#[derive(Clone, Debug, Default)]
pub struct LoginGuard {
    entries: Vec<Entry>,
}

fn delay_for(fails: u32) -> u64 {
    if fails <= FREE_ATTEMPTS {
        return 0;
    }
    let shift = (fails - FREE_ATTEMPTS - 1).min(20);
    (BASE_DELAY_MS << shift).min(MAX_DELAY_MS)
}

impl LoginGuard {
    pub fn new() -> LoginGuard {
        LoginGuard::default()
    }

    fn prune(&mut self, now_ms: u64) {
        self.entries
            .retain(|e| now_ms.saturating_sub(e.last_fail_ms) < FORGET_AFTER_MS);
    }

    /// May `name` try now?
    pub fn check(&mut self, name: &str, now_ms: u64) -> Result<(), Throttled> {
        self.prune(now_ms);
        let Some(e) = self.entries.iter().find(|e| e.name == name) else {
            return Ok(());
        };
        let wait = delay_for(e.fails);
        if wait == 0 {
            return Ok(());
        }
        let ready = e.last_fail_ms.saturating_add(wait);
        if now_ms >= ready {
            Ok(())
        } else {
            // Never longer than the delay itself, even if the clock stepped back.
            Err(Throttled {
                retry_in_ms: (ready - now_ms).min(wait),
            })
        }
    }

    /// Record a wrong password for `name`.
    pub fn failed(&mut self, name: &str, now_ms: u64) {
        self.prune(now_ms);
        if let Some(e) = self.entries.iter_mut().find(|e| e.name == name) {
            e.fails = e.fails.saturating_add(1);
            e.last_fail_ms = now_ms;
            return;
        }
        if self.entries.len() >= MAX_TRACKED {
            // Drop the oldest: the table stays bounded and recent offenders stay tracked.
            if let Some(i) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_fail_ms)
                .map(|(i, _)| i)
            {
                self.entries.remove(i);
            }
        }
        self.entries.push(Entry {
            name: name.to_string(),
            fails: 1,
            last_fail_ms: now_ms,
        });
    }

    /// A correct login clears the record.
    pub fn succeeded(&mut self, name: &str) {
        self.entries.retain(|e| e.name != name);
    }

    /// Failures currently recorded for `name`.
    pub fn failures(&self, name: &str) -> u32 {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map_or(0, |e| e.fails)
    }
}

/// Locks after a period without input.
#[derive(Clone, Copy, Debug)]
pub struct IdleLock {
    timeout_ms: u64,
    last_input_ms: u64,
}

impl IdleLock {
    /// `timeout_ms == 0` means never.
    pub fn new(timeout_ms: u64, now_ms: u64) -> IdleLock {
        IdleLock {
            timeout_ms,
            last_input_ms: now_ms,
        }
    }

    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Input (key or pointer) at `now_ms`.
    pub fn touch(&mut self, now_ms: u64) {
        self.last_input_ms = now_ms;
    }

    /// Should the screen be locked at `now_ms`?
    pub fn due(&self, now_ms: u64) -> bool {
        self.timeout_ms != 0 && now_ms.saturating_sub(self.last_input_ms) >= self.timeout_ms
    }
}

#[cfg(test)]
mod tests;
