use super::*;

#[test]
fn every_permission_value() {
    assert_eq!(with("fs=none").unwrap().fs, FsPerm::None);
    assert_eq!(with("fs=own").unwrap().fs, FsPerm::Own);
    assert_eq!(with("fs=home").unwrap().fs, FsPerm::Home);
    assert_eq!(with("net=none").unwrap().net, NetPerm::None);
    assert_eq!(with("net=http").unwrap().net, NetPerm::Http);
    assert_eq!(with("net=tcp").unwrap().net, NetPerm::Tcp);
    assert_eq!(with("clipboard=none").unwrap().clipboard, ClipPerm::None);
    assert_eq!(with("clipboard=rw").unwrap().clipboard, ClipPerm::Rw);
    assert!(NetPerm::Http.allows_http() && NetPerm::Tcp.allows_http());
    assert!(!NetPerm::None.allows_http());
}

#[test]
fn nonexistent_permissions_are_refused() {
    for kv in [
        "fs=all",
        "fs=root",
        "fs=OWN",
        "fs=",
        "fs=own,home",
        "fs=/",
        "net=any",
        "net=udp",
        "net=https",
        "net=1",
        "clipboard=r",
        "clipboard=ro",
        "clipboard=yes",
        "clipboard=",
        "abi=3",
        "abi=0",
        "abi=",
        "abi=v2",
        "resizable=2",
        "resizable=true",
        "resizable=",
    ] {
        assert!(with(kv).is_err(), "{kv}");
    }
    assert_eq!(with("fs=all"), Err(ManifestError::BadValue("fs")));
    assert_eq!(with("net=udp"), Err(ManifestError::BadValue("net")));
    assert_eq!(
        with("clipboard=ro"),
        Err(ManifestError::BadValue("clipboard"))
    );
}

#[test]
fn abi_values() {
    assert_eq!(with("abi=1").unwrap().abi, Abi::V1);
    assert_eq!(with("abi=2").unwrap().abi, Abi::V2);
}
