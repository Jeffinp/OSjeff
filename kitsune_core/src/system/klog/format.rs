//! format (split out of `klog.rs`).

use super::*;

/// Longest [`format_prefix`] output: `"99999.999 I "`.
pub const PREFIX_LEN: usize = 12;

/// Write the compact line prefix `"  12.345 I "` (seconds.millis, level letter,
/// space) into `out` and return its length (always [`PREFIX_LEN`]).
pub fn format_prefix(e: &Entry<'_>, out: &mut [u8; PREFIX_LEN]) -> usize {
    let secs = e.ts_ms / 1000;
    let ms = e.ts_ms % 1000;
    let mut s = [b' '; 5];
    let mut v = secs;
    for slot in s.iter_mut().rev() {
        *slot = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    // Replace leading zeros (except the last digit) with spaces.
    for slot in s.iter_mut().take(4) {
        if *slot == b'0' {
            *slot = b' ';
        } else {
            break;
        }
    }
    out[..5].copy_from_slice(&s);
    out[5] = b'.';
    out[6] = b'0' + (ms / 100) as u8;
    out[7] = b'0' + ((ms / 10) % 10) as u8;
    out[8] = b'0' + (ms % 10) as u8;
    out[9] = b' ';
    out[10] = e.level.letter();
    out[11] = b' ';
    PREFIX_LEN
}

/// Render every record of `snapshot` that passes `filter` as text, one
/// `"<prefix><thread> <text>\n"` line each, into `out`. `thread_name` maps a
/// scheduler slot to its name. Used by "save to file".
pub fn render_text<'n>(
    snapshot: &[u8],
    filter: &Filter,
    thread_name: impl Fn(u8) -> &'n str,
    out: &mut Vec<u8>,
) {
    for e in records(snapshot) {
        if !filter.matches(&e) {
            continue;
        }
        let mut p = [0u8; PREFIX_LEN];
        let n = format_prefix(&e, &mut p);
        out.extend_from_slice(&p[..n]);
        out.extend_from_slice(thread_name(e.origin).as_bytes());
        out.push(b' ');
        out.extend_from_slice(e.text);
        out.push(b'\n');
    }
}

/// The whole log of `snapshot` as text (every level, the same line format as "save
/// to file"), cut to its newest whole lines if it exceeds `cap` bytes. Returns the
/// text and whether older lines were dropped. This is what the boot-time flush
/// writes to `/var/log/boot.log`: the result never exceeds `cap`.
pub fn dump_bounded<'n>(
    snapshot: &[u8],
    thread_name: impl Fn(u8) -> &'n str,
    cap: usize,
) -> (Vec<u8>, bool) {
    let mut text = Vec::new();
    render_text(snapshot, &Filter::new(), thread_name, &mut text);
    let (body, cut) = tail_lines(&text, cap);
    (body.to_vec(), cut)
}

/// The newest part of a text dump that fits `cap` bytes, starting at a line
/// start (so no line is cut in half), and whether anything was dropped. For
/// file systems with a small file size limit.
pub fn tail_lines(data: &[u8], cap: usize) -> (&[u8], bool) {
    if data.len() <= cap {
        return (data, false);
    }
    let mut start = data.len() - cap;
    // Skip to just after the next newline unless the cut already fell on one.
    if data[start - 1] != b'\n' {
        match data[start..].iter().position(|&b| b == b'\n') {
            Some(i) => start += i + 1,
            None => start = data.len(),
        }
    }
    (&data[start..], true)
}
