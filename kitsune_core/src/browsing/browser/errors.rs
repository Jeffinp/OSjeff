//! What the browser tells the user when a page could not be opened: a short title, one line
//! about the cause and which picture to show. Plain words: no protocol or library talk. The
//! texts come from the catalog (`web.err.*`), so an error page already on screen follows the
//! language.

use super::FailReason;
use crate::i18n::{self, Lang};
use crate::network::tlsverify::CertError;
use crate::tk;

/// Which illustration an error page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorArt {
    /// No network, or the page could not be reached for an unknown reason.
    Offline,
    /// The name does not resolve.
    NotFound,
    /// The server refused or did not answer in time.
    Unreachable,
    /// A certificate problem: the site's identity could not be proven.
    Certificate,
    /// A secure connection could not be set up, or something was blocked.
    Blocked,
}

/// The text of an error page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorInfo {
    pub art: ErrorArt,
    pub title: &'static str,
    /// One line about the cause.
    pub cause: &'static str,
}

/// Catalog key of the line about why the certificate was refused.
pub fn cert_cause_key(e: CertError) -> &'static str {
    match e {
        CertError::Expired => tk!("web.err.cert.expired"),
        CertError::NotYetValid => tk!("web.err.cert.not_yet_valid"),
        CertError::NameMismatch => tk!("web.err.cert.name_mismatch"),
        CertError::SelfSigned => tk!("web.err.cert.self_signed"),
        CertError::UnknownIssuer => tk!("web.err.cert.unknown_issuer"),
        CertError::BadSignature => tk!("web.err.cert.bad_signature"),
        CertError::ClockUnset => tk!("web.err.cert.clock_unset"),
        CertError::BadEncoding | CertError::TooLarge => tk!("web.err.cert.malformed"),
        CertError::ChainTooLong | CertError::NotCa | CertError::Constraint => {
            tk!("web.err.cert.chain")
        }
        CertError::Unsupported => tk!("web.err.cert.unsupported"),
        CertError::Other => tk!("web.err.cert.other"),
    }
}

/// One line about why the certificate was refused, in the language in effect.
pub fn cert_cause(e: CertError) -> &'static str {
    i18n::tr(cert_cause_key(e))
}

/// The art and the catalog keys (title, cause) of a failed navigation.
fn keys(reason: FailReason) -> (ErrorArt, &'static str, &'static str) {
    match reason {
        FailReason::Network => (
            ErrorArt::Offline,
            tk!("web.err.network.title"),
            tk!("web.err.network.cause"),
        ),
        FailReason::Dns => (
            ErrorArt::NotFound,
            tk!("web.err.dns.title"),
            tk!("web.err.dns.cause"),
        ),
        FailReason::Refused => (
            ErrorArt::Unreachable,
            tk!("web.err.refused.title"),
            tk!("web.err.refused.cause"),
        ),
        FailReason::Timeout => (
            ErrorArt::Unreachable,
            tk!("web.err.timeout.title"),
            tk!("web.err.timeout.cause"),
        ),
        FailReason::Tls => (
            ErrorArt::Blocked,
            tk!("web.err.tls.title"),
            tk!("web.err.tls.cause"),
        ),
        FailReason::Cert(e) => (
            ErrorArt::Certificate,
            tk!("web.err.cert.title"),
            cert_cause_key(e),
        ),
        FailReason::RedirectDowngrade => (
            ErrorArt::Blocked,
            tk!("web.err.downgrade.title"),
            tk!("web.err.downgrade.cause"),
        ),
        FailReason::RedirectInvalid => (
            ErrorArt::Blocked,
            tk!("web.err.redirect_bad.title"),
            tk!("web.err.redirect_bad.cause"),
        ),
        FailReason::RedirectLoop => (
            ErrorArt::Blocked,
            tk!("web.err.redirect_loop.title"),
            tk!("web.err.redirect_loop.cause"),
        ),
        FailReason::TooManyRedirects => (
            ErrorArt::Blocked,
            tk!("web.err.too_many.title"),
            tk!("web.err.too_many.cause"),
        ),
        FailReason::WorkerDied => (
            ErrorArt::Offline,
            tk!("web.err.worker.title"),
            tk!("web.err.worker.cause"),
        ),
    }
}

/// The error page for a failed navigation, in `lang`.
pub fn describe_in(lang: Lang, reason: FailReason) -> ErrorInfo {
    let (art, title, cause) = keys(reason);
    ErrorInfo {
        art,
        title: i18n::tr_in(lang, title),
        cause: i18n::tr_in(lang, cause),
    }
}

/// The error page for a failed navigation, in the language in effect (ask again at every
/// frame: a page already on screen follows a language change).
pub fn describe(reason: FailReason) -> ErrorInfo {
    describe_in(i18n::lang(), reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browsing::redirect::RedirectError;

    fn all_reasons() -> alloc::vec::Vec<FailReason> {
        use FailReason as F;
        let mut v = alloc::vec![
            F::Network,
            F::Dns,
            F::Refused,
            F::Timeout,
            F::Tls,
            F::RedirectDowngrade,
            F::RedirectInvalid,
            F::RedirectLoop,
            F::TooManyRedirects,
            F::WorkerDied,
            F::from_redirect(RedirectError::Loop),
        ];
        for e in [
            CertError::BadEncoding,
            CertError::TooLarge,
            CertError::ChainTooLong,
            CertError::Expired,
            CertError::NotYetValid,
            CertError::NameMismatch,
            CertError::SelfSigned,
            CertError::UnknownIssuer,
            CertError::BadSignature,
            CertError::NotCa,
            CertError::Constraint,
            CertError::Unsupported,
            CertError::ClockUnset,
            CertError::Other,
        ] {
            v.push(F::Cert(e));
        }
        v
    }

    #[test]
    fn every_failure_has_a_short_plain_text_in_both_languages() {
        for l in Lang::ALL {
            for r in all_reasons() {
                let i = describe_in(l, r);
                assert!(
                    !i.title.is_empty() && i.title.chars().count() <= 32,
                    "{l:?} {r:?}"
                );
                assert!(
                    i.cause.ends_with('.') && i.cause.chars().count() <= 70,
                    "{l:?} {r:?}: {}",
                    i.cause
                );
                // A missing key would show up as the key itself.
                assert!(!i.title.starts_with("web.") && !i.cause.starts_with("web."));
                // User-facing text never talks about how the system is built.
                for word in ["TLS", "DNS", "Rust", "kernel", "thread", "x509", "SNTP"] {
                    assert!(
                        !i.title.contains(word) && !i.cause.contains(word),
                        "{r:?}: {word}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_two_languages_say_different_things_with_their_accents() {
        let pt = describe_in(Lang::Pt, FailReason::Dns);
        let en = describe_in(Lang::En, FailReason::Dns);
        assert_eq!(pt.title, "Site não encontrado");
        assert_eq!(en.title, "Site not found");
        assert_ne!(pt.cause, en.cause);
        assert_eq!(
            describe_in(Lang::Pt, FailReason::Network).title,
            "Sem conexão"
        );
        assert_eq!(
            describe_in(Lang::En, FailReason::Network).title,
            "No connection"
        );
    }

    #[test]
    fn certificate_errors_get_the_certificate_picture_and_their_own_cause() {
        for l in Lang::ALL {
            let a = describe_in(l, FailReason::Cert(CertError::Expired));
            let b = describe_in(l, FailReason::Cert(CertError::NameMismatch));
            assert_eq!(a.art, ErrorArt::Certificate);
            assert_ne!(a.cause, b.cause);
        }
        assert!(
            describe_in(Lang::Pt, FailReason::Cert(CertError::Expired))
                .cause
                .contains("expirou")
        );
        assert!(
            describe_in(Lang::En, FailReason::Cert(CertError::Expired))
                .cause
                .contains("expired")
        );
    }

    #[test]
    fn pictures_follow_the_kind_of_failure() {
        assert_eq!(describe(FailReason::Network).art, ErrorArt::Offline);
        assert_eq!(describe(FailReason::Dns).art, ErrorArt::NotFound);
        assert_eq!(describe(FailReason::Timeout).art, ErrorArt::Unreachable);
        assert_eq!(describe(FailReason::RedirectLoop).art, ErrorArt::Blocked);
    }
}
