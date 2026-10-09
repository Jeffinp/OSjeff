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
