//! DNS client logic: query/response messages (RFC 1035, A records), a TTL cache
//! and a resolver session that rotates through several servers with timeouts.
//!
//! Like the rest of the network code this is pure: the kernel owns the UDP
//! socket and the clock (milliseconds, injected), this module builds and parses
//! the bytes and decides what to send next and when to give up.
//!
//! * [`build_query`] / [`parse_response`]: one `A IN` question, recursion
//!   desired. Responses are only accepted when the transaction id, the question
//!   and (in [`Resolve`]) the server address match, and answer records only count
//!   if their owner is the name asked for or a CNAME target reached from it, so
//!   an off-path spoofer needs the id and a plausible message, and unrelated
//!   records are never trusted. Compression pointers must point strictly
//!   backwards (no loops).
//! * [`DnsCache`]: positive entries live for the record TTL (clamped to
//!   [`MAX_TTL_SECS`]; a TTL of 0 is not cached), NXDOMAIN for
//!   [`NEGATIVE_TTL_SECS`]; bounded capacity, the entry closest to expiry is the
//!   one evicted.
//! * [`Resolve`]: one lookup. Attempt `k` goes to server `(start + k) mod n`; an
//!   attempt waits [`ATTEMPT_TIMEOUT_MS`], a SERVFAIL/REFUSED/garbled answer moves
//!   to the next server at once, NXDOMAIN and NODATA are final. Each server is
//!   tried up to [`ROUNDS`] times within [`TOTAL_TIMEOUT_MS`].
//! * [`Resolver`] ties them together and remembers which server answered last so
//!   the next lookup starts there (a dead first server costs one timeout, not one
//!   per lookup).

use crate::network::net::{DnsServers, Ipv4};
use alloc::string::String;
use alloc::vec::Vec;

pub const DNS_PORT: u16 = 53;
const HDR: usize = 12;
const TYPE_A: u16 = 1;
const TYPE_CNAME: u16 = 5;
const CLASS_IN: u16 = 1;
/// Longest name on the wire in presentation form (RFC 1035: 255 incl. dots and
/// the root; 253 visible characters).
pub const MAX_NAME: usize = 253;
const MAX_LABEL: usize = 63;
/// Compression pointers followed per name before giving up.
const MAX_JUMPS: usize = 16;
/// CNAME hops accepted in one answer.
const MAX_CNAMES: usize = 8;
/// A records kept from one answer.
pub const MAX_ADDRS: usize = 4;

/// Longest cache lifetime honored for a positive answer.
pub const MAX_TTL_SECS: u32 = 3600;
/// Lifetime of a cached NXDOMAIN.
pub const NEGATIVE_TTL_SECS: u32 = 30;
/// Cache capacity (entries).
pub const CACHE_CAP: usize = 32;

/// How long one attempt (one server) is waited for.
pub const ATTEMPT_TIMEOUT_MS: u64 = 1500;
/// Times each server is tried before the lookup fails.
pub const ROUNDS: usize = 2;
/// Hard bound for a whole lookup.
pub const TOTAL_TIMEOUT_MS: u64 = 8000;

/// Normalize a host name for the wire and the cache: one trailing dot dropped,
/// ASCII letters lowercased. `None` if it is not a valid host name: empty
/// labels, labels over 63 bytes, more than 253 bytes, or characters other than
/// letters, digits, `-` and `_` (internationalized names must arrive already as
/// punycode).
pub fn normalize_name(name: &str) -> Option<String> {
    let n = name.strip_suffix('.').unwrap_or(name);
    if n.is_empty() || n.len() > MAX_NAME {
        return None;
    }
    for label in n.split('.') {
        if label.is_empty() || label.len() > MAX_LABEL {
            return None;
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return None;
        }
    }
    Some(n.to_ascii_lowercase())
}

/// Build an `A IN` query for `name` with transaction id `id` into `out`.
/// Returns the message length, or `None` for an invalid name or a buffer that is
/// too small (`HDR + name + 6` bytes are needed).
pub fn build_query(out: &mut [u8], id: u16, name: &str) -> Option<usize> {
    let name = normalize_name(name)?;
    let need = HDR + name.len() + 2 + 4;
    if out.len() < need {
        return None;
    }
    out[0..2].copy_from_slice(&id.to_be_bytes());
    out[2..4].copy_from_slice(&0x0100u16.to_be_bytes()); // RD
    out[4..6].copy_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out[6..HDR].fill(0);
    let mut o = HDR;
    for label in name.split('.') {
        out[o] = label.len() as u8;
        out[o + 1..o + 1 + label.len()].copy_from_slice(label.as_bytes());
        o += 1 + label.len();
    }
    out[o] = 0;
    o += 1;
    out[o..o + 2].copy_from_slice(&TYPE_A.to_be_bytes());
    out[o + 2..o + 4].copy_from_slice(&CLASS_IN.to_be_bytes());
    Some(o + 4)
}

/// Addresses of an answer (at most [`MAX_ADDRS`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Addrs {
    list: [Ipv4; MAX_ADDRS],
    len: u8,
}

impl Addrs {
    pub fn as_slice(&self) -> &[Ipv4] {
        &self.list[..self.len as usize]
    }
    pub fn first(&self) -> Option<Ipv4> {
        self.as_slice().first().copied()
    }
}

/// What a well-formed, matching response says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reply {
    /// One or more A records; `ttl` is the smallest TTL of the records used.
    Addrs { addrs: Addrs, ttl: u32 },
    /// The name does not exist (rcode 3): authoritative, do not ask others.
    NxDomain,
    /// The name exists but has no A record (NOERROR, empty answer).
    NoData,
    /// The server could not answer (SERVFAIL, REFUSED, truncated ... ); another
    /// server may.
    ServerFail(u8),
}

/// Decode the name at `off`, following compression pointers. Returns the
/// lowercase dotted name and the offset just past the name *in the original
/// position* (a pointer ends it after two bytes).
fn read_name(msg: &[u8], off: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut pos = off;
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            end.get_or_insert(pos + 1);
            break;
        }
        if len & 0xC0 == 0xC0 {
            let lo = *msg.get(pos + 1)? as usize;
            let target = (len & 0x3F) << 8 | lo;
            end.get_or_insert(pos + 2);
            // Strictly backwards: a pointer to itself or forward could loop.
            if target >= pos {
                return None;
            }
            jumps += 1;
            if jumps > MAX_JUMPS {
                return None;
            }
            pos = target;
            continue;
        }
        if len & 0xC0 != 0 {
            return None; // reserved label types
        }
        let label = msg.get(pos + 1..pos + 1 + len)?;
        if !name.is_empty() {
            name.push('.');
        }
        for &b in label {
            name.push(b.to_ascii_lowercase() as char);
        }
        if name.len() > MAX_NAME {
            return None;
        }
        pos += 1 + len;
    }
    Some((name, end?))
}

fn be16(m: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*m.get(o)?, *m.get(o + 1)?]))
}

/// Parse a response to the query `(id, name)`. `None` means "not an answer to
/// this query" (wrong id, a query instead of a response, different question,
/// malformed): the caller keeps waiting. Everything else is a [`Reply`].
pub fn parse_response(msg: &[u8], id: u16, name: &str) -> Option<Reply> {
    let qname = normalize_name(name)?;
    if msg.len() < HDR || be16(msg, 0)? != id {
        return None;
    }
    let flags = be16(msg, 2)?;
    if flags & 0x8000 == 0 {
        return None; // a query, not a response
    }
    let qd = be16(msg, 4)? as usize;
    let an = be16(msg, 6)? as usize;
    if qd != 1 {
        return None;
    }
    // The question must be the one we asked.
    let (qn, mut off) = read_name(msg, HDR)?;
    if qn != qname || be16(msg, off)? != TYPE_A || be16(msg, off + 2)? != CLASS_IN {
        return None;
    }
    off += 4;

    let rcode = (flags & 0x000F) as u8;
    if rcode == 3 {
        return Some(Reply::NxDomain);
    }

    // Answer section: A records owned by the name or by a CNAME target of it.
    let mut owners: Vec<String> = alloc::vec![qname];
    let mut cnames = 0;
    let mut addrs = Addrs {
        list: [Ipv4([0; 4]); MAX_ADDRS],
        len: 0,
    };
    let mut ttl = u32::MAX;
    for _ in 0..an.min(64) {
        let Some((owner, o)) = read_name(msg, off) else {
            break;
        };
        let (Some(rtype), Some(class), Some(rttl), Some(rdlen)) = (
            be16(msg, o),
            be16(msg, o + 2),
            be16(msg, o + 4).zip(be16(msg, o + 6)),
            be16(msg, o + 8),
        ) else {
            break;
        };
        let rttl = (u32::from(rttl.0) << 16) | u32::from(rttl.1);
        let rd = o + 10;
        let rdlen = rdlen as usize;
        let Some(data) = msg.get(rd..rd + rdlen) else {
            break;
        };
        off = rd + rdlen;
        if class != CLASS_IN || !owners.contains(&owner) {
            continue;
        }
        match rtype {
            TYPE_A if rdlen == 4 => {
                if (addrs.len as usize) < MAX_ADDRS {
                    addrs.list[addrs.len as usize] = Ipv4([data[0], data[1], data[2], data[3]]);
                    addrs.len += 1;
                }
                ttl = ttl.min(rttl);
            }
            TYPE_CNAME if cnames < MAX_CNAMES => {
                if let Some((target, _)) = read_name(msg, rd) {
                    cnames += 1;
                    ttl = ttl.min(rttl);
                    owners.push(target);
                }
            }
            _ => {}
        }
    }
    if addrs.len > 0 {
        return Some(Reply::Addrs { addrs, ttl });
    }
    if rcode != 0 {
        return Some(Reply::ServerFail(rcode));
    }
    if flags & 0x0200 != 0 {
        return Some(Reply::ServerFail(0)); // truncated, and no TCP fallback here
    }
    Some(Reply::NoData)
}

// ---- cache ----

/// A cache hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cached {
    Addr(Ipv4),
    NxDomain,
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    what: Cached,
    expires_ms: u64,
}

/// TTL cache of lookups, keyed by normalized name.
#[derive(Clone, Debug, Default)]
pub struct DnsCache {
    entries: Vec<Entry>,
}

impl DnsCache {
    pub fn new() -> DnsCache {
        DnsCache::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drop expired entries.
    pub fn purge(&mut self, now_ms: u64) {
        self.entries.retain(|e| e.expires_ms > now_ms);
    }

    /// The live entry for `name`, if any. An entry is live strictly before its
    /// expiry time.
    pub fn get(&self, name: &str, now_ms: u64) -> Option<Cached> {
        let n = normalize_name(name)?;
        self.entries
            .iter()
            .find(|e| e.name == n && e.expires_ms > now_ms)
            .map(|e| e.what)
    }

    /// Remaining life of the entry for `name` in milliseconds.
    pub fn remaining_ms(&self, name: &str, now_ms: u64) -> Option<u64> {
        let n = normalize_name(name)?;
        self.entries
            .iter()
            .find(|e| e.name == n && e.expires_ms > now_ms)
            .map(|e| e.expires_ms - now_ms)
    }

    /// Cache `addr` for `name` for `ttl_secs` (clamped to [`MAX_TTL_SECS`]; 0 is
    /// not cached, and removes any older entry).
    pub fn insert_addr(&mut self, name: &str, addr: Ipv4, ttl_secs: u32, now_ms: u64) {
        self.insert(name, Cached::Addr(addr), ttl_secs.min(MAX_TTL_SECS), now_ms);
    }

    /// Cache the non-existence of `name` for [`NEGATIVE_TTL_SECS`].
    pub fn insert_nxdomain(&mut self, name: &str, now_ms: u64) {
        self.insert(name, Cached::NxDomain, NEGATIVE_TTL_SECS, now_ms);
    }

    fn insert(&mut self, name: &str, what: Cached, ttl_secs: u32, now_ms: u64) {
        let Some(n) = normalize_name(name) else {
            return;
        };
        self.entries.retain(|e| e.name != n); // replace
        if ttl_secs == 0 {
            return;
        }
        self.purge(now_ms);
        if self.entries.len() >= CACHE_CAP {
            // Evict the entry closest to expiry.
            if let Some(i) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.expires_ms)
                .map(|(i, _)| i)
            {
                self.entries.swap_remove(i);
            }
        }
        self.entries.push(Entry {
            name: n,
            what,
            expires_ms: now_ms.saturating_add(u64::from(ttl_secs) * 1000),
        });
    }
}

// ---- one lookup across several servers ----

/// Final result of a [`Resolve`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Resolved {
        addr: Ipv4,
        ttl: u32,
    },
    NxDomain,
    NoData,
    /// Every attempt failed or timed out.
    Failed,
    /// No DNS server is configured.
    NoServers,
}

/// What the driver should do next for a [`Resolve`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Send the query (built with [`build_query`] and `id`) to `server`.
    Send {
        server: Ipv4,
        id: u16,
    },
    /// Nothing to do before this time (ms).
    Wait(u64),
    Done(Outcome),
}

/// One lookup in progress.
#[derive(Clone, Debug)]
pub struct Resolve {
    name: String,
    servers: DnsServers,
    id: u16,
    start: usize,
    /// Attempts started so far.
    attempts: usize,
    /// When the current attempt is abandoned.
    attempt_deadline: u64,
    /// Whether the first attempt has been issued.
    started: bool,
    total_deadline: u64,
    done: Option<Outcome>,
    /// Index (into `servers`) that produced the final answer.
    answered_by: Option<usize>,
    /// A failure notice from the server being waited on forces the next attempt.
    advance: bool,
}

impl Resolve {
    /// Start resolving `name` against `servers`, first asking
    /// `servers[start % len]`. `id` is the DNS transaction id (any value, but
    /// unpredictable in the kernel: it is the only defense against an off-path
    /// forger).
    pub fn new(name: &str, servers: DnsServers, start: usize, id: u16, now_ms: u64) -> Resolve {
        let normalized = normalize_name(name);
        let mut r = Resolve {
            name: normalized.clone().unwrap_or_default(),
            servers,
            id,
            start,
            attempts: 0,
            attempt_deadline: 0,
            started: false,
            total_deadline: now_ms + TOTAL_TIMEOUT_MS,
            done: None,
            answered_by: None,
            advance: false,
        };
        if normalized.is_none() {
            r.done = Some(Outcome::Failed);
        } else if servers.is_empty() {
            r.done = Some(Outcome::NoServers);
        }
        r
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> u16 {
        self.id
    }

    /// Attempts started so far (a value above 1 means a failover happened).
    pub fn attempts(&self) -> usize {
        self.attempts
    }

    fn max_attempts(&self) -> usize {
        self.servers.len() * ROUNDS
    }

    /// The server the current (or last) attempt goes to.
    fn server_index(&self, attempt: usize) -> usize {
        (self.start + attempt) % self.servers.len().max(1)
    }

    /// Index of the server that supplied the final answer (for the caller to
    /// remember as the preferred one).
    pub fn answered_by(&self) -> Option<usize> {
        self.answered_by
    }

    /// Time-driven step.
    pub fn poll(&mut self, now_ms: u64) -> Step {
        if let Some(o) = self.done {
            return Step::Done(o);
        }
        if now_ms >= self.total_deadline {
            self.done = Some(Outcome::Failed);
            return Step::Done(Outcome::Failed);
        }
        if !self.started || self.advance || now_ms >= self.attempt_deadline {
            if self.attempts >= self.max_attempts() {
                self.done = Some(Outcome::Failed);
                return Step::Done(Outcome::Failed);
            }
            let idx = self.server_index(self.attempts);
            self.started = true;
            self.advance = false;
            self.attempts += 1;
            self.attempt_deadline = (now_ms + ATTEMPT_TIMEOUT_MS).min(self.total_deadline);
            return Step::Send {
                server: self.servers.as_slice()[idx],
                id: self.id,
            };
        }
        Step::Wait(self.attempt_deadline)
    }

    /// Feed a UDP payload received from `from`. Returns the final outcome if
    /// this ends the lookup. Packets from addresses that are not configured
    /// servers, with the wrong id or question, or malformed, are ignored.
    pub fn on_response(&mut self, from: Ipv4, msg: &[u8]) -> Option<Outcome> {
        if self.done.is_some() || !self.started {
            return None;
        }
        let from_idx = self.servers.as_slice().iter().position(|&s| s == from)?;
        let reply = parse_response(msg, self.id, &self.name)?;
        let out = match reply {
            Reply::Addrs { addrs, ttl } => Outcome::Resolved {
                addr: addrs.first()?,
                ttl,
            },
            Reply::NxDomain => Outcome::NxDomain,
            Reply::NoData => Outcome::NoData,
            Reply::ServerFail(_) => {
                // Another server may do better: move on right away.
                self.advance = true;
                return None;
            }
        };
        self.done = Some(out);
        self.answered_by = Some(from_idx);
        Some(out)
    }
}

/// Servers + cache + the rotation memory. The kernel keeps one of these.
#[derive(Clone, Debug)]
pub struct Resolver {
    servers: DnsServers,
    cache: DnsCache,
    /// Server to try first on the next lookup.
    preferred: usize,
}

impl Resolver {
    pub fn new(servers: DnsServers) -> Resolver {
        Resolver {
            servers,
            cache: DnsCache::new(),
            preferred: 0,
        }
    }

    pub fn servers(&self) -> &DnsServers {
        &self.servers
    }

    /// Replace the server list (a lease change): the cache is dropped too, since
    /// the new servers may answer differently (split-horizon, a new network).
    pub fn set_servers(&mut self, servers: DnsServers) {
        if servers != self.servers {
            self.cache.clear();
        }
        self.servers = servers;
        self.preferred = 0;
    }

    pub fn cache(&self) -> &DnsCache {
        &self.cache
    }

    pub fn preferred(&self) -> usize {
        self.preferred
    }

    /// Cached answer for `name`, if any.
    pub fn lookup(&self, name: &str, now_ms: u64) -> Option<Cached> {
        self.cache.get(name, now_ms)
    }

    /// Begin a network lookup for `name`.
    pub fn begin(&self, name: &str, id: u16, now_ms: u64) -> Resolve {
        Resolve::new(name, self.servers, self.preferred, id, now_ms)
    }

    /// Record a finished lookup: cache it and remember which server answered
    /// (or, after a total failure, start with the next one next time).
    pub fn finish(&mut self, r: &Resolve, outcome: Outcome, now_ms: u64) {
        match outcome {
            Outcome::Resolved { addr, ttl } => {
                self.cache.insert_addr(r.name(), addr, ttl, now_ms);
            }
            Outcome::NxDomain => self.cache.insert_nxdomain(r.name(), now_ms),
            Outcome::NoData | Outcome::Failed | Outcome::NoServers => {}
        }
        let n = self.servers.len();
        if n > 0 {
            self.preferred = match (outcome, r.answered_by()) {
                (_, Some(i)) => i,
                (Outcome::Failed, None) => (self.preferred + 1) % n,
                _ => self.preferred,
            };
        }
    }
}

#[cfg(test)]
mod tests;
