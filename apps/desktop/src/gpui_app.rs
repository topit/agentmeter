use agentmeter_desktop::{Locale, MessageKey, Route, ShellState, ThemeMode, ThemePalette};
use gpui::{
    App, Bounds, Context, IntoElement, Render, TitlebarOptions, Window, WindowAppearance,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
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
        Self { state }
    }

    fn nav_item(
        &self,
        route: Route,
        palette: ThemePalette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
            .child(
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
                    .child(
                        div()
                            .mt_3()
                            .max_w(px(560.0))
                            .text_color(rgb(palette.muted_text))
                            .child(locale.text(MessageKey::ShellPlaceholder)),
                    ),
            )
    }
}

fn appearance_is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}
