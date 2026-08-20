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
                background: 0xf5f5f3,
                sidebar: 0xeeeeeb,
                surface: 0xffffff,
                text: 0x222220,
                muted_text: 0x71716c,
                subtle_text: 0x969690,
                border: 0xdededa,
                selected: 0xdededa,
                accent: 0x1478d4,
                accent_text: 0xffffff,
                hover: 0xe5e5e1,
                focus_ring: 0x0b6fc2,
                success: 0x1a7f37,
                warning: 0x9a6700,
                danger: 0xcf222e,
                info: 0x0969da,
                series: [0x1677d2, 0x1a7f37, 0x9a6700, 0x8250df, 0xcf222e, 0x0f766e],
            },
            Self::Dark => ThemePalette {
                background: 0x1b1b1a,
                sidebar: 0x222221,
                surface: 0x252524,
                text: 0xf2f2ef,
                muted_text: 0xadada7,
                subtle_text: 0x7f7f7a,
                border: 0x3a3a38,
                selected: 0x383836,
                accent: 0x57a8f5,
                accent_text: 0x0d2235,
                hover: 0x30302e,
                focus_ring: 0x86c5ff,
                success: 0x3fb950,
                warning: 0xd29922,
                danger: 0xf85149,
                info: 0x58a6ff,
                series: [0x57a8f5, 0x3fb950, 0xd29922, 0xbc8cff, 0xf85149, 0x39c5cf],
            },
        }
    }
}

/// Semantic colors are centralized here so views never branch on light/dark
/// mode or embed literal color values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemePalette {
    pub background: u32,
    pub sidebar: u32,
    pub surface: u32,
    pub text: u32,
    pub muted_text: u32,
    pub subtle_text: u32,
    pub border: u32,
    pub selected: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub hover: u32,
    pub focus_ring: u32,
    pub success: u32,
    pub warning: u32,
    pub danger: u32,
    pub info: u32,
    pub series: [u32; 6],
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

    #[test]
    fn status_colors_stay_distinct_in_both_themes() {
        for palette in [
            ResolvedTheme::Light.palette(),
            ResolvedTheme::Dark.palette(),
        ] {
            assert_ne!(palette.sidebar, palette.background);
            assert_ne!(palette.selected, palette.sidebar);
            assert_ne!(palette.subtle_text, palette.sidebar);
            let status_colors = [
                palette.success,
                palette.warning,
                palette.danger,
                palette.info,
                palette.muted_text,
            ];
            for (index, color) in status_colors.iter().enumerate() {
                assert!(
                    !status_colors[..index].contains(color),
                    "status colors must stay distinguishable"
                );
                assert_ne!(
                    *color, palette.background,
                    "status must be visible on the background"
                );
                assert_ne!(
                    *color, palette.surface,
                    "status must be visible on surfaces"
                );
            }
            for (index, color) in palette.series.iter().enumerate() {
                assert!(!palette.series[..index].contains(color));
                assert_ne!(*color, palette.background);
                assert_ne!(*color, palette.surface);
            }
        }
    }
}
