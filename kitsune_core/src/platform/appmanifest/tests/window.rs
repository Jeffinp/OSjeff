use super::*;

#[test]
fn window_limits() {
    assert_eq!(with("win_w=63"), Err(ManifestError::BadValue("win_w")));
    assert_eq!(with("win_w=1281"), Err(ManifestError::OverLimit("win_w")));
    assert_eq!(with("win_h=801"), Err(ManifestError::OverLimit("win_h")));
    assert_eq!(with("win_h=64").unwrap().win_h, 64);
    assert!(with("win_w=1280\nwin_h=800").is_ok());
    // min above default
    assert_eq!(
        with("win_w=300\nwin_min_w=301"),
        Err(ManifestError::BadValue("win_min_w"))
    );
    assert_eq!(
        with("win_h=300\nwin_min_h=301"),
        Err(ManifestError::BadValue("win_min_h"))
    );
    // a small default shrinks the default minimum with it
    let m = with("win_w=100\nwin_h=100").unwrap();
    assert_eq!((m.win_min_w, m.win_min_h), (100, 100));
}

#[test]
fn content_for_clamps_to_minimum_and_ceiling() {
    let m = with("win_w=400\nwin_h=300\nwin_min_w=200\nwin_min_h=150").unwrap();
    assert_eq!(m.content_for(10, 10), (200, 150));
    assert_eq!(m.content_for(5000, 5000), (MAX_WIN_W, MAX_WIN_H));
    assert_eq!(m.content_for(400, 300), (400, 300));
}

#[test]
fn legacy_manifest_matches_the_old_single_app() {
    let m = Manifest::legacy("snake", "Snake");
    assert_eq!(m.abi, Abi::V1);
    assert_eq!((m.win_w, m.win_h), (692, 414));
    assert!(!m.resizable);
    assert_eq!(m.fs, FsPerm::None);
    assert_eq!(m.net, NetPerm::None);
    assert_eq!(m.granted().mem_bytes, 24 << 20);
    assert_eq!(m.granted().fuel_frame, 20_000_000);
}
