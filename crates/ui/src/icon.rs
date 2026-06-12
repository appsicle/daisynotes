//! The icon vocabulary: a closed enum over the bundled Lucide subset and a
//! small element that renders one, tinted by text color.

use gpui::{App, Hsla, Pixels, SharedString, Window, prelude::*, px, svg};

/// Every icon the app may draw. A closed set keeps icon usage greppable
/// and guarantees each name resolves to an embedded asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// Sidebar toggle.
    PanelLeft,
    /// New entry.
    Plus,
    /// Paper (light) appearance.
    Sun,
    /// Dusk (dark) appearance.
    Moon,
    /// Dismiss / close.
    X,
    /// Confirmation, synced state.
    Check,
    /// Sync state: connected.
    Cloud,
    /// Sync state: offline.
    CloudOff,
    /// Delete entry.
    Trash,
    /// Undo (soft-delete toast).
    Undo,
    /// Settings gear.
    Settings,
}

impl IconName {
    /// All icons, for exhaustive tests and galleries.
    pub const ALL: [IconName; 11] = [
        IconName::PanelLeft,
        IconName::Plus,
        IconName::Sun,
        IconName::Moon,
        IconName::X,
        IconName::Check,
        IconName::Cloud,
        IconName::CloudOff,
        IconName::Trash,
        IconName::Undo,
        IconName::Settings,
    ];

    /// The asset path served by [`crate::assets::MuseAssets`].
    #[must_use]
    pub fn path(&self) -> SharedString {
        SharedString::new_static(match self {
            IconName::PanelLeft => "icons/panel-left.svg",
            IconName::Plus => "icons/plus.svg",
            IconName::Sun => "icons/sun.svg",
            IconName::Moon => "icons/moon.svg",
            IconName::X => "icons/x.svg",
            IconName::Check => "icons/check.svg",
            IconName::Cloud => "icons/cloud.svg",
            IconName::CloudOff => "icons/cloud-off.svg",
            IconName::Trash => "icons/trash-2.svg",
            IconName::Undo => "icons/undo-2.svg",
            IconName::Settings => "icons/settings.svg",
        })
    }
}

/// A single tinted icon. Defaults to 16px and the inherited text color, so
/// hover states on a parent recolor the icon for free.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Option<Hsla>,
}

/// Build an [`Icon`] element for the given name.
pub fn icon(name: IconName) -> Icon {
    Icon {
        name,
        size: px(16.),
        color: None,
    }
}

impl Icon {
    /// Override the icon's square size (default 16px).
    #[must_use]
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Override the tint. Without this the icon inherits the surrounding
    /// text color.
    #[must_use]
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        svg()
            .flex_none()
            .size(self.size)
            .path(self.name.path())
            .when_some(self.color, |this, color| this.text_color(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::MuseAssets;
    use gpui::AssetSource;

    #[test]
    fn every_icon_resolves_to_an_embedded_asset() {
        for name in IconName::ALL {
            let path = name.path();
            assert!(
                matches!(MuseAssets.load(path.as_ref()), Ok(Some(_))),
                "{name:?} -> {path} is not embedded"
            );
        }
    }
}
