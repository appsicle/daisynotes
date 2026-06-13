//! Compile-time embedded assets served to GPUI through [`gpui::AssetSource`].
//!
//! GPUI's SVG renderer loads icon bytes from the registered asset source,
//! rasterizes them, and keeps only the alpha channel as a mask tinted by
//! the element's text color (see gpui `svg_renderer.rs`). Icons are
//! therefore monochrome by construction and tinted with `text_color`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Every icon shipped with the app, embedded at compile time.
///
/// Lucide icons, ISC license (<https://lucide.dev/license>).
static ICONS: &[(&str, &[u8])] = &[
    (
        "icons/check.svg",
        include_bytes!("../../../assets/icons/check.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../../../assets/icons/chevron-down.svg"),
    ),
    (
        "icons/cloud-off.svg",
        include_bytes!("../../../assets/icons/cloud-off.svg"),
    ),
    (
        "icons/cloud.svg",
        include_bytes!("../../../assets/icons/cloud.svg"),
    ),
    (
        "icons/moon.svg",
        include_bytes!("../../../assets/icons/moon.svg"),
    ),
    (
        "icons/panel-left.svg",
        include_bytes!("../../../assets/icons/panel-left.svg"),
    ),
    (
        "icons/plus.svg",
        include_bytes!("../../../assets/icons/plus.svg"),
    ),
    (
        "icons/sun.svg",
        include_bytes!("../../../assets/icons/sun.svg"),
    ),
    (
        "icons/trash-2.svg",
        include_bytes!("../../../assets/icons/trash-2.svg"),
    ),
    (
        "icons/settings.svg",
        include_bytes!("../../../assets/icons/settings.svg"),
    ),
    (
        "icons/undo-2.svg",
        include_bytes!("../../../assets/icons/undo-2.svg"),
    ),
    ("icons/x.svg", include_bytes!("../../../assets/icons/x.svg")),
    // The full-color app mark (raster — rendered via `img`, not the SVG mask
    // path, so it keeps its color instead of being tinted monochrome).
    (
        "icons/daisynotes-mark.png",
        include_bytes!("../../../assets/icons/daisynotes-mark.png"),
    ),
];

/// The app's asset source. Register at startup with
/// `gpui::Application::new().with_assets(DaisyNotesAssets)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DaisyNotesAssets;

impl AssetSource for DaisyNotesAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|&(_, bytes)| Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|&(name, _)| SharedString::new_static(name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_every_embedded_icon() {
        for &(name, bytes) in ICONS {
            let loaded = DaisyNotesAssets.load(name);
            assert!(
                matches!(loaded, Ok(Some(ref data)) if data.as_ref() == bytes),
                "failed to load {name}"
            );
        }
    }

    #[test]
    fn unknown_paths_load_as_none() {
        assert!(matches!(DaisyNotesAssets.load("icons/missing.svg"), Ok(None)));
        assert!(matches!(DaisyNotesAssets.load(""), Ok(None)));
    }

    #[test]
    fn lists_icons_under_prefix() {
        let listed = DaisyNotesAssets.list("icons/");
        assert!(matches!(&listed, Ok(names) if names.len() == ICONS.len()));
        assert!(matches!(DaisyNotesAssets.list("fonts/"), Ok(names) if names.is_empty()));
    }

    #[test]
    fn icons_look_like_lucide_svgs() {
        for &(name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes);
            assert!(
                matches!(text, Ok(svg) if svg.contains("<svg") && svg.contains("viewBox=\"0 0 24 24\"")),
                "{name} is not a 24x24 svg"
            );
        }
    }
}
