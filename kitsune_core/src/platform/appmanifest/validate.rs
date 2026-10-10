//! validate (split out of `appmanifest.rs`).

use super::*;

/// Is `id` a valid app id: `[a-z0-9._-]{1,32}`, starting with a letter or
/// digit, with no `..`?
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    if b.is_empty() || b.len() > MAX_ID_LEN {
        return false;
    }
    if !(b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return false;
    }
    if id.contains("..") {
        return false;
    }
    b.iter()
        .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

pub(super) fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME_LEN
        && b.iter().all(|&c| (0x20..=0x7E).contains(&c))
        && b[0] != b' '
        && b[b.len() - 1] != b' '
}

pub(super) fn parse_u32(v: &str, key: &'static str) -> Result<u32, ManifestError> {
    // Plain decimal: no sign, no spaces, no underscores, no leading zeros
    // (except "0"), at most 10 digits.
    let b = v.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return Err(ManifestError::BadValue(key));
    }
    if b.len() > 1 && b[0] == b'0' {
        return Err(ManifestError::BadValue(key));
    }
    v.parse::<u32>().map_err(|_| ManifestError::BadValue(key))
}

pub(super) fn parse_range(
    v: &str,
    key: &'static str,
    lo: u32,
    hi: u32,
) -> Result<u32, ManifestError> {
    let n = parse_u32(v, key)?;
    if n < lo {
        return Err(ManifestError::BadValue(key));
    }
    if n > hi {
        return Err(ManifestError::OverLimit(key));
    }
    Ok(n)
}

pub(super) fn parse_version(v: &str) -> Result<Version, ManifestError> {
    let mut it = v.split('.');
    let mut part = || -> Result<u16, ManifestError> {
        let s = it.next().ok_or(ManifestError::BadValue("version"))?;
        let n = parse_u32(s, "version")?;
        u16::try_from(n).map_err(|_| ManifestError::BadValue("version"))
    };
    let major = part()?;
    let minor = part()?;
    let patch = part()?;
    if it.next().is_some() {
        return Err(ManifestError::BadValue("version"));
    }
    Ok(Version {
        major,
        minor,
        patch,
    })
}

/// `net_hosts=a.example.com,*.cdn.example.org`: 1..=[`MAX_NET_HOSTS`] distinct,
/// lower-case entries, each a public host name by the destination filter (so
/// `localhost`, single labels and IP literals in private ranges are refused here
/// too), optionally with a leading `*.` for subdomains.
pub(super) fn parse_net_hosts(v: &str) -> Result<Vec<String>, ManifestError> {
    const KEY: &str = "net_hosts";
    if v.is_empty() || v.len() > MAX_NET_HOSTS_LEN {
        return Err(ManifestError::BadValue(KEY));
    }
    let mut out: Vec<String> = Vec::new();
    for entry in v.split(',') {
        let base = entry.strip_prefix("*.").unwrap_or(entry);
        let canonical = base.bytes().all(|c| !c.is_ascii_uppercase());
        // `host_allowed` is the destination filter: a name an app could never reach is
        // not worth listing. Wildcards need a registrable-looking base (a dot).
        if !canonical
            || !crate::platform::appnet::host_allowed(base)
            || (base != entry && !base.contains('.'))
        {
            return Err(ManifestError::BadValue(KEY));
        }
        if out.iter().any(|e| e == entry) {
            return Err(ManifestError::BadValue(KEY));
        }
        if out.len() == MAX_NET_HOSTS {
            return Err(ManifestError::OverLimit(KEY));
        }
        out.push(String::from(entry));
    }
    Ok(out)
}

pub(super) fn key_is_syntactic(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 32
        && !k.starts_with('.')
        && !k.ends_with('.')
        && k.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.')
        })
}

/// Most `name.<lang>` lines a manifest may carry.
pub const MAX_LOCAL_NAMES: usize = 8;

/// A language tag after `name.`: two or three lowercase letters, optionally `-` and two to
/// five lowercase letters or digits (`pt`, `en`, `pt-br`).
pub(super) fn valid_lang_tag(t: &str) -> bool {
    let (lang, rest) = match t.split_once('-') {
        Some((l, r)) => (l, Some(r)),
        None => (t, None),
    };
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|c| c.is_ascii_lowercase())
        && rest.is_none_or(|r| {
            (2..=5).contains(&r.len())
                && r.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A name in a language: printable text (accents and the like allowed), no leading or
/// trailing space, at most [`MAX_NAME_LEN`] characters.
pub(super) fn valid_local_name(n: &str) -> bool {
    let chars = n.chars().count();
    (1..=MAX_NAME_LEN).contains(&chars)
        && n.len() <= 4 * MAX_NAME_LEN
        && !n.chars().any(char::is_control)
        && !n.starts_with(' ')
        && !n.ends_with(' ')
}
