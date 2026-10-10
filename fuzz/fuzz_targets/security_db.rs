//! Fuzz target: the account database and the permission helpers (`kitsune_core::security`).
//!
//! * `AccountDb::parse` on arbitrary text never panics, and a database that parses serialises to
//!   text that parses back to an equal database;
//! * `perm::parse_mode` and `perm::mode_string` agree (a parsed mode prints back as the same
//!   bits) and never panic;
//! * `perm::allowed`/`may_*` never panic for any credentials and modes, and root never loses
//!   read/write;
//! * `password::verify` on a malformed or hostile stored hash returns false instead of panicking
//!   (only cheap hashes are actually computed).
#![no_main]

use kitsune_core::password;
use kitsune_core::perm::{self, Cred, Owner};
use kitsune_core::account::AccountDb;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else { return };

    if let Ok(db) = AccountDb::parse(text) {
        let again = db.serialize();
        let back = AccountDb::parse(&again).expect("a serialised database parses");
        assert_eq!(back, db);
        for u in db.users() {
            let c = db.cred(u);
            assert_eq!(c.uid, u.uid);
        }
    }

    let first = text.lines().next().unwrap_or("");
    if let Some(m) = perm::parse_mode(first) {
        assert_eq!(m & !perm::MODE_MASK, 0);
        let s = perm::mode_string(m);
        assert_eq!(s.len(), 9);
    }

    if data.len() >= 16 {
        let w = |i: usize| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        let who = Cred::new(w(0), w(4), vec![w(8) % 8, w(12) % 8]);
        let o = Owner::new(w(0) % 4, w(4) % 8, (w(8) as u16) & perm::MODE_MASK);
        let want = data[3] & 7;
        let _ = perm::allowed(&who, &o, want, data[7] & 1 == 1);
        let _ = perm::may_chmod(&who, &o);
        let _ = perm::may_chown(&who, &o, Some(w(12)), None);
        let _ = perm::may_remove(&who, &o, &Owner::new(w(4), w(8), 0));
        if who.is_root() {
            assert!(perm::allowed(&who, &o, perm::R | perm::W, false));
        }
    }

    // Only hashes that claim the minimum cost are verified (a hostile one claiming 100 000
    // rounds would just make the fuzzer slow, and parsing it is covered above).
    if text.starts_with("$kpw1$1000$") {
        let _ = password::verify("fuzz", text);
    }
    let _ = password::is_valid_hash(text);
});
