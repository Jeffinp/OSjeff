//! The network stack's pure parts: frames and protocols (`net`, `dns`, `icmp`, `sntp`), DHCP
//! leases (`lease`), counters (`netstats`) and TLS certificate validation (`x509`, `tlsverify`).
//!
//! May depend on: `format` (Unix time for certificate dates). The network never knows about the
//! browser, drawing or apps. See `docs/design/code-structure.md`.

pub mod dns;
pub mod icmp;
pub mod lease;
pub mod net;
pub mod netstats;
pub mod sntp;
pub mod tlsverify;
pub mod x509;
