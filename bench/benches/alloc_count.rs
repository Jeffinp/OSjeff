//! How many heap allocations do the `alloc`-using parts of `kitsune_core` make?
//! (The kernel's free-list allocator is O(free holes) per operation, so the
//! *count* matters as much as the bytes.) Run: `cargo bench --bench alloc_count`.
use kitsune_core::{browser, web};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

struct Counting;
static N: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        N.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        let live = LIVE.fetch_add(l.size() as u64, Relaxed) + l.size() as u64;
        PEAK.fetch_max(live, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as u64, Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn measure<R>(name: &str, input_len: usize, f: impl FnOnce() -> R) {
    N.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(LIVE.load(Relaxed), Relaxed);
    let base = LIVE.load(Relaxed);
    let r = f();
    let (n, b, peak) = (
        N.load(Relaxed),
        BYTES.load(Relaxed),
        PEAK.load(Relaxed) - base,
    );
    let held = LIVE.load(Relaxed) - base;
    drop(r);
    println!(
        "{name:<28} input {input_len:>7} B | {n:>7} allocs ({:.2}/input byte) | {b:>9} B total | peak {peak:>8} B | kept {held:>8} B",
        n as f64 / input_len as f64
    );
}

fn page(paragraphs: usize) -> Vec<u8> {
    let mut s = String::from(
        "<html><head><style>body{background:#fff;color:#222} h1{color:#0d47a1} \
         .card{background:#eef;padding:8px} a{color:#15c}</style></head><body><h1>Title</h1>",
    );
    for i in 0..paragraphs {
        s.push_str(&format!(
            "<div class=\"card\"><h2>Section {i}</h2><p>Lorem ipsum dolor sit amet, \
             <b>consectetur</b> adipiscing elit &amp; sed do <a href=\"/x{i}\">eiusmod</a> \
             tempor incididunt ut labore et dolore magna aliqua.</p><ul><li>one</li>\
             <li>two</li><li>three</li></ul></div>"
        ));
    }
    s.push_str("</body></html>");
    s.into_bytes()
}

fn main() {
    for n in [4usize, 14, 40, 200] {
        let html = page(n);
        let l = html.len();
        measure(&format!("web::render({n} cards)"), l, || {
            web::render(&html, 880)
        });
        measure(&format!("web::parse_html({n})"), l, || {
            web::parse_html(&html)
        });
    }
    let mut resp = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n".to_vec();
    resp.extend_from_slice(&page(40));
    let l = resp.len();
    measure("browser::page_body(40)", l, || browser::page_body(&resp));
}
