//! Tests for images, forms, zoom, find and selection in the `web` engine.

use super::form::*;
use super::imgcache::{ImageLookup, ImgState, NoImages};
use super::layout::img_box;
use super::textops::CharPos;
use super::*;
use crate::Key;

struct Fixed(ImgState);

impl ImageLookup for Fixed {
    fn lookup(&self, _src: &str) -> ImgState {
        self.0
    }
}

fn lay(html: &str, w: i32, zoom: u16, lookup: &dyn ImageLookup) -> Page {
    Doc::parse(html.as_bytes()).layout(&Layout {
        width: w,
        zoom,
        images: lookup,
        metrics: &FixedAdvance,
    })
}

fn image_cmds(p: &Page) -> Vec<(i32, i32, i32, i32, usize)> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Image { x, y, w, h, idx } => Some((*x, *y, *w, *h, *idx)),
            _ => None,
        })
        .collect()
}

fn texts(p: &Page) -> Vec<String> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn form_page(html: &str) -> Page {
    lay(html, 700, 100, &NoImages)
}

fn search_form() -> Vec<FormInfo> {
    form_page(
        "<form action='/busca?velho=1#x' method=get><input type=hidden name=lang value='pt br'><input name=q><input type=submit name=ok value=Go></form>",
    )
    .forms
}

fn typed(forms: &[FormInfo], st: &mut FormState, s: &str) {
    for b in s.bytes() {
        st.on_key(forms, Key::Char(b));
    }
}

fn compose_all(s: &str) -> String {
    let mut c = Compose::new();
    let mut out = String::new();
    for ch in s.chars() {
        out.extend(c.feed(ch).iter());
    }
    out.extend(c.flush().iter());
    out
}

fn text_page() -> Page {
    render(
        b"<p>The quick brown fox</p><p>jumps over the lazy dog. The end.</p>",
        600,
    )
}

fn first_run(p: &Page) -> (i32, i32) {
    p.cmds
        .iter()
        .find_map(|c| match c {
            Cmd::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .unwrap()
}

mod dead_keys;
mod find_page;
mod forms_parsing;
mod forms_state_query;
mod hostile_input;
mod images_layout;
mod selection;
mod sizing;
mod title;
mod zoom;
