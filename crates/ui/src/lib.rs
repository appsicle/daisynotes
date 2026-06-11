//! muse-ui — shared primitives: buttons, popovers, toasts, icons, springs.
//! The visual vocabulary every other UI crate speaks.
//!
//! Everything here is generic chrome: theme-driven (via
//! [`muse_theme::ActiveTheme`]), free of hardcoded colors, and ignorant of
//! editors, entries, and the agent. This crate also embeds the app's static
//! assets — Lucide icons (served through [`assets::MuseAssets`]) and the
//! four bundled content font families ([`fonts::all`]).

pub mod assets;
pub mod fonts;

mod button;
mod containers;
mod divider;
mod icon;
mod switch;
mod text_field;

pub use button::{IconButton, TextButton, icon_button, text_button};
pub use containers::{Card, Pill, card, pill, soft_shadow};
pub use divider::{Divider, divider};
pub use icon::{Icon, IconName, icon};
pub use switch::{Switch, switch};
pub use text_field::{TEXT_FIELD_CONTEXT, TextField, bindings as text_field_bindings};
