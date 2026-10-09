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

use crate::format::unixtime::{NTP_UNIX_OFFSET, is_plausible};

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
mod tests;
