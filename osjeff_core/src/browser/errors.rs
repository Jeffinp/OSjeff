//! What the browser tells the user when a page could not be opened: a short title, one line
//! about the cause and which picture to show. Plain Portuguese: no protocol or library talk.

use super::FailReason;
use crate::tlsverify::CertError;

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

/// One line about why the certificate was refused.
pub fn cert_cause(e: CertError) -> &'static str {
    match e {
        CertError::Expired => "O certificado do site expirou.",
        CertError::NotYetValid => "O certificado do site ainda não é válido.",
        CertError::NameMismatch => "O certificado não vale para este endereço.",
        CertError::SelfSigned => "O certificado foi emitido pelo próprio site.",
        CertError::UnknownIssuer => "Quem emitiu o certificado não é confiável.",
        CertError::BadSignature => "A assinatura do certificado não confere.",
        CertError::ClockUnset => "A hora do sistema ainda não foi confirmada.",
        CertError::BadEncoding | CertError::TooLarge => "O certificado do site está malformado.",
        CertError::ChainTooLong | CertError::NotCa | CertError::Constraint => {
            "A cadeia de certificados do site não é aceitável."
        }
        CertError::Unsupported => "O certificado usa um recurso que não é suportado.",
        CertError::Other => "Não foi possível verificar o certificado do site.",
    }
}

/// The error page for a failed navigation.
pub fn describe(reason: FailReason) -> ErrorInfo {
    let (art, title, cause) = match reason {
        FailReason::Network => (
            ErrorArt::Offline,
            "Sem conexão",
            "Confira a rede e tente de novo.",
        ),
        FailReason::Dns => (
            ErrorArt::NotFound,
            "Site não encontrado",
            "Não achamos o servidor. Confira o endereço digitado.",
        ),
        FailReason::Refused => (
            ErrorArt::Unreachable,
            "Conexão recusada",
            "O servidor não aceitou a conexão.",
        ),
        FailReason::Timeout => (
            ErrorArt::Unreachable,
            "Tempo esgotado",
            "O servidor demorou demais para responder.",
        ),
        FailReason::Tls => (
            ErrorArt::Blocked,
            "Conexão segura recusada",
            "Não foi possível abrir uma conexão segura com o site.",
        ),
        FailReason::Cert(e) => (
            ErrorArt::Certificate,
            "Esta conexão não é segura",
            cert_cause(e),
        ),
        FailReason::RedirectDowngrade => (
            ErrorArt::Blocked,
            "Redirecionamento bloqueado",
            "O site tentou sair de uma conexão segura para uma sem proteção.",
        ),
        FailReason::RedirectInvalid => (
            ErrorArt::Blocked,
            "Redirecionamento inválido",
            "O endereço para onde o site envia você não é válido.",
        ),
        FailReason::RedirectLoop => (
            ErrorArt::Blocked,
            "Redirecionamento em ciclo",
            "O site volta sempre para um endereço já visitado.",
        ),
        FailReason::TooManyRedirects => (
            ErrorArt::Blocked,
            "Redirecionamentos demais",
            "O site redirecionou mais vezes do que o permitido.",
        ),
        FailReason::WorkerDied => (
            ErrorArt::Offline,
            "Carregador parado",
            "O carregador de páginas parou de responder.",
        ),
    };
    ErrorInfo { art, title, cause }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redirect::RedirectError;

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
    fn every_failure_has_a_short_plain_text() {
        for r in all_reasons() {
            let i = describe(r);
            assert!(
                !i.title.is_empty() && i.title.chars().count() <= 32,
                "{r:?}"
            );
            assert!(
                i.cause.ends_with('.') && i.cause.chars().count() <= 70,
                "{r:?}"
            );
            // User-facing text never talks about how the system is built.
            for word in ["TLS", "DNS", "Rust", "kernel", "thread", "x509", "SNTP"] {
                assert!(
                    !i.title.contains(word) && !i.cause.contains(word),
                    "{r:?}: {word}"
                );
            }
        }
    }

    #[test]
    fn certificate_errors_get_the_certificate_picture_and_their_own_cause() {
        let a = describe(FailReason::Cert(CertError::Expired));
        let b = describe(FailReason::Cert(CertError::NameMismatch));
        assert_eq!(a.art, ErrorArt::Certificate);
        assert_ne!(a.cause, b.cause);
        assert!(a.cause.contains("expirou"));
    }

    #[test]
    fn pictures_follow_the_kind_of_failure() {
        assert_eq!(describe(FailReason::Network).art, ErrorArt::Offline);
        assert_eq!(describe(FailReason::Dns).art, ErrorArt::NotFound);
        assert_eq!(describe(FailReason::Timeout).art, ErrorArt::Unreachable);
        assert_eq!(describe(FailReason::RedirectLoop).art, ErrorArt::Blocked);
    }
}
