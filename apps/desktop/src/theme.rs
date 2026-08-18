#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn resolve(self, system_is_dark: bool) -> ResolvedTheme {
        match self {
            Self::System if system_is_dark => ResolvedTheme::Dark,
            Self::System | Self::Light => ResolvedTheme::Light,
            Self::Dark => ResolvedTheme::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedTheme {
    Light,
    Dark,
}

impl ResolvedTheme {
    pub fn palette(self) -> ThemePalette {
        match self {
            Self::Light => ThemePalette {
                background: 0xf7f7f8,
                surface: 0xffffff,
                text: 0x202124,
                muted_text: 0x6f737b,
                border: 0xdfe1e5,
                accent: 0x1677d2,
            },
            Self::Dark => ThemePalette {
                background: 0x151617,
                surface: 0x1f2022,
                text: 0xf2f3f5,
                muted_text: 0xa7abb3,
                border: 0x34363a,
                accent: 0x57a8f5,
            },
        }
    }
}

/// Semantic colors are centralized here so views never branch on light/dark
/// mode or embed literal color values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemePalette {
    pub background: u32,
    pub surface: u32,
    pub text: u32,
    pub muted_text: u32,
    pub border: u32,
    pub accent: u32,
}

#[cfg(test)]
mod tests {
    use super::{ResolvedTheme, ThemeMode};

    #[test]
    fn system_theme_tracks_platform_appearance() {
        assert_eq!(ThemeMode::System.resolve(false), ResolvedTheme::Light);
        assert_eq!(ThemeMode::System.resolve(true), ResolvedTheme::Dark);
    }

    #[test]
    fn explicit_theme_ignores_platform_appearance() {
        assert_eq!(ThemeMode::Light.resolve(true), ResolvedTheme::Light);
        assert_eq!(ThemeMode::Dark.resolve(false), ResolvedTheme::Dark);
    }
}
