//! processes (split out of `activity.rs`).

use super::*;

/// What a row of the process table is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskKind {
    /// A kernel thread.
    Thread,
    /// An app window's process.
    App,
    /// A bookkeeping process (`kernel`, the idle share).
    System,
}

/// State shown in the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskState {
    Running,
    Waiting,
    Suspended,
    Ended,
    Stopped,
}

impl TaskState {
    /// The catalog key of the label.
    pub const fn key(self) -> &'static str {
        match self {
            TaskState::Running => tk!("tasks.state.running"),
            TaskState::Waiting => tk!("tasks.state.waiting"),
            TaskState::Suspended => tk!("tasks.state.suspended"),
            TaskState::Ended => tk!("tasks.state.ended"),
            TaskState::Stopped => tk!("tasks.state.stopped"),
        }
    }

    /// The label in the language in effect.
    pub fn label(self) -> &'static str {
        i18n::tr(self.key())
    }
}

/// Columns of the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Pid,
    Name,
    State,
    Cpu,
    Mem,
    Up,
}

impl Column {
    pub const ALL: [Column; 6] = [
        Column::Pid,
        Column::Name,
        Column::State,
        Column::Cpu,
        Column::Mem,
        Column::Up,
    ];

    /// The catalog key of the header.
    pub const fn key(self) -> &'static str {
        match self {
            Column::Pid => tk!("tasks.col.pid"),
            Column::Name => tk!("tasks.col.name"),
            Column::State => tk!("tasks.col.state"),
            Column::Cpu => tk!("tasks.col.cpu"),
            Column::Mem => tk!("tasks.col.mem"),
            Column::Up => tk!("tasks.col.up"),
        }
    }

    /// The header text in the language in effect.
    pub fn title(self) -> &'static str {
        i18n::tr(self.key())
    }

    /// Sorting a column the first time: text ascending, numbers descending.
    pub const fn default_desc(self) -> bool {
        matches!(self, Column::Cpu | Column::Mem | Column::Up)
    }
}

/// One line of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    /// Stable identity across refreshes (so the selection survives re-sorting).
    pub id: u32,
    /// The internal name (shown in the tooltip).
    pub raw: String,
    /// The name a person sees.
    pub name: String,
    pub kind: TaskKind,
    /// Process-table id; `0` for threads.
    pub pid: u16,
    pub state: TaskState,
    /// Share of the CPU in thousandths; `None` when nobody measures it.
    pub cpu_pm: Option<u16>,
    /// Memory in KiB when known.
    pub mem_kib: Option<u32>,
    pub up_s: u32,
}

impl TaskRow {
    pub fn new(id: u32, raw: &[u8], kind: TaskKind, pid: u16, state: TaskState) -> TaskRow {
        TaskRow {
            id,
            raw: raw.iter().map(|&b| b as char).collect(),
            name: friendly_name(raw),
            kind,
            pid,
            state,
            cpu_pm: None,
            mem_kib: None,
            up_s: 0,
        }
    }

    /// Does `query` (already lowercase) occur in the friendly or internal name?
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || fold(&self.name).contains(query)
            || self.raw.to_ascii_lowercase().contains(query)
    }
}

/// Lowercase with the Portuguese accents removed (`Execução` -> `execucao`), for
/// searching and sorting names.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' => 'a',
            'é' | 'ê' | 'è' | 'É' | 'Ê' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'ô' | 'õ' | 'Ó' | 'Ô' | 'Õ' => 'o',
            'ú' | 'ü' | 'Ú' => 'u',
            'ç' | 'Ç' => 'c',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

pub(super) fn ord_of(a: &TaskRow, b: &TaskRow, col: Column) -> core::cmp::Ordering {
    match col {
        Column::Pid => a.pid.cmp(&b.pid),
        Column::Name => fold(&a.name).cmp(&fold(&b.name)),
        Column::State => (a.state as u8).cmp(&(b.state as u8)),
        Column::Cpu => a.cpu_pm.unwrap_or(0).cmp(&b.cpu_pm.unwrap_or(0)),
        Column::Mem => a.mem_kib.unwrap_or(0).cmp(&b.mem_kib.unwrap_or(0)),
        Column::Up => a.up_s.cmp(&b.up_s),
    }
}

/// Sort in place by `col` (stable: ties keep their previous order, so the list does
/// not shuffle between samples).
pub fn sort_tasks(rows: &mut [TaskRow], col: Column, descending: bool) {
    use core::cmp::Ordering;
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 {
            let o = ord_of(&rows[j - 1], &rows[j], col);
            let out_of_order = if descending {
                o == Ordering::Less
            } else {
                o == Ordering::Greater
            };
            if !out_of_order {
                break;
            }
            rows.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Totals for the footer of the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub processes: u32,
    pub threads: u32,
}

pub fn totals(rows: &[TaskRow]) -> Totals {
    let mut t = Totals::default();
    for r in rows {
        match r.kind {
            TaskKind::Thread => t.threads += 1,
            TaskKind::App | TaskKind::System => t.processes += 1,
        }
    }
    t
}
