//! Drawing a Tarefas window: tabs, section titles and stat cards.

use crate::desktop::apps::tarefas::layout::lay;
use crate::desktop::apps::tarefas::state::H_DOWN;
use crate::desktop::apps::tarefas::state::H_SAMPLE;
use crate::desktop::apps::tarefas::state::tab_names;
use crate::desktop::kit::ui::{self};
use crate::desktop::kit::{self};
use crate::desktop::*;
use crate::text::{self, CALLOUT, FOOTNOTE, TITLE2, TITLE3, Weight};
use core::fmt::Write as _;
use kitsune_core::klog::FixedBuf;

impl Desktop {
    pub(crate) fn draw_tarefas(&self, c: &mut Canvas, r: Rect, st: &TarefasState) {
        let l = lay(r);
        let hv = st.hover.get();
        // The tab bar.
        ui::segmented(c, l.tabs, &tab_names(), st.tab as usize);
        match st.tab {
            0 => self.tf_cpu(c, &l, st),
            1 => self.tf_memory(c, &l, st),
            2 => self.tf_disk(c, &l, st),
            3 => self.tf_network(c, &l, st),
            _ => self.tf_processes(c, &l, st, hv),
        }
        if st.confirm.is_some() {
            self.tf_confirm(c, r, &l, st, hv);
        }
    }

    /// The hovered sample index on a chart tab, if any.
    pub(super) fn hovered_sample(st: &TarefasState) -> Option<usize> {
        let hv = st.hover.get() & !H_DOWN;
        (hv >= H_SAMPLE).then(|| (hv - H_SAMPLE) as usize)
    }

    /// A section title row: the title on the left, `value` on the right in the accent.
    pub(super) fn section(&self, c: &mut Canvas, r: Rect, title: &str, value: &str) {
        text::draw_left(c, r, title, CALLOUT, Weight::Semibold, kit::ink());
        if !value.is_empty() {
            text::draw_right(
                c,
                Rect::new(r.x, r.y, r.w, r.h),
                value,
                TITLE3,
                Weight::Semibold,
                theme::accent(),
            );
        }
    }

    /// A small card with a label and a value, for the right-hand column.
    pub(super) fn stat_card(&self, c: &mut Canvas, r: Rect, name: &str, value: &str, sub: &str) {
        kit::card(c, r);
        kit::label(c, r.x + 16, r.y + 12, r.w - 32, name);
        text::draw_ellipsis(
            c,
            r.x + 16,
            r.y + 28,
            r.w - 32,
            value,
            TITLE2,
            Weight::Semibold,
            kit::ink(),
        );
        if !sub.is_empty() && r.h >= 84 {
            text::draw_ellipsis(
                c,
                r.x + 16,
                r.bottom() - 24,
                r.w - 32,
                sub,
                FOOTNOTE,
                Weight::Regular,
                kit::ink2(),
            );
        }
    }
}

/// `fmt_*` buffers of different sizes, converted to one size for tables of values.
pub(super) trait IntoFb {
    fn into_fb(self) -> FixedBuf<24>;
}

impl<const N: usize> IntoFb for FixedBuf<N> {
    fn into_fb(self) -> FixedBuf<24> {
        let mut b = FixedBuf::<24>::new();
        let _ = b.write_str(core::str::from_utf8(self.as_bytes()).unwrap_or(""));
        b
    }
}
