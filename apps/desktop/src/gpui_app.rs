use std::path::PathBuf;

use agentmeter_desktop::{
    Locale, MessageKey, OverviewLoadErrorKind, OverviewLoadState, OverviewService, OverviewState,
    Route, ShellState, ThemeMode, ThemePalette,
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

        Self {
            state,
            overview,
            _overview_task: overview_task,
        }
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
                    OverviewLoadErrorKind::DataDirectory => MessageKey::OverviewDataDirectoryError,
                    OverviewLoadErrorKind::Database => MessageKey::OverviewDatabaseError,
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

fn macos_data_directory() -> Result<PathBuf, OverviewLoadErrorKind> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"))
        .ok_or(OverviewLoadErrorKind::DataDirectory)
}

fn appearance_is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}
