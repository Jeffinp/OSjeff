#[test]
fn bookmarks_text_roundtrip_and_damage() {
    let items = vec![
        Bookmark {
            url: "http://a.example/".into(),
            title: "A\tpage\n".into(),
        },
        Bookmark {
            url: "https://b.example/x".into(),
            title: String::new(),
        },
    ];
    let text = bookmarks_to_text(&items);
    let back = bookmarks_from_text(text.as_bytes());
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].title, "A page");
    assert_eq!(back[1].url, "https://b.example/x");
    // damage: bad UTF-8, blank lines, duplicates, no tab
    let junk = b"\xff\xfe\n\n\thttp://x\nhttp://d/\tD\nhttp://d/\tagain\nhttp://e/\n";
    let got = bookmarks_from_text(junk);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].url, "http://d/");
    assert_eq!(got[1].title, "");
    // never more than MAX_BOOKMARKS
    let mut big = String::new();
    for i in 0..(MAX_BOOKMARKS + 20) {
        big.push_str(&format!("http://h{i}/\tt\n"));
    }
    assert_eq!(bookmarks_from_text(big.as_bytes()).len(), MAX_BOOKMARKS);
}

#[test]
fn saved_bookmarks_flush_on_change_only() {
    use alloc::rc::Rc;
    use core::cell::RefCell;
    let saved: Rc<RefCell<Vec<Vec<u8>>>> = Rc::default();
    let sink = saved.clone();
    let mut s = SavedBookmarks::load(b"http://a/\tA\n", move |t: &[u8]| {
        sink.borrow_mut().push(t.to_vec())
    });
    assert!(s.contains("http://a/"));
    assert!(!s.add(Bookmark {
        url: "http://a/".into(),
        title: "dup".into()
    }));
    assert!(saved.borrow().is_empty(), "a refused add must not write");
    assert!(s.add(Bookmark {
        url: "http://b/".into(),
        title: "B".into()
    }));
    assert_eq!(saved.borrow().len(), 1);
    assert_eq!(saved.borrow()[0], b"http://a/\tA\nhttp://b/\tB\n");
    assert!(s.remove("http://a/"));
    assert!(!s.remove("http://a/"));
    assert_eq!(saved.borrow().len(), 2);
    assert_eq!(saved.borrow()[1], b"http://b/\tB\n");
}
use super::*;

#[test]
fn parse_plain_host_defaults_https() {
    let u = parse_url(b"example.com").unwrap();
    assert!(u.https);
    assert_eq!(u.port, 443);
    assert_eq!(u.host(), b"example.com");
    assert_eq!(u.path(), b"/");
}

#[test]
fn parse_http_scheme_and_path_and_port() {
    let u = parse_url(b"http://example.com:8080/a/b?x=1").unwrap();
    assert!(!u.https);
    assert_eq!(u.port, 8080);
    assert_eq!(u.host(), b"example.com");
    assert_eq!(u.path(), b"/a/b?x=1");
}

#[test]
fn parse_https_default_port_and_query_only_path() {
    let u = parse_url(b"https://duckduckgo.com/html/?q=rust").unwrap();
    assert!(u.https);
    assert_eq!(u.port, 443);
    assert_eq!(u.host(), b"duckduckgo.com");
    assert_eq!(u.path(), b"/html/?q=rust");
}

/// Regression: an over-long port (`host:99999999999`) overflowed the u32
/// accumulator (panic with overflow checks, silent wrap -> wrong port in
/// release). Out-of-range ports now fall back to the scheme default.
#[test]
fn parse_url_oversized_port_falls_back_to_default() {
    let u = parse_url(b"http://example.com:99999999999/x").unwrap();
    assert_eq!(u.port, 80);
    assert_eq!(u.path(), b"/x");
    // 4294967296 + 80 would wrap to 80 in a u32 accumulator.
    let u = parse_url(b"https://example.com:4294967376/").unwrap();
    assert_eq!(u.port, 443);
    let u = parse_url(b"https://example.com:65536/").unwrap();
    assert_eq!(u.port, 443);
    let u = parse_url(b"https://example.com:65535/").unwrap();
    assert_eq!(u.port, 65535);
}

#[test]
fn parse_rejects_empty_host() {
    assert!(parse_url(b"   ").is_none());
    assert!(parse_url(b"https://").is_none());
}

#[test]
fn url_vs_search_heuristic() {
    assert!(looks_like_url(b"example.com"));
    assert!(looks_like_url(b"http://foo.bar/baz"));
    assert!(!looks_like_url(b"rust programming"));
    assert!(!looks_like_url(b"hello"));
    assert!(!looks_like_url(b"two words.with dot"));
}

#[test]
fn encode_query_escapes() {
    let mut out = [0u8; 64];
    let n = encode_query(b"rust lang & co", &mut out);
    assert_eq!(&out[..n], b"rust+lang+%26+co");
}

#[test]
fn search_url_built() {
    let mut out = [0u8; 128];
    let n = build_search_url(b"rust", &mut out);
    assert_eq!(&out[..n], b"https://www.bing.com/search?q=rust");
}

#[test]
fn http_body_after_headers() {
    let resp = b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<p>hi</p>";
    assert_eq!(http_body(resp), b"<p>hi</p>");
}

#[test]
fn parses_status_and_headers() {
    let resp = b"HTTP/1.1 301 Moved\r\nLocation: https://x.com/y\r\nServer: t\r\n\r\nbody";
    assert_eq!(status_code(resp), Some(301));
    assert_eq!(
        header_value(resp, b"location"),
        Some(&b"https://x.com/y"[..])
    );
    assert_eq!(
        header_value(resp, b"LOCATION"),
        Some(&b"https://x.com/y"[..])
    );
    assert_eq!(header_value(resp, b"missing"), None);
}

#[test]
fn page_body_dechunks() {
    let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
    assert_eq!(page_body(resp), b"Wikipedia");
}

/// Regression: a chunk-size line with too many hex digits overflowed
/// `size * 16` (debug: panic) and, wrapping in release, made `i + size`
/// wrap below `i` so `&body[i..end]` panicked. A remote server controls
/// this, so it must be handled without panicking.
#[test]
fn dechunk_huge_chunk_size_does_not_panic() {
    // 17 hex digits: overflows usize on the multiply.
    let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFFF\r\nabc";
    let _ = page_body(resp);
    // 16 hex digits == usize::MAX: no multiply overflow, but `i + size`
    // wraps below `i` in release builds.
    let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFF\r\nabc";
    let _ = page_body(resp);
    // A valid chunk before the bad one is kept.
    let resp =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\nFFFFFFFFFFFFFFFFFFFF\r\nxyz";
    assert_eq!(page_body(resp), b"abc");
}

#[test]
fn browser_typing_and_caret() {
    let mut b = Browser::new();
    // clear the default url
    while b.caret() > 0 {
        b.on_key(Key::Backspace);
    }
    for &c in b"abc" {
        b.on_key(Key::Char(c));
    }
    assert_eq!(b.url(), b"abc");
    b.on_key(Key::Left);
    b.on_key(Key::Char(b'X'));
    assert_eq!(b.url(), b"abXc");
}

#[test]
fn browser_submit_navigates_url() {
    let mut b = Browser::new();
    assert!(b.is_home());
    for &c in b"example.com" {
        b.on_key(Key::Char(c));
    }
    b.on_key(Key::Enter);
    assert!(!b.is_home());
    let req = b.take_request().unwrap();
    assert_eq!(req, b"https://example.com");
    assert_eq!(b.status(), Status::Loading);
    assert!(b.take_request().is_none()); // consumed
}

#[test]
fn browser_go_home_resets() {
    let mut b = Browser::new();
    b.open(b"example.com");
    let _ = b.take_request();
    b.loaded();
    assert!(!b.is_home());
    b.go_home();
    assert!(b.is_home());
    assert_eq!(b.url(), b"");
    assert_eq!(b.status(), Status::Idle);
}

#[test]
fn browser_submit_searches_free_text() {
    let mut b = Browser::new();
    while b.caret() > 0 {
        b.on_key(Key::Backspace);
    }
    for &c in b"rust lang" {
        b.on_key(Key::Char(c));
    }
    b.on_key(Key::Enter);
    let req = b.take_request().unwrap();
    assert_eq!(req, b"https://www.bing.com/search?q=rust+lang");
}

#[test]
fn browser_loaded_sets_done() {
    let mut b = Browser::new();
    b.open(b"example.com");
    let _ = b.take_request();
    b.loaded();
    assert_eq!(b.status(), Status::Done);
    assert!(!b.is_home());
}

/// Regression: host/path go straight into the request line and `Host:`
/// header, so a CR/LF in a URL (typed, or from a server's `Location`)
/// allowed request splitting. Any ASCII control now makes the URL invalid.
#[test]
fn parse_url_rejects_control_characters() {
    for bad in [
        &b"http://a.com/x\r\nHost: evil"[..],
        b"http://a.com\r\nX: y/",
        b"http://a.com/\0",
        b"http://a.com/p\tq",
        b"http://a.com/p\x7fq",
        b"ex\x01ample.com",
        b"http://a.com/x y\nz",
    ] {
        assert!(parse_url(bad).is_none(), "{:?}", core::str::from_utf8(bad));
    }
    // Leading blanks are still tolerated; spaces and non-ASCII are not controls.
    assert!(parse_url(b" \t http://a.com/x").is_some());
    assert!(parse_url(b"http://a.com/caf\xc3\xa9").is_some());
}

/// Regression: `decode_utf8(&[])` indexed `bytes[0]` and panicked.
#[test]
fn decode_utf8_handles_empty_and_truncated_input() {
    assert_eq!(decode_utf8(&[]), (0, 0));
    assert_eq!(decode_utf8(b"a"), (0x61, 1));
    assert_eq!(decode_utf8("\u{e9}".as_bytes()), (0xE9, 2));
    assert_eq!(decode_utf8(&[0xE2, 0x82]), (0, 1)); // cut mid-sequence
    assert_eq!(decode_utf8(&[0xFF]), (0, 1));
}

/// Regression: `http_get` appended every received byte to a `Vec` with no
/// ceiling (the TLS path had its own 256 KiB check), so a server could
/// stream until the 64 MiB heap was gone. Both paths now share this cap.
#[test]
fn append_capped_never_exceeds_the_cap() {
    let mut v = alloc::vec::Vec::new();
    assert!(!append_capped(&mut v, b"abcd", 10));
    assert!(!append_capped(&mut v, b"efghij", 10)); // exactly full: not truncated
    assert_eq!(v, b"abcdefghij");
    assert!(append_capped(&mut v, b"k", 10)); // one byte over
    assert_eq!(v.len(), 10);
    let mut v = alloc::vec::Vec::new();
    assert!(append_capped(&mut v, &[7u8; 100], 30));
    assert_eq!(v.len(), 30);
    assert!(append_capped(&mut v, b"x", 30));
    assert_eq!(v.len(), 30);
    // Already above the cap (cap lowered): nothing added, no underflow.
    assert!(append_capped(&mut v, b"yz", 5));
    assert_eq!(v.len(), 30);
    // Streaming a huge body in small pieces stays at the cap.
    let mut v = alloc::vec::Vec::new();
    let mut truncated = false;
    for _ in 0..(MAX_RESPONSE_BYTES / 1024 + 100) {
        truncated |= append_capped(&mut v, &[0u8; 1024], MAX_RESPONSE_BYTES);
    }
    assert_eq!(v.len(), MAX_RESPONSE_BYTES);
    assert!(truncated);
    // A body that fits is untouched.
    let mut v = alloc::vec::Vec::new();
    assert!(!append_capped(&mut v, &[1u8; 1024], MAX_RESPONSE_BYTES));
    assert_eq!(v.len(), 1024);
}

#[test]
fn https_is_never_secure_until_the_fetcher_says_verified() {
    let mut b = Browser::new();
    assert_eq!(b.security(), Security::None);
    assert_eq!(b.security().label(), None);
    b.open(b"https://example.com");
    // In flight: no claim either way (and certainly no padlock).
    assert_eq!(b.security(), Security::None);
    let _ = b.take_request();
    b.loaded_with(Conn::Verified, false);
    assert_eq!(b.security(), Security::HttpsVerified);
    assert_eq!(b.security().label_key(), Some("web.sec.secure"));
    assert_eq!(b.security().label(), Some("Conexão segura"));
    assert!(!b.truncated());
    // A bare host is normalised to https: a new load drops the padlock at once.
    b.open(b"example.com");
    assert_eq!(b.security(), Security::None);
    // Plain http says so.
    b.open(b"http://example.com");
    assert_eq!(b.security(), Security::Http);
    assert_eq!(b.security().label_key(), Some("web.sec.insecure"));
    assert_eq!(b.security().label(), Some("Não seguro"));
}

#[test]
fn only_a_verified_load_shows_the_padlock() {
    for conn in [Conn::Plain, Conn::Insecure] {
        let mut b = Browser::new();
        b.open(b"https://example.com");
        let _ = b.take_request();
        b.loaded_with(conn, false);
        assert_ne!(b.security(), Security::HttpsVerified, "{conn:?}");
    }
    let mut b = Browser::new();
    b.open(b"https://example.com");
    let _ = b.take_request();
    b.loaded_with(Conn::Insecure, false);
    assert_eq!(b.security(), Security::HttpsInvalid);
    assert_eq!(b.security().label_key(), Some("web.sec.invalid"));
    assert_eq!(b.security().label(), Some("Certificado inválido"));
    // The page-load entry points that do not carry a connection state never
    // produce a padlock.
    b.loaded();
    assert_ne!(b.security(), Security::HttpsVerified);
}

#[test]
fn final_scheme_after_redirect_wins_over_requested_one() {
    let mut b = Browser::new();
    b.open(b"http://example.com"); // asked for http...
    let _ = b.take_request();
    b.loaded_with(Conn::Verified, false); // ...but ended on verified https
    assert_eq!(b.security(), Security::HttpsVerified);
    b.open(b"https://example.com");
    let _ = b.take_request();
    b.loaded_with(Conn::Plain, false); // (the kernel blocks this; the model stays honest)
    assert_eq!(b.security(), Security::Http);
}

#[test]
fn cert_failure_offers_a_per_origin_session_override() {
    use crate::network::tlsverify::CertError;
    let mut b = Browser::new();
    b.open(b"https://expired.example.com/page");
    let _ = b.take_request();
    assert_eq!(b.insecure_host(), None);
    b.fail_with(FailReason::Cert(CertError::Expired));
    assert!(b.can_continue_insecure());
    assert_eq!(
        errors::describe_in(Lang::Pt, b.fail_reason()).cause,
        "O certificado do site expirou."
    );
    b.continue_insecure();
    assert_eq!(b.status(), Status::Loading);
    assert_eq!(
        b.take_request(),
        Some(&b"https://expired.example.com/page"[..])
    );
    assert_eq!(
        b.insecure_host().as_deref(),
        Some(&b"expired.example.com"[..])
    );
    b.loaded_with(Conn::Insecure, false);
    assert_eq!(b.security(), Security::HttpsInvalid);
    // Another origin is not covered.
    b.open(b"https://other.example.com/");
    assert_eq!(b.insecure_host(), None);
    // Same origin, other path: covered (it is per origin, case-insensitive).
    b.open(b"https://EXPIRED.example.com/x");
    assert_eq!(
        b.insecure_host().as_deref(),
        Some(&b"expired.example.com"[..])
    );
    // http never uses the override.
    b.open(b"http://expired.example.com/");
    assert_eq!(b.insecure_host(), None);
}

#[test]
fn override_is_only_offered_for_certificate_errors() {
    let mut b = Browser::new();
    b.open(b"https://x.example.com");
    let _ = b.take_request();
    for r in [
        FailReason::Network,
        FailReason::Dns,
        FailReason::Refused,
        FailReason::Timeout,
        FailReason::Tls,
        FailReason::RedirectDowngrade,
    ] {
        b.fail_with(r);
        assert!(!b.can_continue_insecure(), "{r:?}");
        b.continue_insecure(); // must be a no-op
        assert_eq!(b.status(), Status::Error);
        assert_eq!(b.insecure_host(), None);
    }
}

#[test]
fn override_list_is_bounded_and_forgets_the_oldest() {
    use crate::network::tlsverify::CertError;
    let mut b = Browser::new();
    for i in 0..MAX_INSECURE_HOSTS + 2 {
        let url = alloc::format!("https://h{i}.example.com/");
        b.open(url.as_bytes());
        let _ = b.take_request();
        b.fail_with(FailReason::Cert(CertError::NameMismatch));
        b.continue_insecure();
        let _ = b.take_request();
    }
    b.open(b"https://h0.example.com/");
    assert_eq!(b.insecure_host(), None, "oldest was evicted");
    b.open(b"https://h9.example.com/");
    assert!(b.insecure_host().is_some());
}

#[test]
fn every_failure_message_is_distinct_for_network_causes() {
    use crate::network::tlsverify::CertError;
    let causes = [
        FailReason::Network,
        FailReason::Dns,
        FailReason::Refused,
        FailReason::Timeout,
        FailReason::Tls,
        FailReason::Cert(CertError::Expired),
    ];
    for (i, a) in causes.iter().enumerate() {
        for l in Lang::ALL {
            let (ta, tb) = (errors::describe_in(l, *a), a);
            // Latin-1 at most: the interface font covers Portuguese accents.
            assert!(ta.cause.chars().all(|c| (c as u32) < 0x100));
            assert!(ta.title.chars().all(|c| (c as u32) < 0x100));
            for b in &causes[i + 1..] {
                assert_ne!(ta.cause, errors::describe_in(l, *b).cause, "{tb:?} {b:?}");
            }
        }
    }
    assert_eq!(
        errors::describe_in(Lang::En, FailReason::Cert(CertError::NameMismatch)).art,
        errors::ErrorArt::Certificate
    );
}

#[test]
fn truncation_and_failure_state() {
    let mut b = Browser::new();
    b.open(b"example.com");
    let _ = b.take_request();
    b.loaded_with(Conn::Verified, true);
    assert!(b.truncated());
    assert_eq!(b.status(), Status::Done);
    // A new navigation clears the flag.
    b.open(b"example.org");
    assert!(!b.truncated());
    let _ = b.take_request();
    b.fail_with(FailReason::RedirectDowngrade);
    assert_eq!(b.status(), Status::Error);
    assert_eq!(b.fail_reason(), FailReason::RedirectDowngrade);
    assert!(
        errors::describe_in(Lang::Pt, b.fail_reason())
            .cause
            .contains("conexão segura")
    );
    assert!(
        errors::describe_in(Lang::En, b.fail_reason())
            .cause
            .contains("secure connection")
    );
    b.fail();
    assert_eq!(b.fail_reason(), FailReason::Network);
    b.go_home();
    assert_eq!(b.security(), Security::None);
}

#[test]
fn redirect_errors_map_to_distinct_reasons() {
    use crate::browsing::redirect::RedirectError as E;
    let all = [E::Invalid, E::Downgrade, E::Loop, E::TooMany];
    let reasons: alloc::vec::Vec<_> = all.iter().map(|&e| FailReason::from_redirect(e)).collect();
    for (i, a) in reasons.iter().enumerate() {
        for b in &reasons[i + 1..] {
            assert_ne!(a, b);
        }
        // Latin-1 at most: the interface font covers Portuguese accents.
        assert!(
            errors::describe_in(Lang::Pt, *a)
                .cause
                .chars()
                .all(|c| (c as u32) < 0x100)
        );
    }
    assert_eq!(
        FailReason::from_redirect(E::Downgrade),
        FailReason::RedirectDowngrade
    );
}

#[test]
fn worker_died_is_a_distinct_failure() {
    let mut b = Browser::new();
    b.open(b"example.com");
    let _ = b.take_request();
    b.fail_with(FailReason::WorkerDied);
    assert_eq!(b.status(), Status::Error);
    assert_eq!(b.fail_reason(), FailReason::WorkerDied);
    assert_ne!(FailReason::WorkerDied, FailReason::Network);
    // The next navigation starts clean.
    b.open(b"example.org");
    assert_eq!(b.status(), Status::Loading);
}

const GZ_PAGE: &str = "1f8b08000000000002ffb3c928c9cdb1b349ca4fa9b4b3c930b4f3cf49b4d107d2360576e95599050ae5f945d9c50a99790afec159a9696936fa057636fa10d5fa60ad0094af60dd41000000";

fn hexv(s: &str) -> alloc::vec::Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn page_body_gunzips_content_encoding_gzip() {
    let mut resp =
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Type: text/html\r\n\r\n".to_vec();
    resp.extend_from_slice(&hexv(GZ_PAGE));
    let body = page_body(&resp);
    assert!(body.starts_with(b"<html><body><h1>Ola</h1>"));
}

#[test]
fn page_body_dechunks_then_gunzips() {
    let gz = hexv(GZ_PAGE);
    let (a, b) = gz.split_at(20);
    let mut resp =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Encoding: GZIP\r\n\r\n".to_vec();
    for part in [a, b] {
        resp.extend_from_slice(alloc::format!("{:x}\r\n", part.len()).as_bytes());
        resp.extend_from_slice(part);
        resp.extend_from_slice(b"\r\n");
    }
    resp.extend_from_slice(b"0\r\n\r\n");
    assert!(page_body(&resp).starts_with(b"<html><body><h1>Ola</h1>"));
}

#[test]
fn page_body_bad_or_unsupported_encoding_gives_a_notice() {
    let resp = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\nnot gzip at all, sorry";
    let body = page_body(resp);
    assert!(String::from_utf8_lossy(&body).contains("Falha ao descompactar"));
    let resp = b"HTTP/1.1 200 OK\r\nContent-Encoding: br\r\n\r\n\x01\x02";
    assert!(String::from_utf8_lossy(&page_body(resp)).contains("não suportada"));
    // identity is a no-op
    let resp = b"HTTP/1.1 200 OK\r\nContent-Encoding: identity\r\n\r\n<p>x</p>";
    assert_eq!(page_body(resp), b"<p>x</p>");
}

fn load(b: &mut Browser, url: &[u8]) {
    b.open(url);
    assert!(b.take_request().is_some());
    b.loaded_with(Conn::Verified, false);
}

#[test]
fn history_back_forward_and_truncation() {
    let mut b = Browser::new();
    assert!(!b.can_back() && !b.can_forward());
    load(&mut b, b"https://a.example/");
    load(&mut b, b"https://b.example/");
    load(&mut b, b"https://c.example/");
    assert_eq!(b.history_len(), 3);
    assert!(b.can_back() && !b.can_forward());
    b.back();
    assert_eq!(b.take_request(), Some(&b"https://b.example/"[..]));
    b.loaded_with(Conn::Verified, false);
    assert_eq!(b.history_len(), 3, "going back adds no entry");
    assert!(b.can_back() && b.can_forward());
    b.back();
    assert_eq!(b.take_request(), Some(&b"https://a.example/"[..]));
    b.loaded_with(Conn::Verified, false);
    assert!(!b.can_back());
    b.back(); // no-op at the start
    assert!(b.take_request().is_none());
    b.forward();
    assert_eq!(b.take_request(), Some(&b"https://b.example/"[..]));
    b.loaded_with(Conn::Verified, false);
    // A new navigation from the middle drops the forward entries.
    load(&mut b, b"https://d.example/");
    assert_eq!(b.history_len(), 3);
    assert!(!b.can_forward());
    assert_eq!(b.url(), b"https://d.example/");
}

#[test]
fn reload_does_not_duplicate_history() {
    let mut b = Browser::new();
    load(&mut b, b"https://a.example/");
    b.reload();
    let _ = b.take_request();
    b.loaded_with(Conn::Verified, false);
    assert_eq!(b.history_len(), 1);
}

#[test]
fn history_is_bounded() {
    let mut b = Browser::new();
    for i in 0..MAX_HISTORY + 10 {
        let u = alloc::format!("https://h{i}.example/");
        load(&mut b, u.as_bytes());
    }
    assert_eq!(b.history_len(), MAX_HISTORY);
    b.back();
    let want = alloc::format!("https://h{}.example/", MAX_HISTORY + 8);
    assert_eq!(b.take_request(), Some(want.as_bytes()));
}

#[test]
fn failed_load_is_not_recorded() {
    let mut b = Browser::new();
    load(&mut b, b"https://a.example/");
    b.open(b"https://broken.example/");
    let _ = b.take_request();
    b.fail_with(FailReason::Dns);
    assert_eq!(b.history_len(), 1);
}

#[test]
fn open_link_resolves_relative_absolute_and_refuses_unsafe() {
    let mut b = Browser::new();
    load(&mut b, b"https://site.example/dir/page.html");
    assert!(b.open_link(b"other.html"));
    assert_eq!(
        b.take_request(),
        Some(&b"https://site.example/dir/other.html"[..])
    );
    b.loaded_with(Conn::Verified, false);
    assert!(b.open_link(b"/root.html"));
    assert_eq!(
        b.take_request(),
        Some(&b"https://site.example/root.html"[..])
    );
    b.loaded_with(Conn::Verified, false);
    assert!(b.open_link(b"https://elsewhere.example/x"));
    assert_eq!(b.take_request(), Some(&b"https://elsewhere.example/x"[..]));
    b.loaded_with(Conn::Verified, false);
    for bad in [
        &b"javascript:alert(1)"[..],
        b"data:text/html,x",
        b"http://plain.example/",
        b"",
        b"a b",
    ] {
        assert!(!b.open_link(bad), "{bad:?}");
    }
    assert!(b.take_request().is_none());
}
