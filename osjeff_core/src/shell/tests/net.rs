//! Network commands (`nslookup`, `curl`, `wget`, `ifconfig`) and Ctrl+C, against
//! [`MockSys`].

use super::*;
use crate::shell::sys::{HttpResponse, MinimalSys, NetInfo};

fn page(status: u16, body: &[u8]) -> HttpResponse {
    HttpResponse {
        status,
        head: alloc::format!("HTTP/1.1 {status} X\r\nContent-Type: text/plain\r\n\r\n")
            .into_bytes(),
        body: body.to_vec(),
        truncated: false,
    }
}

#[test]
fn nslookup_prints_addresses_and_errors() {
    let mut t = T::new();
    t.sys.dns.push(("example.org".into(), [93, 184, 216, 34]));
    t.sys.dns.push(("example.org".into(), [93, 184, 216, 35]));
    let r = t.run("nslookup example.org");
    assert_eq!(r.status, 0);
    assert_eq!(
        r.text(),
        "Name:    example.org\nAddress: 93.184.216.34\nAddress: 93.184.216.35\n"
    );
    // An address literal needs no DNS.
    let r = t.run("nslookup 10.0.2.3");
    assert_eq!(r.status, 0);
    assert!(r.text().contains("Address: 10.0.2.3 (literal)"));
    let r = t.run("nslookup nope.invalid");
    assert_eq!(r.status, 1);
    assert!(r.text().contains("NXDOMAIN"));
    assert_eq!(t.run("nslookup").status, 2);
    assert_eq!(t.run("nslookup a b").status, 2);
}

#[test]
fn curl_prints_the_body_and_saves_files() {
    let mut t = T::new();
    t.sys
        .web
        .push(("http://a.org/x.txt".into(), page(200, b"hello\n")));
    t.sys
        .web
        .push(("https://a.org/err".into(), page(404, b"gone")));
    let r = t.run("curl a.org/x.txt");
    assert_eq!((r.status, r.text().as_str()), (0, "hello\n"));
    assert_eq!(t.sys.fetched, ["http://a.org/x.txt"]);
    // -i shows the head, -o writes a file, -O names it after the URL.
    let r = t.run("curl -i http://a.org/x.txt");
    assert!(r.text().starts_with("HTTP/1.1 200 X\r\n"));
    assert!(r.text().ends_with("\r\n\r\nhello\n"));
    assert_eq!(t.run("curl -s -o got.txt http://a.org/x.txt").text(), "");
    assert_eq!(t.fs.read("/got.txt").unwrap(), b"hello\n");
    assert_eq!(t.run("curl -O http://a.org/x.txt").status, 0);
    assert_eq!(t.fs.read("/x.txt").unwrap(), b"hello\n");
    // HTTP errors are only failures with -f; transport errors always are.
    assert_eq!(t.run("curl https://a.org/err").text(), "gone");
    assert_eq!(t.run("curl -f https://a.org/err").status, 22);
    let r = t.run("curl http://missing.org/");
    assert_eq!(r.status, 7);
    assert!(r.text().contains("(7)"));
    assert_eq!(t.run("curl ftp://a.org/f").status, 1);
    assert_eq!(t.run("curl").status, 2);
    assert_eq!(t.run("curl -z").status, 2);
}

#[test]
fn curl_to_the_screen_is_bounded_by_the_output_limit() {
    let mut t = T::new();
    t.sh.limits.max_output = 1000;
    t.sys
        .web
        .push(("http://big/".into(), page(200, &[b'x'; 5000])));
    let r = t.run("curl http://big/");
    assert_eq!(r.status, 23, "truncated bodies are reported");
    assert!(r.output.len() < 1200);
    assert!(r.truncated);
    assert!(r.text().contains("[output truncated]"));
}

#[test]
fn wget_saves_to_the_working_directory() {
    let mut t = T::with_files();
    t.sys
        .web
        .push(("http://a.org/dir/f.bin".into(), page(200, b"\x00\x01\x02")));
    t.sys
        .web
        .push(("http://a.org/".into(), page(200, b"<html>")));
    t.sys.web.push(("http://a.org/bad".into(), page(500, b"")));
    let r = t.run("cd d; wget http://a.org/dir/f.bin");
    assert_eq!(r.status, 0);
    assert!(r.text().contains("'f.bin' saved [3 bytes]"));
    assert_eq!(t.fs.read("/d/f.bin").unwrap(), [0, 1, 2]);
    t.run("wget -q a.org/");
    assert_eq!(t.fs.read("/d/index.html").unwrap(), b"<html>");
    assert_eq!(t.out("wget -q -O - a.org/"), "<html>");
    t.run("wget -q -O /z.html a.org/");
    assert_eq!(t.fs.read("/z.html").unwrap(), b"<html>");
    assert_eq!(t.run("wget a.org/bad").status, 8);
    assert_eq!(t.run("wget http://nowhere/").status, 4);
    assert_eq!(t.run("wget").status, 2);
}

#[test]
fn ifconfig_describes_the_interface() {
    let mut t = T::new();
    assert_eq!(t.run("ifconfig").status, 1);
    t.sys.net = Some(NetInfo {
        nic: "ne2000".into(),
        link_up: true,
        ip: Some("10.0.2.15".into()),
        prefix: 24,
        gateway: Some("10.0.2.2".into()),
        dns: alloc::vec!["10.0.2.3".into()],
        dhcp: "bound".into(),
        rx_packets: 12,
        rx_bytes: 1536,
        tx_packets: 7,
        tx_bytes: 700,
    });
    let s = t.out("ifconfig");
    assert!(s.contains("eth0: ne2000  link up"));
    assert!(s.contains("inet 10.0.2.15/24  gateway 10.0.2.2"));
    assert!(s.contains("dns 10.0.2.3"));
    assert!(s.contains("RX 12 packets, 1536 bytes (1.5 KiB)"));
    assert!(s.contains("TX 7 packets, 700 bytes (0.6 KiB)"));
}

#[test]
fn unsupported_network_commands_say_so() {
    let mut sh = Shell::new();
    let mut fs = MemFs::new();
    let mut sys = MinimalSys;
    let mut h = Host {
        fs: &mut fs,
        sys: &mut sys,
    };
    for line in ["nslookup example.org", "curl http://x/", "wget http://x/"] {
        let r = sh.run_line(line, &mut h);
        assert_ne!(r.status, 0, "{line}");
        assert!(r.text().contains("not supported"), "{line}: {}", r.text());
    }
    assert_eq!(sh.run_line("ifconfig", &mut h).status, 1);
}

#[test]
fn ctrl_c_stops_a_loop_with_status_130() {
    let mut t = T::new();
    t.sys.interrupt_after = Some(50);
    let r = t.run("while true; do echo x; done");
    assert_eq!(r.status, 130);
    assert!(r.text().contains("sh: interrupted"));
    // Far fewer iterations than the loop limit allowed.
    assert!(r.text().matches('x').count() < 100);
    // The shell is usable again.
    t.sys.interrupt_after = None;
    assert_eq!(t.out("echo ok"), "ok\n");
    assert_eq!(t.sh.last_status(), 0);
}

#[test]
fn ctrl_c_during_the_last_command_is_still_reported() {
    let mut t = T::new();
    // The wait (sleep) is the last command: no later step polls the flag.
    t.sys.interrupt_after = Some(1);
    let r = t.run("sleep 3");
    assert_eq!(r.status, 130);
    assert!(r.text().contains("interrupted"));
}
