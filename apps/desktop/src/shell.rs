use crate::{Locale, MessageKey, ResolvedTheme, ThemeMode, ThemePalette};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Route {
    #[default]
    Overview,
    Activity,
    Sessions,
    Sources,
    Models,
    Pricing,
    Settings,
}

impl Route {
    pub const ALL: [Self; 7] = [
        Self::Overview,
        Self::Activity,
        Self::Sessions,
        Self::Models,
        Self::Pricing,
        Self::Sources,
        Self::Settings,
    ];

    pub const fn message_key(self) -> MessageKey {
        match self {
            Self::Overview => MessageKey::Overview,
            Self::Activity => MessageKey::Activity,
            Self::Sessions => MessageKey::Sessions,
            Self::Sources => MessageKey::Sources,
            Self::Models => MessageKey::Models,
            Self::Pricing => MessageKey::Pricing,
            Self::Settings => MessageKey::Settings,
        }
    }

    pub const fn element_id(self) -> &'static str {
        match self {
            Self::Overview => "nav-overview",
            Self::Activity => "nav-activity",
            Self::Sessions => "nav-sessions",
            Self::Sources => "nav-sources",
            Self::Models => "nav-models",
            Self::Pricing => "nav-pricing",
            Self::Settings => "nav-settings",
        }
    }

    /// Automation hook for headless validation: maps a route name from the
    /// `AGENTMETER_INITIAL_ROUTE` environment variable so continuous
    /// integration can capture every view. Unknown values keep the default.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "overview" => Some(Self::Overview),
            "activity" => Some(Self::Activity),
            "sessions" => Some(Self::Sessions),
            "sources" => Some(Self::Sources),
            "models" => Some(Self::Models),
            "pricing" => Some(Self::Pricing),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellState {
    locale: Locale,
    theme_mode: ThemeMode,
    system_is_dark: bool,
    selected_route: Route,
}

impl ShellState {
    pub const fn new(locale: Locale, theme_mode: ThemeMode, system_is_dark: bool) -> Self {
        Self {
            locale,
            theme_mode,
            system_is_dark,
            selected_route: Route::Overview,
        }
    }

    pub const fn locale(&self) -> Locale {
        self.locale
    }

    pub const fn theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }

    pub const fn selected_route(&self) -> Route {
        self.selected_route
    }

    pub fn select(&mut self, route: Route) {
        self.selected_route = route;
    }

    pub fn set_system_is_dark(&mut self, system_is_dark: bool) {
        self.system_is_dark = system_is_dark;
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    pub fn set_theme_mode(&mut self, theme_mode: ThemeMode) {
        self.theme_mode = theme_mode;
    }

    pub fn label(&self, route: Route) -> &'static str {
        self.locale.text(route.message_key())
    }

    pub fn resolved_theme(&self) -> ResolvedTheme {
        self.theme_mode.resolve(self.system_is_dark)
    }

    pub fn palette(&self) -> ThemePalette {
        self.resolved_theme().palette()
    }
}

#[cfg(test)]
mod tests {
    use super::{Route, ShellState};
    use crate::{Locale, ResolvedTheme, ThemeMode};

    #[test]
    fn exposes_complete_localized_navigation() {
        let english = ShellState::new(Locale::En, ThemeMode::System, false);
        let chinese = ShellState::new(Locale::ZhCn, ThemeMode::System, false);

        assert_eq!(Route::ALL.len(), 7);
        for route in Route::ALL {
            assert!(!english.label(route).is_empty());
            assert!(!chinese.label(route).is_empty());
            assert_ne!(english.label(route), chinese.label(route));
        }
    }

    #[test]
    fn navigation_and_system_appearance_update_portable_state() {
        let mut shell = ShellState::new(Locale::En, ThemeMode::System, false);
        assert_eq!(shell.selected_route(), Route::Overview);
        assert_eq!(shell.resolved_theme(), ResolvedTheme::Light);

        shell.select(Route::Sources);
        shell.set_locale(Locale::ZhCn);
        shell.set_system_is_dark(true);
        assert_eq!(shell.selected_route(), Route::Sources);
        assert_eq!(shell.label(Route::Sources), "数据源");
        assert_eq!(shell.resolved_theme(), ResolvedTheme::Dark);

        shell.set_theme_mode(ThemeMode::Light);
        assert_eq!(shell.selected_route(), Route::Sources);
        assert_eq!(shell.resolved_theme(), ResolvedTheme::Light);
    }

    #[test]
    fn parses_automation_route_names_and_rejects_unknown_values() {
        assert_eq!(Route::from_name("sources"), Some(Route::Sources));
        assert_eq!(Route::from_name(" Settings "), Some(Route::Settings));
        assert_eq!(Route::from_name("OVERVIEW"), Some(Route::Overview));
        assert_eq!(Route::from_name("activity"), Some(Route::Activity));
        assert_eq!(Route::from_name("sessions"), Some(Route::Sessions));
        assert_eq!(Route::from_name("models"), Some(Route::Models));
        assert_eq!(Route::from_name("pricing"), Some(Route::Pricing));
        assert_eq!(Route::from_name("klingon"), None);
        assert_eq!(Route::from_name(""), None);
    }
}
