//! State of a calculator window and the labels of its keys.

use crate::desktop::*;
use core::cell::Cell;
use core::ops::{Deref, DerefMut};
use kitsune_core::calc::pretty;

/// Milliseconds a key stays lit after a keyboard press, and the "Copiado" note stays up.
pub(super) const FLASH_MS: u32 = 140;
pub(super) const COPIED_MS: u32 = 1400;

// Hover keys: the key index (row * 4 + col + 1), or one of these.
pub(super) const H_COPY: u32 = 100;
pub(super) const H_DOWN: u32 = 1 << 31;

/// State of a calculator window: the arithmetic and what the pointer is doing.
pub(crate) struct CalcState {
    pub calc: Calc,
    pub(super) hover: Cell<u32>,
    /// A key lit by the keyboard, and for how many more milliseconds.
    pub(super) flash: Cell<(u8, u32)>,
    pub(super) copied_ms: Cell<u32>,
}

impl CalcState {
    pub(crate) fn new() -> Self {
        Self {
            calc: Calc::new(),
            hover: Cell::new(0),
            flash: Cell::new((0, 0)),
            copied_ms: Cell::new(0),
        }
    }
}

impl Deref for CalcState {
    type Target = Calc;
    fn deref(&self) -> &Calc {
        &self.calc
    }
}

impl DerefMut for CalcState {
    fn deref_mut(&mut self) -> &mut Calc {
        &mut self.calc
    }
}

/// What a key looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Digit,
    Function,
    Operator,
    Equals,
    Memory,
}

pub(super) fn role(k: u8) -> Role {
    match k {
        b'0'..=b'9' | b'.' | b'n' => Role::Digit,
        b'C' | 0x08 | b'%' => Role::Function,
        b'+' | b'-' | b'*' | b'/' => Role::Operator,
        b'=' => Role::Equals,
        _ => Role::Memory,
    }
}

pub(super) fn label(k: u8) -> &'static str {
    match k {
        0x01 => "MC",
        0x02 => "MR",
        0x03 => "M−",
        0x04 => "M+",
        b'C' => "C",
        0x08 => "⌫",
        b'%' => "%",
        b'/' => "÷",
        b'*' => "×",
        b'-' => "−",
        b'+' => "+",
        b'=' => "=",
        b'n' => "±",
        b'.' => {
            if kitsune_core::i18n::locale::decimal_sep(kitsune_core::i18n::lang()) == ',' {
                ","
            } else {
                "."
            }
        }
        b'0' => "0",
        b'1' => "1",
        b'2' => "2",
        b'3' => "3",
        b'4' => "4",
        b'5' => "5",
        b'6' => "6",
        b'7' => "7",
        b'8' => "8",
        _ => "9",
    }
}

/// A history line in the typography of the language in effect: `12 × 3 = 36`, with its
/// decimal separator.
pub(super) fn pretty_line(line: &[u8]) -> String {
    let mut out = String::new();
    for part in line.split(|&b| b == b' ') {
        if !out.is_empty() {
            out.push(' ');
        }
        match part {
            b"*" => out.push('×'),
            b"/" => out.push('÷'),
            b"-" => out.push('−'),
            b"+" | b"=" => out.push(part[0] as char),
            _ => out.push_str(&pretty(part)),
        }
    }
    out
}
