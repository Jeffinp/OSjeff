use super::*;

#[test]
fn quota_ceilings_are_enforced() {
    assert_eq!(with("mem_mib=25"), Err(ManifestError::OverLimit("mem_mib")));
    assert_eq!(with("mem_mib=24").unwrap().mem_mib, 24);
    assert_eq!(with("mem_mib=0"), Err(ManifestError::BadValue("mem_mib")));
    assert_eq!(
        with("mem_mib=99999"),
        Err(ManifestError::OverLimit("mem_mib"))
    );
    assert_eq!(
        with("fuel_frame=20000001"),
        Err(ManifestError::OverLimit("fuel_frame"))
    );
    assert_eq!(with("fuel_frame=20000000").unwrap().fuel_frame, 20_000_000);
    assert_eq!(
        with("fuel_frame=9999"),
        Err(ManifestError::BadValue("fuel_frame"))
    );
    assert_eq!(with("fuel_frame=10000").unwrap().fuel_frame, 10_000);
    assert_eq!(
        with("fuel_frame=4294967295"),
        Err(ManifestError::OverLimit("fuel_frame"))
    );
    assert_eq!(
        with("fs=own\ndisk_kib=4097"),
        Err(ManifestError::OverLimit("disk_kib"))
    );
    assert_eq!(with("fs=own\ndisk_kib=4096").unwrap().disk_kib, 4096);
    assert_eq!(with("max_fds=33"), Err(ManifestError::OverLimit("max_fds")));
    assert_eq!(with("max_fds=0"), Err(ManifestError::BadValue("max_fds")));
    assert_eq!(with("max_fds=32").unwrap().max_fds, 32);
    assert_eq!(
        with("tick_ms=60001"),
        Err(ManifestError::OverLimit("tick_ms"))
    );
    assert_eq!(with("tick_ms=15"), Err(ManifestError::BadValue("tick_ms")));
    assert_eq!(with("tick_ms=16").unwrap().tick_ms, 16);
    assert_eq!(with("tick_ms=0").unwrap().tick_ms, 0);
}

#[test]
fn numbers_are_strict_decimal() {
    for bad in [
        "+4",
        "-4",
        " 4",
        "4 ",
        "0x10",
        "1_0",
        "04",
        "4.0",
        "99999999999",
        "٣",
    ] {
        assert!(with(&format!("mem_mib={bad}")).is_err(), "{bad:?}");
    }
}

#[test]
fn disk_quota_needs_a_filesystem_permission() {
    assert_eq!(
        with("disk_kib=100"),
        Err(ManifestError::BadValue("disk_kib"))
    );
    assert_eq!(with("fs=none\ndisk_kib=0").unwrap().disk_kib, 0);
    assert_eq!(with("fs=own").unwrap().disk_kib, DEFAULT_DISK_KIB);
    assert_eq!(with("fs=home").unwrap().disk_kib, DEFAULT_DISK_KIB);
    assert_eq!(with("fs=own\ndisk_kib=0").unwrap().disk_kib, 0);
}

#[test]
fn granted_quotas() {
    let q = parse(MIN).unwrap().granted();
    assert_eq!(q.mem_bytes, 8 << 20);
    assert_eq!(q.fuel_frame, DEFAULT_FUEL_FRAME);
    assert_eq!(q.disk_bytes, 0);
    assert_eq!(q.max_fds, 16);
    // A manifest built by hand with absurd numbers is clamped again.
    let mut m = parse(MIN).unwrap();
    m.mem_mib = 4000;
    m.fuel_frame = u64::MAX;
    m.disk_kib = u32::MAX;
    m.max_fds = u32::MAX;
    let q = m.granted();
    assert_eq!(q.mem_bytes, (MAX_MEM_MIB as usize) << 20);
    assert_eq!(q.fuel_frame, MAX_FUEL_FRAME);
    assert_eq!(q.disk_bytes, MAX_DISK_KIB as u64 * 1024);
    assert_eq!(q.max_fds, MAX_FDS as usize);
    m.mem_mib = 0;
    m.fuel_frame = 0;
    m.max_fds = 0;
    let q = m.granted();
    assert_eq!(q.mem_bytes, 1 << 20);
    assert_eq!(q.fuel_frame, MIN_FUEL_FRAME);
    assert_eq!(q.max_fds, 1);
}
