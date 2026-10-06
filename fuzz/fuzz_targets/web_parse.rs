//! Fuzz target: everything the browser does with attacker-controlled bytes
//! (a remote server's HTTP response): HTTP header/status/body handling
//! including chunked decoding, URL parsing, entity decoding, CSS parsing, colour
//! parsing and the full HTML -> DOM -> layout pipeline.
//!
//! Input layout: `[viewport_lo, viewport_hi, mode, ...bytes]`.
//!
//! * `mode & 1`: also render the bytes embedded in `<style>` / `style='..'`, so
//!   the CSS value handlers (margin/padding/font-size/colour) are reached.
//! * `mode & 2`: wrap the bytes in a `Transfer-Encoding: chunked` response.
//! * `mode >> 2`: repeat the bytes that many times * 8 (up to ~500x), so the
//!   fuzzer can build deep nesting (`<div><div>...`) from a short input.
//!
//! The renderer runs on a thread with a small stack (512 KiB, i.e. far *more*
//! than the kernel's 80 KiB, but ASAN-instrumented frames are several times
//! bigger than release frames) so unbounded recursion shows up as a
//! stack-overflow crash like it would on the kernel stack.
#![no_main]

use libfuzzer_sys::fuzz_target;
use osjeff_core::browser;
use osjeff_core::web;

const STACK: usize = 512 * 1024;

fn render_all(viewport: i32, body: Vec<u8>, mode: u8) {
    let page = browser::page_body(&body); // includes chunked decoding
    let _ = web::render(&body, viewport);
    let _ = web::render(&page, viewport);
    if mode & 1 != 0 {
        let mut html = Vec::from(&b"<style>p{"[..]);
        html.extend_from_slice(&body);
        html.extend_from_slice(b"}</style><p>x<div style='");
        html.extend_from_slice(&body);
        html.extend_from_slice(b"'>y</div></p>");
        let _ = web::render(&html, viewport);
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }
    let viewport = u16::from_le_bytes([data[0], data[1]]) as i32;
    let mode = data[2];
    let raw = &data[3..];

    // ---- HTTP framing / URL / entity helpers on the raw bytes ----
    let _ = browser::status_code(raw);
    let _ = browser::header_value(raw, b"location");
    let _ = browser::header_value(raw, b"transfer-encoding");
    let _ = browser::http_body(raw);
    if let Some(u) = browser::parse_url(raw) {
        let _ = (u.host(), u.path(), u.port, u.https);
    }
    let _ = browser::looks_like_url(raw);
    let mut q = [0u8; 64];
    let _ = browser::encode_query(raw, &mut q);
    let _ = browser::build_search_url(raw, &mut q);
    let _ = browser::decode_entity(raw);
    if !raw.is_empty() {
        let _ = browser::decode_utf8(raw);
    }
    if let Ok(s) = core::str::from_utf8(raw) {
        let _ = web::parse_color(s);
        let _ = web::parse_css(s);
    }

    // ---- build the (possibly repeated / chunked) body ----
    let reps = ((mode >> 2) as usize) * 8;
    let mut body: Vec<u8> = Vec::new();
    if mode & 2 != 0 {
        body.extend_from_slice(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    }
    if reps == 0 {
        body.extend_from_slice(raw);
    } else {
        // Cap the expanded size (the browser caps pages at 256 KiB).
        for _ in 0..reps {
            if body.len() + raw.len() > 256 * 1024 {
                break;
            }
            body.extend_from_slice(raw);
        }
    }

    // ---- HTML + layout on a small stack ----
    let h = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || render_all(viewport, body, mode))
        .unwrap();
    if let Err(e) = h.join() {
        // Re-raise so libFuzzer records the panic as a crash.
        std::panic::resume_unwind(e);
    }
});
