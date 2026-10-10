//! memory (split out of `activity.rs`).

use super::*;

/// How tight memory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    Normal,
    Attention,
    Critical,
}

impl Pressure {
    /// Pressure from used and total bytes: under 60 % is normal, up to 85 % deserves
    /// attention, above that it is critical.
    pub fn of(used: u64, total: u64) -> Pressure {
        if total == 0 {
            return Pressure::Normal;
        }
        let pm = used.min(total) as u128 * 1000 / total as u128;
        if pm < 600 {
            Pressure::Normal
        } else if pm < 850 {
            Pressure::Attention
        } else {
            Pressure::Critical
        }
    }

    /// The catalog key of the label.
    pub const fn key(self) -> &'static str {
        match self {
            Pressure::Normal => tk!("tasks.pressure.normal"),
            Pressure::Attention => tk!("tasks.pressure.attention"),
            Pressure::Critical => tk!("tasks.pressure.critical"),
        }
    }

    /// The label in the language in effect.
    pub fn label(self) -> &'static str {
        i18n::tr(self.key())
    }
}

/// Used share of `total` in thousandths (0 when `total` is 0).
pub fn permille(used: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    (used.min(total) as u128 * 1000 / total as u128) as u32
}
