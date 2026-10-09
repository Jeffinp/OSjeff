use super::*;

const S1: Ipv4 = Ipv4([10, 0, 2, 3]);
const S2: Ipv4 = Ipv4([8, 8, 8, 8]);

fn servers() -> DnsServers {
    DnsServers::from_slice(&[S1, S2])
}

/// Hand-assembled response to `(id, name)`: `answers` are raw RRs appended
/// after the question, `rcode` goes into the flags.
fn response(id: u16, name: &str, rcode: u8, answers: &[Vec<u8>]) -> Vec<u8> {
    let mut q = [0u8; 300];
    let n = build_query(&mut q, id, name).unwrap();
    let mut m = q[..n].to_vec();
    m[2] = 0x81; // QR, RD
    m[3] = 0x80 | rcode; // RA + rcode
    m[6..8].copy_from_slice(&(answers.len() as u16).to_be_bytes());
    for a in answers {
        m.extend_from_slice(a);
    }
    m
}

/// An RR owned by the question name (pointer to offset 12).
fn rr_ptr(rtype: u16, ttl: u32, rdata: &[u8]) -> Vec<u8> {
    let mut v = vec![0xC0, 0x0C];
    v.extend_from_slice(&rtype.to_be_bytes());
    v.extend_from_slice(&CLASS_IN.to_be_bytes());
    v.extend_from_slice(&ttl.to_be_bytes());
    v.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    v.extend_from_slice(rdata);
    v
}

fn a_rr(ttl: u32, ip: [u8; 4]) -> Vec<u8> {
    rr_ptr(TYPE_A, ttl, &ip)
}

fn wire_name(n: &str) -> Vec<u8> {
    let mut v = Vec::new();
    for l in n.split('.') {
        v.push(l.len() as u8);
        v.extend_from_slice(l.as_bytes());
    }
    v.push(0);
    v
}

#[test]
fn names_are_normalized_and_validated() {
    assert_eq!(
        normalize_name("Example.COM").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        normalize_name("example.com.").as_deref(),
        Some("example.com")
    );
    assert_eq!(normalize_name("a_b-c.d9").as_deref(), Some("a_b-c.d9"));
    for bad in [
        "", ".", "a..b", ".a", "a b", "a/b", "é.com", "a:80", "a.com..",
    ] {
        assert_eq!(normalize_name(bad), None, "{bad:?}");
    }
    let l63 = "a".repeat(63);
    assert!(normalize_name(&l63).is_some());
    assert!(
        normalize_name(&format!("{l63}a")).is_none(),
        "64-byte label"
    );
    let long = [l63.as_str(); 4].join(".");
    assert!(long.len() > MAX_NAME);
    assert!(normalize_name(&long).is_none());
}

#[test]
fn query_layout() {
    let mut out = [0u8; 64];
    let n = build_query(&mut out, 0xBEEF, "Www.Example.com").unwrap();
    assert_eq!(&out[..2], &[0xBE, 0xEF]);
    assert_eq!(&out[2..4], &[0x01, 0x00]); // RD
    assert_eq!(&out[4..6], &[0, 1]);
    assert_eq!(&out[12..n - 4], &wire_name("www.example.com")[..]);
    assert_eq!(&out[n - 4..n], &[0, 1, 0, 1]); // A IN
    // Too small a buffer, invalid names.
    assert_eq!(build_query(&mut out[..n - 1], 1, "www.example.com"), None);
    assert_eq!(build_query(&mut out, 1, "bad name"), None);
}

#[test]
fn plain_answer() {
    let m = response(7, "example.com", 0, &[a_rr(300, [93, 184, 216, 34])]);
    let Some(Reply::Addrs { addrs, ttl }) = parse_response(&m, 7, "example.com") else {
        panic!()
    };
    assert_eq!(addrs.as_slice(), &[Ipv4([93, 184, 216, 34])]);
    assert_eq!(ttl, 300);
}

#[test]
fn cname_chain_and_min_ttl() {
    // www.example.com -> CNAME cdn.example.net -> A (owner = the target).
    let target = wire_name("cdn.example.net");
    let mut a = target.clone();
    a.extend_from_slice(&TYPE_A.to_be_bytes());
    a.extend_from_slice(&CLASS_IN.to_be_bytes());
    a.extend_from_slice(&60u32.to_be_bytes());
    a.extend_from_slice(&4u16.to_be_bytes());
    a.extend_from_slice(&[1, 2, 3, 4]);
    let m = response(
        9,
        "www.example.com",
        0,
        &[rr_ptr(TYPE_CNAME, 900, &target), a],
    );
    let Some(Reply::Addrs { addrs, ttl }) = parse_response(&m, 9, "www.example.com") else {
        panic!()
    };
    assert_eq!(addrs.first(), Some(Ipv4([1, 2, 3, 4])));
    assert_eq!(ttl, 60, "minimum over the chain");
}

#[test]
fn unrelated_owner_is_not_trusted() {
    // An A record for another name (cache-poisoning attempt) is ignored.
    let mut evil = wire_name("evil.test");
    evil.extend_from_slice(&TYPE_A.to_be_bytes());
    evil.extend_from_slice(&CLASS_IN.to_be_bytes());
    evil.extend_from_slice(&60u32.to_be_bytes());
    evil.extend_from_slice(&4u16.to_be_bytes());
    evil.extend_from_slice(&[6, 6, 6, 6]);
    let m = response(9, "www.example.com", 0, &[evil]);
    assert_eq!(
        parse_response(&m, 9, "www.example.com"),
        Some(Reply::NoData)
    );
}

#[test]
fn matching_rules() {
    let m = response(7, "example.com", 0, &[a_rr(1, [1, 1, 1, 1])]);
    assert_eq!(parse_response(&m, 8, "example.com"), None, "wrong id");
    assert_eq!(parse_response(&m, 7, "other.com"), None, "wrong question");
    let mut q = m.clone();
    q[2] &= 0x7F; // QR cleared: a query
    assert_eq!(parse_response(&q, 7, "example.com"), None);
    // Case-insensitive question match (0x20 randomization servers echo it).
    assert!(parse_response(&m, 7, "EXAMPLE.com").is_some());
}

#[test]
fn rcodes() {
    let nx = response(1, "no.example.com", 3, &[]);
    assert_eq!(
        parse_response(&nx, 1, "no.example.com"),
        Some(Reply::NxDomain)
    );
    let sf = response(1, "x.com", 2, &[]);
    assert_eq!(parse_response(&sf, 1, "x.com"), Some(Reply::ServerFail(2)));
    let rf = response(1, "x.com", 5, &[]);
    assert_eq!(parse_response(&rf, 1, "x.com"), Some(Reply::ServerFail(5)));
    let nodata = response(1, "x.com", 0, &[]);
    assert_eq!(parse_response(&nodata, 1, "x.com"), Some(Reply::NoData));
    let mut tc = response(1, "x.com", 0, &[]);
    tc[2] |= 0x02; // TC
    assert_eq!(parse_response(&tc, 1, "x.com"), Some(Reply::ServerFail(0)));
}

#[test]
fn hostile_messages_never_panic() {
    // A forward / self pointer, a pointer loop, an over-long label, a
    // truncated RR, rdlen past the end.
    let base = response(3, "a.com", 0, &[a_rr(5, [1, 2, 3, 4])]);
    let qend = HDR + wire_name("a.com").len() + 4;
    for cut in 0..base.len() {
        let _ = parse_response(&base[..cut], 3, "a.com");
    }
    let mut m = base.clone();
    m[qend] = 0xC0;
    m[qend + 1] = qend as u8; // pointer to itself
    assert!(matches!(
        parse_response(&m, 3, "a.com"),
        Some(Reply::NoData | Reply::ServerFail(_)) | None
    ));
    let mut m = base.clone();
    m[qend] = 0xC0;
    m[qend + 1] = 0xFF; // forward pointer
    let _ = parse_response(&m, 3, "a.com");
    let mut m = base.clone();
    m[qend] = 0x80; // reserved label type
    let _ = parse_response(&m, 3, "a.com");
    // Random bytes after a valid header.
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..2000 {
        let mut m = base.clone();
        for _ in 0..4 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let i = (s as usize) % m.len();
            m[i] = (s >> 20) as u8;
        }
        let _ = parse_response(&m, 3, "a.com");
    }
}

#[test]
fn more_answers_than_the_cap_keep_the_first_few() {
    let rrs: Vec<_> = (1..=7).map(|i| a_rr(10, [9, 9, 9, i])).collect();
    let m = response(2, "m.com", 0, &rrs);
    let Some(Reply::Addrs { addrs, .. }) = parse_response(&m, 2, "m.com") else {
        panic!()
    };
    assert_eq!(addrs.as_slice().len(), MAX_ADDRS);
    assert_eq!(addrs.first(), Some(Ipv4([9, 9, 9, 1])));
}

// ---- cache ----

#[test]
fn cache_honors_ttl_exactly() {
    let mut c = DnsCache::new();
    c.insert_addr("Example.com", Ipv4([1, 2, 3, 4]), 10, 1_000);
    assert_eq!(
        c.get("example.com", 1_000),
        Some(Cached::Addr(Ipv4([1, 2, 3, 4])))
    );
    assert_eq!(
        c.get("EXAMPLE.COM.", 10_999),
        Some(Cached::Addr(Ipv4([1, 2, 3, 4])))
    );
    assert_eq!(c.get("example.com", 11_000), None, "expired at ttl");
    assert_eq!(c.remaining_ms("example.com", 6_000), Some(5_000));
    c.purge(11_000);
    assert!(c.is_empty());
}

#[test]
fn cache_ttl_clamp_zero_and_replace() {
    let mut c = DnsCache::new();
    c.insert_addr("a.com", Ipv4([1, 1, 1, 1]), u32::MAX, 0);
    assert_eq!(
        c.remaining_ms("a.com", 0),
        Some(u64::from(MAX_TTL_SECS) * 1000)
    );
    // Replaced by a newer answer.
    c.insert_addr("a.com", Ipv4([2, 2, 2, 2]), 5, 100);
    assert_eq!(c.len(), 1);
    assert_eq!(c.get("a.com", 100), Some(Cached::Addr(Ipv4([2, 2, 2, 2]))));
    // TTL 0 is not cached and removes the old entry.
    c.insert_addr("a.com", Ipv4([3, 3, 3, 3]), 0, 200);
    assert_eq!(c.get("a.com", 200), None);
    assert!(c.is_empty());
}

#[test]
fn negative_entries_are_short() {
    let mut c = DnsCache::new();
    c.insert_nxdomain("nope.example", 0);
    assert_eq!(c.get("nope.example", 29_999), Some(Cached::NxDomain));
    assert_eq!(c.get("nope.example", 30_000), None);
}

#[test]
fn cache_is_bounded_and_evicts_the_oldest_expiry() {
    let mut c = DnsCache::new();
    for i in 0..CACHE_CAP {
        c.insert_addr(&format!("h{i}.com"), Ipv4([1, 1, 1, 1]), 100 + i as u32, 0);
    }
    assert_eq!(c.len(), CACHE_CAP);
    c.insert_addr("new.com", Ipv4([2, 2, 2, 2]), 500, 0);
    assert_eq!(c.len(), CACHE_CAP);
    assert_eq!(c.get("h0.com", 1), None, "soonest expiry evicted");
    assert!(c.get("h1.com", 1).is_some());
    assert!(c.get("new.com", 1).is_some());
}

#[test]
fn cache_ignores_invalid_names() {
    let mut c = DnsCache::new();
    c.insert_addr("bad name", Ipv4([1, 1, 1, 1]), 10, 0);
    assert!(c.is_empty());
    assert_eq!(c.get("bad name", 0), None);
}

// ---- resolve ----

fn answer_for(r: &Resolve, ip: [u8; 4]) -> Vec<u8> {
    response(r.id(), r.name(), 0, &[a_rr(120, ip)])
}

#[test]
fn first_server_answers() {
    let mut r = Resolve::new("example.com", servers(), 0, 77, 0);
    assert_eq!(r.poll(0), Step::Send { server: S1, id: 77 });
    assert_eq!(r.poll(10), Step::Wait(ATTEMPT_TIMEOUT_MS));
    let m = answer_for(&r, [1, 2, 3, 4]);
    assert_eq!(
        r.on_response(S1, &m),
        Some(Outcome::Resolved {
            addr: Ipv4([1, 2, 3, 4]),
            ttl: 120
        })
    );
    assert_eq!(r.answered_by(), Some(0));
    assert!(matches!(r.poll(20), Step::Done(Outcome::Resolved { .. })));
}

#[test]
fn dead_first_server_fails_over_to_the_second() {
    let mut r = Resolve::new("example.com", servers(), 0, 77, 0);
    assert_eq!(r.poll(0), Step::Send { server: S1, id: 77 });
    assert_eq!(
        r.poll(ATTEMPT_TIMEOUT_MS - 1),
        Step::Wait(ATTEMPT_TIMEOUT_MS)
    );
    // Timeout: the same id goes to the second server.
    assert_eq!(
        r.poll(ATTEMPT_TIMEOUT_MS),
        Step::Send { server: S2, id: 77 }
    );
    assert_eq!(r.attempts(), 2);
    let m = answer_for(&r, [5, 6, 7, 8]);
    assert!(r.on_response(S2, &m).is_some());
    assert_eq!(r.answered_by(), Some(1));
}

#[test]
fn a_late_answer_from_the_slow_first_server_still_counts() {
    let mut r = Resolve::new("example.com", servers(), 0, 5, 0);
    let _ = r.poll(0);
    let _ = r.poll(ATTEMPT_TIMEOUT_MS); // now asking S2
    let m = answer_for(&r, [4, 4, 4, 4]);
    assert!(r.on_response(S1, &m).is_some());
    assert_eq!(r.answered_by(), Some(0));
}

#[test]
fn rotation_cycles_servers_for_rounds_then_fails() {
    let mut r = Resolve::new("example.com", servers(), 0, 1, 0);
    let mut t = 0;
    let mut sent = Vec::new();
    loop {
        match r.poll(t) {
            Step::Send { server, .. } => sent.push(server),
            Step::Wait(until) => t = until,
            Step::Done(o) => {
                assert_eq!(o, Outcome::Failed);
                break;
            }
        }
    }
    assert_eq!(sent, vec![S1, S2, S1, S2]);
    assert!(t <= TOTAL_TIMEOUT_MS);
}

#[test]
fn total_timeout_bounds_a_long_server_list() {
    let many = DnsServers::from_slice(&[S1, S2, Ipv4([1, 1, 1, 1])]);
    let mut r = Resolve::new("example.com", many, 0, 1, 0);
    let mut t = 0;
    loop {
        match r.poll(t) {
            Step::Wait(u) => t = u,
            Step::Send { .. } => {}
            Step::Done(o) => {
                assert_eq!(o, Outcome::Failed);
                break;
            }
        }
    }
    // Three servers twice would take 9 s; the hard bound cuts it at 8 s.
    assert_eq!(t, TOTAL_TIMEOUT_MS);
}

#[test]
fn servfail_moves_to_the_next_server_immediately() {
    let mut r = Resolve::new("example.com", servers(), 0, 3, 0);
    let _ = r.poll(0);
    let sf = response(3, "example.com", 2, &[]);
    assert_eq!(r.on_response(S1, &sf), None);
    // No waiting for the timeout.
    assert_eq!(r.poll(5), Step::Send { server: S2, id: 3 });
    let m = answer_for(&r, [9, 9, 9, 9]);
    assert!(r.on_response(S2, &m).is_some());
}

#[test]
fn nxdomain_is_final_and_not_retried() {
    let mut r = Resolve::new("no.example.com", servers(), 0, 3, 0);
    let _ = r.poll(0);
    let nx = response(3, "no.example.com", 3, &[]);
    assert_eq!(r.on_response(S1, &nx), Some(Outcome::NxDomain));
    assert_eq!(r.poll(1), Step::Done(Outcome::NxDomain));
}

#[test]
fn spoofed_sources_and_ids_are_ignored() {
    let mut r = Resolve::new("example.com", servers(), 0, 3, 0);
    let _ = r.poll(0);
    let m = answer_for(&r, [6, 6, 6, 6]);
    assert_eq!(r.on_response(Ipv4([6, 6, 6, 6]), &m), None, "not a server");
    let bad = response(4, "example.com", 0, &[a_rr(5, [6, 6, 6, 6])]);
    assert_eq!(r.on_response(S1, &bad), None, "wrong id");
    assert_eq!(r.on_response(S1, &[1, 2, 3]), None, "garbage");
    assert!(matches!(r.poll(1), Step::Wait(_)));
}

#[test]
fn no_servers_and_bad_names_end_immediately() {
    let mut r = Resolve::new("example.com", DnsServers::NONE, 0, 1, 0);
    assert_eq!(r.poll(0), Step::Done(Outcome::NoServers));
    let mut r = Resolve::new("bad name", servers(), 0, 1, 0);
    assert_eq!(r.poll(0), Step::Done(Outcome::Failed));
}

#[test]
fn start_index_rotates_the_first_server() {
    let mut r = Resolve::new("example.com", servers(), 1, 1, 0);
    assert_eq!(r.poll(0), Step::Send { server: S2, id: 1 });
    // Start values past the list wrap.
    let mut r = Resolve::new("example.com", servers(), 5, 1, 0);
    assert_eq!(r.poll(0), Step::Send { server: S2, id: 1 });
}

// ---- resolver ----

#[test]
fn resolver_caches_and_remembers_the_working_server() {
    let mut rv = Resolver::new(servers());
    assert_eq!(rv.lookup("example.com", 0), None);
    let mut q = rv.begin("example.com", 9, 0);
    assert_eq!(q.poll(0), Step::Send { server: S1, id: 9 });
    let _ = q.poll(ATTEMPT_TIMEOUT_MS); // S1 is dead
    let m = answer_for(&q, [7, 7, 7, 7]);
    let out = q.on_response(S2, &m).unwrap();
    rv.finish(&q, out, 2_000);
    assert_eq!(rv.preferred(), 1);
    assert_eq!(
        rv.lookup("example.com", 2_000),
        Some(Cached::Addr(Ipv4([7, 7, 7, 7])))
    );
    // TTL 120 s.
    assert!(rv.lookup("example.com", 2_000 + 119_999).is_some());
    assert!(rv.lookup("example.com", 2_000 + 120_000).is_none());
    // The next lookup starts at the server that worked.
    let mut q2 = rv.begin("other.com", 10, 3_000);
    assert_eq!(q2.poll(3_000), Step::Send { server: S2, id: 10 });
}

#[test]
fn total_failure_advances_the_preferred_server() {
    let mut rv = Resolver::new(servers());
    let mut q = rv.begin("x.com", 1, 0);
    let mut t = 0;
    let out = loop {
        match q.poll(t) {
            Step::Wait(u) => t = u,
            Step::Done(o) => break o,
            Step::Send { .. } => {}
        }
    };
    rv.finish(&q, out, t);
    assert_eq!(rv.preferred(), 1);
    assert!(rv.cache().is_empty(), "failures are not cached");
}

#[test]
fn nxdomain_is_negatively_cached() {
    let mut rv = Resolver::new(servers());
    let mut q = rv.begin("no.com", 1, 0);
    let _ = q.poll(0);
    let nx = response(1, "no.com", 3, &[]);
    let out = q.on_response(S1, &nx).unwrap();
    rv.finish(&q, out, 0);
    assert_eq!(rv.lookup("no.com", 1), Some(Cached::NxDomain));
}

#[test]
fn changing_servers_flushes_the_cache() {
    let mut rv = Resolver::new(servers());
    rv.cache.insert_addr("a.com", Ipv4([1, 1, 1, 1]), 100, 0);
    rv.set_servers(servers()); // unchanged: kept
    assert!(rv.lookup("a.com", 1).is_some());
    rv.set_servers(DnsServers::one(Ipv4([9, 9, 9, 9])));
    assert!(rv.lookup("a.com", 1).is_none());
    assert_eq!(rv.preferred(), 0);
}
