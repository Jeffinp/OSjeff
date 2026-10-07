//! Network builtins: `nslookup`, `curl`, `wget`, `ifconfig`.
//!
//! They only translate arguments and results; the work is done by the kernel
//! through [`SysInfo::resolve`], [`SysInfo::http_get`] and
//! [`SysInfo::net_info`] (defaults answer "not supported", so a partial kernel
//! still prints a clear message). `ping` lives in [`super::builtins`].

use super::builtins::{fs_err, parse_opts};
use super::exec::{BuiltinFn, CmdCtx, Registry};
use super::fs::basename;
use super::sys::SysErr;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Most body bytes `curl`/`wget` keep (the kernel fetcher has its own cap).
pub const MAX_DOWNLOAD: usize = 4 << 20;

/// Register the network builtins into `r`.
pub fn register_all(r: &mut Registry) {
    let table: [(&str, &'static str, BuiltinFn); 4] = [
        (
            "nslookup",
            "nslookup HOST: look up the IPv4 addresses of a name",
            nslookup,
        ),
        (
            "curl",
            "curl [-sSfiL] [-o FILE|-O] URL: fetch a http(s) URL",
            curl,
        ),
        (
            "wget",
            "wget [-q] [-O FILE|-] URL: download a http(s) URL to a file",
            wget,
        ),
        ("ifconfig", "ifconfig: show the network interface", ifconfig),
    ];
    for (n, h, f) in table {
        r.register(n, h, f);
    }
}

/// `a.b.c.d` as four bytes, or `None`.
pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut it = s.split('.');
    for slot in &mut out {
        let part = it.next()?;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse::<u8>().ok()?;
    }
    it.next().is_none().then_some(out)
}

fn dotted(a: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

fn nslookup(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "", "", None) else {
        return 2;
    };
    let [host] = o.operands.as_slice() else {
        cx.error("usage: nslookup HOST");
        return 2;
    };
    let host = host.clone();
    if let Some(ip) = parse_ipv4(&host) {
        cx.println(&format!("Name:    {host}"));
        cx.println(&format!("Address: {} (literal)", dotted(ip)));
        return 0;
    }
    match cx.sys.resolve(&host) {
        Ok(addrs) if !addrs.is_empty() => {
            cx.println(&format!("Name:    {host}"));
            for a in addrs {
                cx.println(&format!("Address: {}", dotted(a)));
            }
            0
        }
        Ok(_) | Err(SysErr::HostNotFound) => {
            cx.error(&format!("can't find {host}: NXDOMAIN"));
            1
        }
        Err(e) => {
            cx.error(&format!("{host}: {}", e.message()));
            1
        }
    }
}

/// `http://` is assumed when the URL has no scheme; other schemes are refused.
fn normalize_url(raw: &str) -> Result<String, &'static str> {
    match raw.split_once("://") {
        None => Ok(format!("http://{raw}")),
        Some((s, rest))
            if (s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https"))
                && !rest.is_empty() =>
        {
            Ok(raw.to_string())
        }
        Some(_) => Err("unsupported protocol"),
    }
}

/// The file name `wget`/`curl -O` use for `url`.
fn remote_name(url: &str) -> String {
    let after = url.split_once("://").map_or(url, |(_, r)| r);
    let path = after.split_once('/').map_or("", |(_, p)| p);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let name = basename(path);
    if name.is_empty() {
        String::from("index.html")
    } else {
        name.to_string()
    }
}

/// curl's exit status for a failed transfer.
fn curl_code(e: SysErr) -> i32 {
    match e {
        SysErr::HostNotFound => 6,
        SysErr::Network => 7,
        SysErr::Timeout => 28,
        SysErr::Failed => 56,
        SysErr::Cancelled => 130,
        SysErr::Unsupported | SysErr::NoSuchProcess | SysErr::Denied => 1,
    }
}

fn curl(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "sSfiLO", "o", None) else {
        return 2;
    };
    let [raw] = o.operands.as_slice() else {
        cx.error("usage: curl [-sSfiL] [-o FILE|-O] URL");
        return 2;
    };
    let url = match normalize_url(raw) {
        Ok(u) => u,
        Err(m) => {
            cx.error(&format!("(1) {m}"));
            return 1;
        }
    };
    let quiet = o.has('s') && !o.has('S');
    let dest: Option<String> = if let Some(f) = o.value('o') {
        Some(f.to_string())
    } else if o.has('O') {
        Some(remote_name(&url))
    } else {
        None
    };
    // To the screen the body is bounded like any other output.
    let cap = if dest.is_some() {
        MAX_DOWNLOAD
    } else {
        cx.limits.max_output
    };
    let r = match cx.sys.http_get(&url, cap) {
        Ok(r) => r,
        Err(e) => {
            if !quiet || e == SysErr::Cancelled {
                let what = match e {
                    SysErr::HostNotFound => "Could not resolve host".to_string(),
                    other => other.message().to_string(),
                };
                let code = curl_code(e);
                cx.error(&format!("({code}) {what}: {url}"));
            }
            return curl_code(e);
        }
    };
    if o.has('f') && r.status >= 400 {
        if !quiet {
            cx.error(&format!(
                "(22) The requested URL returned error: {}",
                r.status
            ));
        }
        return 22;
    }
    if o.has('i') {
        cx.out(&r.head);
        if !r.head.ends_with(b"\r\n\r\n") {
            cx.out(b"\r\n\r\n");
        }
    }
    let mut status = 0;
    match dest {
        Some(f) if f != "-" => {
            if let Err(e) = cx.fs.write(&f, &r.body) {
                fs_err(cx, &f, e);
                return 23;
            }
        }
        _ => cx.out(&r.body),
    }
    if r.truncated {
        cx.error(&format!("(23) body cut at {} bytes", r.body.len()));
        status = 23;
    }
    status
}

fn wget(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(o) = parse_opts(cx, "q", "O", None) else {
        return 2;
    };
    let [raw] = o.operands.as_slice() else {
        cx.error("usage: wget [-q] [-O FILE|-] URL");
        return 2;
    };
    let url = match normalize_url(raw) {
        Ok(u) => u,
        Err(m) => {
            cx.error(&format!("{raw}: {m}"));
            return 1;
        }
    };
    let quiet = o.has('q');
    let to_stdout = o.value('O') == Some("-");
    let name = o
        .value('O')
        .filter(|f| *f != "-")
        .map_or_else(|| remote_name(&url), |f| f.to_string());
    if !quiet && !to_stdout {
        cx.println(&format!("Fetching {url}"));
    }
    let r = match cx.sys.http_get(&url, MAX_DOWNLOAD) {
        Ok(r) => r,
        Err(e) => {
            cx.error(&format!("{url}: {}", e.message()));
            return if e == SysErr::Cancelled { 130 } else { 4 };
        }
    };
    if r.status >= 400 {
        cx.error(&format!("{url}: server returned error {}", r.status));
        return 8;
    }
    if to_stdout {
        cx.out(&r.body);
    } else if let Err(e) = cx.fs.write(&name, &r.body) {
        fs_err(cx, &name, e);
        return 1;
    } else if !quiet {
        cx.println(&format!("'{name}' saved [{} bytes]", r.body.len()));
    }
    if r.truncated {
        cx.error(&format!("warning: body cut at {} bytes", r.body.len()));
    }
    0
}

fn kib(bytes: u64) -> String {
    format!("{}.{} KiB", bytes / 1024, bytes % 1024 * 10 / 1024)
}

fn ifconfig(cx: &mut CmdCtx<'_>) -> i32 {
    let Some(i) = cx.sys.net_info() else {
        cx.error("no network interface");
        return 1;
    };
    cx.println(&format!(
        "eth0: {}  link {}",
        i.nic,
        if i.link_up { "up" } else { "down" }
    ));
    match &i.ip {
        Some(ip) => {
            let mut l = format!("  inet {ip}/{}", i.prefix);
            if let Some(g) = &i.gateway {
                l.push_str(&format!("  gateway {g}"));
            }
            cx.println(&l);
        }
        None => cx.println("  no address"),
    }
    if !i.dns.is_empty() {
        let l: Vec<&str> = i.dns.iter().map(String::as_str).collect();
        cx.println(&format!("  dns {}", l.join(" ")));
    }
    if !i.dhcp.is_empty() {
        cx.println(&format!("  dhcp {}", i.dhcp));
    }
    cx.println(&format!(
        "  RX {} packets, {} bytes ({})",
        i.rx_packets,
        i.rx_bytes,
        kib(i.rx_bytes)
    ));
    cx.println(&format!(
        "  TX {} packets, {} bytes ({})",
        i.tx_packets,
        i.tx_bytes,
        kib(i.tx_bytes)
    ));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_literals() {
        assert_eq!(parse_ipv4("10.0.2.3"), Some([10, 0, 2, 3]));
        assert_eq!(parse_ipv4("0.0.0.0"), Some([0; 4]));
        for bad in [
            "",
            "1.2.3",
            "1.2.3.4.5",
            "256.1.1.1",
            "a.b.c.d",
            "1..2.3",
            "+1.2.3.4",
            "1.2.3.4 ",
        ] {
            assert_eq!(parse_ipv4(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn urls_get_a_scheme_or_are_refused() {
        assert_eq!(
            normalize_url("example.org/a").unwrap(),
            "http://example.org/a"
        );
        assert_eq!(normalize_url("https://x/").unwrap(), "https://x/");
        assert_eq!(normalize_url("HTTP://x/").unwrap(), "HTTP://x/");
        assert!(normalize_url("ftp://x/").is_err());
        assert!(normalize_url("file:///etc").is_err());
        assert!(normalize_url("http://").is_err());
    }

    #[test]
    fn remote_names() {
        assert_eq!(remote_name("http://a.org/x/y.txt?q=1#f"), "y.txt");
        assert_eq!(remote_name("http://a.org/"), "index.html");
        assert_eq!(remote_name("http://a.org"), "index.html");
        assert_eq!(remote_name("https://a.org/dir/"), "index.html");
    }

    #[test]
    fn size_text() {
        assert_eq!(kib(0), "0.0 KiB");
        assert_eq!(kib(1536), "1.5 KiB");
    }
}
