//! What the browser can say about the certificate behind a page: who it is for, who
//! issued it, and for how long it is valid. Read from the leaf certificate the TLS client
//! already verified (or that the user chose to accept); showing it adds no trust of its own.

use crate::i18n::{Civil, DateFmt, DateStyle, Lang};
use crate::unixtime::DateTime;
use crate::x509;
use alloc::string::String;

/// A summary of the server certificate of a loaded page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertInfo {
    /// The site the certificate was checked against.
    pub host: String,
    /// The issuer's common name, else its organisation.
    pub issuer: String,
    /// Validity, Unix seconds.
    pub not_before: u64,
    pub not_after: u64,
    /// Certificates the server sent.
    pub chain_len: u8,
    /// The trusted root the chain ends in (a verified connection only).
    pub root: Option<String>,
}

/// Longest issuer name kept.
const MAX_NAME: usize = 64;

fn clean(bytes: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(bytes).into_owned();
    s.retain(|c| !c.is_control());
    if s.len() > MAX_NAME {
        let mut end = MAX_NAME;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

impl CertInfo {
    /// Summarise `leaf` (DER). `None` when it does not parse.
    pub fn from_leaf(
        leaf: &[u8],
        host: &str,
        chain_len: usize,
        root: Option<&str>,
    ) -> Option<CertInfo> {
        let c = x509::parse(leaf).ok()?;
        let issuer = c
            .issuer_cn()
            .filter(|n| !n.is_empty())
            .or_else(|| c.issuer_org())
            .map(clean)
            .unwrap_or_default();
        Some(CertInfo {
            host: clean(host.as_bytes()),
            issuer,
            not_before: c.not_before,
            not_after: c.not_after,
            chain_len: chain_len.min(255) as u8,
            root: root.map(|r| clean(r.as_bytes())),
        })
    }
}

/// A date for Unix seconds in the order of the language in effect (`14/11/2023`,
/// `11/14/2023`).
pub fn format_date(unix: u64) -> String {
    format_date_in(crate::i18n::lang(), unix)
}

/// [`format_date`] in `lang`.
pub fn format_date_in(lang: Lang, unix: u64) -> String {
    let d = DateTime::from_unix(unix);
    let civil = Civil {
        year: d.year.clamp(0, 9999),
        month: d.month,
        day: d.day,
        weekday: 0,
        hour: d.hour,
        minute: d.minute,
        second: d.second,
    };
    alloc::format!(
        "{}",
        DateFmt {
            lang,
            civil,
            style: DateStyle::Short,
            clock24: true,
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_follow_the_order_of_the_language() {
        assert_eq!(format_date_in(Lang::Pt, 0), "01/01/1970");
        assert_eq!(format_date_in(Lang::Pt, 1_700_000_000), "14/11/2023");
        assert_eq!(format_date_in(Lang::En, 1_700_000_000), "11/14/2023");
        let _ = format_date_in(Lang::En, u64::MAX);
        let _ = format_date(u64::MAX);
    }

    #[test]
    fn garbage_is_not_a_certificate() {
        assert!(CertInfo::from_leaf(b"", "a.test", 1, None).is_none());
        assert!(CertInfo::from_leaf(&[0x30, 0x03, 1, 2, 3], "a.test", 1, None).is_none());
    }

    #[test]
    fn names_are_cleaned_and_bounded() {
        assert_eq!(clean(b"a\x00b\nc"), "abc");
        let long = "é".repeat(100);
        let c = clean(long.as_bytes());
        assert!(c.len() <= MAX_NAME && c.chars().all(|ch| ch == 'é'));
    }
}
