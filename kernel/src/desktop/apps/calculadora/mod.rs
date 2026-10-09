//! Calculadora: a rounded keypad with the accent on the operator column, a large display whose
//! text shrinks to fit, the pending expression and the last results above it, and a copy button.
//! The arithmetic is `kitsune_core::calc` (immediate execution, percent, sign, one memory
//! register, history); the key layout and its hit testing are
//! `kitsune_core::layout::{CALC_KEYS, calc_geom, calc_hit}`.

mod input;
mod paint;
mod state;

pub(crate) use state::CalcState;
