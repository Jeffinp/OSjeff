//! Fuzz target: the compressed / chunked HTTP body path (W20): `browser::body_partial`,
//! `page_body_partial`, the strict `body_bytes`, `app_response` and the streaming
//! `Inflater::read_partial`.
//!
//! Input layout: `[mode, cut_lo, cut_hi, ...bytes]`.
//!
//! * `mode & 0x0F` picks the framing of `bytes`: raw gzip, zlib `deflate`, raw `deflate`,
//!   `gzip, gzip`, `GZIP` + `chunked`, identity + `chunked`, hostile header lists, or the whole
//!   input taken as an HTTP response.
//! * `mode & 0x10`: the bytes are first *compressed* by our own encoder (so the stream is
//!   valid), then cut at `cut` (a fraction of the compressed length) and the decoded result must
//!   be a prefix of the original: this is the property the cut-at-the-cap fix guarantees.
//! * `mode & 0x20`: tell the decoder the fetch layer cut the response (`cut` flag).
//!
//! Invariants: never panics; the decoded body is never larger than `MAX_DECODED_BYTES`; a valid
//! stream cut anywhere decodes to a prefix of the original; a complete valid stream decodes to
//! the original with no note.
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::browser::{self, PageNote};
use kitsune_core::deflate::{deflate_fixed, zlib_compress};
use kitsune_core::gzip::MAX_DECODED_BYTES;
use kitsune_core::inflate::{self, Inflater};

fn gzip(plain: &[u8]) -> Vec<u8> {
    let mut v = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
    v.extend_from_slice(&deflate_fixed(plain));
    v.extend_from_slice(&inflate::crc32(plain).to_le_bytes());
    v.extend_from_slice(&(plain.len() as u32).to_le_bytes());
    v
}

fn chunked(body: &[u8], size: usize, last: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for c in body.chunks(size.max(1)) {
        out.extend_from_slice(format!("{:x}\r\n", c.len()).as_bytes());
        out.extend_from_slice(c);
        out.extend_from_slice(b"\r\n");
    }
    if last {
        out.extend_from_slice(b"0\r\n\r\n");
    }
    out
}

fn response(headers: &str, body: &[u8]) -> Vec<u8> {
    let mut r = format!("HTTP/1.1 200 OK\r\n{headers}\r\n").into_bytes();
    r.extend_from_slice(body);
    r
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }
    let mode = data[0];
    let cut_frac = u16::from_le_bytes([data[1], data[2]]) as usize;
    let bytes = &data[3..];
    let flagged_cut = mode & 0x20 != 0;

    // Whatever the bytes are, as a whole response and as bodies under hostile headers.
    let check = |resp: &[u8]| {
        let p = browser::page_body_partial(resp, flagged_cut);
        assert!(p.body.len() <= MAX_DECODED_BYTES.max(256));
        let _ = browser::body_bytes(resp);
        let _ = browser::body_partial(resp, flagged_cut);
        let _ = kitsune_core::appnet::app_response(resp, 1 << 16);
    };
    check(bytes);
    for h in [
        "Content-Encoding: gzip\r\n",
        "Content-Encoding: deflate\r\nContent-Length: 99\r\n",
        "Content-Encoding: gzip, gzip, deflate\r\nTransfer-Encoding: gzip, chunked\r\n",
        "Content-Encoding: br\r\n",
        "Transfer-Encoding: chunked\r\nContent-Length: 18446744073709551616\r\n",
    ] {
        check(&response(h, bytes));
    }

    // Streaming partial reads over the raw bytes, both flavours, in odd piece sizes.
    for zlib in [false, true] {
        let inf = if zlib {
            Inflater::new_zlib(bytes, 1 << 20)
        } else {
            Ok(Inflater::new_raw(bytes, 1 << 20))
        };
        if let Ok(mut inf) = inf {
            let mut buf = [0u8; 777];
            let mut total = 0usize;
            loop {
                let (n, err) = inf.read_partial(&mut buf);
                total += n;
                assert!(total <= 1 << 20);
                if err.is_some() || n == 0 {
                    break;
                }
            }
        }
    }

    // The property: a VALID stream cut anywhere is a prefix, and whole is exact.
    if mode & 0x10 != 0 {
        let plain = &bytes[..bytes.len().min(48 * 1024)];
        let (head, body): (&str, Vec<u8>) = match mode & 0x0F {
            0 => ("Content-Encoding: gzip\r\n", gzip(plain)),
            1 => ("Content-Encoding: deflate\r\n", zlib_compress(plain)),
            2 => ("Content-Encoding: deflate\r\n", deflate_fixed(plain)),
            3 => ("Content-Encoding: gzip, gzip\r\n", gzip(&gzip(plain))),
            4 => (
                "Transfer-Encoding: chunked\r\nContent-Encoding: GZIP\r\n",
                chunked(&gzip(plain), 1 + cut_frac % 700, true),
            ),
            _ => (
                "Transfer-Encoding: chunked\r\n",
                chunked(plain, 1 + cut_frac % 700, true),
            ),
        };
        let full = response(head, &body);
        let head_len = full.len() - body.len();
        let cut = head_len + (body.len() * (cut_frac % 1001)) / 1000;
        // `Err` = nothing decodable yet (inside the gzip header): the page shows a notice.
        if let Ok(p) = browser::body_partial(&full[..cut.min(full.len())], true) {
            assert!(
                plain.starts_with(&p.body),
                "decoded {} bytes that are not a prefix of the {} byte original",
                p.body.len(),
                plain.len()
            );
        }
        let p = browser::page_body_partial(&full, false);
        assert_eq!(p.body, plain);
        assert_eq!(p.note, None::<PageNote>);
    }
});
