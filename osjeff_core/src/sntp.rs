//! SNTP (RFC 4330) client logic, pure: build the 48-byte request, parse and
//! validate the reply, compute offset and delay, and keep a small "trusted
//! clock" model. The kernel owns the UDP socket, the DNS lookup and the local
//! clock; everything that decides whether a reply is believable lives here.
//!
//! Why SNTP at all: TLS certificates carry validity dates, and the machine's
//! only clock is the CMOS RTC, which anybody can have set wrong (or which a
//! hypervisor leaves at 1970). The offset measured here is applied to the
//! *certificate check* only; the RTC is not written.
//!
//! Hardening, in the order a packet meets it:
//!
//! * the reply must be exactly the 48-byte basic packet or longer with only
//!   extension/MAC bytes (those are ignored, never trusted);
//! * mode 4 (server) and version 3 or 4; broadcast/symmetric modes are refused;
//! * leap indicator 3 ("alarm: clock unsynchronized") is refused;
//! * stratum 0 is a kiss-o'-death (`DENY`, `RSTR`, `RATE`...) and is reported as
//!   such; stratum 16 and above are "unsynchronized" and refused;
//! * the *originate* timestamp must echo the transmit timestamp we sent, whose
//!   low 16 bits are random: an off-path attacker cannot forge a reply without
//!   guessing them;
//! * the server's receive/transmit timestamps must be non-zero and in order;
//! * the resulting server time must be a plausible date (2024-01-01..2100) and
//!   the round trip must be sane ([`MAX_DELAY_MS`]).

use crate::unixtime::{NTP_UNIX_OFFSET, is_plausible};

/// UDP port.
pub const PORT: u16 = 123;
/// Size of the basic NTP packet.
pub const PACKET_LEN: usize = 48;
/// Largest round-trip delay accepted (ms). Beyond it the offset error bound
/// (half the delay) is too loose to trust.
pub const MAX_DELAY_MS: i64 = 5_000;
/// Largest absolute offset accepted from one measurement (ms): about 70 years.
/// The plausibility check on the resulting date is the real bound.
pub const MAX_OFFSET_MS: i64 = 70 * 365 * 86_400 * 1000;

/// An NTP timestamp: 32.32 fixed point seconds since 1900-01-01 (era 0), the
/// wire format. Era rollover (2036) is handled in [`NtpTs::to_unix_ms`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NtpTs(pub u64);

impl NtpTs {
    pub const ZERO: NtpTs = NtpTs(0);

    /// From Unix milliseconds. The sub-millisecond fraction is zero.
    pub fn from_unix_ms(ms: u64) -> NtpTs {
        let secs = (ms / 1000).wrapping_add(NTP_UNIX_OFFSET) & 0xFFFF_FFFF;
        let frac = ((ms % 1000) << 32) / 1000;
        NtpTs((secs << 32) | frac)
    }

    /// To Unix milliseconds. Seconds with the top bit clear belong to era 1
    /// (2036..2104), the rest to era 0 (1968..2036).
    pub fn to_unix_ms(self) -> u64 {
        let secs = self.0 >> 32;
        let frac = self.0 & 0xFFFF_FFFF;
        let era_secs = if secs & 0x8000_0000 == 0 {
            secs + (1 << 32)
        } else {
            secs
        };
        let ms = (frac * 1000) >> 32;
        era_secs.wrapping_sub(NTP_UNIX_OFFSET) * 1000 + ms
    }

    /// Replace the low 16 bits (about 15 microseconds of fraction) with `r`.
    pub fn with_nonce(self, r: u16) -> NtpTs {
        NtpTs((self.0 & !0xFFFF) | u64::from(r))
    }

    /// Signed difference `self - other` in milliseconds (wrapping 32.32
    /// arithmetic, so it is right across the era boundary).
    pub fn diff_ms(self, other: NtpTs) -> i64 {
        let d = self.0.wrapping_sub(other.0) as i64; // 32.32, +-68 years
        // Round to nearest ms: (d * 1000) >> 32, in i128 to avoid overflow.
        ((i128::from(d) * 1000 + (1 << 31)) >> 32) as i64
    }

    fn write(self, out: &mut [u8]) {
        out[..8].copy_from_slice(&self.0.to_be_bytes());
    }

    fn read(b: &[u8]) -> NtpTs {
        NtpTs(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

/// Build the client request (mode 3, version 4) carrying `transmit` as the
/// transmit timestamp. The server echoes it back as the originate timestamp.
pub fn build_request(transmit: NtpTs) -> [u8; PACKET_LEN] {
    let mut p = [0u8; PACKET_LEN];
    p[0] = (4 << 3) | 3; // LI 0, VN 4, mode 3 (client)
    transmit.write(&mut p[40..48]);
    p
}

/// A parsed server reply (basic fields only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub leap: u8,
    pub version: u8,
    pub mode: u8,
    pub stratum: u8,
    pub poll: u8,
    pub ref_id: [u8; 4],
    pub reference: NtpTs,
    pub originate: NtpTs,
    pub receive: NtpTs,
    pub transmit: NtpTs,
}

/// Why a reply was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SntpError {
    /// Shorter than 48 bytes.
    TooShort,
    /// Not a server reply (mode other than 4) or an unsupported version.
    BadMode,
    /// Leap indicator 3: the server says its own clock is unsynchronized.
    Unsynchronized,
    /// Stratum 0 with a kiss code (`DENY`, `RSTR`, `RATE`, ...): do not retry
    /// this server now.
    Kiss([u8; 4]),
    /// Stratum 16 or more: the server is not synchronized.
    BadStratum,
    /// The originate timestamp is not the one we sent (stale or forged).
    OriginMismatch,
    /// Zero or out-of-order server timestamps.
    BadTimestamps,
    /// Round trip longer than [`MAX_DELAY_MS`] or negative.
    BadDelay,
    /// The server time is not a plausible date (before 2024 or after 2099).
    Implausible,
}

impl SntpError {
    /// Short ASCII reason for the serial log.
    pub fn reason(self) -> &'static str {
        match self {
            SntpError::TooShort => "short packet",
            SntpError::BadMode => "not a server reply",
            SntpError::Unsynchronized => "server unsynchronized (leap 3)",
            SntpError::Kiss(_) => "kiss-o'-death",
            SntpError::BadStratum => "bad stratum",
            SntpError::OriginMismatch => "origin timestamp mismatch",
            SntpError::BadTimestamps => "bad server timestamps",
            SntpError::BadDelay => "implausible delay",
            SntpError::Implausible => "implausible date",
        }
    }
}

/// Parse the 48-byte header of `buf`; extra bytes (extensions, MAC) are
/// ignored. Only structural checks here, see [`validate`] for the rest.
pub fn parse_reply(buf: &[u8]) -> Result<Reply, SntpError> {
    if buf.len() < PACKET_LEN {
        return Err(SntpError::TooShort);
    }
    let b0 = buf[0];
    Ok(Reply {
        leap: b0 >> 6,
        version: (b0 >> 3) & 7,
        mode: b0 & 7,
        stratum: buf[1],
        poll: buf[2],
        ref_id: [buf[12], buf[13], buf[14], buf[15]],
        reference: NtpTs::read(&buf[16..24]),
        originate: NtpTs::read(&buf[24..32]),
        receive: NtpTs::read(&buf[32..40]),
        transmit: NtpTs::read(&buf[40..48]),
    })
}

/// Everything about a reply that does not need our clock: mode, version,
/// leap, stratum/kiss, origin echo and timestamp sanity.
pub fn validate(r: &Reply, sent: NtpTs) -> Result<(), SntpError> {
    if r.mode != 4 || !(3..=4).contains(&r.version) {
        return Err(SntpError::BadMode);
    }
    if r.stratum == 0 {
        return Err(SntpError::Kiss(r.ref_id));
    }
    if r.stratum >= 16 {
        return Err(SntpError::BadStratum);
    }
    if r.leap == 3 {
        return Err(SntpError::Unsynchronized);
    }
    if r.originate != sent || sent == NtpTs::ZERO {
        return Err(SntpError::OriginMismatch);
    }
    if r.receive == NtpTs::ZERO || r.transmit == NtpTs::ZERO {
        return Err(SntpError::BadTimestamps);
    }
    // T3 must not precede T2 (server processing time is non-negative).
    if r.transmit.diff_ms(r.receive) < 0 {
        return Err(SntpError::BadTimestamps);
    }
    Ok(())
}

/// One completed measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measurement {
    /// Add this to the local clock to get server time (ms).
    pub offset_ms: i64,
    /// Round-trip delay without server processing time (ms).
    pub delay_ms: i64,
    /// Server transmit time as Unix milliseconds.
    pub server_unix_ms: u64,
    pub stratum: u8,
}

/// The standard on-wire computation (RFC 4330 section 5):
/// `offset = ((T2 - T1) + (T3 - T4)) / 2`, `delay = (T4 - T1) - (T3 - T2)`,
/// with `t1` the local transmit time, `t4` the local receive time.
pub fn compute(t1: NtpTs, t4: NtpTs, r: &Reply) -> Result<Measurement, SntpError> {
    let a = r.receive.diff_ms(t1);
    let b = r.transmit.diff_ms(t4);
    let offset_ms = (a + b).div_euclid(2);
    let delay_ms = t4.diff_ms(t1) - r.transmit.diff_ms(r.receive);
    if !(0..=MAX_DELAY_MS).contains(&delay_ms) {
        // A tiny negative value is clock granularity; anything else is a lie.
        if !(-5..0).contains(&delay_ms) {
            return Err(SntpError::BadDelay);
        }
    }
    if offset_ms.abs() > MAX_OFFSET_MS {
        return Err(SntpError::Implausible);
    }
    let server_unix_ms = r.transmit.to_unix_ms();
    if !is_plausible(server_unix_ms / 1000) {
        return Err(SntpError::Implausible);
    }
    Ok(Measurement {
        offset_ms,
        delay_ms: delay_ms.max(0),
        server_unix_ms,
        stratum: r.stratum,
    })
}

/// Parse, validate and measure in one call. `sent` is the transmit timestamp
/// of the request (also T1: the local time it was sent), `t4` the local time of
/// arrival.
pub fn process_reply(sent: NtpTs, t4: NtpTs, buf: &[u8]) -> Result<Measurement, SntpError> {
    let r = parse_reply(buf)?;
    validate(&r, sent)?;
    compute(sent, t4, &r)
}

/// Pick the best of several measurements: the smallest delay (its offset has
/// the tightest error bound), ties to the lower stratum.
pub fn best(ms: &[Measurement]) -> Option<Measurement> {
    ms.iter().copied().min_by_key(|m| (m.delay_ms, m.stratum))
}

/// Where the clock used for certificate validity comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    /// The CMOS RTC, never cross-checked ("hora nao confirmada").
    Rtc,
    /// Corrected by an SNTP measurement.
    Sntp,
}

/// The trusted-clock model: `unix_now = local_unix + offset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedClock {
    offset_ms: i64,
    source: ClockSource,
    /// Delay of the measurement that set the offset (ms), for the log.
    delay_ms: i64,
}

impl Default for TrustedClock {
    fn default() -> Self {
        Self::new()
    }
}

impl TrustedClock {
    /// Unconfirmed: just the local clock.
    pub const fn new() -> Self {
        TrustedClock {
            offset_ms: 0,
            source: ClockSource::Rtc,
            delay_ms: 0,
        }
    }

    /// Apply a measurement.
    pub fn apply(&mut self, m: &Measurement) {
        self.offset_ms = m.offset_ms;
        self.delay_ms = m.delay_ms;
        self.source = ClockSource::Sntp;
    }

    pub fn source(&self) -> ClockSource {
        self.source
    }

    /// True once an SNTP measurement set the offset.
    pub fn confirmed(&self) -> bool {
        self.source == ClockSource::Sntp
    }

    pub fn offset_ms(&self) -> i64 {
        self.offset_ms
    }

    pub fn delay_ms(&self) -> i64 {
        self.delay_ms
    }

    /// Corrected Unix seconds for a local reading in Unix milliseconds. `None`
    /// when the result would not be a plausible date (a broken local clock with
    /// no measurement): the TLS check then refuses to judge validity dates.
    pub fn unix_secs(&self, local_unix_ms: u64) -> Option<u64> {
        let ms = i128::from(local_unix_ms) + i128::from(self.offset_ms);
        let secs = u64::try_from(ms.div_euclid(1000)).ok()?;
        is_plausible(secs).then_some(secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unixtime::DateTime;

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
}
