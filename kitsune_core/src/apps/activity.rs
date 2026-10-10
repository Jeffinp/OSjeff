//! Pure helpers of the **Tarefas** app (the activity monitor): friendly process names,
//! number formatting in the language in effect, the history ring's interpolation for smooth graphs,
//! an exponential smoother, rates from wrapping counters, load averages, the memory
//! pressure level and the sortable process table.
//!
//! Everything is integer arithmetic (the kernel has no FPU in its hot paths) and
//! allocation-light; the kernel owns the sampling and the drawing.

use crate::i18n::{self, Arg, locale};
use crate::system::klog::FixedBuf;
use crate::system::sysmon::Series;
use crate::tk;
use alloc::string::String;
use core::fmt::Write;

mod formatting;
mod load;
mod memory;
mod names;
mod processes;
mod rates;
mod smoothing;
pub use formatting::*;
pub use load::*;
pub use memory::*;
pub use names::*;
pub use processes::*;
pub use rates::*;
pub use smoothing::*;

// ------------------------------------------------------------------ names

// ------------------------------------------------------------------ formatting

// ------------------------------------------------------------------ smoothing

// ------------------------------------------------------------------ rates

// ------------------------------------------------------------------ load

// ------------------------------------------------------------------ memory pressure

// ------------------------------------------------------------------ process table

#[cfg(test)]
mod tests;
