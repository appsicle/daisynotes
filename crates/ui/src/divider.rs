//! A 1px hairline divider.

use gpui::{App, Window, div, prelude::*, px};
use daisynotes_theme::ActiveTheme;

/// A hairline rule, horizontal by default.
#[derive(IntoElement)]
pub struct Divider {
    vertical: bool,
}

/// Build a horizontal 1px hairline that fills its container's width.
pub fn divider() -> Divider {
    Divider { vertical: false }
}

impl Divider {
    /// Turn the rule vertical (1px wide, fills the container's height).
    #[must_use]
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hairline = cx.theme().tokens.hairline;
        let rule = div().flex_none().bg(hairline);
        if self.vertical {
            rule.w(px(1.)).h_full()
        } else {
            rule.h(px(1.)).w_full()
        }
    }
}
