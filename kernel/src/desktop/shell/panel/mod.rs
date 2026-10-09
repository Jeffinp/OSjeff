//! The top panel: the Apps button and Busca at the left, the date and time in the centre
//! (opening the calendar and notification centre) and the status pill at the right (opening
//! Quick Settings); plus the menus (the system menu, a window's menu button, context menus) and
//! the popovers that hang from the panel. See `docs/design/ui-identity.md`.

mod bar;
mod centre;
mod helpers;
mod menus;
mod popovers;

pub(crate) use bar::Notif;
