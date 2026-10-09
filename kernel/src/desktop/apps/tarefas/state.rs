//! State of a Tarefas window: tabs, selection, scroll and the hit-target ids.

use crate::desktop::*;
use core::cell::Cell;
use kitsune_core::activity::{Column, Glide, TaskRow};
use kitsune_core::i18n;
use kitsune_core::tk;

// ------------------------------------------------------------------ state

/// The tab names (catalog keys; see [`tab_names`]).
pub(crate) const TAB_KEYS: [&str; 5] = [
    tk!("tasks.tab.cpu"),
    tk!("tasks.tab.memory"),
    tk!("tasks.tab.disk"),
    tk!("tasks.tab.network"),
    tk!("tasks.tab.processes"),
];
pub(crate) const TAB_PROCESSES: u8 = 4;

/// The tab names in the language in effect.
pub(crate) fn tab_names() -> [&'static str; 5] {
    TAB_KEYS.map(i18n::tr)
}

/// The name of tab `tab` in the language in effect.
pub(crate) fn tab_name(tab: u8) -> &'static str {
    i18n::tr(TAB_KEYS[usize::from(tab).min(TAB_KEYS.len() - 1)])
}

/// Words Busca also knows Tarefas by (catalog keys, matched in both languages), and the tab each
/// opens (the old Monitor lives on as these).
pub(crate) const SEARCH_ALIASES: [(&str, u8); 6] = [
    (tk!("tasks.alias.monitor"), 0),
    (tk!("tasks.alias.performance"), 0),
    (tk!("tasks.alias.processor"), 0),
    (tk!("tasks.tab.memory"), 1),
    (tk!("tasks.tab.disk"), 2),
    (tk!("tasks.tab.network"), 3),
];

pub(super) const ROW_H: i32 = 28;
pub(super) const HEAD_H: i32 = 28;
pub(super) const FOOT_H: i32 = 52;
pub(super) const PAD: i32 = 16;

/// Per-window state.
pub(crate) struct TarefasState {
    pub tab: u8,
    pub sort: Column,
    pub desc: bool,
    /// Rows of the current tab (filtered, sorted).
    pub rows: Vec<TaskRow>,
    /// The app icon of each row, when it has one.
    pub(super) icons: Vec<Option<Icon>>,
    /// CPU share of each row at the previous sample, for the bars to glide from.
    pub(super) prev_cpu: Vec<(u32, u16)>,
    pub sel: Option<u32>,
    pub query: String,
    pub search_focus: bool,
    /// A row waiting for the user's "Encerrar" confirmation.
    pub confirm: Option<u32>,
    /// What the pointer is over (see the `H_*` keys); the top bit is the pressed state.
    pub hover: Cell<u32>,
    /// Scroll position of the process table in pixels.
    pub(super) scroll: Glide,
    pub(super) sb: kitsune_core::widgets::ScrollbarFade,
    /// A short message after an action (`(text, error)`), cleared by the next sample.
    pub msg: Option<(String, bool)>,
}

impl TarefasState {
    pub(crate) fn new(tab: u8) -> Self {
        Self {
            tab: tab.min(TAB_PROCESSES),
            sort: Column::Cpu,
            desc: true,
            rows: Vec::new(),
            icons: Vec::new(),
            prev_cpu: Vec::new(),
            sel: None,
            query: String::new(),
            search_focus: false,
            confirm: None,
            hover: Cell::new(0),
            scroll: Glide::at(0),
            sb: kitsune_core::widgets::ScrollbarFade::new(),
            msg: None,
        }
    }

    /// Approximate heap this window holds (its row cache).
    pub(crate) fn heap_bytes(&self) -> usize {
        self.rows.capacity() * core::mem::size_of::<TaskRow>() + self.icons.capacity() * 2
    }

    pub(super) fn sel_index(&self) -> Option<usize> {
        let id = self.sel?;
        self.rows.iter().position(|r| r.id == id)
    }
}

// Hover keys: what the pointer is over, so the window repaints only when it changes.
pub(super) const H_DOWN: u32 = 1 << 31;
pub(super) const H_END: u32 = 0x10;
pub(super) const H_RESTART: u32 = 0x11;
pub(super) const H_CANCEL: u32 = 0x12;
pub(super) const H_OK: u32 = 0x13;
pub(super) const H_SEARCH: u32 = 0x14;
pub(super) const H_TAB: u32 = 0x20; // + tab
pub(super) const H_HEAD: u32 = 0x100; // + column
pub(super) const H_ROW: u32 = 0x1000; // + row index
pub(super) const H_SAMPLE: u32 = 0x10000; // + sample index

impl TarefasState {
    pub(super) fn scroll_target(&self) -> i32 {
        self.scroll.target()
    }
}
