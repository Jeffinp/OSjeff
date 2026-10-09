//! Process table: a bounded list (at most [`MAX_PROC`] entries) that grows on
//! demand, so every open window can own a process of its own.
//!
//! A "process" here is a bookkeeping entry (name, state, uptime), not an address
//! space: the kernel, the compositor, and each app window. Preemption happens
//! between *kernel threads* (see `kernel/src/sched.rs`), which this table does not
//! model. The Task Manager app views and controls this table; the table
//! itself is pure logic and fully unit-tested.

use alloc::vec::Vec;

/// Maximum number of tracked processes: the two system entries plus one per
/// window of a full window table (`winman::DEFAULT_MAX_WINDOWS`), with slack.
pub const MAX_PROC: usize = 48;
const NAME_CAP: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    Running,
    Suspended,
    Terminated,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcKind {
    /// Cannot be killed by the user (kernel, compositor).
    System,
    /// User application; can be suspended/terminated.
    App,
}

#[derive(Clone, Copy)]
pub struct Process {
    pub pid: u16,
    name: [u8; NAME_CAP],
    name_len: usize,
    pub kind: ProcKind,
    pub state: ProcState,
    /// Accumulated scheduler ticks (a cheap "CPU time" proxy).
    pub ticks: u32,
}

impl Process {
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

pub struct ProcessTable {
    procs: Vec<Process>,
    next_pid: u16,
    selected: usize,
}

impl Default for ProcessTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTable {
    pub fn new() -> Self {
        Self {
            procs: Vec::new(),
            next_pid: 1,
            selected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.procs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.procs.is_empty()
    }

    pub fn at(&self, i: usize) -> Option<&Process> {
        self.procs.get(i)
    }

    /// Spawn a process. Returns its pid, or `None` if the table is full.
    pub fn spawn(&mut self, name: &[u8], kind: ProcKind, state: ProcState) -> Option<u16> {
        if self.procs.len() >= MAX_PROC {
            return None;
        }
        let pid = self.next_pid;
        self.next_pid += 1;
        let n = name.len().min(NAME_CAP);
        let mut p = Process {
            pid,
            name: [0; NAME_CAP],
            name_len: n,
            kind,
            state,
            ticks: 0,
        };
        p.name[..n].copy_from_slice(&name[..n]);
        self.procs.push(p);
        Some(pid)
    }

    fn index_of(&self, pid: u16) -> Option<usize> {
        self.procs.iter().position(|p| p.pid == pid)
    }

    pub fn get(&self, pid: u16) -> Option<&Process> {
        self.index_of(pid).map(|i| &self.procs[i])
    }

    pub fn set_state(&mut self, pid: u16, state: ProcState) -> bool {
        match self.index_of(pid) {
            Some(i) => {
                self.procs[i].state = state;
                true
            }
            None => false,
        }
    }

    /// Remove a process from the table. System processes are protected and
    /// cannot be killed (`false`). Selection is kept in range.
    pub fn kill(&mut self, pid: u16) -> bool {
        let Some(i) = self.index_of(pid) else {
            return false;
        };
        if self.procs[i].kind == ProcKind::System {
            return false;
        }
        self.procs.remove(i);
        let count = self.procs.len();
        if self.selected >= count && count > 0 {
            self.selected = count - 1;
        }
        true
    }

    /// Increment `ticks` for every `Running` process (one scheduler quantum).
    pub fn tick(&mut self) {
        for p in self.procs.iter_mut() {
            if p.state == ProcState::Running {
                p.ticks += 1;
            }
        }
    }

    pub fn running(&self) -> usize {
        self.procs
            .iter()
            .filter(|p| p.state == ProcState::Running)
            .count()
    }

    // ---- selection (Task Manager cursor) ----

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_pid(&self) -> Option<u16> {
        self.at(self.selected).map(|p| p.pid)
    }

    pub fn select_next(&mut self) {
        if !self.procs.is_empty() {
            self.selected = (self.selected + 1) % self.procs.len();
        }
    }

    pub fn select_prev(&mut self) {
        let n = self.procs.len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> ProcessTable {
        let mut t = ProcessTable::new();
        t.spawn(b"kernel", ProcKind::System, ProcState::Running);
        t.spawn(b"shell", ProcKind::App, ProcState::Running);
        t.spawn(b"editor", ProcKind::App, ProcState::Suspended);
        t
    }

    #[test]
    fn spawn_assigns_incrementing_pids() {
        let t = table();
        assert_eq!(t.len(), 3);
        assert_eq!(t.at(0).unwrap().pid, 1);
        assert_eq!(t.at(1).unwrap().pid, 2);
        assert_eq!(t.at(2).unwrap().pid, 3);
        assert_eq!(t.at(0).unwrap().name(), b"kernel");
    }

    #[test]
    fn spawn_caps_name_length() {
        let mut t = ProcessTable::new();
        let pid = t.spawn(
            b"a-very-long-process-name",
            ProcKind::App,
            ProcState::Running,
        );
        assert!(pid.is_some());
        assert_eq!(t.at(0).unwrap().name().len(), NAME_CAP);
    }

    #[test]
    fn spawn_full_table_returns_none() {
        let mut t = ProcessTable::new();
        for _ in 0..MAX_PROC {
            assert!(t.spawn(b"p", ProcKind::App, ProcState::Running).is_some());
        }
        assert!(
            t.spawn(b"overflow", ProcKind::App, ProcState::Running)
                .is_none()
        );
        assert_eq!(t.len(), MAX_PROC);
    }

    #[test]
    fn table_holds_one_process_per_window_of_a_full_window_table() {
        // Two system entries + a full table of windows must fit.
        const { assert!(MAX_PROC >= 2 + crate::windowing::winman::DEFAULT_MAX_WINDOWS) };
    }

    #[test]
    fn killing_in_the_middle_keeps_order_and_pids_unique() {
        let mut t = ProcessTable::new();
        let pids: Vec<u16> = (0..20)
            .map(|_| t.spawn(b"p", ProcKind::App, ProcState::Running).unwrap())
            .collect();
        for &p in pids.iter().step_by(3) {
            assert!(t.kill(p));
        }
        let left: Vec<u16> = (0..t.len()).map(|i| t.at(i).unwrap().pid).collect();
        let mut sorted = left.clone();
        sorted.sort_unstable();
        assert_eq!(left, sorted); // spawn order preserved
        // A new spawn never reuses a dead pid.
        let fresh = t.spawn(b"p", ProcKind::App, ProcState::Running).unwrap();
        assert!(!pids.contains(&fresh));
    }

    #[test]
    fn get_and_set_state() {
        let mut t = table();
        assert_eq!(t.get(2).unwrap().state, ProcState::Running);
        assert!(t.set_state(2, ProcState::Suspended));
        assert_eq!(t.get(2).unwrap().state, ProcState::Suspended);
        assert!(!t.set_state(999, ProcState::Running));
    }

    #[test]
    fn kill_removes_app_and_shifts() {
        let mut t = table();
        assert!(t.kill(2)); // kill shell
        assert_eq!(t.len(), 2);
        assert_eq!(t.at(0).unwrap().pid, 1);
        assert_eq!(t.at(1).unwrap().pid, 3); // editor shifted up
    }

    #[test]
    fn kill_protects_system_process() {
        let mut t = table();
        assert!(!t.kill(1)); // kernel is System
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn kill_unknown_pid_is_false() {
        let mut t = table();
        assert!(!t.kill(123));
    }

    #[test]
    fn tick_only_advances_running() {
        let mut t = table();
        t.tick();
        t.tick();
        assert_eq!(t.get(1).unwrap().ticks, 2); // kernel running
        assert_eq!(t.get(2).unwrap().ticks, 2); // shell running
        assert_eq!(t.get(3).unwrap().ticks, 0); // editor suspended
    }

    #[test]
    fn running_count() {
        let t = table();
        assert_eq!(t.running(), 2);
    }

    #[test]
    fn selection_wraps_both_directions() {
        let mut t = table();
        assert_eq!(t.selected(), 0);
        t.select_prev();
        assert_eq!(t.selected(), 2); // wrapped to end
        t.select_next();
        assert_eq!(t.selected(), 0); // wrapped to start
        t.select_next();
        assert_eq!(t.selected(), 1);
        assert_eq!(t.selected_pid(), Some(2));
    }

    #[test]
    fn selection_clamped_after_kill() {
        let mut t = table();
        t.select_next();
        t.select_next(); // selected = 2 (editor)
        t.kill(3); // remove editor
        assert!(t.selected() < t.len());
        assert_eq!(t.selected(), 1);
    }

    #[test]
    fn empty_table_selection_is_safe() {
        let mut t = ProcessTable::new();
        t.select_next();
        t.select_prev();
        assert_eq!(t.selected_pid(), None);
        assert!(t.is_empty());
    }
}
