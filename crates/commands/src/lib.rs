//! muse-commands — gpui actions, the keymap, and command dispatch.
//! Menus and shortcuts route through here so they can never diverge.
//!
//! This crate owns exactly three declarative artifacts, and no behavior:
//!
//! 1. **Actions** — every user intent in Muse, declared once: workspace-scope
//!    intents (entries, sidebar, theme, Muse itself) and editor-scope intents
//!    (formatting, history, clipboard, deletion, motion, selection, voice).
//! 2. **The keymap** — [`keybindings()`], the single table mapping
//!    macOS-standard keystrokes to those actions, scoped by
//!    [`WORKSPACE_CONTEXT`] and [`EDITOR_CONTEXT`].
//! 3. **Native menus** — [`app_menus()`], the macOS menu bar. Menu items
//!    dispatch the very same actions the keymap binds, so the two surfaces
//!    cannot drift apart.
//!
//! The editor and workspace entities implement the handlers. This crate must
//! not know about documents, pixels, or persistence.

mod actions;
mod keymap;
mod menus;

pub use actions::*;
pub use keymap::{EDITOR_CONTEXT, WORKSPACE_CONTEXT, keybindings};
pub use menus::app_menus;
