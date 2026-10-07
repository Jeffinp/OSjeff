//! CMOS RTC register decoding: BCD, 12h/24h and time-zone shift.

/// Wall-clock time as read from the RTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    pub h: u8,
    pub m: u8,
    pub s: u8,
}

/// Status register B: bit 2 set = binary values (clear = BCD).
const REGB_BINARY: u8 = 0x04;
/// Status register B: bit 1 set = 24-hour mode (clear = 12-hour with PM flag).
const REGB_24H: u8 = 0x02;

/// Converts one packed-BCD byte to binary.
pub fn bcd_to_bin(v: u8) -> u8 {
    (v & 0x0F) + (v >> 4) * 10
}

/// Decodes raw seconds/minutes/hours registers per status register B and
/// applies `tz_offset_hours` (UTC -> local), wrapping the hour into `0..24`.
///
/// In 12-hour mode the PM flag lives in bit 7 of the hours register.
pub fn decode(raw_s: u8, raw_m: u8, raw_h: u8, regb: u8, tz_offset_hours: i32) -> Time {
    let (mut s, mut m, mut h) = (raw_s, raw_m, raw_h);

    if regb & REGB_BINARY == 0 {
        s = bcd_to_bin(s);
        m = bcd_to_bin(m);
        let pm = h & 0x80 != 0; // 12h mode keeps PM flag in high bit
        h = ((h & 0x0F) + ((h & 0x70) >> 4) * 10) | (if pm { 0x80 } else { 0 });
    }

    // Convert 12h -> 24h when the RTC is in 12h mode.
    if regb & REGB_24H == 0 {
        let pm = h & 0x80 != 0;
        h &= 0x7F;
        if pm && h != 12 {
            h += 12;
        } else if !pm && h == 12 {
            h = 0;
        }
    }

    let local = (h as i32 + tz_offset_hours).rem_euclid(24) as u8;
    Time { h: local, m, s }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN24: u8 = 0x04 | 0x02;
    const BCD24: u8 = 0x02;
    const BCD12: u8 = 0x00;
    const BIN12: u8 = 0x04;

    #[test]
    fn bcd_digits() {
        assert_eq!(bcd_to_bin(0x00), 0);
        assert_eq!(bcd_to_bin(0x09), 9);
        assert_eq!(bcd_to_bin(0x10), 10);
        assert_eq!(bcd_to_bin(0x59), 59);
        assert_eq!(bcd_to_bin(0x99), 99);
    }

    #[test]
    fn bcd_with_invalid_nibbles_does_not_overflow() {
        // Not valid BCD, but the arithmetic must stay in range (no overflow).
        assert_eq!(bcd_to_bin(0xFF), 15 + 15 * 10);
        assert_eq!(bcd_to_bin(0xA0), 100);
    }

    #[test]
    fn binary_24h_passthrough() {
        let t = decode(30, 45, 13, BIN24, 0);
        assert_eq!(
            t,
            Time {
                h: 13,
                m: 45,
                s: 30
            }
        );
    }

    #[test]
    fn bcd_24h() {
        let t = decode(0x59, 0x08, 0x23, BCD24, 0);
        assert_eq!(t, Time { h: 23, m: 8, s: 59 });
    }

    #[test]
    fn timezone_shift_wraps_both_ways() {
        assert_eq!(decode(0, 0, 2, BIN24, -3).h, 23); // 02:00 UTC -> 23:00 prev day
        assert_eq!(decode(0, 0, 23, BIN24, 3).h, 2);
        assert_eq!(decode(0, 0, 12, BIN24, -3).h, 9);
        assert_eq!(decode(0, 0, 5, BIN24, -48).h, 5); // multi-day offsets wrap too
    }

    #[test]
    fn twelve_hour_bcd_midnight_noon_and_afternoon() {
        // 12:xx AM -> 0, 12:xx PM -> 12, 1 PM -> 13, 11 PM -> 23, 1 AM -> 1.
        assert_eq!(decode(0, 0, 0x12, BCD12, 0).h, 0);
        assert_eq!(decode(0, 0, 0x12 | 0x80, BCD12, 0).h, 12);
        assert_eq!(decode(0, 0, 0x01 | 0x80, BCD12, 0).h, 13);
        assert_eq!(decode(0, 0, 0x11 | 0x80, BCD12, 0).h, 23);
        assert_eq!(decode(0, 0, 0x01, BCD12, 0).h, 1);
    }

    #[test]
    fn twelve_hour_binary_mode() {
        assert_eq!(decode(0, 0, 12, BIN12, 0).h, 0);
        assert_eq!(decode(0, 0, 12 | 0x80, BIN12, 0).h, 12);
        assert_eq!(decode(0, 0, 3 | 0x80, BIN12, 0).h, 15);
    }

    #[test]
    fn twelve_hour_with_timezone() {
        // 1 AM UTC with -3 -> 22:00 previous day.
        assert_eq!(decode(0, 0, 0x01, BCD12, -3).h, 22);
        // 11 PM UTC with -3 -> 20:00.
        assert_eq!(decode(0, 0, 0x11 | 0x80, BCD12, -3).h, 20);
    }

    #[test]
    fn minutes_and_seconds_unaffected_by_hour_logic() {
        let t = decode(0x45, 0x30, 0x12 | 0x80, BCD12, -3);
        assert_eq!(t, Time { h: 9, m: 30, s: 45 });
    }

    #[test]
    fn malformed_hours_register_stays_in_0_to_23() {
        for raw in 0..=255u8 {
            for regb in [BIN24, BCD24, BCD12, BIN12] {
                let t = decode(0, 0, raw, regb, -3);
                assert!(t.h < 24, "raw={raw:#x} regb={regb:#x} -> {}", t.h);
            }
        }
    }
}
