//! The TLS server-certificate verifier plugged into `embedded-tls`.
//!
//! `embedded-tls` asks its [`CryptoProvider`] for a [`TlsVerifier`]; the stock
//! `UnsecureProvider` has none, which the library treats as "skip verification".
//! [`Provider`] always has one: [`Verifier`], whose checks live in
//! `osjeff_core::tlsverify` (chain building and validation by `rustls-webpki`,
//! against the embedded trust store, at the *trusted* time from `clock`):
//!
//! 1. `verify_certificate`: the server's whole chain is validated for the host
//!    name the user asked for; the leaf is kept;
//! 2. `verify_signature`: the `CertificateVerify` signature over the handshake
//!    transcript must verify with the leaf's key (this is what proves the
//!    server holds the private key).
//!
//! Any failure aborts the handshake and leaves the reason in
//! [`Verifier::failure`]. The only way past a failure is the explicit,
//! per-origin, per-session override the user clicks on the error page
//! ([`Verifier::allow_insecure`]): the failure is then recorded in
//! [`Verifier::overridden`] and the connection is reported as *insecure*,
//! never as verified.

use alloc::string::String;
use alloc::vec::Vec;
use embedded_tls::TlsError;
use embedded_tls::blocking::{
    Aes128GcmSha256, CertificateEntryRef, CertificateRef, CertificateVerifyRef, CryptoProvider,
    TlsCipherSuite, TlsVerifier,
};
use osjeff_core::tlsverify::{self, CertError, TrustStore, Verified};
use rand_core::CryptoRngCore;
use sha2::Digest;

/// What a finished (or failed) handshake established about the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Chain and signature verified.
    Verified(Verified),
    /// Verification failed and the user had allowed this origin.
    Overridden(CertError),
}

pub struct Verifier {
    host: String,
    store: TrustStore,
    allow_insecure: bool,
    leaf: Vec<u8>,
    transcript: Option<Vec<u8>>,
    chain: Option<Verified>,
    /// Why verification failed (also set when the override let it through).
    failure: Option<CertError>,
    /// Failure was ignored because of the user's override.
    overridden: bool,
}

impl Verifier {
    pub fn new(allow_insecure: bool) -> Verifier {
        Verifier {
            host: String::new(),
            store: TrustStore::embedded(),
            allow_insecure,
            leaf: Vec::new(),
            transcript: None,
            chain: None,
            failure: None,
            overridden: false,
        }
    }

    /// Why the last handshake was refused (or overridden), if it was.
    pub fn failure(&self) -> Option<CertError> {
        self.failure
    }

    /// What the completed handshake proved. `None` when verification never
    /// ran to completion.
    pub fn outcome(&self) -> Option<Outcome> {
        if self.overridden {
            return self.failure.map(Outcome::Overridden);
        }
        if self.failure.is_some() {
            return None;
        }
        self.chain.map(Outcome::Verified)
    }

    pub fn root_name(&self, v: &Verified) -> &'static str {
        v.root
            .and_then(TrustStore::manifest_entry)
            .map_or("?", |(_, name)| name)
    }

    fn fail(&mut self, e: CertError) -> Result<(), TlsError> {
        self.failure = Some(e);
        if self.allow_insecure {
            self.overridden = true;
            crate::serial_println!(
                "tls: certificate check FAILED ({}) for {}: continuing INSECURELY (user override)",
                e.reason(),
                self.host
            );
            Ok(())
        } else {
            crate::serial_println!(
                "tls: certificate check FAILED ({}) for {}",
                e.reason(),
                self.host
            );
            Err(TlsError::InvalidCertificate)
        }
    }
}

fn scheme_code(s: embedded_tls::SignatureScheme) -> u16 {
    use embedded_tls::SignatureScheme as S;
    match s {
        S::RsaPkcs1Sha256 => 0x0401,
        S::RsaPkcs1Sha384 => 0x0501,
        S::RsaPkcs1Sha512 => 0x0601,
        S::EcdsaSecp256r1Sha256 => 0x0403,
        S::EcdsaSecp384r1Sha384 => 0x0503,
        S::RsaPssRsaeSha256 => 0x0804,
        S::RsaPssRsaeSha384 => 0x0805,
        S::RsaPssRsaeSha512 => 0x0806,
        _ => 0,
    }
}

impl<CS: TlsCipherSuite> TlsVerifier<CS> for Verifier {
    fn set_hostname_verification(&mut self, hostname: &str) -> Result<(), TlsError> {
        self.host = String::from(hostname);
        Ok(())
    }

    fn verify_certificate(
        &mut self,
        transcript: &CS::Hash,
        cert: CertificateRef,
    ) -> Result<(), TlsError> {
        let mut chain: Vec<&[u8]> = Vec::new();
        for e in cert.entries.iter() {
            match e {
                CertificateEntryRef::X509(der) => chain.push(der),
                CertificateEntryRef::RawPublicKey(_) => return self.fail(CertError::Unsupported),
            }
        }
        if self.host.is_empty() {
            // The browser always names the host; never validate "for nobody".
            return self.fail(CertError::NameMismatch);
        }
        self.transcript = Some(transcript.clone().finalize().to_vec());
        if let Some(first) = chain.first() {
            self.leaf = first.to_vec();
        }
        let now = crate::clock::trusted_unix_secs();
        match tlsverify::verify_chain(&chain, &self.host, now, &self.store) {
            Ok(v) => {
                self.chain = Some(v);
                Ok(())
            }
            Err(e) => self.fail(e),
        }
    }

    fn verify_signature(&mut self, verify: CertificateVerifyRef) -> Result<(), TlsError> {
        if self.overridden {
            // The user chose to go on without authenticating this server.
            return Ok(());
        }
        let Some(hash) = self.transcript.take() else {
            return self.fail(CertError::BadSignature);
        };
        let msg = tlsverify::tls13_server_verify_message(&hash);
        let code = scheme_code(verify.signature_scheme);
        match tlsverify::verify_handshake_signature(&self.leaf, code, &msg, verify.signature) {
            Ok(()) => Ok(()),
            Err(e) => self.fail(e),
        }
    }
}

/// The provider handed to `TlsConnection::open`: an RNG plus the verifier. It
/// has no client certificate and no signer.
pub struct Provider<'a, R> {
    pub rng: R,
    pub verifier: &'a mut Verifier,
}

impl<R: CryptoRngCore> CryptoProvider for Provider<'_, R> {
    type CipherSuite = Aes128GcmSha256;
    type Signature = [u8; 0];

    fn rng(&mut self) -> impl CryptoRngCore {
        &mut self.rng
    }

    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Self::CipherSuite>, TlsError> {
        Ok(&mut *self.verifier)
    }
}
