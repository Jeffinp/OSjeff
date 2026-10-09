//! State of the taskbar: pinned apps, springs, the icon being dragged and the tooltip.

use crate::desktop::*;
use kitsune_core::anim::{Spring, Tween};
use kitsune_core::taskbar::Hit;

/// Seconds the pointer must rest on an icon before its label shows.
pub(super) const TIP_DELAY: f32 = 0.35;
/// Peak height of the first launch hop.
pub(super) const BOUNCE_PX: f32 = 12.0;

/// The apps the bar starts with, in order.
pub(crate) const DEFAULT_PINNED: [Kind; 8] = [
    Kind::Files,
    Kind::Browser,
    Kind::Terminal,
    Kind::Editor,
    Kind::Calculator,
    Kind::Viewer,
    Kind::TaskMgr,
    Kind::Settings,
];

/// A pinned icon being dragged: the item it started on, the pointer x and the slot it hovers.
pub(crate) struct IconDrag {
    pub from: usize,
    pub x: i32,
    pub slot: usize,
}

/// Live state of the bar.
pub(crate) struct TaskbarState {
    pub pinned: Vec<Kind>,
    pub hover: Option<Hit>,
    /// Seconds the pointer has rested on `hover` (the tooltip waits for [`TIP_DELAY`]).
    pub rest: f32,
    pub tip: Tween,
    /// Launch hops: `(kind, seconds since the launch)`.
    pub bounce: Vec<(Kind, f32)>,
    /// Lift of each icon under the pointer, and each icon's current x (reordering slides).
    pub lift: Vec<(Kind, Spring)>,
    pub xs: Vec<(Kind, Spring)>,
    /// The icon pressed (and where) while the button is down: a click if it was not dragged.
    pub press: Option<(usize, i32, i32)>,
    pub drag: Option<IconDrag>,
    /// Windows that *Mostrar área de trabalho* minimised, to bring them back.
    pub hidden: Vec<WindowId>,
    /// How many icons the last layout had (a change snaps the slides instead of animating).
    pub count: usize,
    /// Set when something changed that needs a repaint even without motion.
    pub dirty: bool,
}

impl TaskbarState {
    pub(crate) fn new() -> TaskbarState {
        TaskbarState {
            pinned: DEFAULT_PINNED.to_vec(),
            hover: None,
            rest: 0.0,
            tip: Tween::at(0.0),
            bounce: Vec::new(),
            lift: Vec::new(),
            xs: Vec::new(),
            press: None,
            drag: None,
            hidden: Vec::new(),
            count: 0,
            dirty: true,
        }
    }
}

impl Kind {
    /// The key the launcher's *Recentes* remembers a built-in app by.
    pub(crate) fn recent_key(self) -> String {
        alloc::format!("sys:{}", self.proc_name())
    }

    /// Can the app be pinned to the bar?
    pub(crate) fn pinnable(self) -> bool {
        !matches!(self, Kind::WasmApp | Kind::Gallery)
    }
}

pub(super) fn spring_of(v: &mut Vec<(Kind, Spring)>, k: Kind, init: f32) -> &mut Spring {
    if let Some(i) = v.iter().position(|(kk, _)| *kk == k) {
        &mut v[i].1
    } else {
        v.push((k, Spring::pixels(init, 420.0, 30.0)));
        &mut v.last_mut().expect("just pushed").1
    }
}
