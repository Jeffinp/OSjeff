//! Painting a laid-out web page: the kernel half of the `web` engine.
//!
//! The engine produces a display list in page coordinates and asks for its text widths through
//! [`kitsune_core::web::TextMetrics`]; [`KernelMetrics`] answers with the same glyph engine that
//! draws the text here (Inter for text, JetBrains Mono for `<pre>` and `<code>`), so a line that
//! was wrapped to fit really does fit.
//!
//! Italic has no face of its own: runs are slanted by 12 degrees at draw time (the metrics do
//! not change). Bold is Inter Semibold. A character the faces do not have (CJK, emoji, most
//! non-Latin scripts) is drawn as a hollow box instead of the `?` stand-in, and measured as one.
//!
//! The page area keeps the page's own colours in both appearances: nothing here reads the system
//! palette except the accent (selection, caret, focus ring).

mod cache;
mod metrics;
mod page;

pub(crate) use cache::PaintCache;
pub(crate) use metrics::KernelMetrics;
