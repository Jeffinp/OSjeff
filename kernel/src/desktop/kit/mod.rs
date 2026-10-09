//! Shared desktop widgets and drawing helpers: the control toolkit, charts, glass surfaces,
//! cached app art, window-geometry helpers and the hover/animation clock of the system apps.

pub(super) mod appart;
pub(super) mod appui;
pub(super) mod charts;
pub(super) mod glass;
pub(super) mod live;
pub(super) mod ui;
pub(super) mod widgets;

pub(crate) use charts::*;
