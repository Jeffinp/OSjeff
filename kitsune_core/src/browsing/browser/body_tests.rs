//! Response bodies that are cut, chunked, stacked or damaged: the page still renders what
//! arrived, with a note saying why it is partial (W20: a CDN home page overflowed the response
//! cap in the middle of its gzip stream and showed only "Falha ao descompactar").

use super::*;
use crate::format::deflate::{deflate_fixed, deflate_stored, zlib_compress};
use crate::format::gzip::MAX_DECODED_BYTES;
use crate::format::inflate::crc32;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// An HTML page of about `kib` KiB with varied text (so it neither collapses to nothing nor
/// stays tiny when compressed).
fn page(kib: usize) -> Vec<u8> {
    let mut s = String::from("<html><body>");
    let mut i = 0u32;
    while s.len() < kib * 1024 {
        let h = i.wrapping_mul(2_654_435_761);
        s.push_str(&format!(
            "<p id=\"p{i}\">linha {i}: {h:08x} lorem ipsum {} dolor sit amet</p>\n",
            h.rotate_left(7)
        ));
        i += 1;
    }
    s.push_str("</body></html>");
    s.into_bytes()
}

fn gzip_of(raw_deflate: &[u8], plain: &[u8]) -> Vec<u8> {
    let mut v = alloc::vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
    v.extend_from_slice(raw_deflate);
    v.extend_from_slice(&crc32(plain).to_le_bytes());
    v.extend_from_slice(&(plain.len() as u32).to_le_bytes());
    v
}

fn response(headers: &str, body: &[u8]) -> Vec<u8> {
    let mut r = format!("HTTP/1.1 200 OK\r\n{headers}\r\n").into_bytes();
    r.extend_from_slice(body);
    r
}

fn chunked(body: &[u8], size: usize, last: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for c in body.chunks(size) {
        out.extend_from_slice(format!("{:x}\r\n", c.len()).as_bytes());
        out.extend_from_slice(c);
        out.extend_from_slice(b"\r\n");
    }
    if last {
        out.extend_from_slice(b"0\r\n\r\n");
    }
    out
}

#[test]
fn the_cap_is_big_enough_for_a_real_gzip_home_page() {
    // ~300 KiB on the wire used to be refused at 256 KiB.
    const { assert!(MAX_RESPONSE_BYTES >= 1024 * 1024) };
    const { assert!(MAX_DECODED_BYTES >= 4 * MAX_RESPONSE_BYTES) };
}

#[test]
fn gzip_cut_by_the_size_cap_renders_the_decoded_prefix() {
    let html = page(400);
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let full = response("Content-Encoding: gzip\r\n", &gz);
    // the fetch layer stopped reading part way through the compressed body
    let cut_at = full.len() * 2 / 3;
    let resp = &full[..cut_at];

    let p = page_body_partial(resp, true);
    assert_eq!(p.note, Some(PageNote::Truncated));
    assert!(p.body.len() > 10_000, "decoded {} bytes", p.body.len());
    assert!(
        html.starts_with(&p.body),
        "the prefix must be real page bytes"
    );
    assert!(!String::from_utf8_lossy(&p.body).contains("Falha ao descompactar"));
    // the plain entry point shows it too, no error page
    assert!(page_body(resp).starts_with(b"<html><body><p id=\"p0\">"));
}

#[test]
fn gzip_cut_without_the_flag_is_incomplete_not_an_error() {
    let html = page(100);
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let resp = response("Content-Encoding: gzip\r\n", &gz[..gz.len() / 2]);
    let p = page_body_partial(&resp, false);
    assert_eq!(p.note, Some(PageNote::Incomplete));
    assert!(html.starts_with(&p.body) && p.body.len() > 1000);
}

#[test]
fn every_cut_of_a_gzip_response_yields_a_prefix_or_a_clean_error() {
    let html = page(24);
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let full = response("Content-Encoding: gzip\r\n", &gz);
    let head = full.len() - gz.len();
    let mut last = 0;
    for cut in (head..=full.len()).step_by(37).chain([full.len()]) {
        let p = page_body_partial(&full[..cut], true);
        if cut == full.len() {
            assert_eq!(p.note, Some(PageNote::Truncated)); // flagged cut, yet complete: still a notice
            assert_eq!(p.body, html);
        } else if p.note.is_some() && html.starts_with(&p.body) {
            assert!(p.body.len() >= last, "prefix shrank at cut {cut}");
            last = p.body.len();
        } else {
            // nothing decodable yet (inside the gzip header): the explanatory page
            assert!(p.body.starts_with(b"<p>"), "cut {cut}");
        }
    }
    assert!(last > html.len() / 2);
}

#[test]
fn complete_gzip_has_no_note() {
    let html = page(50);
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let resp = response("Content-Encoding: gzip\r\n", &gz);
    let p = page_body_partial(&resp, false);
    assert_eq!(p.note, None);
    assert_eq!(p.body, html);
}

#[test]
fn chunked_gzip_cut_mid_chunk_renders_the_prefix() {
    let html = page(120);
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let body = chunked(&gz, 4000, true);
    let resp = response(
        "Transfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n",
        &body[..body.len() / 2],
    );
    let p = page_body_partial(&resp, true);
    assert_eq!(p.note, Some(PageNote::Truncated));
    assert!(p.body.len() > 5000 && html.starts_with(&p.body));
    // the whole thing, properly terminated: complete
    let resp = response(
        "Transfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n",
        &body,
    );
    let p = page_body_partial(&resp, false);
    assert_eq!((p.note, p.body), (None, html));
}

#[test]
fn chunked_without_its_last_chunk_is_incomplete() {
    let resp = response(
        "Transfer-Encoding: chunked\r\n",
        &chunked(b"<p>hello world</p>", 5, false),
    );
    let p = page_body_partial(&resp, false);
    assert_eq!(p.body, b"<p>hello world</p>");
    assert_eq!(p.note, Some(PageNote::Incomplete));
    let resp = response(
        "Transfer-Encoding: chunked\r\n",
        &chunked(b"<p>hello world</p>", 5, true),
    );
    assert_eq!(page_body_partial(&resp, false).note, None);
}

#[test]
fn transfer_encoding_list_and_casing() {
    let html = b"<p>listed</p>";
    let body = chunked(html, 4, true);
    for te in ["gzip, chunked", "Chunked", "  CHUNKED  "] {
        let resp = response(&format!("transfer-encoding: {te}\r\n"), &body);
        assert_eq!(page_body(&resp), html, "{te}");
    }
}

#[test]
fn short_content_length_is_incomplete() {
    let resp = response("Content-Length: 100\r\n", b"<p>only a little</p>");
    let p = page_body_partial(&resp, false);
    assert_eq!(p.note, Some(PageNote::Incomplete));
    assert_eq!(p.body, b"<p>only a little</p>");
    let resp = response("Content-Length: 19\r\n", b"<p>only a little</p>");
    assert_eq!(page_body_partial(&resp, false).note, None);
}

#[test]
fn deflate_zlib_and_raw_both_decode() {
    let html = page(30);
    let z = response("Content-Encoding: deflate\r\n", &zlib_compress(&html));
    assert_eq!(page_body_partial(&z, false).body, html);
    let r = response("Content-Encoding: Deflate\r\n", &deflate_fixed(&html));
    assert_eq!(page_body_partial(&r, false).body, html);
    let r = response("Content-Encoding: deflate\r\n", &deflate_stored(&html));
    let p = page_body_partial(&r, false);
    assert_eq!((p.note, p.body), (None, html));
}

#[test]
fn cut_zlib_deflate_is_not_retried_as_garbage_raw() {
    let html = page(60);
    let z = zlib_compress(&html);
    let resp = response("Content-Encoding: deflate\r\n", &z[..z.len() / 2]);
    let p = page_body_partial(&resp, true);
    assert_eq!(p.note, Some(PageNote::Truncated));
    assert!(p.body.len() > 1000 && html.starts_with(&p.body));
}

#[test]
fn cut_raw_deflate_renders_the_prefix() {
    let html = page(60);
    let d = deflate_fixed(&html);
    let resp = response("Content-Encoding: deflate\r\n", &d[..d.len() / 2]);
    let p = page_body_partial(&resp, false);
    assert_eq!(p.note, Some(PageNote::Incomplete));
    assert!(p.body.len() > 1000 && html.starts_with(&p.body));
}

#[test]
fn bad_gzip_trailer_still_renders_with_a_note() {
    let html = page(20);
    let mut gz = gzip_of(&deflate_fixed(&html), &html);
    let n = gz.len();
    gz[n - 8] ^= 0xFF; // CRC
    let p = page_body_partial(&response("Content-Encoding: gzip\r\n", &gz), false);
    assert_eq!(p.note, Some(PageNote::BadChecksum));
    assert_eq!(p.body, html);
    // a wrong ISIZE as well
    let mut gz = gzip_of(&deflate_fixed(&html), &html);
    let n = gz.len();
    gz[n - 1] ^= 1;
    let p = page_body_partial(&response("Content-Encoding: gzip\r\n", &gz), false);
    assert_eq!(
        (p.note, p.body),
        (Some(PageNote::BadChecksum), html.clone())
    );
    // the trailer missing altogether (cut right after the deflate stream)
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let p = page_body_partial(
        &response("Content-Encoding: gzip\r\n", &gz[..gz.len() - 8]),
        false,
    );
    assert_eq!(p.body, html);
    assert!(p.note.is_some());
}

#[test]
fn damaged_deflate_in_the_middle_keeps_the_good_part() {
    let html = page(80);
    let mut gz = gzip_of(&deflate_stored(&html), &html);
    // stored blocks: break the second block's NLEN so the stream dies after block one
    let second = 10 + 5 + 65535;
    gz[second + 3] ^= 0xFF;
    let p = page_body_partial(&response("Content-Encoding: gzip\r\n", &gz), false);
    assert_eq!(p.note, Some(PageNote::Damaged));
    assert_eq!(p.body.len(), 65535);
    assert!(html.starts_with(&p.body));
}

#[test]
fn decoded_size_limit_shows_the_first_part() {
    // far more than MAX_DECODED_BYTES once inflated, tiny on the wire
    let big = alloc::vec![b'a'; MAX_DECODED_BYTES + 100_000];
    let gz = gzip_of(&deflate_fixed(&big), &big);
    assert!(gz.len() < 100_000);
    let p = page_body_partial(&response("Content-Encoding: gzip\r\n", &gz), false);
    assert_eq!(p.note, Some(PageNote::Truncated));
    assert_eq!(p.body.len(), MAX_DECODED_BYTES);
}

#[test]
fn stacked_and_cased_content_encodings() {
    let html = page(10);
    let inner = gzip_of(&deflate_fixed(&html), &html);
    let outer = gzip_of(&deflate_fixed(&inner), &inner);
    for ce in ["gzip, gzip", "GZip,  x-gzip", " gzip , identity , gzip "] {
        let resp = response(&format!("Content-Encoding: {ce}\r\n"), &outer);
        let p = page_body_partial(&resp, false);
        assert_eq!((p.note, p.body), (None, html.clone()), "{ce}");
    }
    // `identity` alone and empty are no-ops
    let resp = response("Content-Encoding: identity\r\n", b"<p>x</p>");
    assert_eq!(page_body(&resp), b"<p>x</p>");
}

#[test]
fn unsupported_codings_are_refused_with_a_notice_never_garbage() {
    for ce in ["br", "zstd", "gzip, br", "br, gzip", "compress"] {
        let resp = response(
            &format!("Content-Encoding: {ce}\r\n"),
            b"\x01\x02\x03binary",
        );
        let body = page_body(&resp);
        assert!(
            String::from_utf8_lossy(&body).contains("não suportada"),
            "{ce}"
        );
    }
}

#[test]
fn too_many_stacked_codings_are_refused() {
    let resp = response("Content-Encoding: gzip, gzip, gzip, gzip, gzip\r\n", b"xx");
    assert!(String::from_utf8_lossy(&page_body(&resp)).contains("não suportada"));
}

#[test]
fn the_request_offers_only_what_is_decoded() {
    // Documented contract of kernel/src/netstack.rs `build_request`: `Accept-Encoding: gzip,
    // deflate` is every coding `gzip::Encoding` can decode, and nothing else.
    for ok in ["gzip", "deflate"] {
        assert_ne!(
            crate::format::gzip::Encoding::parse(ok.as_bytes()),
            crate::format::gzip::Encoding::Unsupported
        );
    }
    for no in ["br", "zstd", "compress"] {
        assert_eq!(
            crate::format::gzip::Encoding::parse(no.as_bytes()),
            crate::format::gzip::Encoding::Unsupported
        );
    }
}

#[test]
fn app_response_gets_the_partial_body_flagged_cut() {
    let html = page(60);
    let gz = gzip_of(&deflate_fixed(&html), &html);
    let resp = response("Content-Encoding: gzip\r\n", &gz[..gz.len() / 2]);
    let (body, cut) = crate::platform::appnet::app_response(&resp, 1 << 20).unwrap();
    assert!(cut && html.starts_with(&body) && body.len() > 1000);
}

#[test]
fn browser_keeps_the_note_for_the_banner() {
    let mut b = Browser::new();
    b.open(b"example.com");
    assert!(b.take_request().is_some());
    b.loaded_with_note(Conn::Verified, Some(PageNote::Truncated));
    assert!(b.truncated());
    assert_eq!(b.note(), Some(PageNote::Truncated));
    assert_eq!(
        PageNote::Truncated.label(),
        "Página cortada no limite de tamanho"
    );
    b.open(b"example.org");
    assert!(b.take_request().is_some());
    b.loaded_with(Conn::Verified, false);
    assert_eq!(b.note(), None);
    assert!(!b.truncated());
}

#[test]
fn hostile_chunk_sizes_and_headers_never_panic() {
    for body in [
        &b"ffffffffffffffffffffffff\r\nabc"[..],
        b"-5\r\nabc",
        b"\r\n\r\n",
        b"5\r\nab",
        b"0",
        b"zz\r\n",
    ] {
        let resp = response(
            "Transfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n",
            body,
        );
        let _ = page_body_partial(&resp, true);
        let _ = page_body_partial(&resp, false);
    }
    let resp = response("Content-Length: 99999999999999999999999\r\n", b"x");
    assert_eq!(page_body_partial(&resp, false).body, b"x");
}
