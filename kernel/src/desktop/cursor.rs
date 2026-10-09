//! The pointer: which sprite to show (arrow, hand over links, I-beam over text)
//! and drawing it. The sprites themselves are vector shapes from
//! `kitsune_core::pointer`, rendered once and cached here; the box size and hotspots
//! live in that module so `main.rs` only needs `CURSOR_W` / `CURSOR_H`.

use super::*;
use kitsune_core::pointer::{self, Shape};
use kitsune_core::raster::Surface;

static SPRITES: RacyCell<Option<[Surface; 3]>> = RacyCell::new(None);

fn sprite(s: Shape) -> &'static Surface {
    // SAFETY: only the compositor thread draws the pointer; the reference is used at once.
    // NOTE: not guaranteed by the type: safe fn returning a `'static` reference.
    let slot = unsafe { &mut *SPRITES.get() };
    let all = slot.get_or_insert_with(|| {
        [
            pointer::render(Shape::Arrow),
            pointer::render(Shape::Hand),
            pointer::render(Shape::IBeam),
        ]
    });
    &all[s as usize]
}

impl Desktop {
    /// Is the pointer over editable text (editor, terminal, the address bar, the
    /// Busca field)? Then it is drawn as an I-beam.
    fn cursor_is_text(&self) -> bool {
        let (cx, cy) = (self.cursor_x, self.cursor_y);
        if let Some(s) = self.shell.search.as_ref().filter(|s| !s.closing) {
            let g = kitsune_core::chrome::spotlight_geom(self.sw, self.sh, s.hits.len());
            return g.field.contains(cx, cy);
        }
        if self.modal_open() || self.overlay_open() || self.drag.is_some() {
            return false;
        }
        let Some(w) = self.topmost_at(cx, cy) else {
            return false;
        };
        let Some(win) = self.wm.get(w) else {
            return false;
        };
        if win.rect.y + TITLE_H > cy {
            return false;
        }
        match &win.app.app {
            App::Editor(_) => self.editor_text_at(w, cx, cy),
            App::Terminal(_) => self.term_text_at(w, cx, cy),
            App::Browser(_) => self.browser_cursor_text(win, cx, cy),
            _ => false,
        }
    }

    /// Which sprite the pointer shows now.
    pub(crate) fn cursor_shape(&self) -> Shape {
        if self.cursor_is_hand() {
            Shape::Hand
        } else if self.cursor_is_text() {
            Shape::IBeam
        } else {
            Shape::Arrow
        }
    }

    pub(crate) fn draw_cursor(&self, c: &mut Canvas) {
        let shape = self.cursor_shape();
        let (x, y) = self.cursor();
        c.blit_surface(sprite(shape), x, y, 256);
    }
}
