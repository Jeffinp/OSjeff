//! The compositor's pure core: damage regions, the scene description and the engine that plans
//! repaints. See `docs/design/compositor.md`.
//!
//! The kernel owns pixels and describes what is on screen ([`Scene`]); [`Engine::plan`] says what
//! to repaint ([`Plan`]); the two meet in the [`Painter`] trait. [`sim`] is a model painter and
//! world used by the differential tests and the fuzz target to prove that the incremental result
//! is byte-identical to a full redraw.

mod engine;
mod region;
mod scene;
pub mod sim;

#[cfg(test)]
mod tests;

pub use engine::{Engine, Painter, Plan, Policy, Step};
pub use region::{MAX_RECTS, Region, area, subtract};
pub use scene::{Layer, LayerId, Look, Scene};
