//! An animated on/off toggle for settings. The knob slides
//! along [`daisynotes_theme::motion::MOVE`] with the spring-settle curve.

use gpui::{
    Animation, AnimationExt as _, App, BoxShadow, ElementId, Window, div, point, prelude::*, px,
};
use daisynotes_theme::{ActiveTheme, Appearance, motion};

const TRACK_W: f32 = 34.0;
const TRACK_H: f32 = 20.0;
const TRACK_PAD: f32 = 2.0;
const KNOB: f32 = TRACK_H - 2.0 * TRACK_PAD;
const TRAVEL: f32 = TRACK_W - KNOB - 2.0 * TRACK_PAD;

/// Handler invoked with the value the switch wants to become.
type ChangeHandler = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// An animated on/off toggle. Stateless: the caller owns `checked` and
/// flips it in [`Switch::on_change`].
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

/// Build a [`Switch`] reflecting `checked`.
pub fn switch(id: impl Into<ElementId>, checked: bool) -> Switch {
    Switch {
        id: id.into(),
        checked,
        disabled: false,
        on_change: None,
    }
}

impl Switch {
    /// Handle toggles; the argument is the requested new value.
    #[must_use]
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }

    /// Disable interaction and dim the control.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let tokens = theme.tokens;
        // Both palettes want a light knob: paper-white in Paper, warm
        // off-white ink in Dusk (Dusk's surface would vanish on the track).
        let knob_color = match theme.appearance {
            Appearance::Paper => tokens.surface,
            Appearance::Dusk => tokens.ink,
        };
        let track_color = if self.checked {
            tokens.accent
        } else {
            tokens.hairline
        };
        let checked = self.checked;
        let (from, to) = if checked {
            (0.0, TRAVEL)
        } else {
            (TRAVEL, 0.0)
        };

        let knob = div()
            .flex_none()
            .size(px(KNOB))
            .rounded_full()
            .bg(knob_color)
            .shadow(vec![BoxShadow {
                color: tokens.shadow,
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
            }])
            // Keying the animation id on `checked` restarts the slide
            // exactly when the value flips; afterwards the finished
            // animation pins the knob at its destination.
            .with_animation(
                ElementId::NamedInteger("muse-switch-knob".into(), u64::from(checked)),
                Animation::new(motion::MOVE).with_easing(motion::spring),
                move |knob, t| knob.ml(px(from + (to - from) * t)),
            );

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .w(px(TRACK_W))
            .h(px(TRACK_H))
            .p(px(TRACK_PAD))
            .rounded_full()
            .bg(track_color)
            .when(self.disabled, |this| this.opacity(0.4))
            .when(!self.disabled, gpui::Styled::cursor_pointer)
            .when_some(
                self.on_change.filter(|_| !self.disabled),
                |this, on_change| {
                    this.on_click(move |_, window, cx| on_change(&!checked, window, cx))
                },
            )
            .child(knob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_geometry_fits_the_track() {
        // The knob plus padding must exactly fill the track height, and the
        // travel distance keeps the knob inside the track at both ends.
        assert!((KNOB + 2.0 * TRACK_PAD - TRACK_H).abs() < f32::EPSILON);
        assert!((TRACK_PAD + KNOB + TRAVEL + TRACK_PAD - TRACK_W).abs() < f32::EPSILON);
        const { assert!(TRAVEL > 0.0) };
    }
}
