//! Floating overlays: the format pill that blooms above a settled
//! selection, and the margin-note card. Both render through
//! `deferred(anchored(...))` so they never affect layout — nothing shifts.

use gpui::{
    Action, Animation, AnimationExt as _, AnyElement, Context, Corner, Div, FontWeight,
    MouseButton, SharedString, Stateful, Window, anchored, deferred, div, point, prelude::*, px,
};
use daisynotes_core::{FontFamily, InlineStyle};
use daisynotes_theme::{ActiveTheme, Tokens, fonts, layout as metrics, motion};
use daisynotes_ui::{IconName, card, icon, pill, soft_shadow};

use crate::notes::{AnnotationTone, reveal_prefix};
use crate::{Editor, PillMenu, notes};

/// The base sizes offered by the size dropdown (points), scrollable.
const SIZE_MENU: [f32; 16] = [
    10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0, 64.0,
];

/// Half of the pill's approximate width, used to center it over the
/// selection before its real layout is known.
const PILL_HALF_W: f32 = 150.0;

/// Build the format pill overlay, if it should be visible.
pub(crate) fn render_pill(
    editor: &Editor,
    _window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    if !editor.pill_shown {
        return None;
    }
    let range = editor.sel.range();
    if range.is_empty() {
        return None;
    }
    let snap = editor.snapshot.clone()?;
    let tokens = cx.theme().tokens;

    let tiles = editor.doc.spans().runs_in(range.clone());
    let entire = |get: fn(&InlineStyle) -> bool| -> bool {
        !tiles.is_empty() && tiles.iter().all(|(_, style)| get(style))
    };
    let bold = entire(|s| s.bold);
    let italic = entire(|s| s.italic);
    let underline = entire(|s| s.underline);
    let strike = entire(|s| s.strike);

    let voice = editor.doc.voice();
    let menu = editor.pill_menu();

    let (start_x, start_y) = snap.caret_point(range.start);
    let anchor_at = snap.to_window((start_x, start_y));
    let position = point(anchor_at.x - px(PILL_HALF_W), anchor_at.y - px(10.0));

    let row = div()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(glyph_toggle("pill-bold", "B", bold, daisynotes_commands::Bold, &tokens, |el| {
            el.font_weight(FontWeight::BOLD)
        }))
        .child(glyph_toggle("pill-italic", "I", italic, daisynotes_commands::Italic, &tokens, |el| {
            el.italic()
        }))
        .child(glyph_toggle(
            "pill-underline",
            "U",
            underline,
            daisynotes_commands::Underline,
            &tokens,
            |el| el.underline(),
        ))
        .child(glyph_toggle(
            "pill-strike",
            "S",
            strike,
            daisynotes_commands::Strikethrough,
            &tokens,
            |el| el.line_through(),
        ))
        .child(seperator(&tokens))
        .child(family_dropdown(voice.family, menu == Some(PillMenu::Family), &tokens, cx))
        .child(seperator(&tokens))
        .child(size_dropdown(voice.size, menu == Some(PillMenu::Size), &tokens, cx));

    let body = pill().child(row);

    let bloom = div().occlude().child(body).with_animation(
        "pill-bloom",
        Animation::new(motion::FADE).with_easing(motion::ease_out_quint),
        |el, t| el.opacity(t).mt(px(3.0 * (1.0 - t))),
    );

    Some(
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::BottomLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(bloom),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// One of the B/I/U/S toggles: the glyph rendered in its own format.
fn glyph_toggle<A: Action + Clone>(
    id: &'static str,
    label: &'static str,
    active: bool,
    action: A,
    tokens: &Tokens,
    format: impl FnOnce(Div) -> Div,
) -> impl IntoElement {
    let hover_bg = tokens.hairline.opacity(0.6);
    let color = if active { tokens.accent } else { tokens.ink_secondary };
    let shell = format(
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(22.0))
            .h(px(22.0))
            .rounded(px(metrics::RADIUS_SM))
            .text_size(px(metrics::UI_SMALL))
            .font_family(fonts::FONT_UI)
            .text_color(color),
    );
    shell
        .id(id)
        .cursor_pointer()
        .when(active, |el| el.bg(tokens.hairline.opacity(0.5)))
        .hover(move |style| style.bg(hover_bg))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            window.dispatch_action(Box::new(action.clone()), cx);
        })
        .child(label)
}

/// The human-readable name of a content font family.
fn family_label(family: FontFamily) -> SharedString {
    SharedString::new_static(match family {
        FontFamily::Literata => "Literata",
        FontFamily::Inter => "Inter",
        FontFamily::Quattro => "iA Writer Quattro",
        FontFamily::Mono => "JetBrains Mono",
    })
}

/// A dropdown trigger: the current value, a chevron, and a click that toggles
/// `kind`'s menu. The menu is appended by the caller as an absolute child.
fn dropdown_trigger(
    id: &'static str,
    label: SharedString,
    open: bool,
    tokens: &Tokens,
    cx: &mut Context<Editor>,
    kind: PillMenu,
) -> Stateful<Div> {
    let hover_bg = tokens.hairline.opacity(0.6);
    let ink = if open { tokens.ink } else { tokens.ink_secondary };
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.0))
        .h(px(22.0))
        .px(px(8.0))
        .rounded(px(metrics::RADIUS_SM))
        .text_size(px(metrics::UI_SMALL))
        .font_family(fonts::FONT_UI)
        .text_color(ink)
        .cursor_pointer()
        .when(open, |el| el.bg(tokens.hairline.opacity(0.5)))
        .hover(move |style| style.bg(hover_bg))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |editor, _: &gpui::MouseDownEvent, _window, cx| {
                editor.toggle_pill_menu(kind, cx)
            }),
        )
        .child(label)
        .child(icon(IconName::ChevronDown).size(px(11.0)).color(tokens.ink_tertiary))
}

/// The family dropdown: a trigger plus, when open, the family menu below it.
fn family_dropdown(
    current: FontFamily,
    open: bool,
    tokens: &Tokens,
    cx: &mut Context<Editor>,
) -> Stateful<Div> {
    let mut trigger =
        dropdown_trigger("pill-family", family_label(current), open, tokens, cx, PillMenu::Family);
    if open {
        trigger = trigger.child(family_menu(current, tokens, cx));
    }
    trigger
}

/// The size dropdown: a trigger showing the current points, plus the menu.
fn size_dropdown(current: f32, open: bool, tokens: &Tokens, cx: &mut Context<Editor>) -> Stateful<Div> {
    let label = SharedString::from(format!("{current:.0}"));
    let mut trigger = dropdown_trigger("pill-size", label, open, tokens, cx, PillMenu::Size);
    if open {
        trigger = trigger.child(size_menu(current, tokens, cx));
    }
    trigger
}

/// The floating menu shell, dropped just below its trigger.
fn menu_shell(tokens: &Tokens) -> Div {
    div()
        .occlude()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .p(px(4.0))
        .bg(tokens.surface_lifted)
        .border_1()
        .border_color(tokens.hairline)
        .rounded(px(metrics::RADIUS_MD))
        .shadow(soft_shadow(tokens))
}

/// Position a menu shell as an absolute child just under the trigger.
fn menu_drop(inner: impl IntoElement) -> Div {
    div().absolute().top_full().left_0().mt(px(6.0)).child(inner)
}

/// One selectable menu row: a label, a check when it's the current value.
fn menu_row(
    id: gpui::ElementId,
    selected: bool,
    tokens: &Tokens,
    label: impl IntoElement,
    on_click: impl Fn(&mut Editor, &mut Context<Editor>) + 'static,
    cx: &mut Context<Editor>,
) -> Stateful<Div> {
    let hover_bg = tokens.hairline.opacity(0.6);
    let accent = tokens.accent;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(metrics::RADIUS_SM))
        .cursor_pointer()
        .when(selected, |el| el.bg(tokens.hairline.opacity(0.5)))
        .hover(move |style| style.bg(hover_bg))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |editor, _: &gpui::MouseDownEvent, _window, cx| on_click(editor, cx)),
        )
        .child(label)
        .child(if selected {
            icon(IconName::Check).size(px(12.0)).color(accent).into_any_element()
        } else {
            div().w(px(12.0)).into_any_element()
        })
}

/// The font-family menu: every family named in its own typeface.
fn family_menu(current: FontFamily, tokens: &Tokens, cx: &mut Context<Editor>) -> Div {
    let families = [
        (FontFamily::Literata, fonts::FONT_SERIF),
        (FontFamily::Inter, fonts::FONT_SANS),
        (FontFamily::Quattro, fonts::FONT_QUATTRO),
        (FontFamily::Mono, fonts::FONT_MONO),
    ];
    let ink = tokens.ink;
    let mut shell = menu_shell(tokens).min_w(px(196.0));
    for (family, font) in families {
        let label = div()
            .font_family(font)
            .text_size(px(metrics::UI_TEXT))
            .text_color(ink)
            .child(family_label(family));
        shell = shell.child(menu_row(
            ("pill-family-item", family as usize).into(),
            current == family,
            tokens,
            label,
            move |editor, cx| editor.choose_family(family, cx),
            cx,
        ));
    }
    menu_drop(shell)
}

/// The font-size menu: a scrollable column of point sizes.
fn size_menu(current: f32, tokens: &Tokens, cx: &mut Context<Editor>) -> Div {
    let ink = tokens.ink;
    let mut list = div()
        .id("pill-size-scroll")
        .max_h(px(232.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(1.0));
    for &size in &SIZE_MENU {
        let label = div()
            .font_family(fonts::FONT_UI)
            .text_size(px(metrics::UI_TEXT))
            .text_color(ink)
            .child(SharedString::from(format!("{size:.0}")));
        list = list.child(menu_row(
            ("pill-size-item", size as usize).into(),
            (current - size).abs() < 0.5,
            tokens,
            label,
            move |editor, cx| editor.choose_size(size, cx),
            cx,
        ));
    }
    menu_drop(menu_shell(tokens).min_w(px(108.0)).child(list))
}

fn seperator(tokens: &Tokens) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(1.0))
        .h(px(12.0))
        .mx(px(4.0))
        .bg(tokens.hairline.opacity(0.7))
}

/// Build the margin-note card overlay (open card or a dismissed card
/// mid-recede), if any.
pub(crate) fn render_card(
    editor: &Editor,
    _window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    let tokens = cx.theme().tokens;

    // A dismissed card recedes in place, inert.
    if let Some(closing) = &editor.closing_card {
        let shell = div()
            .max_w(px(280.0))
            .child(card_content(
                closing.tone,
                closing.body.clone(),
                None::<Div>,
                &tokens,
            ))
            .with_animation(
                ("card-recede", closing.id as usize),
                Animation::new(motion::FADE).with_easing(motion::ease_out_quint),
                |el, t| el.opacity(1.0 - t),
            );
        return Some(
            deferred(
                anchored()
                    .position(closing.position + point(px(-6.0), px(-14.0)))
                    .anchor(Corner::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(shell),
            )
            .with_priority(2)
            .into_any_element(),
        );
    }

    let open = editor.card.as_ref()?;
    let slot = editor.notes.iter().find(|slot| slot.ann.id == open.id)?;
    let center = slot.last_center?;
    let snap = editor.snapshot.clone()?;
    let marker_at = snap.to_window(center);
    // The card blooms above the citation caret (never covering the quoted
    // line), overlapping the marker slightly so the pointer can travel
    // marker → card without a dead gap closing it.
    let position = point(marker_at.x - px(14.0), marker_at.y - px(4.0));

    let id = open.id;
    let hover_ink = tokens.ink;
    let dismiss = div()
        .id(("note-dismiss", id as usize))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(18.0))
        .h(px(18.0))
        .rounded(px(metrics::RADIUS_SM))
        .text_color(tokens.ink_tertiary)
        .cursor_pointer()
        .hover(move |style| style.text_color(hover_ink))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |editor, _: &gpui::MouseDownEvent, _window, cx| {
                editor.dismiss_note(id, cx);
            }),
        )
        .child(icon(IconName::X).size(px(10.0)));

    let shell = div()
        .id(("note-card", id as usize))
        .occlude()
        .max_w(px(280.0))
        .on_hover(cx.listener(|editor, hovered: &bool, _window, cx| {
            editor.set_card_hovered(*hovered, cx);
        }))
        .child(card_content(slot.ann.tone, slot.ann.body.clone(), Some(dismiss), &tokens))
        .with_animation(
            ("card-bloom", id as usize),
            Animation::new(motion::NOTE_BLOOM).with_easing(motion::ease_out_quint),
            |el, t| el.opacity(t).mt(px(3.0 * (1.0 - t))),
        );

    Some(
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::BottomLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(shell),
        )
        .with_priority(2)
        .into_any_element(),
    )
}

/// The card itself: a quiet lowercase tone label, an optional dismiss, and
/// the body revealing word by word.
fn card_content(
    tone: AnnotationTone,
    body: SharedString,
    dismiss: Option<impl IntoElement>,
    tokens: &Tokens,
) -> impl IntoElement {
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(metrics::UI_SMALL))
                .font_family(fonts::FONT_UI)
                .text_color(tokens.ink_tertiary)
                .child(tone.label()),
        )
        .children(dismiss);

    let ink = tokens.ink;
    let reveal = div()
        .mt(px(4.0))
        .text_size(px(metrics::UI_TEXT))
        .font_family(fonts::FONT_UI)
        .text_color(ink)
        .with_animation(
            "card-reveal",
            Animation::new(notes::REVEAL),
            move |el, t| {
                el.child(SharedString::from(reveal_prefix(&body, t).to_string()))
            },
        );

    card().child(header).child(reveal)
}
