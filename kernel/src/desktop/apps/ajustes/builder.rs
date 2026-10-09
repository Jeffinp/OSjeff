//! The immediate-mode page builder: rows, switches, sliders, fields.

use super::state::COL_MAX;
use super::state::DOWN;
use super::state::ROW;
use super::state::*;
use crate::desktop::kit;
use crate::desktop::kit::ui::{self, ButtonKind};
use crate::desktop::*;
use crate::text::{self, BODY, FOOTNOTE, TITLE1, Weight};
use kitsune_core::settings::Settings;
use kitsune_core::widgets as wg;

// ------------------------------------------------------------------ immediate-mode page builder

pub(super) struct Probe {
    pub(super) px: i32,
    pub(super) py: i32,
    /// Match this control wherever the pointer is (a slider being dragged).
    pub(super) only: Option<u32>,
}

/// The page builder: draws when it has a canvas, finds a hit when it has a probe.
pub(super) struct Ui<'u, 'a> {
    pub(super) c: Option<&'u mut Canvas<'a>>,
    /// The visible part of the page.
    pub(super) view: Rect,
    pub(super) x: i32,
    pub(super) w: i32,
    pub(super) y: i32,
    probe: Option<Probe>,
    pub(super) hit: Option<(u32, Rect)>,
    pub(super) hv: u32,
    pub(super) st: &'u SettingsState,
    pub(super) s: Settings,
}

impl<'u, 'a> Ui<'u, 'a> {
    pub(super) fn new(
        c: Option<&'u mut Canvas<'a>>,
        pane: Rect,
        st: &'u SettingsState,
        probe: Option<Probe>,
    ) -> Self {
        let w = (pane.w - 56).min(COL_MAX);
        Ui {
            c,
            view: pane,
            x: pane.x + 28,
            w,
            y: pane.y + 24 - st.scroll.value(),
            probe,
            hit: None,
            hv: st.hover.get(),
            st,
            s: crate::settings::get(),
        }
    }

    /// Register `r` as control `id` for a probe; `true` when the probe is on it.
    pub(super) fn hit(&mut self, r: Rect, id: u32) -> bool {
        let Some(p) = &self.probe else {
            return false;
        };
        let inside = match p.only {
            Some(o) => o == id,
            None => r.contains(p.px, p.py) && self.view.contains(p.px, p.py),
        };
        if inside && self.hit.is_none() {
            self.hit = Some((id, r));
        }
        inside
    }

    pub(super) fn hovered(&self, id: u32) -> bool {
        self.hv & !DOWN == id
    }

    fn state(&self, id: u32, enabled: bool) -> ui::Control {
        kit::control_state(self.hovered(id), self.hv & DOWN != 0, enabled)
    }

    /// The page title.
    pub(super) fn title(&mut self, t: &str) {
        let r = Rect::new(self.x, self.y, self.w, 32);
        if let Some(c) = self.c.as_deref_mut() {
            text::draw_left(c, r, t, TITLE1, Weight::Semibold, kit::ink());
        }
        self.y += 48;
    }

    /// A small heading above a card.
    pub(super) fn header(&mut self, t: &str) {
        let r = Rect::new(self.x + 4, self.y, self.w - 8, 20);
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            text::draw_left(c, r, t, FOOTNOTE, Weight::Medium, kit::ink2());
        }
        self.y += 26;
    }

    /// A card `h` high; the cursor moves past it.
    pub(super) fn card(&mut self, h: i32) -> Rect {
        let r = Rect::new(self.x, self.y, self.w, h);
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            kit::card(c, r);
        }
        self.y += h + 22;
        r
    }

    /// Row `i` of a card.
    pub(super) fn row(&mut self, card: Rect, i: i32) -> Rect {
        let r = Rect::new(card.x, card.y + i * ROW, card.w, ROW);
        if i > 0
            && let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::separator(c, Rect::new(r.x + 16, r.y, r.w - 32, 1));
        }
        r
    }

    /// A row's label (and an optional line under it) at the left.
    pub(super) fn label(&mut self, r: Rect, t: &str, sub: &str, enabled: bool) {
        let Some(c) = self.c.as_deref_mut() else {
            return;
        };
        if self.view.intersection(&r).is_none() {
            return;
        }
        let col = if enabled { kit::ink() } else { kit::ink3() };
        if sub.is_empty() {
            text::draw_left(
                c,
                Rect::new(r.x + 16, r.y, r.w - 32 - 120, r.h),
                t,
                BODY,
                Weight::Regular,
                col,
            );
        } else {
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.y + 8,
                r.w - 160,
                t,
                BODY,
                Weight::Regular,
                col,
            );
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.y + 27,
                r.w - 160,
                sub,
                FOOTNOTE,
                Weight::Regular,
                kit::ink2(),
            );
        }
    }

    /// A row with a switch; `knob` is its animated position.
    pub(super) fn row_switch(
        &mut self,
        card: Rect,
        i: i32,
        t: &str,
        sub: &str,
        knob: i32,
        id: u32,
    ) -> bool {
        let r = self.row(card, i);
        self.label(r, t, sub, true);
        let sw = wg::switch_rect(
            r.right() - 16 - wg::SWITCH_W,
            r.y + (ROW - wg::SWITCH_H) / 2,
        );
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::switch(c, sw, knob, true);
        }
        self.hit(r, id)
    }

    /// A row with a slider and its value text.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn row_slider(
        &mut self,
        card: Rect,
        i: i32,
        t: &str,
        v: i32,
        (min, max): (i32, i32),
        value_text: &str,
        enabled: bool,
        id: u32,
    ) -> bool {
        let r = self.row(card, i);
        self.label(r, t, "", enabled);
        let slider = Rect::new(
            r.x + 190,
            r.y + (ROW - 24) / 2,
            (r.w - 190 - 16 - 64).max(60),
            24,
        );
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::slider(c, slider, v, min, max, enabled);
            text::draw_right(
                c,
                Rect::new(r.right() - 16 - 56, r.y, 56, r.h),
                value_text,
                BODY,
                Weight::Regular,
                if enabled { kit::ink2() } else { kit::ink3() },
            );
        }
        if enabled {
            self.hit(slider.inflated(6), id)
        } else {
            false
        }
    }

    pub(super) fn button(
        &mut self,
        r: Rect,
        t: &str,
        kind: ButtonKind,
        id: u32,
        enabled: bool,
    ) -> bool {
        let st = self.state(id, enabled);
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::push_button(c, r, t, kind, st);
        }
        enabled && self.hit(r, id)
    }

    pub(super) fn segmented(&mut self, r: Rect, labels: &[&str], sel: usize, id: u32) {
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::segmented(c, r, labels, sel);
        }
        for (i, s) in wg::segmented_rects(r, labels.len()).into_iter().enumerate() {
            self.hit(s, id + i as u32);
        }
    }

    pub(super) fn field(&mut self, r: Rect, t: &str, placeholder: &str, focus: bool, id: u32) {
        if let Some(c) = self.c.as_deref_mut()
            && self.view.intersection(&r).is_some()
        {
            ui::text_field(c, r, t, placeholder, focus, focus);
        }
        self.hit(r, id);
    }
}
