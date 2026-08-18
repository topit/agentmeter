use std::path::PathBuf;

use agentmeter_core::{
    AppPreferences, AppearancePreference, LanguagePreference, SourceHealthState,
};
use agentmeter_desktop::{
    LocalDataErrorKind, Locale, MessageKey, OverviewLoadState, OverviewService, OverviewState,
    PreferencesService, Route, SettingsLoadState, SettingsState, ShellState, SourceCard,
    SourcesLoadState, SourcesService, SourcesState, ThemeMode, ThemePalette, appearance_option_key,
    language_option_key, resolved_locale, resolved_theme_mode,
};
use gpui::{
    AnyElement, App, Bounds, Context, IntoElement, Render, Task, TitlebarOptions, Window,
    WindowAppearance, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1_120.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("AgentMeter".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| NavigationShell::new(window, cx)),
        )
        .expect("failed to open AgentMeter window");
        cx.activate(true);
    });
}

struct NavigationShell {
    state: ShellState,
    overview: OverviewState,
    _overview_task: Task<()>,
    sources: SourcesState,
    _sources_task: Task<()>,
    settings: SettingsState,
    _settings_task: Task<()>,
    system_locale: Locale,
}

impl NavigationShell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = Locale::from_language_tag(std::env::var("LANG").as_deref().unwrap_or("en"));
        let state = ShellState::new(
            locale,
            ThemeMode::System,
            appearance_is_dark(window.appearance()),
        );
        cx.observe_window_appearance(window, |this, window, cx| {
            this.state
                .set_system_is_dark(appearance_is_dark(window.appearance()));
            cx.notify();
        })
        .detach();

        let mut overview = OverviewState::default();
        let request = overview.begin_request();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            OverviewService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        let overview_task = cx.spawn(async move |this, cx| {
            let result = load.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        this.overview.apply_snapshot(request, snapshot);
                    }
                    Err(error) => {
                        this.overview.apply_error(request, error);
                    }
                }
                cx.notify();
            })
            .ok();
        });

        let mut sources = SourcesState::default();
        let request = sources.begin_request();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            SourcesService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        let sources_task = cx.spawn(async move |this, cx| {
            let result = load.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        this.sources.apply_snapshot(request, snapshot);
                    }
                    Err(error) => {
                        this.sources.apply_error(request, error);
                    }
                }
                cx.notify();
            })
            .ok();
        });

        let mut settings = SettingsState::default();
        let request = settings.begin_load();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            PreferencesService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        let settings_task = cx.spawn(async move |this, cx| {
            let result = load.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(preferences) => {
                        if this.settings.apply_loaded(request, preferences) {
                            this.apply_preferences_to_shell();
                        }
                    }
                    Err(error) => {
                        this.settings.apply_load_error(request, error);
                    }
                }
                cx.notify();
            })
            .ok();
        });

        Self {
            state,
            overview,
            _overview_task: overview_task,
            sources,
            _sources_task: sources_task,
            settings,
            _settings_task: settings_task,
            system_locale: locale,
        }
    }

    fn apply_preferences_to_shell(&mut self) {
        let Some(preferences) = self.settings.preferences() else {
            return;
        };
        self.state
            .set_locale(resolved_locale(self.system_locale, preferences.language));
        self.state
            .set_theme_mode(resolved_theme_mode(preferences.appearance));
    }

    /// Persists the complete preference snapshot off the render path. The
    /// selection has already applied optimistically; the save result only
    /// records success or a visible, localized failure.
    fn persist_preferences(&mut self, preferences: AppPreferences, cx: &mut Context<Self>) {
        let request = self.settings.begin_save();
        let save = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            PreferencesService::in_data_directory(data_directory)
                .save(preferences)
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
            let result = save.await;
            this.update(cx, |this, cx| {
                this.settings.apply_save_result(request, result);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn nav_item(
        &self,
        route: Route,
        palette: ThemePalette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.state.selected_route() == route;
        let label = self.state.label(route);
        let item = div()
            .id(route.element_id())
            .focusable()
            .tab_stop(true)
            .role(gpui::Role::Button)
            .aria_label(label)
            .w_full()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(if selected {
                palette.accent
            } else {
                palette.surface
            }))
            .cursor_pointer()
            .focus_visible(move |style| style.border_2().border_color(rgb(palette.focus_ring)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state.select(route);
                cx.notify();
            }))
            .child(label);
        if selected {
            item.bg(rgb(palette.accent))
                .text_color(rgb(palette.accent_text))
        } else {
            item.text_color(rgb(palette.text))
                .hover(move |style| style.bg(rgb(palette.hover)))
        }
    }

    fn overview_content(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        match self.overview.load_state() {
            OverviewLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::OverviewLoading))
                .into_any_element(),
            OverviewLoadState::Empty => div()
                .mt_6()
                .max_w(px(560.0))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.surface))
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(locale.text(MessageKey::OverviewEmptyTitle)),
                )
                .child(
                    div()
                        .mt_2()
                        .text_color(rgb(palette.muted_text))
                        .child(locale.text(MessageKey::OverviewEmptyBody)),
                )
                .into_any_element(),
            OverviewLoadState::Error(error) => {
                let message = match error {
                    LocalDataErrorKind::DataDirectory => MessageKey::OverviewDataDirectoryError,
                    LocalDataErrorKind::Database => MessageKey::OverviewDatabaseError,
                };
                div()
                    .mt_6()
                    .max_w(px(560.0))
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.surface))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(locale.text(MessageKey::OverviewErrorTitle)),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(message)),
                    )
                    .into_any_element()
            }
            OverviewLoadState::Populated | OverviewLoadState::Partial => {
                self.overview_metrics(palette).into_any_element()
            }
        }
    }

    fn sources_content(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        match self.sources.load_state() {
            SourcesLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::SourcesLoading))
                .into_any_element(),
            SourcesLoadState::Empty => div()
                .mt_6()
                .max_w(px(560.0))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.surface))
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(locale.text(MessageKey::SourcesEmptyTitle)),
                )
                .child(
                    div()
                        .mt_2()
                        .text_color(rgb(palette.muted_text))
                        .child(locale.text(MessageKey::SourcesEmptyBody)),
                )
                .into_any_element(),
            SourcesLoadState::Error(error) => {
                let message = match error {
                    LocalDataErrorKind::DataDirectory => MessageKey::OverviewDataDirectoryError,
                    LocalDataErrorKind::Database => MessageKey::OverviewDatabaseError,
                };
                div()
                    .mt_6()
                    .max_w(px(560.0))
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.surface))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(locale.text(MessageKey::SourcesErrorTitle)),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(message)),
                    )
                    .into_any_element()
            }
            SourcesLoadState::Populated => {
                let snapshot = self
                    .sources
                    .snapshot()
                    .expect("populated sources state must contain a snapshot");
                div()
                    .mt_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(snapshot.sources.iter().map(|health| {
                        source_card(SourceCard::from_health(health, locale), locale, palette)
                    }))
                    .into_any_element()
            }
        }
    }

    fn settings_content(&self, palette: ThemePalette, cx: &mut Context<Self>) -> AnyElement {
        let locale = self.state.locale();
        match self.settings.load_state() {
            SettingsLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::SettingsLoading))
                .into_any_element(),
            SettingsLoadState::Error(error) => {
                let message = match error {
                    LocalDataErrorKind::DataDirectory => MessageKey::OverviewDataDirectoryError,
                    LocalDataErrorKind::Database => MessageKey::OverviewDatabaseError,
                };
                div()
                    .mt_6()
                    .max_w(px(560.0))
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.surface))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(locale.text(MessageKey::SettingsErrorTitle)),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(message)),
                    )
                    .into_any_element()
            }
            SettingsLoadState::Loaded => {
                let preferences = self
                    .settings
                    .preferences()
                    .expect("loaded settings state must contain preferences");
                let mut content = div()
                    .mt_6()
                    .max_w(px(560.0))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.surface))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(locale.text(MessageKey::SettingsLanguage)),
                            )
                            .child(
                                div().flex().flex_row().flex_wrap().gap_2().children(
                                    [
                                        LanguagePreference::System,
                                        LanguagePreference::English,
                                        LanguagePreference::SimplifiedChinese,
                                    ]
                                    .into_iter()
                                    .map(|option| {
                                        option_control(
                                            language_option_id(option),
                                            locale.text(language_option_key(option)),
                                            preferences.language == option,
                                            palette,
                                            move |this, _, _, cx| {
                                                let Some(mut preferences) =
                                                    this.settings.preferences()
                                                else {
                                                    return;
                                                };
                                                preferences.language = option;
                                                this.settings.select_language(preferences.language);
                                                this.apply_preferences_to_shell();
                                                this.persist_preferences(preferences, cx);
                                            },
                                            cx,
                                        )
                                    }),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.surface))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(locale.text(MessageKey::SettingsAppearance)),
                            )
                            .child(
                                div().flex().flex_row().flex_wrap().gap_2().children(
                                    [
                                        AppearancePreference::System,
                                        AppearancePreference::Light,
                                        AppearancePreference::Dark,
                                    ]
                                    .into_iter()
                                    .map(|option| {
                                        option_control(
                                            appearance_option_id(option),
                                            locale.text(appearance_option_key(option)),
                                            preferences.appearance == option,
                                            palette,
                                            move |this, _, _, cx| {
                                                let Some(mut preferences) =
                                                    this.settings.preferences()
                                                else {
                                                    return;
                                                };
                                                preferences.appearance = option;
                                                this.settings
                                                    .select_appearance(preferences.appearance);
                                                this.apply_preferences_to_shell();
                                                this.persist_preferences(preferences, cx);
                                            },
                                            cx,
                                        )
                                    }),
                                ),
                            ),
                    );
                if self.settings.save_error().is_some() {
                    content = content.child(
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(palette.danger))
                            .text_color(rgb(palette.danger))
                            .child(locale.text(MessageKey::SettingsSaveError)),
                    );
                }
                content.into_any_element()
            }
        }
    }

    fn overview_metrics(&self, palette: ThemePalette) -> impl IntoElement {
        let locale = self.state.locale();
        let snapshot = self
            .overview
            .snapshot()
            .expect("loaded overview state must contain a snapshot");
        let total_tokens = snapshot
            .tokens
            .checked_total()
            .map(|value| locale.format_count(value))
            .unwrap_or_else(|| locale.text(MessageKey::NotAvailable).to_owned());
        let provider_cost = snapshot
            .costs
            .provider_reported_usd
            .map(|value| locale.format_usd(value))
            .unwrap_or_else(|| locale.text(MessageKey::NotAvailable).to_owned());
        let estimated_cost = snapshot
            .costs
            .api_equivalent_estimate_usd
            .map(|value| locale.format_usd(value))
            .unwrap_or_else(|| locale.text(MessageKey::NotAvailable).to_owned());
        let health_message = if self.overview.load_state() == OverviewLoadState::Partial {
            MessageKey::OverviewPartial
        } else {
            MessageKey::HealthHealthy
        };

        div()
            .mt_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.surface))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(locale.text(MessageKey::CollectionHealth)),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(health_message)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .child(metric_card(
                        locale.text(MessageKey::TotalTokens),
                        total_tokens,
                        palette,
                    ))
                    .child(metric_card(
                        locale.text(MessageKey::Sessions),
                        locale.format_count(snapshot.session_count),
                        palette,
                    ))
                    .child(metric_card(
                        locale.text(MessageKey::ActiveDays),
                        locale.format_count(snapshot.active_days),
                        palette,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .child(metric_card(
                        locale.text(MessageKey::Models),
                        locale.format_count(snapshot.model_count),
                        palette,
                    ))
                    .child(metric_card(
                        locale.text(MessageKey::ProviderReportedCost),
                        provider_cost,
                        palette,
                    ))
                    .child(metric_card(
                        locale.text(MessageKey::ApiEquivalentCost),
                        estimated_cost,
                        palette,
                    )),
            )
    }
}

impl Render for NavigationShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.state.palette();
        let locale = self.state.locale();
        let selected = self.state.selected_route();
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(palette.background))
            .text_color(rgb(palette.text))
            .child(
                div()
                    .w(px(232.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .bg(rgb(palette.surface))
                    .border_r_1()
                    .border_color(rgb(palette.border))
                    .child(
                        div()
                            .pb_3()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("AgentMeter"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_sm()
                                    .text_color(rgb(palette.muted_text))
                                    .child(locale.text(MessageKey::AppSubtitle)),
                            ),
                    )
                    .children(
                        Route::ALL
                            .into_iter()
                            .map(|route| self.nav_item(route, palette, cx)),
                    ),
            )
            .child({
                let content = if selected == Route::Overview {
                    self.overview_content(palette)
                } else if selected == Route::Sources {
                    self.sources_content(palette)
                } else if selected == Route::Settings {
                    self.settings_content(palette, cx)
                } else {
                    div()
                        .mt_3()
                        .max_w(px(560.0))
                        .text_color(rgb(palette.muted_text))
                        .child(locale.text(MessageKey::ShellPlaceholder))
                        .into_any_element()
                };
                div()
                    .flex_1()
                    .h_full()
                    .p_8()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.state.label(selected)),
                    )
                    .child(content)
            })
    }
}

fn metric_card(label: &'static str, value: String, palette: ThemePalette) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(160.0))
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.surface))
        .child(
            div()
                .text_sm()
                .text_color(rgb(palette.muted_text))
                .child(label),
        )
        .child(
            div()
                .mt_2()
                .text_xl()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(value),
        )
}

/// Maps a typed source state to a semantic palette color. The status label is
/// always rendered next to the color so color is never the only carrier.
fn status_palette_color(state: SourceHealthState, palette: ThemePalette) -> u32 {
    match state {
        SourceHealthState::Healthy => palette.success,
        SourceHealthState::Partial | SourceHealthState::UnsupportedSchema => palette.warning,
        SourceHealthState::SetupRequired => palette.info,
        SourceHealthState::Error => palette.danger,
        SourceHealthState::Disabled => palette.muted_text,
    }
}

fn source_card(card: SourceCard, locale: Locale, palette: ThemePalette) -> impl IntoElement {
    let status_color = status_palette_color(card.state, palette);
    div()
        .max_w(px(760.0))
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.surface))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(card.adapter_id.clone()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(status_color))
                        .text_color(rgb(status_color))
                        .text_sm()
                        .child(card.status_label),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(palette.muted_text))
                .child(card.identity.clone()),
        )
        .child(
            div().mt_2().flex().flex_col().gap_1().children(
                card.detail
                    .iter()
                    .map(|(key, value)| detail_row(locale.text(*key), value.clone(), palette)),
            ),
        )
        .children((!card.warnings.is_empty()).then(|| {
            div()
                .mt_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(locale.text(MessageKey::SourcesWarnings)),
                )
                .children(card.warnings.iter().map(|warning| {
                    div()
                        .text_sm()
                        .text_color(rgb(palette.muted_text))
                        .child(warning.clone())
                }))
        }))
        .children(card.error.clone().map(|error| {
            div()
                .mt_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.danger))
                        .child(locale.text(MessageKey::SourcesErrorLabel)),
                )
                .child(div().text_sm().text_color(rgb(palette.danger)).child(error))
        }))
        .children(card.remediation_label.map(|remediation| {
            div()
                .mt_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(locale.text(MessageKey::SourcesRemediation)),
                )
                .child(div().mt_1().text_sm().child(remediation))
        }))
}

fn detail_row(label: &'static str, value: String, palette: ThemePalette) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .child(
            div()
                .w(px(180.0))
                .flex_none()
                .text_sm()
                .text_color(rgb(palette.muted_text))
                .child(label),
        )
        .child(div().text_sm().child(value))
}

/// One selectable preference option. Selecting invokes a real
/// application-service save command, so every rendered control is actionable.
fn option_control(
    id: &'static str,
    label: &'static str,
    selected: bool,
    palette: ThemePalette,
    on_select: impl Fn(
        &mut NavigationShell,
        &gpui::ClickEvent,
        &mut Window,
        &mut Context<NavigationShell>,
    ) + 'static,
    cx: &mut Context<NavigationShell>,
) -> AnyElement {
    let item = div()
        .id(id)
        .focusable()
        .tab_stop(true)
        .role(gpui::Role::Button)
        .aria_label(label)
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if selected {
            palette.accent
        } else {
            palette.border
        }))
        .cursor_pointer()
        .focus_visible(move |style| style.border_2().border_color(rgb(palette.focus_ring)))
        .on_click(cx.listener(on_select))
        .child(label);
    if selected {
        item.bg(rgb(palette.accent))
            .text_color(rgb(palette.accent_text))
            .into_any_element()
    } else {
        item.text_color(rgb(palette.text))
            .hover(move |style| style.bg(rgb(palette.hover)))
            .into_any_element()
    }
}

fn language_option_id(option: LanguagePreference) -> &'static str {
    match option {
        LanguagePreference::System => "settings-language-system",
        LanguagePreference::English => "settings-language-english",
        LanguagePreference::SimplifiedChinese => "settings-language-chinese",
    }
}

fn appearance_option_id(option: AppearancePreference) -> &'static str {
    match option {
        AppearancePreference::System => "settings-appearance-system",
        AppearancePreference::Light => "settings-appearance-light",
        AppearancePreference::Dark => "settings-appearance-dark",
    }
}

fn macos_data_directory() -> Result<PathBuf, LocalDataErrorKind> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"))
        .ok_or(LocalDataErrorKind::DataDirectory)
}

fn appearance_is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}
