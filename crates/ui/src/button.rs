//! Ghost buttons: transparent at rest, a soft hairline tint on hover,
//! slightly deeper when pressed. Calm, never loud.

use gpui::{App, ClickEvent, ElementId, Pixels, SharedString, Window, div, prelude::*, px};
use daisynotes_theme::{ActiveTheme, layout};

/// Shared click-handler shape for both button kinds.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A square ghost button wrapping a single icon — the topbar's vocabulary.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    name: crate::IconName,
    icon_size: Pixels,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

/// Build an [`IconButton`].
pub fn icon_button(id: impl Into<ElementId>, name: crate::IconName) -> IconButton {
    IconButton {
        id: id.into(),
        name,
        icon_size: px(16.),
        selected: false,
        disabled: false,
        on_click: None,
    }
}

impl IconButton {
    /// Handle clicks. Without a handler the button is inert but still
    /// renders its hover affordance.
    #[must_use]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Render in the toggled-on state (e.g. sidebar visible): tinted
    /// background and full-strength ink.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Disable interaction and dim the button.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Override the inner icon size (default 16px).
    #[must_use]
    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = size;
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = cx.theme().tokens;
        let hover_bg = tokens.hairline.opacity(0.6);
        let pressed_bg = tokens.hairline;

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(28.))
            .rounded(px(layout::RADIUS_SM))
            .text_color(if self.selected {
                tokens.ink
            } else {
                tokens.ink_secondary
            })
            .when(self.selected, |this| this.bg(hover_bg))
            .when(self.disabled, |this| this.opacity(0.4))
            .when(!self.disabled, |this| {
                this.cursor_pointer()
                    .hover(move |style| style.bg(hover_bg).text_color(tokens.ink))
                    .active(move |style| style.bg(pressed_bg))
            })
            .when_some(
                self.on_click.filter(|_| !self.disabled),
                |this, on_click| {
                    this.on_click(move |event, window, cx| on_click(event, window, cx))
                },
            )
            // gpui svg elements do not inherit the parent's text color; the
            // icon must carry its tint explicitly or it renders invisible.
            .child(crate::icon(self.name).size(self.icon_size).color(if self.selected {
                tokens.ink
            } else {
                tokens.ink_secondary
            }))
    }
}

/// A ghost button carrying a short text label.
#[derive(IntoElement)]
pub struct TextButton {
    id: ElementId,
    label: SharedString,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

/// Build a [`TextButton`].
pub fn text_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> TextButton {
    TextButton {
        id: id.into(),
        label: label.into(),
        disabled: false,
        on_click: None,
    }
}

impl TextButton {
    /// Handle clicks.
    #[must_use]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Disable interaction and dim the button.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for TextButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = cx.theme().tokens;
        let hover_bg = tokens.hairline.opacity(0.6);
        let pressed_bg = tokens.hairline;

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(layout::RADIUS_SM))
            .text_size(px(layout::UI_BODY))
            .text_color(tokens.ink_secondary)
            .when(self.disabled, |this| this.opacity(0.4))
            .when(!self.disabled, |this| {
                this.cursor_pointer()
                    .hover(move |style| style.bg(hover_bg).text_color(tokens.ink))
                    .active(move |style| style.bg(pressed_bg))
            })
            .when_some(
                self.on_click.filter(|_| !self.disabled),
                |this, on_click| {
                    this.on_click(move |event, window, cx| on_click(event, window, cx))
                },
            )
            .child(self.label)
    }
}
