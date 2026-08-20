use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use agentmeter_core::{
    AppPreferences, AppearancePreference, LanguagePreference, SourceHealthState,
};
use agentmeter_desktop::{
    ActivityDimension, ActivityGranularity, ActivityLoadState, ActivityMetric, ActivityService,
    ActivityState, CancellationToken, ExportFormat, ExportService, ExportState, IngestionService,
    IngestionUiState, LocalDataErrorKind, Locale, MessageKey, ModelCard, ModelsPricingLoadState,
    ModelsPricingService, ModelsPricingState, OverviewLoadState, OverviewService, OverviewState,
    PreferencesService, RateCard, Route, SessionCard, SessionsLoadState, SessionsService,
    SessionsState, SettingsLoadState, SettingsState, ShellState, SourceCard, SourcesLoadState,
    SourcesService, SourcesState, ThemeMode, ThemePalette, appearance_option_key,
    language_option_key, resolved_locale, resolved_theme_mode,
};
use gpui::{
    AnyElement, App, Bounds, Context, IntoElement, Render, TitlebarOptions, Window,
    WindowAppearance, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.0), px(720.0)), cx);
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
    activity: ActivityState,
    sessions: SessionsState,
    models_pricing: ModelsPricingState,
    sources: SourcesState,
    settings: SettingsState,
    export: ExportState,
    ingestion: IngestionUiState,
    ingestion_token: CancellationToken,
    system_locale: Locale,
}

impl NavigationShell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = Locale::from_language_tag(std::env::var("LANG").as_deref().unwrap_or("en"));
        let mut state = ShellState::new(
            locale,
            ThemeMode::System,
            appearance_is_dark(window.appearance()),
        );
        if let Some(route) = std::env::var("AGENTMETER_INITIAL_ROUTE")
            .ok()
            .as_deref()
            .and_then(Route::from_name)
        {
            state.select(route);
        }
        cx.observe_window_appearance(window, |this, window, cx| {
            this.state
                .set_system_is_dark(appearance_is_dark(window.appearance()));
            cx.notify();
        })
        .detach();

        let mut shell = Self {
            state,
            overview: OverviewState::default(),
            activity: ActivityState::default(),
            sessions: SessionsState::default(),
            models_pricing: ModelsPricingState::default(),
            sources: SourcesState::default(),
            settings: SettingsState::default(),
            export: ExportState::default(),
            ingestion: IngestionUiState::default(),
            ingestion_token: CancellationToken::new(),
            system_locale: locale,
        };
        shell.reload_all_snapshots(cx);
        shell.reload_settings(cx);
        shell.run_ingestion(cx);
        shell
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

    fn reload_activity(
        &mut self,
        granularity: ActivityGranularity,
        dimension: ActivityDimension,
        cx: &mut Context<Self>,
    ) {
        let request = self.activity.begin_request(granularity, dimension);
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            ActivityService::in_data_directory(data_directory)
                .load(granularity, dimension)
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        this.activity.apply_snapshot(request, snapshot);
                    }
                    Err(error) => {
                        this.activity.apply_error(request, error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reload_overview(&mut self, cx: &mut Context<Self>) {
        let request = self.overview.begin_request();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            OverviewService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
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
        })
        .detach();
    }

    fn reload_sessions(&mut self, cx: &mut Context<Self>) {
        let request = self.sessions.begin_request();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            SessionsService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        this.sessions.apply_snapshot(request, snapshot);
                    }
                    Err(error) => {
                        this.sessions.apply_error(request, error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reload_sources(&mut self, cx: &mut Context<Self>) {
        let request = self.sources.begin_request();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            SourcesService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
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
        })
        .detach();
    }

    fn reload_models_pricing(&mut self, cx: &mut Context<Self>) {
        let request = self.models_pricing.begin_request();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            ModelsPricingService::in_data_directory(data_directory)
                .load_or_apply_bundled(current_unix_ms())
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        if this.models_pricing.apply_snapshot(request, snapshot) {
                            let granularity = this.activity.granularity();
                            let dimension = this.activity.dimension();
                            this.reload_overview(cx);
                            this.reload_activity(granularity, dimension, cx);
                        }
                    }
                    Err(error) => {
                        this.models_pricing.apply_error(request, error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reload_settings(&mut self, cx: &mut Context<Self>) {
        let request = self.settings.begin_load();
        let load = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            PreferencesService::in_data_directory(data_directory)
                .load()
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
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
        })
        .detach();
    }

    /// Refreshes every view snapshot after the ledger changed. Called once
    /// at startup and after each ingestion run.
    fn reload_all_snapshots(&mut self, cx: &mut Context<Self>) {
        let granularity = self.activity.granularity();
        let dimension = self.activity.dimension();
        self.reload_overview(cx);
        self.reload_activity(granularity, dimension, cx);
        self.reload_sessions(cx);
        self.reload_models_pricing(cx);
        self.reload_sources(cx);
    }

    /// Runs one collection pass through the enabled local adapters on the
    /// background executor, then refreshes every view snapshot. Single-use
    /// generations reject overlapping runs.
    fn run_ingestion(&mut self, cx: &mut Context<Self>) {
        self.ingestion_token.cancel();
        let token = CancellationToken::new();
        self.ingestion_token = token.clone();
        let (request, progress) = self.ingestion.begin_scan();
        let scan = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            IngestionService::with_default_local_adapters(data_directory)
                .scan_and_ingest_reported(current_unix_ms(), &token, &progress)
                .map_err(|error| error.kind())
        });
        // Poll the shared progress counters so long first scans stay visibly
        // alive instead of reading as an empty application.
        cx.spawn(async move |this, cx| {
            while this
                .update(cx, |this, cx| {
                    let running = this.ingestion.running();
                    if running {
                        cx.notify();
                    }
                    running
                })
                .unwrap_or(false)
            {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(400))
                    .await;
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                if this.ingestion.apply_scan_result(request, result) {
                    this.reload_all_snapshots(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Requests cooperative cancellation of the running scan; committed
    /// sources stay committed and a later rescan resumes cleanly.
    fn cancel_ingestion(&mut self) {
        self.ingestion_token.cancel();
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

    /// Runs an explicit user-requested export through the application
    /// service on the background executor; out-of-order completions cannot
    /// overwrite a newer export's result.
    fn run_export(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        let request = self.export.begin_export();
        let export = cx.background_executor().spawn(async move {
            let data_directory = macos_data_directory()?;
            ExportService::in_data_directory(data_directory)
                .export_to_file(format, current_unix_ms())
                .map_err(|error| error.kind())
        });
        cx.spawn(async move |this, cx| {
            let result = export.await;
            this.update(cx, |this, cx| {
                this.export.apply_result(request, result);
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
            .h(px(36.0))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(if selected {
                palette.border
            } else {
                palette.sidebar
            }))
            .cursor_pointer()
            .focus_visible(move |style| style.border_2().border_color(rgb(palette.focus_ring)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state.select(route);
                cx.notify();
            }))
            .child(
                div()
                    .w(px(3.0))
                    .h(px(14.0))
                    .rounded_sm()
                    .bg(rgb(if selected {
                        palette.accent
                    } else {
                        palette.sidebar
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(if selected {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .child(label),
            );
        if selected {
            item.bg(rgb(palette.selected)).text_color(rgb(palette.text))
        } else {
            item.text_color(rgb(palette.text))
                .hover(move |style| style.bg(rgb(palette.hover)))
        }
    }

    fn nav_group_label(&self, key: MessageKey, palette: ThemePalette) -> impl IntoElement + use<> {
        div()
            .mt_3()
            .mb_1()
            .px_3()
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(palette.subtle_text))
            .child(self.state.locale().text(key))
    }

    fn overview_content(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        match self.overview.load_state() {
            OverviewLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::OverviewLoading))
                .into_any_element(),
            OverviewLoadState::Empty if self.ingestion.running() => div()
                .mt_6()
                .max_w(px(560.0))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(palette.accent))
                .bg(rgb(palette.surface))
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(locale.text(MessageKey::OverviewCollecting)),
                )
                .child(
                    div()
                        .mt_2()
                        .text_sm()
                        .text_color(rgb(palette.muted_text))
                        .child(locale.text(MessageKey::OverviewCollectingBody)),
                )
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

    fn activity_content(&self, palette: ThemePalette, cx: &mut Context<Self>) -> AnyElement {
        let locale = self.state.locale();
        let body = match self.activity.load_state() {
            ActivityLoadState::Loading => div()
                .mt_4()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::ActivityLoading))
                .into_any_element(),
            ActivityLoadState::Empty => div()
                .mt_4()
                .max_w(px(640.0))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.surface))
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(locale.text(MessageKey::ActivityEmptyTitle)),
                )
                .child(
                    div()
                        .mt_2()
                        .text_color(rgb(palette.muted_text))
                        .child(locale.text(MessageKey::ActivityEmptyBody)),
                )
                .into_any_element(),
            ActivityLoadState::Error(error) => {
                let message = match error {
                    LocalDataErrorKind::DataDirectory => MessageKey::OverviewDataDirectoryError,
                    LocalDataErrorKind::Database => MessageKey::OverviewDatabaseError,
                };
                div()
                    .mt_4()
                    .max_w(px(640.0))
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.surface))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(locale.text(MessageKey::ActivityErrorTitle)),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(message)),
                    )
                    .into_any_element()
            }
            ActivityLoadState::Populated => self.activity_chart(palette),
        };
        div()
            .mt_5()
            .child(self.activity_controls(palette, cx))
            .child(body)
            .into_any_element()
    }

    fn activity_controls(&self, palette: ThemePalette, cx: &mut Context<Self>) -> AnyElement {
        let locale = self.state.locale();
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_6()
            .child(
                div()
                    .child(locale.text(MessageKey::ActivityGranularity))
                    .child(div().mt_2().flex().gap_2().children([
                        self.activity_granularity_option(
                            ActivityGranularity::Daily,
                            MessageKey::ActivityDaily,
                            palette,
                            cx,
                        ),
                        self.activity_granularity_option(
                            ActivityGranularity::Weekly,
                            MessageKey::ActivityWeekly,
                            palette,
                            cx,
                        ),
                        self.activity_granularity_option(
                            ActivityGranularity::Monthly,
                            MessageKey::ActivityMonthly,
                            palette,
                            cx,
                        ),
                    ])),
            )
            .child(div().child(locale.text(MessageKey::ActivityMetric)).child(
                div().mt_2().flex().gap_2().children([
                    self.activity_metric_option(
                        ActivityMetric::Tokens,
                        MessageKey::ActivityTokens,
                        palette,
                        cx,
                    ),
                    self.activity_metric_option(
                        ActivityMetric::Cost,
                        MessageKey::ActivityCost,
                        palette,
                        cx,
                    ),
                ]),
            ))
            .child(div().child(locale.text(MessageKey::ActivityGroupBy)).child(
                div().mt_2().flex().gap_2().children([
                    self.activity_dimension_option(
                        ActivityDimension::Client,
                        MessageKey::ActivityClient,
                        palette,
                        cx,
                    ),
                    self.activity_dimension_option(
                        ActivityDimension::Provider,
                        MessageKey::ActivityProvider,
                        palette,
                        cx,
                    ),
                    self.activity_dimension_option(
                        ActivityDimension::Model,
                        MessageKey::ActivityModel,
                        palette,
                        cx,
                    ),
                ]),
            ))
            .into_any_element()
    }

    fn activity_granularity_option(
        &self,
        value: ActivityGranularity,
        label: MessageKey,
        palette: ThemePalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.activity.granularity() == value;
        let id = match value {
            ActivityGranularity::Daily => "activity-daily",
            ActivityGranularity::Weekly => "activity-weekly",
            ActivityGranularity::Monthly => "activity-monthly",
        };
        let label = self.state.locale().text(label);
        activity_option(id, label, selected, palette)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.reload_activity(value, this.activity.dimension(), cx);
            }))
            .into_any_element()
    }

    fn activity_dimension_option(
        &self,
        value: ActivityDimension,
        label: MessageKey,
        palette: ThemePalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.activity.dimension() == value;
        let id = match value {
            ActivityDimension::Client => "activity-client",
            ActivityDimension::Provider => "activity-provider",
            ActivityDimension::Model => "activity-model",
        };
        let label = self.state.locale().text(label);
        activity_option(id, label, selected, palette)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.reload_activity(this.activity.granularity(), value, cx);
            }))
            .into_any_element()
    }

    fn activity_metric_option(
        &self,
        value: ActivityMetric,
        label: MessageKey,
        palette: ThemePalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.activity.metric() == value;
        let id = match value {
            ActivityMetric::Tokens => "activity-tokens",
            ActivityMetric::Cost => "activity-cost",
        };
        let label = self.state.locale().text(label);
        activity_option(id, label, selected, palette)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.activity.set_metric(value);
                cx.notify();
            }))
            .into_any_element()
    }

    fn activity_chart(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        let snapshot = self
            .activity
            .snapshot()
            .expect("populated activity state must contain a snapshot");
        let periods: Vec<&str> = snapshot
            .points
            .iter()
            .map(|point| point.period_start_utc.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let metric = self.activity.metric();
        let raw_value = |point: &agentmeter_desktop::ActivityPoint| match metric {
            ActivityMetric::Tokens => point.tokens,
            ActivityMetric::Cost => point
                .api_equivalent_estimate_usd
                .map_or(0, agentmeter_core::NanoUsd::as_nanos),
        };
        let period_totals: BTreeMap<&str, u64> = periods
            .iter()
            .map(|period| {
                let total = snapshot
                    .points
                    .iter()
                    .filter(|point| point.period_start_utc == *period)
                    .map(&raw_value)
                    .fold(0_u64, |total, value| {
                        total
                            .checked_add(value)
                            .expect("validated activity period total overflowed")
                    });
                (*period, total)
            })
            .collect();
        let maximum = period_totals.values().copied().max().unwrap_or(1).max(1);
        let series: Vec<&str> = snapshot
            .points
            .iter()
            .map(|point| point.series.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let rows: Vec<AnyElement> = periods
            .into_iter()
            .map(|period| {
                let points: Vec<_> = snapshot
                    .points
                    .iter()
                    .filter(|point| point.period_start_utc == period)
                    .collect();
                let total = period_totals[period];
                let total_value = match metric {
                    ActivityMetric::Tokens => locale.format_count(total),
                    ActivityMetric::Cost
                        if points
                            .iter()
                            .any(|point| point.api_equivalent_estimate_usd.is_some()) =>
                    {
                        locale.format_usd(agentmeter_core::NanoUsd::from_nanos(total))
                    }
                    ActivityMetric::Cost => locale.text(MessageKey::NotAvailable).to_owned(),
                };
                let segments: Vec<AnyElement> = points
                    .iter()
                    .map(|point| {
                        let value = raw_value(point);
                        let width = if value == 0 {
                            0.0
                        } else {
                            (value as f64 / maximum as f64 * 360.0).max(3.0) as f32
                        };
                        let color_index = series
                            .iter()
                            .position(|series| *series == point.series)
                            .unwrap_or(0)
                            % palette.series.len();
                        div()
                            .h(px(12.0))
                            .w(px(width))
                            .bg(rgb(palette.series[color_index]))
                            .into_any_element()
                    })
                    .collect();
                let legends: Vec<AnyElement> = points
                    .iter()
                    .map(|point| {
                        let value = match metric {
                            ActivityMetric::Tokens => locale.format_count(point.tokens),
                            ActivityMetric::Cost => point
                                .api_equivalent_estimate_usd
                                .map(|cost| locale.format_usd(cost))
                                .unwrap_or_else(|| {
                                    locale.text(MessageKey::NotAvailable).to_owned()
                                }),
                        };
                        let label = if point.series.is_empty() {
                            locale.text(MessageKey::NotAvailable)
                        } else {
                            point.series.as_str()
                        };
                        let color_index = series
                            .iter()
                            .position(|series| *series == point.series)
                            .unwrap_or(0)
                            % palette.series.len();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(8.0))
                                    .rounded_sm()
                                    .bg(rgb(palette.series[color_index])),
                            )
                            .child(format!("{label}: {value}"))
                            .into_any_element()
                    })
                    .collect();
                div()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(palette.border))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(period.to_owned())
                            .child(total_value),
                    )
                    .child(
                        div()
                            .mt_2()
                            .h(px(12.0))
                            .flex()
                            .rounded_sm()
                            .overflow_hidden()
                            .children(segments),
                    )
                    .child(div().mt_2().flex().flex_wrap().gap_3().children(legends))
                    .when(
                        points.iter().any(|point| point.unpriced_event_count != 0),
                        |element| {
                            element.child(
                                div()
                                    .mt_1()
                                    .text_sm()
                                    .text_color(rgb(palette.warning))
                                    .child(locale.text(MessageKey::ActivityUnpriced)),
                            )
                        },
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .id("activity-chart")
            .mt_4()
            .max_w(px(720.0))
            .max_h(px(430.0))
            .overflow_y_scroll()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(rgb(palette.border))
            .bg(rgb(palette.surface))
            .children(rows)
            .into_any_element()
    }

    fn sessions_content(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        match self.sessions.load_state() {
            SessionsLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::SessionsLoading))
                .into_any_element(),
            SessionsLoadState::Empty => div()
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
                        .child(locale.text(MessageKey::SessionsEmptyTitle)),
                )
                .child(
                    div()
                        .mt_2()
                        .text_color(rgb(palette.muted_text))
                        .child(locale.text(MessageKey::SessionsEmptyBody)),
                )
                .into_any_element(),
            SessionsLoadState::Error(error) => {
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
                            .child(locale.text(MessageKey::SessionsErrorTitle)),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(message)),
                    )
                    .into_any_element()
            }
            SessionsLoadState::Populated => {
                let snapshot = self
                    .sessions
                    .snapshot()
                    .expect("populated sessions state must contain a snapshot");
                div()
                    .id("sessions-list")
                    .mt_5()
                    .max_h(px(600.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(snapshot.sessions.iter().map(|session| {
                        session_card(SessionCard::from_summary(session, locale), locale, palette)
                    }))
                    .into_any_element()
            }
        }
    }

    fn models_content(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        match self.models_pricing.load_state() {
            ModelsPricingLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::ModelsLoading))
                .into_any_element(),
            ModelsPricingLoadState::Error(error) => models_pricing_error(
                locale.text(MessageKey::ModelsErrorTitle),
                error,
                locale,
                palette,
            ),
            ModelsPricingLoadState::Populated => {
                let snapshot = self
                    .models_pricing
                    .snapshot()
                    .expect("populated models/pricing state must contain a snapshot");
                if snapshot.models.is_empty() {
                    return div()
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
                                .child(locale.text(MessageKey::ModelsEmptyTitle)),
                        )
                        .child(
                            div()
                                .mt_2()
                                .text_color(rgb(palette.muted_text))
                                .child(locale.text(MessageKey::ModelsEmptyBody)),
                        )
                        .into_any_element();
                }
                div()
                    .id("models-list")
                    .mt_5()
                    .max_h(px(600.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(snapshot.models.iter().map(|model| {
                        model_card(ModelCard::from_summary(model, locale), locale, palette)
                    }))
                    .into_any_element()
            }
        }
    }

    fn pricing_content(&self, palette: ThemePalette) -> AnyElement {
        let locale = self.state.locale();
        match self.models_pricing.load_state() {
            ModelsPricingLoadState::Loading => div()
                .mt_3()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::PricingLoading))
                .into_any_element(),
            ModelsPricingLoadState::Error(error) => models_pricing_error(
                locale.text(MessageKey::PricingErrorTitle),
                error,
                locale,
                palette,
            ),
            ModelsPricingLoadState::Populated => {
                let snapshot = self
                    .models_pricing
                    .snapshot()
                    .expect("populated models/pricing state must contain a snapshot");
                let unavailable = locale.text(MessageKey::NotAvailable).to_owned();
                let (applied_dataset, dataset_updated, priced_events, unpriced_events) = snapshot
                    .applied
                    .as_ref()
                    .map(|applied| {
                        (
                            format!("{}@{}", applied.source, applied.version),
                            locale.format_unix_ms_utc(applied.dataset_updated_at_unix_ms),
                            locale.format_count(applied.priced_event_count),
                            locale.format_count(applied.unpriced_event_count),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            locale.text(MessageKey::PricingNotApplied).to_owned(),
                            unavailable.clone(),
                            unavailable.clone(),
                            unavailable,
                        )
                    });
                div()
                    .mt_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .max_w(px(760.0))
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.surface))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(detail_row(
                                locale.text(MessageKey::PricingDatasetSource),
                                snapshot.dataset_source.clone(),
                                palette,
                            ))
                            .child(detail_row(
                                locale.text(MessageKey::PricingDatasetVersion),
                                snapshot.dataset_version.clone(),
                                palette,
                            ))
                            .child(detail_row(
                                locale.text(MessageKey::PricingAppliedDataset),
                                applied_dataset,
                                palette,
                            ))
                            .child(detail_row(
                                locale.text(MessageKey::PricingDatasetUpdated),
                                dataset_updated,
                                palette,
                            ))
                            .child(detail_row(
                                locale.text(MessageKey::PricingPricedEvents),
                                priced_events,
                                palette,
                            ))
                            .child(detail_row(
                                locale.text(MessageKey::PricingUnpricedEvents),
                                unpriced_events,
                                palette,
                            )),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(locale.text(MessageKey::PricingRatesPerMillion)),
                    )
                    .child(
                        div()
                            .id("pricing-rates-list")
                            .max_h(px(390.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .children(snapshot.rates.iter().map(|rate| {
                                rate_card(RateCard::from_summary(rate, locale), locale, palette)
                            })),
                    )
                    .into_any_element()
            }
        }
    }

    fn sources_content(&self, palette: ThemePalette, cx: &mut Context<Self>) -> AnyElement {
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
                let mut content = div()
                    .mt_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(if self.ingestion.running() {
                                let (processed, discovered) = self.ingestion.progress();
                                div()
                                    .text_sm()
                                    .text_color(rgb(palette.muted_text))
                                    .child(locale.text(MessageKey::SourcesRescanning))
                                    .child(
                                        div().text_sm().text_color(rgb(palette.muted_text)).child(
                                            format!(
                                                "{} / {}",
                                                locale.format_count(processed),
                                                locale.format_count(discovered)
                                            ),
                                        ),
                                    )
                                    .into_any_element()
                            } else {
                                option_control(
                                    "sources-rescan",
                                    locale.text(MessageKey::SourcesRescan),
                                    false,
                                    palette,
                                    move |this, _, _, cx| {
                                        this.run_ingestion(cx);
                                    },
                                    cx,
                                )
                            })
                            .children(self.ingestion.running().then(|| {
                                option_control(
                                    "sources-cancel-scan",
                                    locale.text(MessageKey::SourcesCancelScan),
                                    false,
                                    palette,
                                    move |this, _, _, _| {
                                        this.cancel_ingestion();
                                    },
                                    cx,
                                )
                            })),
                    )
                    .children(snapshot.sources.iter().map(|health| {
                        source_card(SourceCard::from_health(health, locale), locale, palette)
                    }));
                if self.ingestion.error().is_some() {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(rgb(palette.danger))
                            .child(locale.text(MessageKey::SourcesScanError)),
                    );
                } else if self.ingestion.cancelled() {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(MessageKey::SourcesScanCancelled)),
                    );
                }
                content.into_any_element()
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
                let mut export_card = div()
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
                            .child(locale.text(MessageKey::SettingsExport)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(MessageKey::SettingsExportBody)),
                    )
                    .child(
                        div().flex().flex_row().flex_wrap().gap_2().children(
                            [
                                (
                                    ExportFormat::Json,
                                    "settings-export-json",
                                    MessageKey::SettingsExportJson,
                                ),
                                (
                                    ExportFormat::Csv,
                                    "settings-export-csv",
                                    MessageKey::SettingsExportCsv,
                                ),
                            ]
                            .into_iter()
                            .map(|(format, id, key)| {
                                option_control(
                                    id,
                                    locale.text(key),
                                    false,
                                    palette,
                                    move |this, _, _, cx| {
                                        this.run_export(format, cx);
                                    },
                                    cx,
                                )
                            }),
                        ),
                    );
                if self.export.running() {
                    export_card = export_card.child(
                        div()
                            .text_sm()
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(MessageKey::SettingsLoading)),
                    );
                }
                if let Some(summary) = self.export.summary() {
                    export_card = export_card
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(locale.text(MessageKey::SettingsExported)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
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
                                                .child(locale.text(MessageKey::SettingsExportPath)),
                                        )
                                        .child(div().text_sm().child(format!(
                                            "AgentMeter/exports/{}",
                                            summary.file_name
                                        ))),
                                )
                                .child(
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
                                                .child(
                                                    locale.text(MessageKey::SettingsExportEvents),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .child(locale.format_count(summary.event_count)),
                                        ),
                                ),
                        );
                }
                if self.export.error().is_some() {
                    export_card = export_card.child(
                        div()
                            .text_sm()
                            .text_color(rgb(palette.danger))
                            .child(locale.text(MessageKey::SettingsExportError)),
                    );
                }
                content
                    .child(export_card)
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
                                    .child(locale.text(MessageKey::SettingsData)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(palette.muted_text))
                                    .child(locale.text(MessageKey::SettingsDataBody)),
                            ),
                    )
                    .into_any_element()
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
            MessageKey::HealthPartial
        } else {
            MessageKey::HealthHealthy
        };
        let token_total = snapshot.tokens.checked_total().unwrap_or(0);
        let token_rows = [
            (
                locale.text(MessageKey::ModelsInputTokens),
                snapshot.tokens.input,
                palette.series[0],
            ),
            (
                locale.text(MessageKey::ModelsOutputTokens),
                snapshot.tokens.output,
                palette.series[1],
            ),
            (
                locale.text(MessageKey::ModelsCacheReadTokens),
                snapshot.tokens.cache_read,
                palette.series[2],
            ),
            (
                locale.text(MessageKey::ModelsCacheWriteTokens),
                snapshot.tokens.cache_write,
                palette.series[3],
            ),
            (
                locale.text(MessageKey::ModelsReasoningTokens),
                snapshot.tokens.reasoning,
                palette.series[4],
            ),
        ];

        div()
            .mt_6()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_6()
                    .pb_5()
                    .border_b_1()
                    .border_color(rgb(palette.border))
                    .child(headline_metric(
                        locale.text(MessageKey::TotalTokens),
                        total_tokens,
                        palette,
                    ))
                    .child(headline_metric(
                        locale.text(MessageKey::ApiEquivalentCost),
                        estimated_cost,
                        palette,
                    ))
                    .child(headline_metric(
                        locale.text(MessageKey::ProviderReportedCost),
                        provider_cost,
                        palette,
                    )),
            )
            .child(div().flex().flex_row().justify_between().children([
                metadata_metric(
                    locale.text(MessageKey::Sessions),
                    locale.format_count(snapshot.session_count),
                    palette,
                ),
                metadata_metric(
                    locale.text(MessageKey::ActiveDays),
                    locale.format_count(snapshot.active_days),
                    palette,
                ),
                metadata_metric(
                    locale.text(MessageKey::Models),
                    locale.format_count(snapshot.model_count),
                    palette,
                ),
                metadata_metric(
                    locale.text(MessageKey::OverviewEvents),
                    locale.format_count(snapshot.event_count),
                    palette,
                ),
            ]))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.surface))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(locale.text(MessageKey::OverviewTokenComposition)),
                            )
                            .child(div().mt_4().flex().flex_col().gap_3().children(
                                token_rows.into_iter().map(|(label, value, color)| {
                                    token_composition_row(
                                        label,
                                        value,
                                        token_total,
                                        color,
                                        locale,
                                        palette,
                                    )
                                }),
                            )),
                    )
                    .child(
                        div()
                            .w(px(220.0))
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
                                    .mt_2()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(7.0)).rounded_full().bg(rgb(
                                        if self.overview.load_state() == OverviewLoadState::Partial
                                        {
                                            palette.warning
                                        } else {
                                            palette.success
                                        },
                                    )))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(palette.muted_text))
                                            .child(locale.text(health_message)),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_5()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(locale.text(MessageKey::OverviewUsageQuality)),
                            )
                            .child(div().mt_2().flex().flex_col().gap_2().children([
                                quality_row(
                                    locale.text(MessageKey::ConfidenceExact),
                                    snapshot.data_quality.exact_events,
                                    locale,
                                    palette,
                                ),
                                quality_row(
                                    locale.text(MessageKey::ConfidenceDerived),
                                    snapshot.data_quality.derived_events,
                                    locale,
                                    palette,
                                ),
                                quality_row(
                                    locale.text(MessageKey::ConfidenceEstimated),
                                    snapshot.data_quality.estimated_events,
                                    locale,
                                    palette,
                                ),
                                quality_row(
                                    locale.text(MessageKey::PricingUnpricedEvents),
                                    snapshot.data_quality.unpriced_events,
                                    locale,
                                    palette,
                                ),
                                quality_row(
                                    locale.text(MessageKey::OverviewSources),
                                    snapshot.source_health.sources.len() as u64,
                                    locale,
                                    palette,
                                ),
                            ])),
                    ),
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
                    .w(px(216.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_4()
                    .bg(rgb(palette.sidebar))
                    .border_r_1()
                    .border_color(rgb(palette.border))
                    .child(
                        div()
                            .pb_4()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(32.0))
                                    .rounded_lg()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(rgb(palette.accent))
                                    .text_color(rgb(palette.accent_text))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("A"),
                            )
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("AgentMeter"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(palette.muted_text))
                                            .child(locale.text(MessageKey::AppSubtitle)),
                                    ),
                            ),
                    )
                    .child(self.nav_group_label(MessageKey::NavUsage, palette))
                    .children(
                        [Route::Overview, Route::Activity, Route::Sessions]
                            .into_iter()
                            .map(|route| self.nav_item(route, palette, cx)),
                    )
                    .child(self.nav_group_label(MessageKey::NavInsights, palette))
                    .children(
                        [Route::Models, Route::Pricing]
                            .into_iter()
                            .map(|route| self.nav_item(route, palette, cx)),
                    )
                    .child(div().flex_1())
                    .child(self.nav_group_label(MessageKey::NavSystem, palette))
                    .children(
                        [Route::Sources, Route::Settings]
                            .into_iter()
                            .map(|route| self.nav_item(route, palette, cx)),
                    ),
            )
            .child({
                let content = if selected == Route::Overview {
                    self.overview_content(palette)
                } else if selected == Route::Activity {
                    self.activity_content(palette, cx)
                } else if selected == Route::Sessions {
                    self.sessions_content(palette)
                } else if selected == Route::Models {
                    self.models_content(palette)
                } else if selected == Route::Pricing {
                    self.pricing_content(palette)
                } else if selected == Route::Sources {
                    self.sources_content(palette, cx)
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
                    .px_8()
                    .py_6()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.state.label(selected)),
                    )
                    .child(content)
            })
    }
}

fn headline_metric(label: &'static str, value: String, palette: ThemePalette) -> impl IntoElement {
    div()
        .flex_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(palette.subtle_text))
                .child(label),
        )
        .child(
            div()
                .mt_2()
                .text_2xl()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(value),
        )
}

fn metadata_metric(label: &'static str, value: String, palette: ThemePalette) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_baseline()
        .gap_2()
        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(value))
        .child(
            div()
                .text_xs()
                .text_color(rgb(palette.muted_text))
                .child(label),
        )
}

fn token_composition_row(
    label: &'static str,
    value: u64,
    total: u64,
    color: u32,
    locale: Locale,
    palette: ThemePalette,
) -> impl IntoElement {
    let width = if total == 0 {
        0.0
    } else {
        ((value as f64 / total as f64) * 260.0).max(if value == 0 { 0.0 } else { 2.0 })
    };
    div()
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_xs()
                .child(label)
                .child(
                    div()
                        .text_color(rgb(palette.muted_text))
                        .child(locale.format_count(value)),
                ),
        )
        .child(
            div()
                .mt_1()
                .h(px(4.0))
                .w_full()
                .rounded_full()
                .bg(rgb(palette.selected))
                .child(
                    div()
                        .h_full()
                        .w(px(width as f32))
                        .rounded_full()
                        .bg(rgb(color)),
                ),
        )
}

fn quality_row(
    label: &'static str,
    value: u64,
    locale: Locale,
    palette: ThemePalette,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .text_xs()
        .text_color(rgb(palette.muted_text))
        .child(label)
        .child(locale.format_count(value))
}

fn activity_option(
    id: &'static str,
    label: &'static str,
    selected: bool,
    palette: ThemePalette,
) -> gpui::Stateful<gpui::Div> {
    let option = div()
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
        .child(label);
    if selected {
        option
            .bg(rgb(palette.accent))
            .text_color(rgb(palette.accent_text))
    } else {
        option
            .text_color(rgb(palette.text))
            .hover(move |style| style.bg(rgb(palette.hover)))
    }
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

fn session_card(card: SessionCard, locale: Locale, palette: ThemePalette) -> impl IntoElement {
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
                .text_sm()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(MessageKey::SessionsSessionId)),
        )
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(card.session_id),
        )
        .child(
            div().mt_2().flex().flex_col().gap_1().children(
                card.detail
                    .into_iter()
                    .map(|(key, value)| detail_row(locale.text(key), value, palette)),
            ),
        )
        .children(card.unpriced.then(|| {
            div()
                .mt_2()
                .text_sm()
                .text_color(rgb(palette.warning))
                .child(locale.text(MessageKey::ActivityUnpriced))
        }))
}

fn models_pricing_error(
    title: &'static str,
    error: LocalDataErrorKind,
    locale: Locale,
    palette: ThemePalette,
) -> AnyElement {
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
                .child(title),
        )
        .child(
            div()
                .mt_2()
                .text_color(rgb(palette.muted_text))
                .child(locale.text(message)),
        )
        .into_any_element()
}

fn model_card(card: ModelCard, locale: Locale, palette: ThemePalette) -> impl IntoElement {
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
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(card.model),
        )
        .child(
            div().mt_2().flex().flex_col().gap_1().children(
                card.detail
                    .into_iter()
                    .map(|(key, value)| detail_row(locale.text(key), value, palette)),
            ),
        )
        .children(card.unpriced.then(|| {
            div()
                .mt_2()
                .text_sm()
                .text_color(rgb(palette.warning))
                .child(locale.text(MessageKey::ActivityUnpriced))
        }))
}

fn rate_card(card: RateCard, locale: Locale, palette: ThemePalette) -> impl IntoElement {
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
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(card.key),
        )
        .child(detail_row(
            locale.text(MessageKey::PricingAliases),
            card.aliases,
            palette,
        ))
        .children(
            card.detail
                .into_iter()
                .map(|(key, value)| detail_row(locale.text(key), value, palette)),
        )
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

fn current_unix_ms() -> i64 {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(milliseconds).unwrap_or(i64::MAX)
}

fn appearance_is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}
