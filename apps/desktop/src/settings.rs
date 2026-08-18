use agentmeter_app::LocalDataErrorKind;
use agentmeter_core::{AppPreferences, AppearancePreference, LanguagePreference};

use crate::{Locale, ThemeMode};

#[derive(Debug, Eq, PartialEq)]
pub struct SettingsRequest(u64);

#[derive(Debug, Eq, PartialEq)]
pub struct SaveRequest(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsLoadState {
    #[default]
    Loading,
    Loaded,
    Error(LocalDataErrorKind),
}

/// Presentation state for the Settings screen. Selections apply optimistically
/// so the UI repaints immediately; persistence runs through the application
/// service off the render path, and single-use request generations reject
/// out-of-order load and save completions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsState {
    latest_load: u64,
    latest_save: u64,
    load_state: SettingsLoadState,
    preferences: Option<AppPreferences>,
    save_error: Option<LocalDataErrorKind>,
}

impl SettingsState {
    pub fn begin_load(&mut self) -> SettingsRequest {
        self.latest_load = self
            .latest_load
            .checked_add(1)
            .expect("settings load generation overflowed");
        self.load_state = SettingsLoadState::Loading;
        SettingsRequest(self.latest_load)
    }

    pub fn apply_loaded(&mut self, request: SettingsRequest, preferences: AppPreferences) -> bool {
        if request.0 != self.latest_load {
            return false;
        }
        self.load_state = SettingsLoadState::Loaded;
        self.preferences = Some(preferences);
        true
    }

    pub fn apply_load_error(
        &mut self,
        request: SettingsRequest,
        error: LocalDataErrorKind,
    ) -> bool {
        if request.0 != self.latest_load {
            return false;
        }
        self.load_state = SettingsLoadState::Error(error);
        true
    }

    pub fn begin_save(&mut self) -> SaveRequest {
        self.latest_save = self
            .latest_save
            .checked_add(1)
            .expect("settings save generation overflowed");
        self.save_error = None;
        SaveRequest(self.latest_save)
    }

    /// Applies a selection immediately. The next save persists the complete
    /// preference snapshot, so rapid consecutive selections stay last-write-wins.
    pub fn select_language(&mut self, language: LanguagePreference) {
        if let Some(preferences) = self.preferences.as_mut() {
            preferences.language = language;
        }
    }

    pub fn select_appearance(&mut self, appearance: AppearancePreference) {
        if let Some(preferences) = self.preferences.as_mut() {
            preferences.appearance = appearance;
        }
    }

    pub fn apply_save_result(
        &mut self,
        request: SaveRequest,
        result: Result<(), LocalDataErrorKind>,
    ) -> bool {
        if request.0 != self.latest_save {
            return false;
        }
        self.save_error = result.err();
        true
    }

    pub const fn load_state(&self) -> SettingsLoadState {
        self.load_state
    }

    pub const fn preferences(&self) -> Option<AppPreferences> {
        self.preferences
    }

    pub const fn save_error(&self) -> Option<LocalDataErrorKind> {
        self.save_error
    }
}

/// Resolves the display locale for a language preference; `System` keeps the
/// platform locale detected at startup.
pub const fn resolved_locale(system: Locale, language: LanguagePreference) -> Locale {
    match language {
        LanguagePreference::System => system,
        LanguagePreference::English => Locale::En,
        LanguagePreference::SimplifiedChinese => Locale::ZhCn,
    }
}

pub const fn resolved_theme_mode(appearance: AppearancePreference) -> ThemeMode {
    match appearance {
        AppearancePreference::System => ThemeMode::System,
        AppearancePreference::Light => ThemeMode::Light,
        AppearancePreference::Dark => ThemeMode::Dark,
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::LocalDataErrorKind;
    use agentmeter_core::{AppPreferences, AppearancePreference, LanguagePreference};

    use super::{SettingsLoadState, SettingsState, resolved_locale, resolved_theme_mode};
    use crate::{Locale, ThemeMode};

    #[test]
    fn rejects_an_out_of_order_loaded_snapshot_and_error() {
        let mut state = SettingsState::default();
        let first = state.begin_load();
        let second = state.begin_load();

        assert!(state.apply_loaded(
            second,
            AppPreferences {
                language: LanguagePreference::SimplifiedChinese,
                appearance: AppearancePreference::Dark,
            }
        ));
        assert!(!state.apply_loaded(first, AppPreferences::default()));
        assert_eq!(state.load_state(), SettingsLoadState::Loaded);
        assert_eq!(
            state.preferences().unwrap().language,
            LanguagePreference::SimplifiedChinese,
            "a stale completion must not replace the current preferences"
        );

        let stale = state.begin_load();
        let current = state.begin_load();
        assert!(!state.apply_load_error(stale, LocalDataErrorKind::Database));
        assert!(state.apply_load_error(current, LocalDataErrorKind::DataDirectory));
        assert_eq!(
            state.load_state(),
            SettingsLoadState::Error(LocalDataErrorKind::DataDirectory)
        );
    }

    #[test]
    fn selections_apply_optimistically_and_save_results_reject_stale_responses() {
        let mut state = SettingsState::default();
        let request = state.begin_load();
        assert!(state.apply_loaded(request, AppPreferences::default()));

        state.select_language(LanguagePreference::SimplifiedChinese);
        state.select_appearance(AppearancePreference::Dark);
        assert_eq!(
            state.preferences(),
            Some(AppPreferences {
                language: LanguagePreference::SimplifiedChinese,
                appearance: AppearancePreference::Dark,
            })
        );

        let first_save = state.begin_save();
        let second_save = state.begin_save();
        assert!(!state.apply_save_result(first_save, Err(LocalDataErrorKind::Database)));
        assert_eq!(state.save_error(), None, "a stale save cannot set an error");

        assert!(state.apply_save_result(second_save, Err(LocalDataErrorKind::Database)));
        assert_eq!(state.save_error(), Some(LocalDataErrorKind::Database));

        let retry = state.begin_save();
        assert_eq!(state.save_error(), None, "a new attempt clears the error");
        assert!(state.apply_save_result(retry, Ok(())));
        assert_eq!(state.save_error(), None);
    }

    #[test]
    fn selections_before_loading_are_ignored() {
        let mut state = SettingsState::default();

        state.select_language(LanguagePreference::English);

        assert_eq!(state.preferences(), None);
    }

    #[test]
    fn resolves_language_and_appearance_preferences() {
        assert_eq!(
            resolved_locale(Locale::ZhCn, LanguagePreference::System),
            Locale::ZhCn
        );
        assert_eq!(
            resolved_locale(Locale::ZhCn, LanguagePreference::English),
            Locale::En
        );
        assert_eq!(
            resolved_theme_mode(AppearancePreference::Dark),
            ThemeMode::Dark
        );
        assert_eq!(
            resolved_theme_mode(AppearancePreference::System),
            ThemeMode::System
        );
    }
}
