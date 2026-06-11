//! Muse — a quiet place to write, with a companion in the margin.
//!
//! The composition root. `main` owns the startup sequence only: logging,
//! assets, fonts, storage, the theme global, the keymap and menus, the api
//! worker, and the one window. Everything stateful lives in
//! [`workspace::Workspace`]; this file stays thin by design.

mod muse_flow;
mod persistence;
mod settings;
mod workspace;

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Application, Bounds, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, point, px, size,
};
use muse_storage::Store;
use muse_theme::Theme;

use crate::persistence::Boot;
use crate::workspace::Workspace;

fn main() {
    init_tracing();

    Application::new()
        .with_assets(muse_ui::assets::MuseAssets)
        .run(|cx: &mut App| {
            // Fonts must exist before the first frame (no-shift doctrine).
            if let Err(err) = cx.text_system().add_fonts(muse_ui::fonts::all()) {
                tracing::error!(%err, "failed to register bundled fonts");
            }

            let store = open_store();
            let boot = Boot::load(&store);

            // The theme global must exist before any view renders, dressed
            // in the persisted preset/custom pair.
            cx.set_global(Theme {
                appearance: boot.appearance,
                tokens: boot.pair.tokens_for(boot.appearance),
            });
            cx.bind_keys(muse_commands::keybindings());
            cx.bind_keys(muse_ui::text_field_bindings());
            // Settings closes on Escape from anywhere in the workspace
            // (the editor's own Escape binding wins while it has focus).
            cx.bind_keys([gpui::KeyBinding::new(
                "escape",
                muse_commands::Cancel,
                Some(muse_commands::WORKSPACE_CONTEXT),
            )]);
            cx.set_menus(muse_commands::app_menus());

            let api = muse_api::spawn();

            let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::new_static("Muse")),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(18.))),
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(720.), px(480.))),
                ..Default::default()
            };

            // Muse is a single-window app: closing the window quits.
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let opened = cx.open_window(options, |window, cx| {
                cx.new(|cx| Workspace::new(store, api, boot, window, cx))
            });
            if let Err(err) = opened {
                tracing::error!(%err, "failed to open the main window");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
}

/// Logging to `/tmp/muse-debug.log` (truncated on each launch) so it's
/// readable even when the process is started as a .app bundle.
/// `RUST_LOG` overrides the default filter.
fn init_tracing() {
    use std::fs::OpenOptions;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "muse=debug,muse_api=debug,muse_agent=debug,muse_local=debug,muse_storage=info",
        )
    });

    // Truncate the log on each fresh launch, then open for appending.
    let _ = std::fs::write("/tmp/muse-debug.log", "");
    if let Ok(file) = OpenOptions::new().append(true).open("/tmp/muse-debug.log") {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(file)
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init();
    }
}

/// Open the on-disk store, falling back to an in-memory database rather than
/// crashing; only a doubly-broken environment exits, and it does so cleanly.
fn open_store() -> Arc<Store> {
    match Store::open_default() {
        Ok(store) => Arc::new(store),
        Err(err) => {
            tracing::error!(%err, "could not open the local database; using memory");
            match Store::open_in_memory() {
                Ok(store) => Arc::new(store),
                Err(err) => {
                    tracing::error!(%err, "could not open an in-memory database");
                    eprintln!("muse: local storage is unavailable");
                    std::process::exit(1);
                }
            }
        }
    }
}
