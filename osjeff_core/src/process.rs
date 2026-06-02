//! Cooperative process table. Fixed-capacity, allocation-free.
//!
//! OSjeff has no preemptive scheduler yet (that needs the IDT/PIT step), so a
//! "process" here is a managed, schedulable entity: the kernel, the compositor,
//! and each app. The Task Manager app views and controls this table; the table
//! itself is pure logic and fully unit-tested.

/// Maximum number of tracked processes.
pub const MAX_PROC: usize = 8;
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
    procs: [Process; MAX_PROC],
    count: usize,
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
        const EMPTY: Process = Process {
            pid: 0,
            name: [0; NAME_CAP],
            name_len: 0,
            kind: ProcKind::App,
            state: ProcState::Terminated,
            ticks: 0,
        };
        Self {
            procs: [EMPTY; MAX_PROC],
            count: 0,
            next_pid: 1,
            selected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn at(&self, i: usize) -> Option<&Process> {
        if i < self.count {
            Some(&self.procs[i])
        } else {
            None
        }
    }

    /// Spawn a process. Returns its pid, or `None` if the table is full.
    pub fn spawn(&mut self, name: &[u8], kind: ProcKind, state: ProcState) -> Option<u16> {
        if self.count >= MAX_PROC {
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
        self.procs[self.count] = p;
        self.count += 1;
        Some(pid)
    }

    fn index_of(&self, pid: u16) -> Option<usize> {
        (0..self.count).find(|&i| self.procs[i].pid == pid)
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
        for j in i..self.count - 1 {
            self.procs[j] = self.procs[j + 1];
        }
        self.count -= 1;
        if self.selected >= self.count && self.count > 0 {
            self.selected = self.count - 1;
        }
        true
    }

    /// Increment `ticks` for every `Running` process (one scheduler quantum).
    pub fn tick(&mut self) {
        for i in 0..self.count {
            if self.procs[i].state == ProcState::Running {
                self.procs[i].ticks += 1;
            }
        }
    }

    pub fn running(&self) -> usize {
        (0..self.count)
            .filter(|&i| self.procs[i].state == ProcState::Running)
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
        if self.count > 0 {
            self.selected = (self.selected + 1) % self.count;
        }
    }

    pub fn select_prev(&mut self) {
        if self.count > 0 {
            self.selected = (self.selected + self.count - 1) % self.count;
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
        assert!(t
            .spawn(b"overflow", ProcKind::App, ProcState::Running)
            .is_none());
        assert_eq!(t.len(), MAX_PROC);
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
