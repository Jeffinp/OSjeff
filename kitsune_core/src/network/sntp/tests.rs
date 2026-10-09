use super::*;
use crate::format::unixtime::DateTime;

const NOW_MS: u64 = 1_791_376_245_000; // 2026-10-07 12:30:45 UTC

fn reply_bytes(
    origin: NtpTs,
    recv: NtpTs,
    xmit: NtpTs,
    stratum: u8,
    leap: u8,
    mode: u8,
) -> [u8; 48] {
    let mut p = [0u8; 48];
    p[0] = (leap << 6) | (4 << 3) | mode;
    p[1] = stratum;
    p[2] = 6;
    p[12..16].copy_from_slice(b"GPS\0");
    origin.write(&mut p[24..32]);
    recv.write(&mut p[32..40]);
    xmit.write(&mut p[40..48]);
    p
}

fn ts(ms: u64) -> NtpTs {
    NtpTs::from_unix_ms(ms)
}

#[test]
fn request_layout() {
    let p = build_request(NtpTs(0x0102_0304_0506_0708));
    assert_eq!(p.len(), 48);
    assert_eq!(p[0], 0x23); // LI=0 VN=4 mode=3
    assert_eq!(&p[40..48], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(p[1..40].iter().all(|&b| b == 0));
}

#[test]
fn ts_roundtrip_ms() {
    for ms in [
        0u64,
        1,
        999,
        1000,
        NOW_MS,
        NOW_MS + 1,
        NOW_MS + 999,
        4_000_000_000_000,
    ] {
        let back = NtpTs::from_unix_ms(ms).to_unix_ms();
        assert!(back == ms || back + 1 == ms, "{ms} -> {back}");
    }
}

#[test]
fn ts_epoch_constants() {
    assert_eq!(NtpTs::from_unix_ms(0).0 >> 32, NTP_UNIX_OFFSET);
    assert_eq!(NtpTs(NTP_UNIX_OFFSET << 32).to_unix_ms(), 0);
}

#[test]
fn era1_after_2036() {
    // 2040-01-01T00:00:00Z
    let ms = DateTime {
        year: 2040,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    }
    .to_unix()
    .unwrap()
        * 1000;
    let t = NtpTs::from_unix_ms(ms);
    assert!(t.0 >> 32 < 0x8000_0000, "wrapped into era 1");
    assert_eq!(t.to_unix_ms(), ms);
}

#[test]
fn diff_across_era_boundary() {
    // 2036-02-07T06:28:15Z is the era rollover; 10 s each side.
    let roll = (1u64 << 32) - NTP_UNIX_OFFSET;
    let a = NtpTs::from_unix_ms(roll * 1000 - 10_000);
    let b = NtpTs::from_unix_ms(roll * 1000 + 10_000);
    assert_eq!(b.diff_ms(a), 20_000);
    assert_eq!(a.diff_ms(b), -20_000);
}

#[test]
fn diff_zero_and_sign() {
    let a = ts(NOW_MS);
    assert_eq!(a.diff_ms(a), 0);
    assert_eq!(ts(NOW_MS + 1500).diff_ms(a), 1500);
    assert_eq!(a.diff_ms(ts(NOW_MS + 1500)), -1500);
}

#[test]
fn nonce_only_touches_low_bits() {
    let a = ts(NOW_MS);
    let n = a.with_nonce(0xBEEF);
    assert_eq!(n.0 & 0xFFFF, 0xBEEF);
    assert_eq!(n.0 >> 16, a.0 >> 16);
    assert!(n.diff_ms(a).abs() <= 1);
}

#[test]
fn parse_too_short() {
    assert_eq!(parse_reply(&[0u8; 47]), Err(SntpError::TooShort));
    assert_eq!(parse_reply(&[]), Err(SntpError::TooShort));
}

#[test]
fn parse_fields() {
    let p = reply_bytes(ts(1000), ts(2000), ts(3000), 2, 0, 4);
    let r = parse_reply(&p).unwrap();
    assert_eq!((r.leap, r.version, r.mode, r.stratum), (0, 4, 4, 2));
    assert_eq!(&r.ref_id, b"GPS\0");
    assert_eq!(r.originate, ts(1000));
    assert_eq!(r.receive, ts(2000));
    assert_eq!(r.transmit, ts(3000));
}

#[test]
fn extension_bytes_are_ignored() {
    let t1 = ts(NOW_MS).with_nonce(7);
    let mut v = reply_bytes(t1, ts(NOW_MS + 5), ts(NOW_MS + 6), 2, 0, 4).to_vec();
    v.extend_from_slice(&[0xAA; 20]); // MAC / extension
    assert!(process_reply(t1, ts(NOW_MS + 10), &v).is_ok());
}

#[test]
fn happy_path_zero_offset() {
    let t1 = ts(NOW_MS).with_nonce(0x1234);
    let t2 = ts(NOW_MS + 20);
    let t3 = ts(NOW_MS + 21);
    let t4 = ts(NOW_MS + 41);
    let m = process_reply(t1, t4, &reply_bytes(t1, t2, t3, 2, 0, 4)).unwrap();
    assert!(m.offset_ms.abs() <= 1, "offset {}", m.offset_ms);
    assert!((m.delay_ms - 40).abs() <= 1, "delay {}", m.delay_ms);
    assert_eq!(m.stratum, 2);
}

#[test]
fn offset_when_local_clock_is_behind() {
    // Local clock is 1 hour behind the server.
    let skew = 3_600_000u64;
    let t1 = ts(NOW_MS - skew).with_nonce(9);
    let t2 = ts(NOW_MS + 15);
    let t3 = ts(NOW_MS + 16);
    let t4 = ts(NOW_MS - skew + 31);
    let m = process_reply(t1, t4, &reply_bytes(t1, t2, t3, 1, 0, 4)).unwrap();
    assert!(
        (m.offset_ms - skew as i64).abs() <= 2,
        "offset {}",
        m.offset_ms
    );
}

#[test]
fn offset_when_local_clock_is_ahead() {
    let skew = 86_400_000u64 * 400;
    let t1 = ts(NOW_MS + skew).with_nonce(9);
    let t2 = ts(NOW_MS + 15);
    let t3 = ts(NOW_MS + 16);
    let t4 = ts(NOW_MS + skew + 31);
    let m = process_reply(t1, t4, &reply_bytes(t1, t2, t3, 3, 0, 4)).unwrap();
    assert!((m.offset_ms + skew as i64).abs() <= 2);
}

#[test]
fn local_clock_at_1970_is_corrected() {
    // A VM whose RTC reads 1970: offset is ~ 56 years, still fine.
    let t1 = ts(5_000).with_nonce(1);
    let t4 = ts(5_060);
    let m = process_reply(
        t1,
        t4,
        &reply_bytes(t1, ts(NOW_MS + 25), ts(NOW_MS + 26), 2, 0, 4),
    )
    .unwrap();
    let clock = {
        let mut c = TrustedClock::new();
        c.apply(&m);
        c
    };
    let fixed = clock.unix_secs(5_100).unwrap();
    assert!((fixed as i64 - (NOW_MS / 1000) as i64).abs() <= 1);
}

#[test]
fn asymmetric_delay_error_bounded_by_half_delay() {
    // All 100 ms of delay on the way out: offset error is 50 ms.
    let t1 = ts(NOW_MS).with_nonce(5);
    let t2 = ts(NOW_MS + 100);
    let t3 = ts(NOW_MS + 100);
    let t4 = ts(NOW_MS + 100);
    let m = process_reply(t1, t4, &reply_bytes(t1, t2, t3, 2, 0, 4)).unwrap();
    assert!(m.offset_ms.abs() <= m.delay_ms / 2 + 1);
}

#[test]
fn rejects_wrong_mode() {
    let t1 = ts(NOW_MS).with_nonce(1);
    for mode in [0u8, 1, 2, 3, 5, 6, 7] {
        let p = reply_bytes(t1, ts(NOW_MS), ts(NOW_MS), 2, 0, mode);
        assert_eq!(
            process_reply(t1, ts(NOW_MS + 5), &p),
            Err(SntpError::BadMode),
            "mode {mode}"
        );
    }
}

#[test]
fn rejects_bad_version() {
    let t1 = ts(NOW_MS).with_nonce(1);
    let mut p = reply_bytes(t1, ts(NOW_MS), ts(NOW_MS), 2, 0, 4);
    p[0] = (2 << 3) | 4; // VN 2
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 5), &p),
        Err(SntpError::BadMode)
    );
    p[0] = (3 << 3) | 4; // VN 3 is fine
    assert!(process_reply(t1, ts(NOW_MS + 5), &p).is_ok());
}

#[test]
fn kiss_of_death_codes() {
    let t1 = ts(NOW_MS).with_nonce(1);
    for code in [b"DENY", b"RSTR", b"RATE"] {
        let mut p = reply_bytes(t1, ts(NOW_MS), ts(NOW_MS), 0, 0, 4);
        p[12..16].copy_from_slice(code);
        assert_eq!(
            process_reply(t1, ts(NOW_MS + 5), &p),
            Err(SntpError::Kiss(*code))
        );
    }
}

#[test]
fn stratum_16_and_above_refused() {
    let t1 = ts(NOW_MS).with_nonce(1);
    for s in [16u8, 17, 100, 255] {
        let p = reply_bytes(t1, ts(NOW_MS), ts(NOW_MS), s, 0, 4);
        assert_eq!(
            process_reply(t1, ts(NOW_MS + 5), &p),
            Err(SntpError::BadStratum)
        );
    }
}

#[test]
fn stratum_1_to_15_accepted() {
    let t1 = ts(NOW_MS).with_nonce(1);
    for s in 1..=15u8 {
        let p = reply_bytes(t1, ts(NOW_MS + 1), ts(NOW_MS + 2), s, 0, 4);
        assert!(process_reply(t1, ts(NOW_MS + 4), &p).is_ok(), "stratum {s}");
    }
}

#[test]
fn leap_alarm_refused_other_leaps_ok() {
    let t1 = ts(NOW_MS).with_nonce(1);
    let p = reply_bytes(t1, ts(NOW_MS + 1), ts(NOW_MS + 2), 2, 3, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 4), &p),
        Err(SntpError::Unsynchronized)
    );
    for leap in 0..=2u8 {
        let p = reply_bytes(t1, ts(NOW_MS + 1), ts(NOW_MS + 2), 2, leap, 4);
        assert!(process_reply(t1, ts(NOW_MS + 4), &p).is_ok());
    }
}

#[test]
fn origin_must_echo_request() {
    let t1 = ts(NOW_MS).with_nonce(0xAAAA);
    let other = ts(NOW_MS).with_nonce(0xAAAB);
    let p = reply_bytes(other, ts(NOW_MS + 1), ts(NOW_MS + 2), 2, 0, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 4), &p),
        Err(SntpError::OriginMismatch)
    );
    let p = reply_bytes(NtpTs::ZERO, ts(NOW_MS + 1), ts(NOW_MS + 2), 2, 0, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 4), &p),
        Err(SntpError::OriginMismatch)
    );
}

#[test]
fn zero_sent_timestamp_never_matches() {
    let p = reply_bytes(NtpTs::ZERO, ts(NOW_MS + 1), ts(NOW_MS + 2), 2, 0, 4);
    assert_eq!(
        process_reply(NtpTs::ZERO, ts(NOW_MS + 4), &p),
        Err(SntpError::OriginMismatch)
    );
}

#[test]
fn zero_server_timestamps_refused() {
    let t1 = ts(NOW_MS).with_nonce(1);
    let p = reply_bytes(t1, NtpTs::ZERO, ts(NOW_MS), 2, 0, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 4), &p),
        Err(SntpError::BadTimestamps)
    );
    let p = reply_bytes(t1, ts(NOW_MS), NtpTs::ZERO, 2, 0, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 4), &p),
        Err(SntpError::BadTimestamps)
    );
}

#[test]
fn transmit_before_receive_refused() {
    let t1 = ts(NOW_MS).with_nonce(1);
    let p = reply_bytes(t1, ts(NOW_MS + 50), ts(NOW_MS + 10), 2, 0, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 60), &p),
        Err(SntpError::BadTimestamps)
    );
}

#[test]
fn huge_delay_refused() {
    let t1 = ts(NOW_MS).with_nonce(1);
    let t4 = ts(NOW_MS + MAX_DELAY_MS as u64 + 100);
    let p = reply_bytes(t1, ts(NOW_MS + 10), ts(NOW_MS + 11), 2, 0, 4);
    assert_eq!(process_reply(t1, t4, &p), Err(SntpError::BadDelay));
}

#[test]
fn negative_delay_refused() {
    // Server claims it spent longer than the whole round trip.
    let t1 = ts(NOW_MS).with_nonce(1);
    let p = reply_bytes(t1, ts(NOW_MS + 10), ts(NOW_MS + 500), 2, 0, 4);
    assert_eq!(
        process_reply(t1, ts(NOW_MS + 20), &p),
        Err(SntpError::BadDelay)
    );
}

#[test]
fn server_in_the_past_is_implausible() {
    // Server answers 2010: before 2024-01-01.
    let ms_2010 = 1_262_304_000_000u64;
    let t1 = ts(NOW_MS).with_nonce(1);
    let p = reply_bytes(t1, ts(ms_2010), ts(ms_2010 + 1), 2, 0, 4);
    assert!(matches!(
        process_reply(t1, ts(NOW_MS + 4), &p),
        Err(SntpError::Implausible)
    ));
}

#[test]
fn server_in_year_2100_is_implausible() {
    let far = 4_102_444_800_000u64; // 2100-01-01
    let t1 = ts(far).with_nonce(1);
    let p = reply_bytes(t1, ts(far + 1), ts(far + 2), 2, 0, 4);
    assert!(matches!(
        process_reply(t1, ts(far + 4), &p),
        Err(SntpError::Implausible)
    ));
}

#[test]
fn best_prefers_lowest_delay() {
    let a = Measurement {
        offset_ms: 10,
        delay_ms: 80,
        server_unix_ms: NOW_MS,
        stratum: 1,
    };
    let b = Measurement {
        offset_ms: 20,
        delay_ms: 30,
        server_unix_ms: NOW_MS,
        stratum: 3,
    };
    let c = Measurement {
        offset_ms: 30,
        delay_ms: 30,
        server_unix_ms: NOW_MS,
        stratum: 2,
    };
    assert_eq!(best(&[a, b, c]), Some(c));
    assert_eq!(best(&[]), None);
}

#[test]
fn trusted_clock_starts_unconfirmed() {
    let c = TrustedClock::new();
    assert!(!c.confirmed());
    assert_eq!(c.source(), ClockSource::Rtc);
    assert_eq!(c.offset_ms(), 0);
    assert_eq!(c.unix_secs(NOW_MS), Some(NOW_MS / 1000));
}

#[test]
fn trusted_clock_refuses_an_implausible_rtc() {
    let c = TrustedClock::new();
    assert_eq!(c.unix_secs(0), None); // RTC at 1970 and no SNTP
    assert_eq!(c.unix_secs(5_000_000_000_000), None); // year 2128
}

#[test]
fn trusted_clock_applies_offset() {
    let mut c = TrustedClock::new();
    c.apply(&Measurement {
        offset_ms: 90_000,
        delay_ms: 12,
        server_unix_ms: NOW_MS,
        stratum: 2,
    });
    assert!(c.confirmed());
    assert_eq!(c.delay_ms(), 12);
    assert_eq!(c.unix_secs(NOW_MS), Some(NOW_MS / 1000 + 90));
}

#[test]
fn trusted_clock_negative_offset() {
    let mut c = TrustedClock::new();
    c.apply(&Measurement {
        offset_ms: -3_600_000,
        delay_ms: 5,
        server_unix_ms: NOW_MS,
        stratum: 2,
    });
    assert_eq!(c.unix_secs(NOW_MS), Some(NOW_MS / 1000 - 3600));
}

#[test]
fn trusted_clock_offset_cannot_produce_implausible_date() {
    let mut c = TrustedClock::new();
    c.apply(&Measurement {
        offset_ms: -(NOW_MS as i64) - 1000,
        delay_ms: 5,
        server_unix_ms: NOW_MS,
        stratum: 2,
    });
    assert_eq!(c.unix_secs(NOW_MS), None);
}

#[test]
fn reasons_are_ascii_and_nonempty() {
    for e in [
        SntpError::TooShort,
        SntpError::BadMode,
        SntpError::Unsynchronized,
        SntpError::Kiss(*b"DENY"),
        SntpError::BadStratum,
        SntpError::OriginMismatch,
        SntpError::BadTimestamps,
        SntpError::BadDelay,
        SntpError::Implausible,
    ] {
        assert!(!e.reason().is_empty() && e.reason().is_ascii());
    }
}

#[test]
fn garbage_never_panics() {
    let t1 = ts(NOW_MS).with_nonce(1);
    let mut seed = 0x1234_5678u32;
    for len in 0..120 {
        let mut buf = [0u8; 120];
        for b in buf.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (seed >> 24) as u8;
        }
        let _ = process_reply(t1, ts(NOW_MS + 5), &buf[..len]);
    }
}
