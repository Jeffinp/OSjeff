//! Tarefas, the task manager and resource monitor.
//!
//! Tabs: Processos, CPU, Memória, Disco, Rede. The sampler (`sysmon`) keeps the history the
//! graphs draw; `rows` builds the process table; `actions` ends and restarts tasks; `input`
//! handles keys, clicks and hover; `paint/` has one file per tab.

mod actions;
mod input;
mod layout;
mod paint;
mod rows;
mod state;
mod sysmon;

pub(crate) use state::{SEARCH_ALIASES, TarefasState, tab_name};
pub use sysmon::SysInputs;
pub(crate) use sysmon::SysMon;
