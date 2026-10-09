use super::*;

#[test]
fn mem_free_saturates() {
    let m = MemInfo { total: 5, used: 9 };
    assert_eq!(m.free(), 0);
    assert_eq!(MemInfo { total: 10, used: 4 }.free(), 6);
}

#[test]
fn mock_kill_removes_process() {
    let mut s = MockSys::default();
    assert_eq!(s.kill(7, 9), Ok(()));
    assert_eq!(s.kill(7, 9), Err(SysErr::NoSuchProcess));
    assert_eq!(s.killed, [(7, 9)]);
}

#[test]
fn mock_sleep_advances_uptime() {
    let mut s = MockSys::default();
    let before = s.uptime_ms();
    s.sleep_ms(500);
    assert_eq!(s.uptime_ms(), before + 500);
}

#[test]
fn minimal_sys_defaults_are_unsupported() {
    let mut s = MinimalSys;
    assert_eq!(s.kill(1, 9), Err(SysErr::Unsupported));
    assert_eq!(s.ping("x", 1), Err(SysErr::Unsupported));
    assert!(s.procs().is_empty());
    assert_eq!(s.hostname(), "kitsune");
}

#[test]
fn syserr_messages() {
    for e in [
        SysErr::Unsupported,
        SysErr::NoSuchProcess,
        SysErr::Denied,
        SysErr::Network,
        SysErr::HostNotFound,
        SysErr::Timeout,
        SysErr::Cancelled,
        SysErr::Failed,
    ] {
        assert!(!e.message().is_empty());
    }
}

#[test]
fn minimal_sys_network_defaults() {
    let mut s = MinimalSys;
    assert_eq!(s.resolve("example.org"), Err(SysErr::Unsupported));
    assert_eq!(s.http_get("http://x/", 10), Err(SysErr::Unsupported));
    assert!(s.net_info().is_none());
    assert!(!s.interrupted());
}

#[test]
fn mock_interrupt_after_polls() {
    let mut s = MockSys::default();
    assert!(!s.interrupted());
    s.interrupt_after = Some(2);
    assert!(!s.interrupted()); // 2nd poll since creation
    assert!(s.interrupted());
}

#[test]
fn mock_http_truncates_to_the_requested_size() {
    let mut s = MockSys::default();
    s.web.push((
        String::from("http://a/"),
        HttpResponse {
            status: 200,
            body: alloc::vec![7; 100],
            ..HttpResponse::default()
        },
    ));
    let r = s.http_get("http://a/", 10).unwrap();
    assert_eq!((r.body.len(), r.truncated), (10, true));
    assert_eq!(s.http_get("http://b/", 10), Err(SysErr::Network));
    assert_eq!(s.fetched, ["http://a/", "http://b/"]);
}
