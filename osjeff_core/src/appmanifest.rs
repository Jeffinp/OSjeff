//! App packages: the `osjeff.manifest` and `osjeff.icon` custom sections of a
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

use crate::image::{self, Image};
use crate::png;
use crate::wasmsec::{self, WasmError};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Name of the custom section holding the manifest text.
pub const MANIFEST_SECTION: &str = "osjeff.manifest";
/// Name of the custom section holding the icon PNG.
pub const ICON_SECTION: &str = "osjeff.icon";

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
    pub name: String,
    pub version: Version,
    pub abi: Abi,
    pub fs: FsPerm,
    pub net: NetPerm,
    pub clipboard: ClipPerm,
    /// `net_hosts=`: the only destinations `net_http_get` may reach (`example.com`
    /// exactly, or `*.example.com` for any subdomain); empty means any public host
    /// the destination filter ([`crate::appnet`]) lets through. Only with `net=http`
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

fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME_LEN
        && b.iter().all(|&c| (0x20..=0x7E).contains(&c))
        && b[0] != b' '
        && b[b.len() - 1] != b' '
}

fn parse_u32(v: &str, key: &'static str) -> Result<u32, ManifestError> {
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

fn parse_range(v: &str, key: &'static str, lo: u32, hi: u32) -> Result<u32, ManifestError> {
    let n = parse_u32(v, key)?;
    if n < lo {
        return Err(ManifestError::BadValue(key));
    }
    if n > hi {
        return Err(ManifestError::OverLimit(key));
    }
    Ok(n)
}

fn parse_version(v: &str) -> Result<Version, ManifestError> {
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
fn parse_net_hosts(v: &str) -> Result<Vec<String>, ManifestError> {
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
            || !crate::appnet::host_allowed(base)
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

fn key_is_syntactic(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 32
        && k.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
}

macro_rules! set_once {
    ($slot:expr, $key:expr, $val:expr) => {{
        if $slot.is_some() {
            return Err(ManifestError::DuplicateKey($key));
        }
        $slot = Some($val);
    }};
}

impl Manifest {
    /// Parses and validates the text of an `osjeff.manifest` section.
    pub fn parse(data: &[u8]) -> Result<Manifest, ManifestError> {
        if data.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge);
        }
        let text = core::str::from_utf8(data).map_err(|_| ManifestError::NotUtf8)?;

        let mut id: Option<String> = None;
        let mut name: Option<String> = None;
        let mut version = None;
        let mut abi = None;
        let mut fs = None;
        let mut net = None;
        let mut clipboard = None;
        let mut net_hosts: Option<Vec<String>> = None;
        let mut mem_mib = None;
        let mut fuel_frame = None;
        let mut disk_kib = None;
        let mut max_fds = None;
        let mut tick_ms = None;
        let mut win_w = None;
        let mut win_h = None;
        let mut win_min_w = None;
        let mut win_min_h = None;
        let mut resizable = None;

        let mut lines = 0usize;
        for raw in text.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            lines += 1;
            if lines > MAX_MANIFEST_LINES {
                return Err(ManifestError::TooManyLines);
            }
            let (key, val) = line.split_once('=').ok_or(ManifestError::Syntax)?;
            if !key_is_syntactic(key) {
                return Err(ManifestError::Syntax);
            }
            if val.bytes().any(|c| c < 0x20 || c == 0x7F) {
                return Err(ManifestError::Syntax);
            }
            match key {
                "id" => {
                    if !valid_id(val) {
                        return Err(ManifestError::BadValue("id"));
                    }
                    set_once!(id, "id", String::from(val));
                }
                "name" => {
                    if !valid_name(val) {
                        return Err(ManifestError::BadValue("name"));
                    }
                    set_once!(name, "name", String::from(val));
                }
                "version" => set_once!(version, "version", parse_version(val)?),
                "abi" => {
                    let a = match val {
                        "1" => Abi::V1,
                        "2" => Abi::V2,
                        _ => return Err(ManifestError::BadValue("abi")),
                    };
                    set_once!(abi, "abi", a);
                }
                "fs" => {
                    let p = match val {
                        "none" => FsPerm::None,
                        "own" => FsPerm::Own,
                        "home" => FsPerm::Home,
                        _ => return Err(ManifestError::BadValue("fs")),
                    };
                    set_once!(fs, "fs", p);
                }
                "net" => {
                    let p = match val {
                        "none" => NetPerm::None,
                        "http" => NetPerm::Http,
                        "tcp" => NetPerm::Tcp,
                        _ => return Err(ManifestError::BadValue("net")),
                    };
                    set_once!(net, "net", p);
                }
                "net_hosts" => set_once!(net_hosts, "net_hosts", parse_net_hosts(val)?),
                "clipboard" => {
                    let p = match val {
                        "none" => ClipPerm::None,
                        "rw" => ClipPerm::Rw,
                        _ => return Err(ManifestError::BadValue("clipboard")),
                    };
                    set_once!(clipboard, "clipboard", p);
                }
                "mem_mib" => set_once!(
                    mem_mib,
                    "mem_mib",
                    parse_range(val, "mem_mib", 1, MAX_MEM_MIB)?
                ),
                "fuel_frame" => {
                    let n = parse_u32(val, "fuel_frame")? as u64;
                    if n < MIN_FUEL_FRAME {
                        return Err(ManifestError::BadValue("fuel_frame"));
                    }
                    if n > MAX_FUEL_FRAME {
                        return Err(ManifestError::OverLimit("fuel_frame"));
                    }
                    set_once!(fuel_frame, "fuel_frame", n);
                }
                "disk_kib" => set_once!(
                    disk_kib,
                    "disk_kib",
                    parse_range(val, "disk_kib", 0, MAX_DISK_KIB)?
                ),
                "max_fds" => {
                    set_once!(max_fds, "max_fds", parse_range(val, "max_fds", 1, MAX_FDS)?)
                }
                "tick_ms" => {
                    let n = parse_u32(val, "tick_ms")?;
                    if n != 0 && n < MIN_TICK_MS {
                        return Err(ManifestError::BadValue("tick_ms"));
                    }
                    if n > MAX_TICK_MS {
                        return Err(ManifestError::OverLimit("tick_ms"));
                    }
                    set_once!(tick_ms, "tick_ms", n);
                }
                "win_w" => set_once!(
                    win_w,
                    "win_w",
                    parse_range(val, "win_w", MIN_WIN_DIM, MAX_WIN_W)?
                ),
                "win_h" => set_once!(
                    win_h,
                    "win_h",
                    parse_range(val, "win_h", MIN_WIN_DIM, MAX_WIN_H)?
                ),
                "win_min_w" => set_once!(
                    win_min_w,
                    "win_min_w",
                    parse_range(val, "win_min_w", MIN_WIN_DIM, MAX_WIN_W)?
                ),
                "win_min_h" => set_once!(
                    win_min_h,
                    "win_min_h",
                    parse_range(val, "win_min_h", MIN_WIN_DIM, MAX_WIN_H)?
                ),
                "resizable" => {
                    let r = match val {
                        "0" => false,
                        "1" => true,
                        _ => return Err(ManifestError::BadValue("resizable")),
                    };
                    set_once!(resizable, "resizable", r);
                }
                k if k.starts_with("x-") => {}
                _ => return Err(ManifestError::UnknownKey),
            }
        }

        let id = id.ok_or(ManifestError::Missing("id"))?;
        let name = name.ok_or(ManifestError::Missing("name"))?;
        let version = version.ok_or(ManifestError::Missing("version"))?;
        let fs = fs.unwrap_or(FsPerm::None);
        let net = net.unwrap_or(NetPerm::None);
        let net_hosts = net_hosts.unwrap_or_default();
        if !net_hosts.is_empty() && !net.allows_http() {
            // An allow-list for a permission the app does not have is a mistake.
            return Err(ManifestError::BadValue("net_hosts"));
        }
        let win_w = win_w.unwrap_or(DEFAULT_WIN_W);
        let win_h = win_h.unwrap_or(DEFAULT_WIN_H);
        let win_min_w = win_min_w.unwrap_or(win_w.min(200));
        let win_min_h = win_min_h.unwrap_or(win_h.min(120));
        if win_min_w > win_w {
            return Err(ManifestError::BadValue("win_min_w"));
        }
        if win_min_h > win_h {
            return Err(ManifestError::BadValue("win_min_h"));
        }
        let disk_kib = match fs {
            FsPerm::None => {
                if disk_kib.unwrap_or(0) != 0 {
                    return Err(ManifestError::BadValue("disk_kib"));
                }
                0
            }
            _ => disk_kib.unwrap_or(DEFAULT_DISK_KIB),
        };
        Ok(Manifest {
            id,
            name,
            version,
            abi: abi.unwrap_or(Abi::V2),
            fs,
            net,
            net_hosts,
            clipboard: clipboard.unwrap_or(ClipPerm::None),
            mem_mib: mem_mib.unwrap_or(DEFAULT_MEM_MIB),
            fuel_frame: fuel_frame.unwrap_or(DEFAULT_FUEL_FRAME),
            disk_kib,
            max_fds: max_fds.unwrap_or(DEFAULT_MAX_FDS),
            tick_ms: tick_ms.unwrap_or(0),
            win_w,
            win_h,
            win_min_w,
            win_min_h,
            resizable: resizable.unwrap_or(true),
        })
    }

    /// The manifest of a module that has none (DOOM, `cdemo`, old builds): the
    /// behaviour the single embedded app always had.
    pub fn legacy(id: &str, name: &str) -> Manifest {
        Manifest {
            id: String::from(id),
            name: String::from(name),
            version: Version {
                major: 0,
                minor: 0,
                patch: 0,
            },
            abi: Abi::V1,
            fs: FsPerm::None,
            net: NetPerm::None,
            net_hosts: Vec::new(),
            clipboard: ClipPerm::None,
            mem_mib: MAX_MEM_MIB,
            fuel_frame: MAX_FUEL_FRAME,
            disk_kib: 0,
            max_fds: 1,
            tick_ms: 0,
            win_w: 692,
            win_h: 414,
            win_min_w: 692,
            win_min_h: 414,
            resizable: false,
        }
    }

    /// The quotas actually granted: the request clamped to the system ceilings
    /// (defence in depth: [`parse`](Self::parse) already refuses more, but a
    /// manifest built any other way must not exceed them either).
    pub fn granted(&self) -> Quotas {
        Quotas {
            mem_bytes: (self.mem_mib.clamp(1, MAX_MEM_MIB) as usize) << 20,
            fuel_frame: self.fuel_frame.clamp(MIN_FUEL_FRAME, MAX_FUEL_FRAME),
            disk_bytes: (self.disk_kib.min(MAX_DISK_KIB) as u64) * 1024,
            max_fds: self.max_fds.clamp(1, MAX_FDS) as usize,
        }
    }

    /// Window size (outer) for this app: content + the 28x56 frame.
    pub fn content_for(&self, w: u32, h: u32) -> (u32, u32) {
        (
            w.clamp(self.win_min_w, MAX_WIN_W),
            h.clamp(self.win_min_h, MAX_WIN_H),
        )
    }
}

/// Resource limits enforced on one running app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quotas {
    pub mem_bytes: usize,
    pub fuel_frame: u64,
    pub disk_bytes: u64,
    pub max_fds: usize,
}

/// Why an icon was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconError {
    TooLarge,
    /// Header unreadable or not a PNG.
    NotPng,
    /// Wider or taller than 64.
    Dimensions,
    /// The pixel data did not decode.
    Corrupt,
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IconError::TooLarge => "icon is larger than 64 KiB",
            IconError::NotPng => "icon is not a PNG",
            IconError::Dimensions => "icon is larger than 64x64",
            IconError::Corrupt => "icon PNG is corrupt",
        })
    }
}

/// Decodes an `osjeff.icon` payload. The size and dimensions are checked from
/// the header **before** any pixel is decoded.
pub fn decode_icon(data: &[u8]) -> Result<Image, IconError> {
    if data.len() > MAX_ICON_BYTES {
        return Err(IconError::TooLarge);
    }
    let h = png::read_header(data).map_err(|_| IconError::NotPng)?;
    if h.width > MAX_ICON_DIM || h.height > MAX_ICON_DIM {
        return Err(IconError::Dimensions);
    }
    image::decode(data).map_err(|_| IconError::Corrupt)
}

/// Why a package was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageError {
    Wasm(WasmError),
    NoManifest,
    DuplicateManifest,
    DuplicateIcon,
    Manifest(ManifestError),
    Icon(IconError),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::Wasm(e) => e.fmt(f),
            PackageError::NoManifest => f.write_str("package has no osjeff.manifest section"),
            PackageError::DuplicateManifest => f.write_str("package has two manifests"),
            PackageError::DuplicateIcon => f.write_str("package has two icons"),
            PackageError::Manifest(e) => e.fmt(f),
            PackageError::Icon(e) => e.fmt(f),
        }
    }
}

/// A parsed package: manifest plus the optional decoded icon.
#[derive(Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub icon: Option<Image>,
}

/// Reads and validates the manifest and icon of `wasm`.
pub fn parse_package(wasm: &[u8]) -> Result<Package, PackageError> {
    let (n, m) =
        wasmsec::find_custom(wasm, MANIFEST_SECTION.as_bytes()).map_err(PackageError::Wasm)?;
    if n > 1 {
        return Err(PackageError::DuplicateManifest);
    }
    let m = m.ok_or(PackageError::NoManifest)?;
    let (ni, icon) =
        wasmsec::find_custom(wasm, ICON_SECTION.as_bytes()).map_err(PackageError::Wasm)?;
    if ni > 1 {
        return Err(PackageError::DuplicateIcon);
    }
    let manifest = Manifest::parse(m).map_err(PackageError::Manifest)?;
    let icon = match icon {
        Some(d) => Some(decode_icon(d).map_err(PackageError::Icon)?),
        None => None,
    };
    Ok(Package { manifest, icon })
}

/// Like [`parse_package`], but a module **without** a manifest yields `None`
/// (a legacy app) instead of an error. Any other problem is still an error.
pub fn parse_package_opt(wasm: &[u8]) -> Result<Option<Package>, PackageError> {
    match parse_package(wasm) {
        Ok(p) => Ok(Some(p)),
        Err(PackageError::NoManifest) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests;
