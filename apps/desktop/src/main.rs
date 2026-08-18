#[cfg(target_os = "macos")]
mod gpui_app;

#[cfg(target_os = "macos")]
fn main() {
    gpui_app::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    use agentmeter_desktop::{Locale, MessageKey, ThemeMode};

    let locale = Locale::from_language_tag(std::env::var("LANG").as_deref().unwrap_or("en"));
    let theme = ThemeMode::System;

    println!("{} — {theme:?}", locale.text(MessageKey::AppSubtitle));
}
