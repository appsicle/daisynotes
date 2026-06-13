//! The keymap: one declarative table from macOS-standard keystrokes to
//! actions. The native menus display these same bindings, so this table is
//! the single source of truth for every shortcut in Muse.

use gpui::KeyBinding;

use crate::actions::{
    Backspace, Bold, Cancel, Copy, Cut, DecreaseSize, Delete, DeleteToLineStart,
    DeleteWordBackward, IncreaseSize, InsertNewline, Italic, MoveDown, MoveLeft, MoveRight,
    MoveToEnd, MoveToLineEnd, MoveToLineStart, MoveToStart, MoveUp, MoveWordLeft, MoveWordRight,
    MuseNow, NewEntry, OpenSettings, Paste, Quit, Redo, SelectAll, SelectDown, SelectLeft,
    SelectRight,
    SelectToEnd, SelectToLineEnd, SelectToLineStart, SelectToStart, SelectUp, SelectWordLeft,
    SelectWordRight, Strikethrough, ToggleMuseMuted, ToggleSidebar, ToggleTheme, Underline, Undo,
};

/// Key context the editor sets on its focusable element. Editor-scope
/// bindings only fire while the caret lives in the page.
pub const EDITOR_CONTEXT: &str = "DaisyNotesEditor";

/// Key context the workspace sets on its root element. Workspace-scope
/// bindings fire anywhere inside the window — including while the editor has
/// focus, since the editor is a descendant of the workspace.
pub const WORKSPACE_CONTEXT: &str = "DaisyNotesWorkspace";

/// The complete key binding table for Muse, ready for
/// `gpui::App::bind_keys`.
///
/// Quit is bound without a context so ⌘Q works even when nothing has focus;
/// everything else is scoped to [`WORKSPACE_CONTEXT`] or [`EDITOR_CONTEXT`].
pub fn keybindings() -> Vec<KeyBinding> {
    let workspace = Some(WORKSPACE_CONTEXT);
    let editor = Some(EDITOR_CONTEXT);
    vec![
        // -- Workspace ------------------------------------------------------
        KeyBinding::new("cmd-n", NewEntry, workspace),
        KeyBinding::new("cmd-\\", ToggleSidebar, workspace),
        KeyBinding::new("cmd-j", MuseNow, workspace),
        KeyBinding::new("cmd-shift-j", ToggleMuseMuted, workspace),
        KeyBinding::new("cmd-shift-d", ToggleTheme, workspace),
        KeyBinding::new("cmd-,", OpenSettings, workspace),
        KeyBinding::new("cmd-q", Quit, None),
        // -- Editor: formatting ---------------------------------------------
        KeyBinding::new("cmd-b", Bold, editor),
        KeyBinding::new("cmd-i", Italic, editor),
        KeyBinding::new("cmd-u", Underline, editor),
        KeyBinding::new("cmd-shift-x", Strikethrough, editor),
        KeyBinding::new("cmd-=", IncreaseSize, editor),
        KeyBinding::new("cmd-+", IncreaseSize, editor),
        KeyBinding::new("cmd--", DecreaseSize, editor),
        // -- Editor: history -------------------------------------------------
        KeyBinding::new("cmd-z", Undo, editor),
        KeyBinding::new("cmd-shift-z", Redo, editor),
        // -- Editor: clipboard ------------------------------------------------
        KeyBinding::new("cmd-c", Copy, editor),
        KeyBinding::new("cmd-x", Cut, editor),
        KeyBinding::new("cmd-v", Paste, editor),
        KeyBinding::new("cmd-a", SelectAll, editor),
        // -- Editor: deletion -------------------------------------------------
        KeyBinding::new("backspace", Backspace, editor),
        KeyBinding::new("delete", Delete, editor),
        KeyBinding::new("alt-backspace", DeleteWordBackward, editor),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, editor),
        // -- Editor: insertion & escape ---------------------------------------
        KeyBinding::new("enter", InsertNewline, editor),
        KeyBinding::new("escape", Cancel, editor),
        // -- Editor: caret motion ----------------------------------------------
        KeyBinding::new("left", MoveLeft, editor),
        KeyBinding::new("right", MoveRight, editor),
        KeyBinding::new("up", MoveUp, editor),
        KeyBinding::new("down", MoveDown, editor),
        KeyBinding::new("alt-left", MoveWordLeft, editor),
        KeyBinding::new("alt-right", MoveWordRight, editor),
        KeyBinding::new("cmd-left", MoveToLineStart, editor),
        KeyBinding::new("cmd-right", MoveToLineEnd, editor),
        KeyBinding::new("cmd-up", MoveToStart, editor),
        KeyBinding::new("cmd-down", MoveToEnd, editor),
        // -- Editor: selection --------------------------------------------------
        KeyBinding::new("shift-left", SelectLeft, editor),
        KeyBinding::new("shift-right", SelectRight, editor),
        KeyBinding::new("shift-up", SelectUp, editor),
        KeyBinding::new("shift-down", SelectDown, editor),
        KeyBinding::new("alt-shift-left", SelectWordLeft, editor),
        KeyBinding::new("alt-shift-right", SelectWordRight, editor),
        KeyBinding::new("cmd-shift-left", SelectToLineStart, editor),
        KeyBinding::new("cmd-shift-right", SelectToLineEnd, editor),
        KeyBinding::new("cmd-shift-up", SelectToStart, editor),
        KeyBinding::new("cmd-shift-down", SelectToEnd, editor),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Expands to an array of registered action names for the given types.
    macro_rules! action_names {
        ($($action:ty),* $(,)?) => {
            [$(<$action as gpui::Action>::name_for_type()),*]
        };
    }

    #[test]
    fn keybindings_builds() {
        // 7 workspace bindings + 39 editor bindings (IncreaseSize is bound
        // twice: cmd-= and cmd-+).
        assert_eq!(keybindings().len(), 46);
    }

    #[test]
    fn every_editor_action_is_bound() {
        let bound: HashSet<&str> = keybindings()
            .iter()
            .map(|binding| binding.action().name())
            .collect();
        let expected = action_names![
            Bold,
            Italic,
            Underline,
            Strikethrough,
            IncreaseSize,
            DecreaseSize,
            Undo,
            Redo,
            Copy,
            Cut,
            Paste,
            SelectAll,
            Backspace,
            Delete,
            DeleteWordBackward,
            DeleteToLineStart,
            InsertNewline,
            Cancel,
            MoveLeft,
            MoveRight,
            MoveUp,
            MoveDown,
            MoveWordLeft,
            MoveWordRight,
            MoveToLineStart,
            MoveToLineEnd,
            MoveToStart,
            MoveToEnd,
            SelectLeft,
            SelectRight,
            SelectUp,
            SelectDown,
            SelectWordLeft,
            SelectWordRight,
            SelectToLineStart,
            SelectToLineEnd,
            SelectToStart,
            SelectToEnd,
        ];
        assert_eq!(expected.len(), 38);
        for name in expected {
            assert!(bound.contains(name), "no keybinding for {name}");
        }
    }

    #[test]
    fn every_workspace_action_is_bound() {
        let bound: HashSet<&str> = keybindings()
            .iter()
            .map(|binding| binding.action().name())
            .collect();
        let expected = action_names![
            NewEntry,
            ToggleSidebar,
            ToggleTheme,
            MuseNow,
            ToggleMuseMuted,
            Quit,
        ];
        for name in expected {
            assert!(bound.contains(name), "no keybinding for {name}");
        }
    }
}
