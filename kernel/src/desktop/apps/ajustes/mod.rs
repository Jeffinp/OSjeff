//! Ajustes, the settings app: a sidebar of sections and an immediate-mode page per section.
//!
//! `state` holds the window state, `builder` the row / switch / slider widgets, `pages/` one
//! file per group of sections, `actions` what applying a setting does, `input` the events and
//! `paint` the window.

mod actions;
mod builder;
mod input;
mod pages;
mod paint;
mod state;
mod users_logic;

pub(crate) use state::{ABOUT, SettingsState};
