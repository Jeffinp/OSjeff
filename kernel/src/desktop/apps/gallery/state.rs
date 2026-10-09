//! State and layout of the component gallery.

use crate::desktop::*;
use kitsune_core::tk;

pub(super) const TABS: [&str; 5] = [
    tk!("kit.tab.controls"),
    tk!("kit.tab.type"),
    tk!("kit.tab.colors"),
    tk!("kit.tab.icons"),
    tk!("kit.tab.shell"),
];

/// State of the gallery window: the widgets it shows are real and interactive.
pub(crate) struct GalleryState {
    pub tab: usize,
    pub segment: usize,
    pub switch_on: bool,
    pub slider: i32,
    pub checks: [bool; 2],
    pub radio: usize,
    pub field: String,
    pub field_focus: bool,
    pub list_sel: usize,
}

impl GalleryState {
    pub(crate) fn new() -> GalleryState {
        GalleryState {
            tab: 0,
            segment: 1,
            switch_on: true,
            slider: 40,
            checks: [true, false],
            radio: 0,
            field: String::new(),
            field_focus: false,
            list_sel: 1,
        }
    }
}

/// Rectangles of the interactive widgets of the "Controles" page.
pub(super) struct Layout {
    pub(super) tabs: Rect,
    pub(super) segmented: Rect,
    pub(super) switch: Rect,
    pub(super) slider: Rect,
    pub(super) checks: [Rect; 2],
    pub(super) radios: [Rect; 3],
    pub(super) field: Rect,
    pub(super) list: Rect,
}

pub(super) fn layout(body: Rect) -> Layout {
    let x = body.x + 24;
    let col2 = body.x + 556;
    let mut y = body.y + 60;
    let tabs = Rect::new(body.x + 24, body.y + 16, 480, 28);
    let _ = y;
    // Row 1 is the buttons (drawn, not interactive here).
    y += 40;
    let segmented = Rect::new(x, y, 300, 28);
    y += 44;
    let switch = crate::desktop::wlogic::switch_rect(x, y);
    let slider = Rect::new(x + 60, y - 2, 240, 26);
    y += 40;
    let checks = [Rect::new(x, y, 150, 20), Rect::new(x + 160, y, 150, 20)];
    y += 32;
    let radios = [
        Rect::new(x, y, 96, 20),
        Rect::new(x + 100, y, 96, 20),
        Rect::new(x + 200, y, 96, 20),
    ];
    y += 36;
    let field = Rect::new(x, y, 300, 30);
    let list = Rect::new(col2, body.y + 60, (body.w - 556 - 24).max(160), 132);
    Layout {
        tabs,
        segmented,
        switch,
        slider,
        checks,
        radios,
        field,
        list,
    }
}
