//! The `Aa` popover — the entry's voice controls: family, size, weight.
//!
//! The popover floats below-right of the `Aa` button via `deferred` +
//! `anchored`, over a full-window backdrop that dismisses it on
//! click-away (the dismissing click is swallowed, macOS-popover style).
//! Edits emit [`TopbarEvent`]s and keep the popover open.

use gpui::{
    AnyElement, ClickEvent, Context, Corner, Div, KeyDownEvent, Pixels, Size, Stateful, anchored,
    deferred, div, point, prelude::*, px,
};
use muse_core::FontFamily;
use muse_theme::{ActiveTheme, FONT_MONO, FONT_QUATTRO, FONT_SANS, FONT_SERIF, layout};
use muse_ui::{IconName, card, divider, icon, text_button};

use crate::{Topbar, TopbarEvent, steps};

/// Inner row width; with the card's 12px padding on each side this yields
/// the ~260px popover from the spec.
const ROW_W: f32 = 236.0;

/// Display label and registered font name for each family row.
const FAMILIES: [(FontFamily, &str, &str); 4] = [
    (FontFamily::Literata, "Literata", FONT_SERIF),
    (FontFamily::Inter, "Inter", FONT_SANS),
    (FontFamily::Quattro, "iA Writer Quattro", FONT_QUATTRO),
    (FontFamily::Mono, "JetBrains Mono", FONT_MONO),
];

impl Topbar {
    /// The `Aa` button plus, when open, its anchored popover and the
    /// click-away backdrop.
    pub(crate) fn render_aa_control(&self, viewport: Size<Pixels>, cx: &mut Context<Self>) -> Div {
        let tokens = cx.theme().tokens;
        let open = self.popover_open;
        let hover_bg = tokens.hairline.opacity(0.6);
        let pressed_bg = tokens.hairline;

        let button = div()
            .id("aa-button")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(28.))
            .rounded(px(layout::RADIUS_SM))
            .text_size(px(layout::UI_TEXT))
            .text_color(if open {
                tokens.ink
            } else {
                tokens.ink_secondary
            })
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg).text_color(tokens.ink))
            .active(move |style| style.bg(pressed_bg))
            .when(open, |this| this.bg(hover_bg))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                if this.popover_open {
                    this.dismiss_popover(window, cx);
                } else {
                    this.open_popover(window, cx);
                }
            }))
            .child("Aa");

        div().relative().child(button).when(open, |this| {
            this.child(self.render_backdrop(viewport, cx))
                .child(self.render_popover(cx))
        })
    }

    /// A full-window invisible layer under the popover: any press on it
    /// dismisses (and the press goes no further).
    fn render_backdrop(&self, viewport: Size<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id("aa-backdrop")
                    .w(viewport.width)
                    .h(viewport.height)
                    .occlude()
                    .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                        this.dismiss_popover(window, cx);
                    })),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }

    /// The popover card, anchored below the `Aa` button with its right
    /// edge aligned to the button's.
    fn render_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let font_rows = FAMILIES
            .iter()
            .enumerate()
            .map(|(ix, &(family, label, font))| {
                self.font_row(ix, family, label, font, cx)
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        deferred(
            anchored()
                .anchor(Corner::TopRight)
                // The button is 28px square; 6px of air below it.
                .offset(point(px(28.), px(34.)))
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .occlude()
                        .track_focus(&self.popover_focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.dismiss_popover(window, cx);
                            }
                        }))
                        .child(
                            card().child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .children(font_rows)
                                    .child(div().w(px(ROW_W)).py(px(6.)).child(divider()))
                                    .child(self.size_row(cx))
                                    .child(self.weight_row(cx)),
                            ),
                        ),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// One font family row, its name set in that family.
    fn font_row(
        &self,
        ix: usize,
        family: FontFamily,
        label: &'static str,
        font: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let tokens = cx.theme().tokens;
        let active = self.voice.family == family;
        let hover_bg = tokens.hairline.opacity(0.5);

        div()
            .id(("aa-font-row", ix))
            .flex()
            .items_center()
            .justify_between()
            .w(px(ROW_W))
            .h(px(32.))
            .px(px(8.))
            .rounded(px(layout::RADIUS_SM))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                this.voice.family = family;
                cx.emit(TopbarEvent::SetFamily(family));
                cx.notify();
            }))
            .child(
                div()
                    .font_family(font)
                    .text_size(px(15.))
                    .text_color(tokens.ink)
                    .child(label),
            )
            .when(active, |this| {
                this.child(
                    icon(IconName::Check)
                        .size(px(14.))
                        .color(tokens.ink_secondary),
                )
            })
    }

    /// "Size" label, − / + steppers, and the current size in the mono face.
    fn size_row(&self, cx: &mut Context<Self>) -> Div {
        let tokens = cx.theme().tokens;
        let size = self.voice.size;
        let down = steps::prev_size(size);
        let up = steps::next_size(size);

        div()
            .flex()
            .items_center()
            .w(px(ROW_W))
            .h(px(32.))
            .px(px(8.))
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(layout::UI_HEADER))
                    .text_color(tokens.ink_tertiary)
                    .child("SIZE"),
            )
            .child(div().flex_1())
            .child(
                text_button("aa-size-down", "−")
                    .disabled(steps::same_step(down, size))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.voice.size = down;
                        cx.emit(TopbarEvent::SetSize(down));
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .w(px(26.))
                    .flex()
                    .justify_center()
                    .font_family(FONT_MONO)
                    .text_size(px(layout::UI_TEXT))
                    .text_color(tokens.ink)
                    .child(format!("{}", size.round() as i32)),
            )
            .child(
                text_button("aa-size-up", "+")
                    .disabled(steps::same_step(up, size))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.voice.size = up;
                        cx.emit(TopbarEvent::SetSize(up));
                        cx.notify();
                    })),
            )
    }

    /// "Weight" label and the five-stop control: rings whose stroke grows
    /// with the weight they set.
    fn weight_row(&self, cx: &mut Context<Self>) -> Div {
        let tokens = cx.theme().tokens;
        let active = steps::nearest_weight_stop(self.voice.weight);
        let hover_bg = tokens.hairline.opacity(0.5);

        let stops = steps::WEIGHT_STOPS
            .iter()
            .enumerate()
            .map(|(ix, &stop)| {
                let selected = stop == active;
                #[allow(clippy::cast_precision_loss)] // ix ∈ 0..5
                let stroke = 1.0 + ix as f32 * 0.4;
                div()
                    .id(("aa-weight-stop", ix))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(24.))
                    .rounded_full()
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.voice.weight = stop;
                        cx.emit(TopbarEvent::SetWeight(stop));
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(11.))
                            .rounded_full()
                            .border(px(stroke))
                            .border_color(if selected {
                                tokens.ink
                            } else {
                                tokens.ink_tertiary
                            })
                            .when(selected, |dot| dot.bg(tokens.ink.opacity(0.2))),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .flex()
            .items_center()
            .w(px(ROW_W))
            .h(px(32.))
            .px(px(8.))
            .child(
                div()
                    .text_size(px(layout::UI_HEADER))
                    .text_color(tokens.ink_tertiary)
                    .child("WEIGHT"),
            )
            .child(div().flex_1())
            .child(div().flex().items_center().gap(px(2.)).children(stops))
    }
}
