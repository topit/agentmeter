use agentmeter_desktop::{Locale, MessageKey, ThemeMode};

fn main() {
    let locale = Locale::from_language_tag(std::env::var("LANG").as_deref().unwrap_or("en"));
    let theme = ThemeMode::System;

    println!("{} — {theme:?}", locale.text(MessageKey::AppSubtitle));
}
