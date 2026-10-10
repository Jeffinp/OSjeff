//! manifest (split out of `appmanifest.rs`).

use super::*;

impl Manifest {
    /// Parses and validates the text of an `kitsune.manifest` section.
    pub fn parse(data: &[u8]) -> Result<Manifest, ManifestError> {
        if data.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge);
        }
        let text = core::str::from_utf8(data).map_err(|_| ManifestError::NotUtf8)?;

        let mut id: Option<String> = None;
        let mut name: Option<String> = None;
        let mut names: Vec<(String, String)> = Vec::new();
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
                k if k.starts_with("name.") => {
                    let tag = &k["name.".len()..];
                    if !valid_lang_tag(tag) {
                        return Err(ManifestError::UnknownKey);
                    }
                    if !valid_local_name(val) {
                        return Err(ManifestError::BadValue("name.<lang>"));
                    }
                    if names.iter().any(|(t, _)| t == tag) {
                        return Err(ManifestError::DuplicateKey("name.<lang>"));
                    }
                    // A tag for a language the system does not have is fine (kept, unused), but
                    // not without bound.
                    if names.len() >= MAX_LOCAL_NAMES {
                        return Err(ManifestError::OverLimit("name.<lang>"));
                    }
                    names.push((String::from(tag), String::from(val)));
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
            names,
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
            names: Vec::new(),
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

    /// The name to show in `lang`: the `name.<lang>` of that language (the full tag, then its
    /// primary part: `pt-br`, then `pt`), else the plain `name`.
    pub fn name_in(&self, lang: Lang) -> &str {
        let code = lang.code();
        let primary = code.split('-').next().unwrap_or(code);
        let find = |want: &str| {
            self.names
                .iter()
                .find(|(t, _)| t.eq_ignore_ascii_case(want))
                .map(|(_, n)| n.as_str())
        };
        find(code).or_else(|| find(primary)).unwrap_or(&self.name)
    }

    /// The name to show in the language in effect.
    pub fn display_name(&self) -> &str {
        self.name_in(i18n::lang())
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
