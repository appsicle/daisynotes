//! muse-topbar — the chrome strip across the top of the window: sidebar
//! toggle on the left, a calm empty center, and the right cluster — the
//! `Aa` voice popover and the theme toggle.
//!
//! This crate owns presentation and dispatch only. Sidebar / theme
//! clicks dispatch `muse-commands` actions for the app to handle; edits
//! made in the `Aa` popover are emitted as [`TopbarEvent`]s for the app to
//! forward to the editor. It must not know about documents, entries,
//! storage, or the agent.

mod popover;
mod steps;

use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, ElementId, EventEmitter, FocusHandle, Hsla,
    MouseButton, Window, div, prelude::*, px, svg,
};
use muse_commands::{ToggleSidebar, ToggleTheme};
use muse_core::{FontFamily, Voice};
use muse_theme::{ActiveTheme, Appearance, layout, motion};
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
    /// The current entry's voice, mirrored into the `Aa` popover.
    /// Updated optimistically by popover edits and authoritatively by
    /// [`Topbar::set_voice`].
    pub(crate) voice: Voice,
    pub(crate) appearance: Appearance,
    pub(crate) scrolled: bool,
    /// While the sidebar is open its header carries the toggle button,
    /// so the topbar hides its own.
    sidebar_open: bool,
    pub(crate) popover_open: bool,
    /// Focused while the popover is open so Escape reaches it.
    pub(crate) popover_focus: FocusHandle,
    /// Where focus was before the popover took it; restored on dismissal.
    pub(crate) previous_focus: Option<FocusHandle>,
    /// The appearance as of the last frame; lets the first frame render
    /// without a crossfade even if the app sets Dusk before first paint.
    rendered_appearance: Option<Appearance>,
    /// Same idea for the scrolled hairline.
    rendered_scrolled: Option<bool>,
    /// Becomes true on the first observed appearance change; gates the
    /// icon crossfade so launch is still.
    appearance_animates: bool,
    /// Becomes true on the first observed scroll change; gates the
    /// hairline fade so launch is still.
    hairline_animates: bool,
}

impl EventEmitter<TopbarEvent> for Topbar {}

impl Topbar {
    /// A topbar at rest: default voice, Paper, unscrolled.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Topbar {
            voice: Voice::default(),
            appearance: Appearance::default(),
            scrolled: false,
            sidebar_open: false,
            popover_open: false,
            popover_focus: cx.focus_handle(),
            previous_focus: None,
            rendered_appearance: None,
            rendered_scrolled: None,
            appearance_animates: false,
            hairline_animates: false,
        }
    }

    /// Reflect the current entry's voice in the `Aa` popover.
    pub fn set_voice(&mut self, voice: Voice, cx: &mut Context<Self>) {
        if self.voice != voice {
            self.voice = voice;
            cx.notify();
        }
    }

    /// Accepted for API compatibility; the topbar no longer renders an
    /// orb, so agent-state changes are a no-op here.
    pub fn set_orb(&mut self, _state: OrbState, _cx: &mut Context<Self>) {}

    /// Accepted for API compatibility; the topbar no longer renders an
    /// orb, so mute changes are a no-op here.
    pub fn set_muted(&mut self, _muted: bool, _cx: &mut Context<Self>) {}

    /// Which icon the theme toggle shows: Sun in Dusk (inviting the
    /// light), Moon in Paper.
    pub fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        if self.appearance != appearance {
            self.appearance = appearance;
            cx.notify();
        }
    }

    /// Whether content has scrolled under the topbar; fades the bottom
    /// hairline in and out.
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

    /// Close the `Aa` popover if open, restoring whatever had focus
    /// before it opened. Safe to call any time (e.g. from the app's
    /// Cancel handling).
    pub fn dismiss_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.popover_open {
            return;
        }
        self.popover_open = false;
        if let Some(previous) = self.previous_focus.take() {
            window.focus(&previous);
        }
        cx.notify();
    }

    pub(crate) fn open_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popover_open = true;
        self.previous_focus = window.focused(cx);
        window.focus(&self.popover_focus);
        cx.notify();
    }

    /// The sun/moon toggle: one button, two stacked icons crossfading
    /// over [`motion::FADE`] — pure opacity, no layout change.
    fn render_theme_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        let hover_bg = tokens.hairline.opacity(0.6);
        let pressed_bg = tokens.hairline;
        let appearance = self.appearance;
        // Sun in Dusk (inviting the light), Moon in Paper.
        let (shown, hidden) = match appearance {
            Appearance::Dusk => (IconName::Sun, IconName::Moon),
            Appearance::Paper => (IconName::Moon, IconName::Sun),
        };

        // gpui's `svg` paints only with a color set on the svg element
        // itself — it does not inherit the parent's `text_color` — so each
        // icon carries an explicit tint, brightening to ink while the
        // button (the "theme-toggle" group) is hovered.
        let layer = move |name: IconName| {
            let resting: Hsla = tokens.ink_secondary;
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .flex_none()
                        .size(px(16.))
                        .path(name.path())
                        .text_color(resting)
                        .group_hover("theme-toggle", |style| style.text_color(tokens.ink)),
                )
        };

        let button = div()
            .id("toggle-theme")
            .group("theme-toggle")
            .relative()
            .flex_none()
            .size(px(28.))
            .rounded(px(layout::RADIUS_SM))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
            .active(move |style| style.bg(pressed_bg))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleTheme), cx));

        if self.appearance_animates {
            let key = match appearance {
                Appearance::Paper => 0_u64,
                Appearance::Dusk => 1_u64,
            };
            button
                .child(layer(hidden).with_animation(
                    ElementId::NamedInteger("theme-icon-out".into(), key),
                    Animation::new(motion::FADE).with_easing(motion::ease_in_out),
                    |el, t| el.opacity(1.0 - t),
                ))
                .child(layer(shown).with_animation(
                    ElementId::NamedInteger("theme-icon-in".into(), key),
                    Animation::new(motion::FADE).with_easing(motion::ease_in_out),
                    |el, t| el.opacity(t),
                ))
                .into_any_element()
        } else {
            button.child(layer(shown)).into_any_element()
        }
    }

    /// The bottom-edge hairline, present only once content has scrolled —
    /// a fade over [`motion::FADE`], never a pop.
    fn render_hairline(&self, cx: &mut Context<Self>) -> AnyElement {
        let hairline = cx.theme().tokens.hairline;
        let scrolled = self.scrolled;
        let rule = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(1.))
            .bg(hairline);

        if self.hairline_animates {
            rule.with_animation(
                ElementId::NamedInteger("topbar-hairline".into(), u64::from(scrolled)),
                Animation::new(motion::FADE).with_easing(motion::ease_out_quint),
                move |el, t| el.opacity(if scrolled { t } else { 1.0 - t }),
            )
            .into_any_element()
        } else {
            rule.opacity(if scrolled { 1.0 } else { 0.0 })
                .into_any_element()
        }
    }
}

impl Render for Topbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Animations gate on the first *observed* change so that state set
        // before the first frame (restored theme, restored scroll) renders
        // still, with no launch fades.
        if self
            .rendered_appearance
            .is_some_and(|prev| prev != self.appearance)
        {
            self.appearance_animates = true;
        }
        self.rendered_appearance = Some(self.appearance);
        if self
            .rendered_scrolled
            .is_some_and(|prev| prev != self.scrolled)
        {
            self.hairline_animates = true;
        }
        self.rendered_scrolled = Some(self.scrolled);

        let viewport = window.viewport_size();

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
            // The center stays empty on purpose. Calm.
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.render_aa_control(viewport, cx))
                    .child(self.render_theme_toggle(cx)),
            )
            .child(self.render_hairline(cx))
    }
}
