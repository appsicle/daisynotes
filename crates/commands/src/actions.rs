//! Every user intent in Muse, declared once as a gpui action.
//!
//! Workspace-scope actions live in the `muse::` namespace; editor-scope
//! actions live in the `editor::` namespace. No other crate may declare
//! actions — handlers belong to the workspace and editor entities, but the
//! vocabulary belongs here.

// ---------------------------------------------------------------------------
// Workspace-scope unit actions (handled by the workspace root entity).
// ---------------------------------------------------------------------------

gpui::actions!(
    muse,
    [
        /// Shows the standard About panel for Muse.
        About,
        /// Creates a new entry and moves focus to its first line.
        NewEntry,
        /// Shows or hides the entries sidebar, re-centering the page.
        ToggleSidebar,
        /// Switches between the Paper (light) and Dusk (dark) themes.
        ToggleTheme,
        /// Asks Muse to read the current entry now, without waiting for a pause.
        MuseNow,
        /// Mutes or unmutes Muse for the current entry.
        ToggleMuseMuted,
        /// Opens or closes the settings pane.
        OpenSettings,
        /// Quits Muse.
        Quit,
    ]
);

// ---------------------------------------------------------------------------
// Editor-scope unit actions (handled by the editor entity).
// ---------------------------------------------------------------------------

gpui::actions!(
    editor,
    [
        /// Toggles bold on the selected text.
        Bold,
        /// Toggles italic on the selected text.
        Italic,
        /// Toggles underline on the selected text.
        Underline,
        /// Toggles strikethrough on the selected text.
        Strikethrough,
        /// Steps the entry's base font size up one notch.
        IncreaseSize,
        /// Steps the entry's base font size down one notch.
        DecreaseSize,
        /// Undoes the last edit group.
        Undo,
        /// Redoes the last undone edit group.
        Redo,
        /// Copies the selection to the clipboard.
        Copy,
        /// Cuts the selection to the clipboard.
        Cut,
        /// Pastes the clipboard at the caret.
        Paste,
        /// Selects the whole entry.
        SelectAll,
        /// Deletes the character before the caret, or the selection.
        Backspace,
        /// Deletes the character after the caret, or the selection.
        Delete,
        /// Deletes the word before the caret.
        DeleteWordBackward,
        /// Deletes from the caret to the start of the line.
        DeleteToLineStart,
        /// Inserts a newline at the caret.
        InsertNewline,
        /// Cancels the current transient state (selection, overlay).
        Cancel,
        /// Moves the caret one character left.
        MoveLeft,
        /// Moves the caret one character right.
        MoveRight,
        /// Moves the caret one display line up.
        MoveUp,
        /// Moves the caret one display line down.
        MoveDown,
        /// Moves the caret one word left.
        MoveWordLeft,
        /// Moves the caret one word right.
        MoveWordRight,
        /// Moves the caret to the start of the line.
        MoveToLineStart,
        /// Moves the caret to the end of the line.
        MoveToLineEnd,
        /// Moves the caret to the start of the entry.
        MoveToStart,
        /// Moves the caret to the end of the entry.
        MoveToEnd,
        /// Extends the selection one character left.
        SelectLeft,
        /// Extends the selection one character right.
        SelectRight,
        /// Extends the selection one display line up.
        SelectUp,
        /// Extends the selection one display line down.
        SelectDown,
        /// Extends the selection one word left.
        SelectWordLeft,
        /// Extends the selection one word right.
        SelectWordRight,
        /// Extends the selection to the start of the line.
        SelectToLineStart,
        /// Extends the selection to the end of the line.
        SelectToLineEnd,
        /// Extends the selection to the start of the entry.
        SelectToStart,
        /// Extends the selection to the end of the entry.
        SelectToEnd,
    ]
);

// ---------------------------------------------------------------------------
// Parameterized editor-scope actions (dispatched by the format pill and the
// `Aa` voice popover).
// ---------------------------------------------------------------------------

/// Applies one of the four ink-palette colors to the selected text.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde::Deserialize,
    gpui::private::schemars::JsonSchema,
    gpui::Action,
)]
#[action(namespace = editor)]
#[schemars(crate = "gpui::private::schemars")]
pub struct SetInk {
    /// Index into the 4-color ink palette; `None` clears back to plain ink.
    pub ink: Option<u8>,
}

/// Sets the entry's voice font family.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde::Deserialize,
    gpui::private::schemars::JsonSchema,
    gpui::Action,
)]
#[action(namespace = editor)]
#[schemars(crate = "gpui::private::schemars")]
pub struct SetFamily {
    /// Font family index: 0 Literata, 1 Inter, 2 iA Writer Quattro,
    /// 3 JetBrains Mono.
    pub family: u8,
}

/// Sets the entry's voice base weight (variable-font axis, 300–700).
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde::Deserialize,
    gpui::private::schemars::JsonSchema,
    gpui::Action,
)]
#[action(namespace = editor)]
#[schemars(crate = "gpui::private::schemars")]
pub struct SetWeight {
    /// The base font weight to apply to the whole entry.
    pub weight: u16,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::private::serde_json::json;

    fn build<A: gpui::Action>(value: gpui::private::serde_json::Value) -> Box<dyn gpui::Action> {
        A::build(value).unwrap_or_else(|error| {
            panic!("{} failed to build from JSON: {error}", A::name_for_type())
        })
    }

    #[test]
    fn parameterized_actions_build_from_json() {
        let ink = build::<SetInk>(json!({ "ink": 2 }));
        assert!(ink.partial_eq(&SetInk { ink: Some(2) }));

        let cleared = build::<SetInk>(json!({ "ink": null }));
        assert!(cleared.partial_eq(&SetInk { ink: None }));

        let family = build::<SetFamily>(json!({ "family": 3 }));
        assert!(family.partial_eq(&SetFamily { family: 3 }));

        let weight = build::<SetWeight>(json!({ "weight": 450 }));
        assert!(weight.partial_eq(&SetWeight { weight: 450 }));
    }

    #[test]
    fn action_names_are_namespaced() {
        assert_eq!(
            <NewEntry as gpui::Action>::name_for_type(),
            "muse::NewEntry"
        );
        assert_eq!(<Quit as gpui::Action>::name_for_type(), "muse::Quit");
        assert_eq!(<Bold as gpui::Action>::name_for_type(), "editor::Bold");
        assert_eq!(<SetInk as gpui::Action>::name_for_type(), "editor::SetInk");
    }
}
