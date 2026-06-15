//! The native macOS menu bar. Every item dispatches an action from this
//! crate — the same actions the keymap binds — so menus and shortcuts can
//! never diverge.

use gpui::{Menu, MenuItem, OsAction};

use crate::actions::{
    About, Bold, Copy, Cut, DecreaseSize, IncreaseSize, Italic, MuseNow, NewEntry, Paste, Quit,
    OpenSettings, Redo, SelectAll, Strikethrough, ToggleMuseMuted, ToggleSidebar, ToggleTheme,
    Underline, Undo,
};

/// The menu bar for Daisy Notes, ready for `gpui::App::set_menus`.
///
/// The first menu becomes the application menu (macOS titles it with the app
/// name). Cut/Copy/Paste/Select All/Undo/Redo carry their [`OsAction`] so the
/// system can route them through the native responder chain when appropriate.
pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Daisy Notes".into(),
            items: vec![
                MenuItem::action("About Daisy Notes", About),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Daisy Notes", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("New Entry", NewEntry)],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", Undo, OsAction::Undo),
                MenuItem::os_action("Redo", Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", Cut, OsAction::Cut),
                MenuItem::os_action("Copy", Copy, OsAction::Copy),
                MenuItem::os_action("Paste", Paste, OsAction::Paste),
                MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            ],
        },
        Menu {
            name: "Format".into(),
            items: vec![
                MenuItem::action("Bold", Bold),
                MenuItem::action("Italic", Italic),
                MenuItem::action("Underline", Underline),
                MenuItem::action("Strikethrough", Strikethrough),
                MenuItem::separator(),
                MenuItem::action("Bigger", IncreaseSize),
                MenuItem::action("Smaller", DecreaseSize),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
                MenuItem::action("Toggle Appearance", ToggleTheme),
            ],
        },
        Menu {
            name: "Muse".into(),
            items: vec![
                MenuItem::action("Read Now", MuseNow),
                MenuItem::action("Mute Muse", ToggleMuseMuted),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_menus_builds_with_expected_structure() {
        let menus = app_menus();
        let names: Vec<&str> = menus.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(names, ["Daisy Notes", "File", "Edit", "Format", "View", "Muse"]);

        let action_count: usize = menus
            .iter()
            .flat_map(|menu| menu.items.iter())
            .filter(|item| matches!(item, MenuItem::Action { .. }))
            .count();
        // 3 + 1 + 6 + 6 + 2 + 2 action items (separators excluded).
        assert_eq!(action_count, 20);
    }

    #[test]
    fn menu_actions_use_registered_names() {
        for menu in app_menus() {
            for item in &menu.items {
                if let MenuItem::Action { action, .. } = item {
                    let name = action.name();
                    assert!(
                        name.starts_with("muse::") || name.starts_with("editor::"),
                        "menu action {name} is outside this crate's namespaces"
                    );
                }
            }
        }
    }
}
