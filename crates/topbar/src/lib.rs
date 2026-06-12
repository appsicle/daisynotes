//! muse-topbar — the chrome strip across the top of the window: sidebar
//! toggle on the left, a calm empty center, and the right cluster — the
//! `Aa` voice popover and the theme toggle.
//!
//! This crate owns presentation and dispatch only. Sidebar / theme
//! clicks dispatch `muse-commands` actions for the app to handle; edits
//! made in the `Aa` popover are emitted as [`TopbarEvent`]s for the app to
//! forward to the editor. It must not know about documents, entries,
//! storage, or the agent.

use gpui::{Context, EventEmitter, MouseButton, Window, div, prelude::*, px};
use muse_commands::ToggleSidebar;
use muse_core::FontFamily;
use muse_theme::{Appearance, layout};
use muse_ui::{IconName, icon_button};

/// The agent's state machine, seen from the chrome (PLAN §2). The orb that
/// once visualized this has been removed; the type remains so the app's
/// wiring keeps compiling, and so a future presence indicator can reuse it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrbState {
    /// Muse is idle alongside you — slow 4s breathing.
    #[default]
    Resting,
    /// Muse is reading the entry — a slightly quicker shimmer.
    Reading,
    /// Muse is considering — a gentle 1.2s pulse, brighter glow.
    Thinking,
    /// Muse left a note — steady, with a small accent dot.
    HasNote,
}

/// Edits made inside the `Aa` popover. The topbar never touches the
/// document; the app forwards these to the editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TopbarEvent {
    /// A font family row was chosen.
    SetFamily(FontFamily),
    /// The size stepper picked a new base size (always one of
    /// [`muse_core::SIZE_STEPS`]).
    SetSize(f32),
    /// A weight stop was chosen (300–700).
    SetWeight(u16),
}

/// The topbar entity: 52px of transparent chrome over the window
/// background. The app keeps it current via the `set_*` methods and
/// subscribes to [`TopbarEvent`] for voice edits.
pub struct Topbar {
    pub(crate) scrolled: bool,
    /// While the sidebar is open its header carries the toggle button,
    /// so the topbar hides its own.
    sidebar_open: bool,
}

impl EventEmitter<TopbarEvent> for Topbar {}

impl Topbar {
    /// A topbar at rest: unscrolled, sidebar open.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Topbar {
            scrolled: false,
            sidebar_open: false,
        }
    }

    /// Accepted for API compatibility; the topbar no longer renders an
    /// orb, so agent-state changes are a no-op here.
    pub fn set_orb(&mut self, _state: OrbState, _cx: &mut Context<Self>) {}

    /// Accepted for API compatibility; the topbar no longer renders an
    /// orb, so mute changes are a no-op here.
    pub fn set_muted(&mut self, _muted: bool, _cx: &mut Context<Self>) {}

    /// Accepted for API compatibility; the theme toggle moved into the
    /// settings pane, so appearance changes are a no-op here.
    pub fn set_appearance(&mut self, _appearance: Appearance, _cx: &mut Context<Self>) {}

    /// Whether content has scrolled under the topbar. The old bottom
    /// hairline is gone — the editor pane fades its own top edge — so this
    /// is state-only now, kept for the app's wiring.
    pub fn set_scrolled(&mut self, scrolled: bool, cx: &mut Context<Self>) {
        if self.scrolled != scrolled {
            self.scrolled = scrolled;
            cx.notify();
        }
    }

    /// Whether the sidebar is open; hides the topbar's toggle button
    /// while the sidebar header shows its own.
    pub fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar_open != open {
            self.sidebar_open = open;
            cx.notify();
        }
    }

    /// Accepted for API compatibility; the `Aa` popover moved onto the
    /// selection pill, so there is nothing to dismiss.
    pub fn dismiss_popover(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

}

impl Render for Topbar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(layout::TOPBAR_H))
            // The traffic lights live in the first 80px.
            .pl(px(80.))
            .pr(px(12.))
            // Double-clicking empty chrome performs the native titlebar
            // action (zoom by default, per the user's macOS preference).
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                }
            })
            // When the sidebar is open, its own header carries the toggle.
            .when(!self.sidebar_open, |this| {
                this.child(icon_button("toggle-sidebar", IconName::PanelLeft).on_click(
                    |_, window, cx| window.dispatch_action(Box::new(ToggleSidebar), cx),
                ))
            })
            // The center (and right) stay empty on purpose. Calm.
            .child(div().flex_1())
    }
}
