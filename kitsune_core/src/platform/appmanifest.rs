//! App packages: the `kitsune.manifest` and `kitsune.icon` custom sections of a
//! `.wasm`, parsed and validated.
//!
//! One file is one app. The manifest is plain text, one `key=value` per line
//! (no JSON, no serde); see `docs/design/apps.md` §2 for the table of keys. The
//! parser is strict by design: a repeated key, an unknown key, an invalid value
//! or an unknown permission is an **error**, never silently ignored (only keys
//! starting `x-` are skipped, reserved for extensions). Requested quotas above
//! the system ceilings ([`MAX_MEM_MIB`], [`MAX_FUEL_FRAME`], ...) are refused
//! too, so an installed app can never hold more than the system grants.
//!
//! Nothing here allocates without a bound: the manifest is at most
//! [`MAX_MANIFEST_BYTES`], the icon at most [`MAX_ICON_BYTES`] / 64x64.

use crate::format::image::{self, Image};
use crate::format::png;
use crate::i18n::{self, Arg, Lang};
use crate::platform::wasmsec::{self, WasmError};
use crate::tk;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

macro_rules! set_once {
    ($slot:expr, $key:expr, $val:expr) => {{
        if $slot.is_some() {
            return Err(ManifestError::DuplicateKey($key));
        }
        $slot = Some($val);
    }};
}

mod manifest;
mod package;
mod validate;
pub use package::*;
pub use validate::*;

/// Name of the custom section holding the manifest text.
pub const MANIFEST_SECTION: &str = "kitsune.manifest";
/// Name of the custom section holding the icon PNG.
pub const ICON_SECTION: &str = "kitsune.icon";
/// Name the manifest section had when the system was called OSjeff. Packages built
/// with it are still accepted (compatibility); new builds use [`MANIFEST_SECTION`].
pub const LEGACY_MANIFEST_SECTION: &str = "osjeff.manifest";
/// Old name of [`ICON_SECTION`], accepted for the same reason.
pub const LEGACY_ICON_SECTION: &str = "osjeff.icon";

pub const MAX_MANIFEST_BYTES: usize = 4096;
pub const MAX_MANIFEST_LINES: usize = 64;
pub const MAX_ICON_BYTES: usize = 64 * 1024;
pub const MAX_ICON_DIM: u32 = 64;

/// Most entries of `net_hosts`, and its longest value.
pub const MAX_NET_HOSTS: usize = 8;
pub const MAX_NET_HOSTS_LEN: usize = 256;

pub const MAX_ID_LEN: usize = 32;
pub const MAX_NAME_LEN: usize = 24;

// ---- system ceilings (what an app may *ask* for; the installer refuses more) ----
pub const MAX_MEM_MIB: u32 = 24;
pub const MIN_FUEL_FRAME: u64 = 10_000;
pub const MAX_FUEL_FRAME: u64 = 20_000_000;
pub const MAX_DISK_KIB: u32 = 4096;
pub const MAX_FDS: u32 = 32;
pub const MIN_TICK_MS: u32 = 16;
pub const MAX_TICK_MS: u32 = 60_000;
pub const MAX_WIN_W: u32 = 1280;
pub const MAX_WIN_H: u32 = 800;
pub const MIN_WIN_DIM: u32 = 64;

// ---- defaults ----
pub const DEFAULT_MEM_MIB: u32 = 8;
pub const DEFAULT_FUEL_FRAME: u64 = 4_000_000;
pub const DEFAULT_DISK_KIB: u32 = 256;
pub const DEFAULT_MAX_FDS: u32 = 16;
pub const DEFAULT_WIN_W: u32 = 640;
pub const DEFAULT_WIN_H: u32 = 400;

/// `fs=` permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsPerm {
    None,
    /// `/data/<id>/`
    Own,
    /// `/home`
    Home,
}

/// `net=` permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetPerm {
    None,
    Http,
    Tcp,
}

impl NetPerm {
    /// May the app call `net_http_get`? (`tcp` implies `http`.)
    pub fn allows_http(self) -> bool {
        !matches!(self, NetPerm::None)
    }
}

/// `clipboard=` permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipPerm {
    None,
    Rw,
}

/// `abi=`: which host module the app imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Abi {
    /// `host.*` (continuous `render` loop; snake, plasma, DOOM).
    V1,
    /// `osj.*` (event driven).
    V2,
}

/// `N.N.N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A validated manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    /// The name in any language that has no `name.<lang>` of its own (plain ASCII).
    pub name: String,
    /// `name.<lang>=` lines: a language tag as written (`pt`, `pt-br`, `en`) and the name in it.
    /// Read them with [`Manifest::name_in`] or [`Manifest::display_name`].
    pub names: Vec<(String, String)>,
    pub version: Version,
    pub abi: Abi,
    pub fs: FsPerm,
    pub net: NetPerm,
    pub clipboard: ClipPerm,
    /// `net_hosts=`: the only destinations `net_http_get` may reach (`example.com`
    /// exactly, or `*.example.com` for any subdomain); empty means any public host
    /// the destination filter ([`crate::platform::appnet`]) lets through. Only with `net=http`
    /// or `tcp`.
    pub net_hosts: Vec<String>,
    pub mem_mib: u32,
    pub fuel_frame: u64,
    pub disk_kib: u32,
    pub max_fds: u32,
    pub tick_ms: u32,
    /// Default size of the window's content area.
    pub win_w: u32,
    pub win_h: u32,
    pub win_min_w: u32,
    pub win_min_h: u32,
    pub resizable: bool,
}

/// Why a manifest (or package) was refused. The payload names the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestError {
    TooLarge,
    NotUtf8,
    TooManyLines,
    /// A line with no `=`, an empty key, or a key with invalid characters.
    Syntax,
    DuplicateKey(&'static str),
    UnknownKey,
    Missing(&'static str),
    /// The value of this key is malformed or out of range.
    BadValue(&'static str),
    /// The value is well formed but above the system ceiling.
    OverLimit(&'static str),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::TooLarge => f.write_str("manifest is too large"),
            ManifestError::NotUtf8 => f.write_str("manifest is not valid UTF-8"),
            ManifestError::TooManyLines => f.write_str("manifest has too many lines"),
            ManifestError::Syntax => f.write_str("manifest syntax error"),
            ManifestError::DuplicateKey(k) => write!(f, "duplicate manifest key `{k}`"),
            ManifestError::UnknownKey => f.write_str("unknown manifest key"),
            ManifestError::Missing(k) => write!(f, "manifest key `{k}` is required"),
            ManifestError::BadValue(k) => write!(f, "invalid value for `{k}`"),
            ManifestError::OverLimit(k) => write!(f, "`{k}` is above the system limit"),
        }
    }
}

impl ManifestError {
    /// The reason in `lang`, in words for the person installing the app.
    pub fn message_in(&self, lang: Lang) -> alloc::string::String {
        let (key, arg) = match *self {
            ManifestError::TooLarge => (tk!("apps.err.manifest_large"), None),
            ManifestError::NotUtf8 => (tk!("apps.err.manifest_utf8"), None),
            ManifestError::TooManyLines => (tk!("apps.err.manifest_lines"), None),
            ManifestError::Syntax => (tk!("apps.err.manifest_syntax"), None),
            ManifestError::DuplicateKey(k) => (tk!("apps.err.manifest_dup"), Some(k)),
            ManifestError::UnknownKey => (tk!("apps.err.manifest_unknown"), None),
            ManifestError::Missing(k) => (tk!("apps.err.manifest_missing"), Some(k)),
            ManifestError::BadValue(k) => (tk!("apps.err.manifest_bad"), Some(k)),
            ManifestError::OverLimit(k) => (tk!("apps.err.manifest_limit"), Some(k)),
        };
        i18n::tr_fmt_in(lang, key, &[("key", Arg::Str(arg.unwrap_or("")))])
    }
}

#[cfg(test)]
mod tests;
