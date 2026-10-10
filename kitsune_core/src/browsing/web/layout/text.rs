//! text (split out of `layout.rs`).

use super::*;

/// Characters that may break between each other (no spaces in the script).
pub(super) fn breaks_anywhere(c: char) -> bool {
    matches!(c as u32,
        0x0E00..=0x0EFF
        | 0x2E80..=0x9FFF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF
        | 0x1F000..=0x1FAFF
        | 0x20000..=0x3FFFF)
}

/// Width in q8 pixels (1/256) of `text`.
pub(super) fn wq(p: &Painter, text: &str, f: Font) -> i32 {
    // The metrics report whole pixels; the kernel's engine is more exact through
    // `width_q8`, which defaults to `width * 256`.
    p.m.width_q8(text, f).clamp(0, 1 << 26)
}

/// Byte length of the longest prefix of `text` (at least one character) that fits
/// in `max_q8`.
pub(super) fn fit_prefix(p: &Painter, text: &str, f: Font, max_q8: i32) -> usize {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .skip(1)
        .chain([text.len()])
        .collect();
    if bounds.is_empty() {
        return text.len();
    }
    // Binary search for the largest count of characters whose width fits.
    let (mut lo, mut hi) = (0usize, bounds.len()); // lo fits (0 chars), hi may not
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if wq(p, &text[..bounds[mid - 1]], f) <= max_q8 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    bounds[lo.max(1) - 1]
}

pub(super) fn push_word(p: &mut Painter, text: String, st: Style, any: bool, nb: bool) {
    if text.is_empty() {
        return;
    }
    let sp = p.space.take();
    p.items.push(Item::Word {
        text,
        st,
        sp,
        any,
        nb,
    });
}

/// Turn the text of a DOM node into words (collapsing whitespace) or, in
/// preformatted text, lines.
#[allow(unused_assignments)]
pub(super) fn push_text(p: &mut Painter, text: &str, c: &Computed) {
    if p.chars >= MAX_TEXT_CHARS {
        return;
    }
    let st = p.style(c);
    let transformed;
    let text = if c.transform == super::super::style::Transform::None {
        text
    } else {
        transformed = transform_text(text, c.transform);
        transformed.as_str()
    };
    if c.ws == Ws::Pre {
        let mut lines = text.split('\n').peekable();
        while let Some(line) = lines.next() {
            if !line.is_empty() {
                let expanded = expand_tabs(line);
                let n = expanded.chars().count().min(MAX_WORD_CHARS * 4);
                p.chars += n;
                let cut: String = expanded.chars().take(n).collect();
                // The leading text belongs to the line, spaces included.
                p.space = None;
                p.items.push(Item::Word {
                    text: cut,
                    st,
                    sp: None,
                    any: true,
                    nb: false,
                });
            }
            if lines.peek().is_some() {
                p.items.push(Item::Br(st));
            }
        }
        return;
    }
    let nowrap = c.ws == Ws::NoWrap;
    // The word being read is `text[start..]` up to the current position.
    let mut start: Option<usize> = None;
    let mut word_any = false;
    let mut emitted = 0usize;
    let mut count = 0usize;
    macro_rules! finish {
        ($end:expr) => {
            if let Some(from) = start.take() {
                p.chars += count;
                count = 0;
                emitted += 1;
                push_word(
                    p,
                    String::from(&text[from..$end]),
                    st,
                    word_any,
                    nowrap && emitted > 1,
                );
            }
        };
    }
    let mut stop = text.len();
    for (i, ch) in text.char_indices() {
        if ch != '\u{a0}' && (ch.is_whitespace() || (ch.is_ascii() && is_html_space(ch as u8))) {
            finish!(i);
            p.space = Some(st.font);
            continue;
        }
        if ch == '\u{200b}' {
            finish!(i);
            continue;
        }
        let any = breaks_anywhere(ch);
        if start.is_some() && (any != word_any || count >= MAX_WORD_CHARS) {
            finish!(i);
        }
        if p.chars + count >= MAX_TEXT_CHARS {
            stop = i;
            break;
        }
        word_any = any;
        if start.is_none() {
            start = Some(i);
        }
        count += 1;
    }
    finish!(stop);
}

pub(super) fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return String::from(line);
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let n = 4 - col % 4;
            for _ in 0..n {
                out.push(' ');
            }
            col += n;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}
