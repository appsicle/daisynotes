//! The settings pane — a centered overlay card over a faint scrim. Owns
//! appearance switching, theme presets, the custom three-color theme
//! builder, Muse's personality, and the API key. Opened with ⌘, (or the
//! app menu), closed by Escape, the X, or clicking the scrim. Pure
//! overlay: the page behind it never shifts.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, ClickEvent, Context, ElementId, Entity,
    Focusable as _, SharedString, Window, div, prelude::*, px,
};
use daisynotes_agent::Chattiness;
use daisynotes_local::{DownloadState, LocalModel};
use daisynotes_commands as cmd;
use daisynotes_theme::{
    ActiveTheme as _, Appearance, ThemePair, derive_tokens, fonts, hex_from_hsla, hsla_from_hex,
    layout,
};
use daisynotes_topbar::OrbState;
use daisynotes_ui::{IconName, TextField, icon, icon_button, soft_shadow, text_button};

use crate::workspace::{UpdateCheck, Workspace};

/// Settings card width.
const CARD_W: f32 = 560.0;
/// How long the tiny check lingers after a key is saved.
const SAVED_CHECK_LINGER: Duration = Duration::from_secs(2);
/// How often the open pane polls an in-flight model download.
const DOWNLOAD_POLL: Duration = Duration::from_millis(250);
/// The download progress bar's width and height.
const PROGRESS_W: f32 = 120.0;
const PROGRESS_H: f32 = 4.0;

impl Workspace {
    /// ⌘, — toggle the settings pane.
    pub(crate) fn act_open_settings(
        &mut self,
        _: &cmd::OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_open {
            self.close_settings(window, cx);
        } else {
            self.open_settings(cx);
        }
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.theme_menu_open = false;
        self.refill_custom_fields(cx);
        self.api_field.update(cx, |field, cx| {
            field.set_value("", cx);
            field.set_invalid(false, cx);
        });
        self.api_saved = false;
        if matches!(self.local.download_state(), DownloadState::Downloading { .. }) {
            self.poll_download(cx);
        }
        cx.notify();
    }

    /// Close the pane and hand focus back to the page.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.theme_menu_open = false;
        // Retire the download poll; the crate keeps downloading on its own.
        self.download_poll_generation = self.download_poll_generation.wrapping_add(1);
        window.focus(&self.editor.focus_handle(cx));
        cx.notify();
    }

    /// Pre-fill the six custom hex fields from the current theme pair.
    fn refill_custom_fields(&mut self, cx: &mut Context<Self>) {
        let values = [
            hex_from_hsla(self.theme_pair.light.accent),
            hex_from_hsla(self.theme_pair.light.bg),
            hex_from_hsla(self.theme_pair.light.ink),
            hex_from_hsla(self.theme_pair.dark.accent),
            hex_from_hsla(self.theme_pair.dark.bg),
            hex_from_hsla(self.theme_pair.dark.ink),
        ];
        for (field, value) in self.custom_fields.iter().zip(values) {
            field.update(cx, |field, cx| {
                field.set_value(value, cx);
                field.set_invalid(false, cx);
            });
        }
    }

    // ── Section behaviors ──────────────────────────────────────────────────

    /// A preset swatch was clicked: adopt its pair for both modes.
    fn apply_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        let presets = daisynotes_theme::presets();
        let Some(preset) = presets.get(index) else {
            return;
        };
        self.persist_setting("theme.preset", preset.name);
        self.apply_theme_pair(preset.pair(), cx);
        self.refill_custom_fields(cx);
        cx.notify();
    }

    /// Apply the six custom hex fields. Invalid fields get flagged and
    /// nothing changes until all six parse.
    fn apply_custom(&mut self, cx: &mut Context<Self>) {
        let mut colors = [None; 6];
        for (i, field) in self.custom_fields.iter().enumerate() {
            let parsed = hsla_from_hex(field.read(cx).value());
            field.update(cx, |field, cx| field.set_invalid(parsed.is_none(), cx));
            colors[i] = parsed;
        }
        let [Some(la), Some(lb), Some(lf), Some(da), Some(db), Some(df)] = colors else {
            return;
        };
        let pair = ThemePair {
            light: derive_tokens(la, lb, lf),
            dark: derive_tokens(da, db, df),
        };
        self.persist_setting(
            "theme.custom.light",
            &format!(
                "{},{},{}",
                hex_from_hsla(la),
                hex_from_hsla(lb),
                hex_from_hsla(lf)
            ),
        );
        self.persist_setting(
            "theme.custom.dark",
            &format!(
                "{},{},{}",
                hex_from_hsla(da),
                hex_from_hsla(db),
                hex_from_hsla(df)
            ),
        );
        self.persist_setting("theme.preset", "custom");
        self.apply_theme_pair(pair, cx);
        cx.notify();
    }

    /// Personality segment: persist and retune every live trigger engine.
    fn set_chattiness(&mut self, chattiness: Chattiness, cx: &mut Context<Self>) {
        self.chattiness = chattiness;
        self.persist_setting(
            "chattiness",
            match chattiness {
                Chattiness::Quiet => "quiet",
                Chattiness::Occasional => "occasional",
                Chattiness::Chatty => "chatty",
            },
        );
        for engine in self.engines.values_mut() {
            engine.set_chattiness(chattiness);
        }
        cx.notify();
    }

    /// Save the API key to the Keychain, re-resolve, and wake the agent —
    /// no relaunch needed.
    fn save_api_key(&mut self, cx: &mut Context<Self>) {
        let key = self.api_field.read(cx).value().trim().to_string();
        if key.is_empty() {
            return;
        }
        if !daisynotes_api::store_api_key(&key) {
            self.api_field
                .update(cx, |field, cx| field.set_invalid(true, cx));
            return;
        }
        self.key_missing = daisynotes_api::resolve_api_key().is_none();
        self.api_field.update(cx, |field, cx| field.set_value("", cx));
        if !self.key_missing && !self.muted {
            self.topbar
                .update(cx, |topbar, cx| topbar.set_orb(OrbState::Resting, cx));
        }
        self.api_saved = true;
        self.api_saved_generation = self.api_saved_generation.wrapping_add(1);
        let generation = self.api_saved_generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVED_CHECK_LINGER).await;
            this.update(cx, |this, cx| {
                if this.api_saved_generation == generation {
                    this.api_saved = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Kick off a model download and watch it while the pane is open.
    fn download_model(&mut self, model: LocalModel, cx: &mut Context<Self>) {
        self.local.start_download(model);
        self.poll_download(cx);
        cx.notify();
    }

    /// A generation-guarded 250ms poll, alive only while the pane is open
    /// and a download is in flight. When the download finishes the row
    /// simply reads Installed — and the agent's gate sees the model on its
    /// next tick, no fanfare.
    fn poll_download(&mut self, cx: &mut Context<Self>) {
        self.download_poll_generation = self.download_poll_generation.wrapping_add(1);
        let generation = self.download_poll_generation;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DOWNLOAD_POLL).await;
                let proceed = this.update(cx, |this, cx| {
                    if this.download_poll_generation != generation || !this.settings_open {
                        return false;
                    }
                    cx.notify();
                    matches!(
                        this.local.download_state(),
                        DownloadState::Downloading { .. }
                    )
                });
                if !proceed.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    /// The overlay, or `None` while closed.
    pub(crate) fn render_settings(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.settings_open {
            return None;
        }
        let tokens = cx.theme().tokens;
        let max_h = f32::from(window.viewport_size().height) * 0.84;

        let card = div()
            .occlude()
            .w(px(CARD_W))
            .max_h(px(max_h))
            .bg(tokens.surface)
            .border_1()
            .border_color(tokens.hairline)
            .rounded(px(layout::RADIUS_LG))
            .shadow(soft_shadow(&tokens))
            .overflow_hidden()
            .child(
                div()
                    .id("settings-scroll")
                    .overflow_y_scroll()
                    .max_h(px(max_h))
                    .px(px(28.))
                    .pt(px(24.))
                    .pb(px(28.))
                    .flex()
                    .flex_col()
                    .child(self.render_settings_title(cx))
                    .child(section_rule("Appearance", cx))
                    .child(self.render_appearance_section(cx))
                    .child(section_rule("Theme", cx))
                    .child(self.render_preset_section(cx))
                    .child(self.render_custom_section(cx))
                    .child(section_rule("Muse", cx))
                    .child(self.render_muse_section(cx))
                    .child(self.render_local_rows(cx))
                    .child(section_rule("API key", cx))
                    .child(self.render_api_section(cx))
                    .child(section_rule("About", cx))
                    .child(self.render_about_section(cx)),
            );

        Some(
            div()
                .id("settings-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(tokens.bg.alpha(0.45))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.close_settings(window, cx);
                }))
                .child(card)
                .into_any_element(),
        )
    }

    /// The masthead: "Settings" set in the serif display voice — the one
    /// editorial flourish — with the close affordance opposite.
    fn render_settings_title(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        div()
            .flex()
            .items_center()
            .justify_between()
            .pb(px(6.))
            .child(
                div()
                    .font_family(fonts::FONT_SERIF)
                    .text_size(px(24.))
                    .text_color(tokens.ink)
                    .child("Settings"),
            )
            .child(icon_button("settings-close", IconName::X).on_click(cx.listener(
                |this, _: &ClickEvent, window, cx| {
                    this.close_settings(window, cx);
                },
            )))
            .into_any_element()
    }

    fn render_appearance_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let appearance = cx.theme().appearance;
        let light = self.segment(
            "appearance-light",
            "Paper",
            appearance == Appearance::Paper,
            cx.listener(|this, _: &ClickEvent, _window, cx| {
                if cx.theme().appearance != Appearance::Paper {
                    this.set_appearance(Appearance::Paper, cx);
                }
            }),
            cx,
        );
        let dark = self.segment(
            "appearance-dark",
            "Dusk",
            appearance == Appearance::Dusk,
            cx.listener(|this, _: &ClickEvent, _window, cx| {
                if cx.theme().appearance != Appearance::Dusk {
                    this.set_appearance(Appearance::Dusk, cx);
                }
            }),
            cx,
        );
        setting_row(
            "Mode",
            "Which side of the pair you write on.",
            segmented(light, dark, None, cx),
            cx,
        )
    }

    /// The theme picker: a dropdown naming the current pair; the menu
    /// blooms over the content below (deferred — nothing shifts).
    fn render_preset_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        let appearance = cx.theme().appearance;
        let presets = daisynotes_theme::presets();
        let current_name = presets
            .iter()
            .find(|preset| preset.pair() == self.theme_pair)
            .map_or("Custom", |preset| preset.name);

        let button = div()
            .id("theme-dropdown")
            .flex()
            .items_center()
            .justify_between()
            .w(px(190.))
            .px(px(10.))
            .py(px(5.))
            .rounded(px(layout::RADIUS_SM))
            .border_1()
            .border_color(tokens.hairline)
            .cursor_pointer()
            .hover(move |style| style.bg(tokens.hairline.opacity(0.4)))
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.theme_menu_open = !this.theme_menu_open;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(swatch(self.theme_pair.tokens_for(appearance)))
                    .child(
                        div()
                            .text_size(px(layout::UI_TEXT))
                            .text_color(tokens.ink)
                            .child(SharedString::from(current_name.to_string())),
                    ),
            )
            .child(
                div()
                    .text_size(px(9.))
                    .text_color(tokens.ink_tertiary)
                    .child("\u{25BC}"),
            );

        let menu = self.theme_menu_open.then(|| {
            let items = presets
                .iter()
                .enumerate()
                .map(|(index, preset)| {
                    let shown = preset.pair().tokens_for(appearance);
                    let active = preset.pair() == self.theme_pair;
                    div()
                        .id(SharedString::from(format!("preset-{index}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(10.))
                        .py(px(6.))
                        .cursor_pointer()
                        .hover(move |style| style.bg(tokens.hairline.opacity(0.4)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            this.theme_menu_open = false;
                            this.apply_preset(index, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(swatch(shown))
                                .child(
                                    div()
                                        .text_size(px(layout::UI_TEXT))
                                        .text_color(tokens.ink)
                                        .child(SharedString::from(preset.name)),
                                ),
                        )
                        .when(active, |this| {
                            this.child(
                                icon(IconName::Check)
                                    .size(px(12.))
                                    .color(tokens.accent),
                            )
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            gpui::deferred(
                div()
                    .occlude()
                    .absolute()
                    .top(px(34.))
                    .left_0()
                    .w(px(190.))
                    .py(px(4.))
                    .bg(tokens.surface_lifted)
                    .border_1()
                    .border_color(tokens.hairline)
                    .rounded(px(layout::RADIUS_MD))
                    .shadow(soft_shadow(&tokens))
                    .flex()
                    .flex_col()
                    .children(items),
            )
            .with_priority(3)
        });

        setting_row(
            "Palette",
            "A light and dark pair, switched by Mode.",
            div()
                .relative()
                .child(button)
                .children(menu)
                .into_any_element(),
            cx,
        )
    }

    fn render_custom_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        let column = |title: &'static str, fields: [&Entity<TextField>; 3], cx: &mut Context<Self>| {
            let tokens = cx.theme().tokens;
            let labeled = |label: &'static str, field: &Entity<TextField>| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(layout::UI_SMALL))
                            .text_color(tokens.ink_tertiary)
                            .child(label),
                    )
                    .child(field.clone())
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(layout::UI_HEADER))
                        .text_color(tokens.ink_secondary)
                        .child(title),
                )
                .child(labeled("Accent", fields[0]))
                .child(labeled("Background", fields[1]))
                .child(labeled("Foreground", fields[2]))
        };
        let [la, lb, lf, da, db, df] = &self.custom_fields;
        let light = column("PAPER", [la, lb, lf], cx);
        let dark = column("DUSK", [da, db, df], cx);

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(layout::UI_TEXT))
                            .text_color(tokens.ink)
                            .child("Make it yours"),
                    )
                    .child(text_button("custom-apply", "Apply").on_click(cx.listener(
                        |this, _: &ClickEvent, _window, cx| {
                            this.apply_custom(cx);
                        },
                    ))),
            )
            .child(div().flex().gap(px(16.)).child(light).child(dark))
            .into_any_element()
    }

    fn render_muse_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let seg = |this: &Self,
                   id: &'static str,
                   label: &'static str,
                   value: Chattiness,
                   cx: &mut Context<Self>| {
            this.segment(
                id,
                label,
                this.chattiness == value,
                cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.set_chattiness(value, cx);
                }),
                cx,
            )
        };
        let quiet = seg(self, "muse-quiet", "Quiet", Chattiness::Quiet, cx);
        let occasional = seg(
            self,
            "muse-occasional",
            "Occasional",
            Chattiness::Occasional,
            cx,
        );
        let chatty = seg(self, "muse-chatty", "Chatty", Chattiness::Chatty, cx);
        setting_row(
            "Personality",
            "How often Muse speaks up in the margin.",
            segmented(quiet, occasional, Some(chatty), cx),
            cx,
        )
    }

    fn render_local_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .pt(px(16.))
            .child(self.render_model_row(LocalModel::Light, cx))
            .child(self.render_model_row(LocalModel::Standard, cx))
            .into_any_element()
    }

    /// One model row: name + size on the left; on the right whatever the
    /// model's state earns — Download, progress, Installed, or Retry.
    fn render_model_row(&self, model: LocalModel, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        let (download_id, retry_id) = match model {
            LocalModel::Light => ("local-light-download", "local-light-retry"),
            LocalModel::Standard => ("local-standard-download", "local-standard-retry"),
        };

        let installed = daisynotes_local::model_path(model).is_file();
        let right: AnyElement = if installed {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon(IconName::Check).size(px(14.)).color(tokens.accent))
                .child(
                    div()
                        .text_size(px(layout::UI_SMALL))
                        .text_color(tokens.ink_tertiary)
                        .child("Installed"),
                )
                .into_any_element()
        } else {
            match self.local.download_state() {
                DownloadState::Downloading {
                    model: active,
                    received,
                    total,
                } if active == model => self.render_download_progress(model, received, total, cx),
                DownloadState::Failed { model: failed, .. } if failed == model => {
                    text_button(retry_id, "Retry")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            this.download_model(model, cx);
                        }))
                        .into_any_element()
                }
                _ => text_button(download_id, "Download")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.download_model(model, cx);
                    }))
                    .into_any_element(),
            }
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(layout::UI_TEXT))
                            .text_color(tokens.ink)
                            .child(model.display_name()),
                    )
                    .child(
                        div()
                            .text_size(px(layout::UI_SMALL))
                            .text_color(tokens.ink_tertiary)
                            .child(model.size_label()),
                    ),
            )
            .child(right)
            .into_any_element()
    }

    /// The compact in-flight bar: hairline track, accent fill tracking
    /// received/total — or a softly pulsing accent bar when the server
    /// never said how big the file is.
    fn render_download_progress(
        &self,
        model: LocalModel,
        received: u64,
        total: Option<u64>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = cx.theme().tokens;
        let track = div()
            .w(px(PROGRESS_W))
            .h(px(PROGRESS_H))
            .rounded_full()
            .bg(tokens.hairline);
        match total.filter(|total| *total > 0) {
            Some(total) => {
                let fraction = (received as f32 / total as f32).clamp(0.0, 1.0);
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        track.child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(tokens.accent)
                                .w(px(PROGRESS_W * fraction)),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(layout::UI_SMALL))
                            .text_color(tokens.ink_tertiary)
                            .child(SharedString::from(format!(
                                "{}%",
                                (fraction * 100.0).round() as u32
                            ))),
                    )
                    .into_any_element()
            }
            None => track
                .child(
                    div()
                        .h_full()
                        .w_full()
                        .rounded_full()
                        .bg(tokens.accent)
                        .with_animation(
                            ElementId::Name(
                                match model {
                                    LocalModel::Light => "local-light-pulse",
                                    LocalModel::Standard => "local-standard-pulse",
                                }
                                .into(),
                            ),
                            Animation::new(Duration::from_millis(1100)).repeat(),
                            |el, t| el.opacity(0.25 + 0.5 * (1.0 - (2.0 * t - 1.0).abs())),
                        ),
                )
                .into_any_element(),
        }
    }

    fn render_api_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(14.))
            .child(
                div()
                    .text_size(px(layout::UI_SMALL))
                    .text_color(tokens.ink_tertiary)
                    .child("With a Claude key, Muse thinks in the cloud; without one, on-device."),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(self.api_field.clone()))
                    .when(self.api_saved, |this| {
                        this.child(icon(IconName::Check).size(px(14.)).color(tokens.accent))
                    })
                    .child(text_button("api-save", "Save").on_click(cx.listener(
                        |this, _: &ClickEvent, _window, cx| {
                            this.save_api_key(cx);
                        },
                    ))),
            )
            .into_any_element()
    }

    /// The About row: the running version, with a manual update check beside
    /// it that gives immediate in-pane feedback. Updates also download
    /// silently in the background (Sparkle); this is just the on-demand check.
    fn render_about_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.theme().tokens;
        let muted = |text: SharedString| {
            div()
                .text_size(px(layout::UI_SMALL))
                .text_color(tokens.ink_tertiary)
                .child(text)
                .into_any_element()
        };
        let control: AnyElement = match &self.update_check {
            UpdateCheck::Idle => text_button("check-updates", "Check for Updates")
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.run_update_check(cx);
                }))
                .into_any_element(),
            UpdateCheck::Checking => muted("Checking…".into()),
            UpdateCheck::Error => muted("Couldn’t check".into()),
            UpdateCheck::UpToDate => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon(IconName::Check).size(px(14.)).color(tokens.accent))
                .child(muted("Up to date".into()))
                .into_any_element(),
            UpdateCheck::Available(version) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(layout::UI_SMALL))
                        .text_color(tokens.accent)
                        .child(SharedString::from(format!("Version {version} available"))),
                )
                .child(text_button("update-restart", "Restart to Update").on_click(
                    cx.listener(|_this, _: &ClickEvent, _window, _cx| {
                        crate::updater::check_for_updates();
                    }),
                ))
                .into_any_element(),
        };
        setting_row(
            "Daisy Notes",
            concat!("Version ", env!("CARGO_PKG_VERSION")),
            control,
            cx,
        )
    }

    /// Fetch the appcast and compare its newest version to this build, updating
    /// `update_check` for immediate feedback. Also nudges Sparkle's real check
    /// (which owns the download/install). Transient verdicts revert after a
    /// few seconds; the generation guard drops a stale in-flight result.
    fn run_update_check(&mut self, cx: &mut Context<Self>) {
        if self.update_check == UpdateCheck::Checking {
            return;
        }
        self.update_check = UpdateCheck::Checking;
        self.update_check_generation = self.update_check_generation.wrapping_add(1);
        let generation = self.update_check_generation;
        cx.notify();
        crate::updater::check_for_updates();

        cx.spawn(async move |this, cx| {
            // The blocking fetch runs on a background thread, off the UI.
            let xml = cx
                .background_executor()
                .spawn(async { crate::updater::fetch_appcast() })
                .await;
            let verdict = match xml
                .as_deref()
                .and_then(crate::updater::latest_version)
            {
                Some(latest)
                    if crate::updater::version_gt(&latest, &crate::updater::current_version()) =>
                {
                    UpdateCheck::Available(latest)
                }
                Some(_) => UpdateCheck::UpToDate,
                None => UpdateCheck::Error,
            };
            let transient = matches!(verdict, UpdateCheck::UpToDate | UpdateCheck::Error);
            this.update(cx, |this, cx| {
                if this.update_check_generation == generation {
                    this.update_check = verdict;
                    cx.notify();
                }
            })
            .ok();
            if transient {
                cx.background_executor()
                    .timer(Duration::from_secs(5))
                    .await;
                this.update(cx, |this, cx| {
                    if this.update_check_generation == generation {
                        this.update_check = UpdateCheck::Idle;
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// One pill of a segmented control.
    fn segment(
        &self,
        id: &'static str,
        label: &'static str,
        selected: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = cx.theme().tokens;
        div()
            .id(id)
            .px(px(10.))
            .py(px(3.))
            .rounded(px(layout::RADIUS_SM - 2.0))
            .text_size(px(layout::UI_SMALL))
            .cursor_pointer()
            .text_color(if selected {
                tokens.ink
            } else {
                tokens.ink_secondary
            })
            .when(selected, |this| {
                this.bg(tokens.surface_lifted)
                    .border_1()
                    .border_color(tokens.hairline)
            })
            .when(!selected, |this| {
                this.hover(move |style| style.bg(tokens.hairline.opacity(0.4)))
            })
            .on_click(on_click)
            .child(label)
            .into_any_element()
    }
}

/// A small two-tone disk previewing a palette: background disk, accent core.
fn swatch(tokens: daisynotes_theme::Tokens) -> gpui::Div {
    div()
        .flex_none()
        .size(px(16.))
        .rounded_full()
        .bg(tokens.bg)
        .border_1()
        .border_color(tokens.ink.alpha(0.25))
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(px(7.)).rounded_full().bg(tokens.accent))
}

/// A grouped segmented control shell: members sit inside one hairline track.
fn segmented(
    first: AnyElement,
    second: AnyElement,
    third: Option<AnyElement>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let tokens = cx.theme().tokens;
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(layout::RADIUS_SM))
        .bg(tokens.hairline.opacity(0.35))
        .child(first)
        .child(second)
        .children(third)
        .into_any_element()
}

/// One settings row: label + one-line description on the left, the control
/// right-aligned. The editorial grid every section shares.
fn setting_row(
    label: &'static str,
    description: &'static str,
    control: AnyElement,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let tokens = cx.theme().tokens;
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .pt(px(14.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(layout::UI_TEXT))
                        .text_color(tokens.ink)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(layout::UI_SMALL))
                        .text_color(tokens.ink_tertiary)
                        .child(description),
                ),
        )
        .child(control)
        .into_any_element()
}

/// An editorial section rule: the uppercase kicker, then a hairline running
/// to the card's edge — the magazine section break.
fn section_rule(label: &'static str, cx: &mut Context<Workspace>) -> gpui::Div {
    let tokens = cx.theme().tokens;
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .pt(px(22.))
        .child(
            div()
                .flex_none()
                .text_size(px(layout::UI_HEADER))
                .text_color(tokens.ink_tertiary)
                .child(SharedString::from(label.to_uppercase())),
        )
        .child(div().flex_1().h(px(1.)).bg(tokens.hairline))
}

