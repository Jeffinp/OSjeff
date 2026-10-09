//! Tarefas, the task manager and resource monitor.

pub(super) mod logic;

pub use logic::SysInputs;
pub(crate) use logic::{SEARCH_ALIASES, SysMon, TarefasState, tab_name};
