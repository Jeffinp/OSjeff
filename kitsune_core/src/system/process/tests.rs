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
