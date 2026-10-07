//! Fuzz target: the parts of the browser that read hostile page content beyond
//! plain text: `<img>` (sizes, `src`/`alt`, `data:` URIs, base64), `<form>`
//! controls and their editing / query encoding, the layout with images and
//! zoom, find in page, selection, and the image cache.
//!
//! Input layout: `[mode, zoom, viewport_lo, viewport_hi, ...bytes]`.
//!
//! * `mode & 3`: how the bytes are used as HTML: raw; inside `<img src=...>`
//!   / `alt=...` / `width=...`; inside `<form action=...><input name=... value=...>`;
//!   as a `data:image/png;base64,` payload.
//! * `mode >> 2`: the state every `<img>` is reported in (pending, ready with
//!   a size taken from the bytes, unsupported, failed, too big, too many).
//! * `zoom`: mapped onto `[0, 400]` (the engine clamps to 50..=300).
//!
//! Invariants checked after every layout: no coordinate overflows, every
//! `Cmd::Image` has a positive box and a valid `idx`, every link hit and
//! control box refers to something that exists, the display list is bounded.
#![no_main]

use libfuzzer_sys::fuzz_target;
use osjeff_core::Key;
use osjeff_core::base64;
use osjeff_core::web::form::{FormState, MAX_QUERY, MAX_VALUE};
use osjeff_core::web::imgcache::{
    self, ImageCache, ImageLookup, ImgFail, ImgState, Loaded, NoImages, PageImages,
};
use osjeff_core::web::{Cmd, Doc, Layout, Page};

const STACK: usize = 512 * 1024;

struct Fixed(ImgState);

impl ImageLookup for Fixed {
    fn lookup(&self, _src: &str) -> ImgState {
        self.0
    }
}

fn state_from(sel: u8, a: u8, b: u8) -> ImgState {
    match sel % 6 {
        0 => ImgState::Pending,
        1 => ImgState::Ready {
            w: i32::from(a) * 40 + 1,
            h: i32::from(b) * 40 + 1,
        },
        2 => ImgState::Unsupported,
        3 => ImgState::Failed,
        4 => ImgState::TooBig,
        _ => ImgState::TooMany,
    }
}

fn check_page(p: &Page) {
    assert!(p.height >= 0);
    assert!(p.cmds.len() <= 200_000);
    for c in &p.cmds {
        match c {
            Cmd::Image { w, h, idx, x, y } => {
                assert!(*w >= 1 && *h >= 1 && *x >= 0 && *y >= 0);
                assert!(*idx < p.images.len());
            }
            Cmd::Rect { x, y, w, h, .. } => assert!(*x >= 0 && *y >= 0 && *w >= 0 && *h >= 0),
            Cmd::Text { x, y, .. } => assert!(*x >= 0 && *y >= 0),
        }
    }
    for h in &p.hits {
        assert!(h.link < p.links.len());
    }
    for f in &p.fields {
        let form = p.forms.get(f.form).expect("form index");
        assert!(f.field < form.fields.len());
        assert!(f.w >= 1 && f.h >= 1);
    }
}

fn exercise(data: &[u8]) {
    if data.len() < 4 {
        return;
    }
    let mode = data[0];
    let zoom = u32::from(data[1]) * 400 / 255;
    let viewport = i32::from(u16::from_le_bytes([data[2], data[3]]) % 3000);
    let raw = &data[4..];
    let text = String::from_utf8_lossy(raw).into_owned();
    let esc: String = text
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '<' | '>'))
        .take(600)
        .collect();

    let html = match mode & 3 {
        0 => text.clone(),
        1 => format!(
            "<p>a <img src='{esc}' alt='{esc}' width='{esc}' height='{}'> b <a href='{esc}'><img src='{esc}'></a></p>",
            raw.len()
        ),
        2 => format!(
            "<form action='{esc}' method='{}'><input name='{esc}' value='{esc}' size='{}'><input type=hidden name=h value='{esc}'><button name=b value='{esc}'>{esc}</button><input type=password name=p></form>",
            if raw.len() % 2 == 0 { "get" } else { "post" },
            raw.len()
        ),
        _ => {
            let b64 = base64::encode(raw);
            format!("<img src='data:image/png;base64,{b64}' alt='x'><img src='data:image/png;base64,{esc}'>")
        }
    };

    let state = state_from(mode >> 2, raw.first().copied().unwrap_or(0), raw.get(1).copied().unwrap_or(0));
    let doc = Doc::parse(html.as_bytes());
    let _ = doc.title();
    let mut page = None;
    for z in [zoom as u16, 50, 100, 300] {
        let lookups: [&dyn ImageLookup; 2] = [&Fixed(state), &NoImages];
        for l in lookups {
            let p = doc.layout(&Layout {
                width: viewport,
                zoom: z,
                images: l,
            });
            check_page(&p);
            page = Some(p);
        }
    }
    let page = page.expect("a page");

    // ---- forms: edit with the bytes as keys, then submit ----
    let mut st = FormState::new(&page.forms);
    for (fi, f) in page.forms.iter().enumerate() {
        for i in 0..f.fields.len() {
            if st.set_focus(&page.forms, fi, i) {
                for &b in raw.iter().take(64) {
                    let key = match b % 16 {
                        0 => Key::Backspace,
                        1 => Key::Delete,
                        2 => Key::Left,
                        3 => Key::Right,
                        4 => Key::Home,
                        5 => Key::End,
                        6 => Key::Tab,
                        7 => Key::Esc,
                        8 => Key::Enter,
                        _ => Key::Char(b),
                    };
                    let _ = st.on_key(&page.forms, key);
                    assert!(st.value(fi, i).len() <= MAX_VALUE);
                    assert!(st.value(fi, i).is_char_boundary(st.caret().min(st.value(fi, i).len())) || st.focus().is_none());
                }
                st.insert_str(&page.forms, &text);
                let _ = st.visible(&page.forms, fi, i, (viewport as usize % 40) + 1);
            }
            if let Ok(q) = st.query(&page.forms, fi, Some(i)) {
                assert!(q.len() <= MAX_QUERY + 2000);
                assert!(q.is_ascii());
            }
        }
        if let Ok(t) = st.target(&page.forms, fi, None) {
            assert!(t.contains('?'));
        }
        let _ = st.tab(&page.forms, mode & 1 == 0);
    }

    // ---- find / selection ----
    let _ = page.find(&text.chars().take(8).collect::<String>());
    let a = (i32::from(raw.first().copied().unwrap_or(0)) * 4, i32::from(raw.get(1).copied().unwrap_or(0)) * 4);
    let b = (i32::from(raw.get(2).copied().unwrap_or(9)) * 4, i32::from(raw.get(3).copied().unwrap_or(9)) * 8);
    if let Some(r) = page.select(a, b) {
        let _ = page.selection_spans(r);
        let _ = page.selection_text(r);
    }

    // ---- decoders and the cache ----
    let _ = base64::decode(raw, 4096);
    let _ = base64::parse_data_uri(&text);
    let _ = base64::decode_data_image(&text, 4096);
    let _ = imgcache::decode_for_page(raw, viewport as usize);
    let _ = imgcache::decode_data_uri(&text, viewport as usize);
    let _ = imgcache::peek_dims(raw);
    let _ = imgcache::image_key(b"http://h.test/dir/page.html", &text);
    let _ = imgcache::image_key(b"https://h.test/", &text);

    let mut cache = ImageCache::new();
    for (i, &b) in raw.iter().take(200).enumerate() {
        let key = format!("k{}", b % 20);
        match b % 7 {
            0 => cache.begin_page(),
            1 | 2 => {
                cache.want(&key, None);
            }
            3 => {
                if let Some((k, _)) = cache.next_pending() {
                    let res = if b & 1 == 0 {
                        Ok(Loaded {
                            orig_w: 10,
                            orig_h: 10,
                            img: osjeff_core::image::Image::new(
                                (b as usize % 200) + 1,
                                (i % 150) + 1,
                                0xFF00_0000,
                            )
                            .expect("small image"),
                        })
                    } else {
                        Err(ImgFail::Failed)
                    };
                    cache.finish(&k, res);
                }
            }
            4 => {
                let _ = cache.fit_to(&key, b as usize % 300, i % 300);
            }
            5 => cache.requeue_loading(),
            _ => {
                let _ = cache.state(&key);
                let _ = cache.image(&key);
            }
        }
        assert!(cache.bytes() <= imgcache::CACHE_BYTES + 4 * 4096 * 4096 / 16);
        assert!(cache.len() <= imgcache::MAX_ENTRIES + 1);
    }
    let pi = PageImages {
        cache: &cache,
        base: b"http://h.test/",
    };
    let _ = pi.lookup(&text);
}

fuzz_target!(|data: &[u8]| {
    let data = data.to_vec();
    // A small stack, like the kernel's, so unbounded recursion is a crash here too.
    let h = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || exercise(&data))
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
});
