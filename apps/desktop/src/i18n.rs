#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    En,
    ZhCn,
}

impl Locale {
    pub fn from_language_tag(tag: &str) -> Self {
        let normalized = tag.trim().to_ascii_lowercase().replace('_', "-");
        if normalized == "zh" || normalized.starts_with("zh-") {
            Self::ZhCn
        } else {
            Self::En
        }
    }

    pub fn text(self, key: MessageKey) -> &'static str {
        match (self, key) {
            (Self::En, MessageKey::AppSubtitle) => "Local agent usage",
            (Self::En, MessageKey::Overview) => "Overview",
            (Self::En, MessageKey::Sessions) => "Sessions",
            (Self::En, MessageKey::Sources) => "Sources",
            (Self::En, MessageKey::Models) => "Models",
            (Self::En, MessageKey::Pricing) => "Pricing",
            (Self::En, MessageKey::Settings) => "Settings",
            (Self::En, MessageKey::CollectionHealth) => "Collection health",
            (Self::En, MessageKey::NeedsAttention) => "Needs attention",
            (Self::ZhCn, MessageKey::AppSubtitle) => "本地 Agent 用量",
            (Self::ZhCn, MessageKey::Overview) => "概览",
            (Self::ZhCn, MessageKey::Sessions) => "会话",
            (Self::ZhCn, MessageKey::Sources) => "数据源",
            (Self::ZhCn, MessageKey::Models) => "模型",
            (Self::ZhCn, MessageKey::Pricing) => "计价",
            (Self::ZhCn, MessageKey::Settings) => "设置",
            (Self::ZhCn, MessageKey::CollectionHealth) => "采集健康度",
            (Self::ZhCn, MessageKey::NeedsAttention) => "需要处理",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKey {
    AppSubtitle,
    Overview,
    Sessions,
    Sources,
    Models,
    Pricing,
    Settings,
    CollectionHealth,
    NeedsAttention,
}

#[cfg(test)]
mod tests {
    use super::{Locale, MessageKey};

    #[test]
    fn selects_chinese_for_common_language_tags() {
        assert_eq!(Locale::from_language_tag("zh-CN"), Locale::ZhCn);
        assert_eq!(Locale::from_language_tag("zh_CN.UTF-8"), Locale::ZhCn);
    }

    #[test]
    fn falls_back_to_english() {
        assert_eq!(Locale::from_language_tag("fr-FR"), Locale::En);
        assert_eq!(Locale::En.text(MessageKey::Overview), "Overview");
    }

    #[test]
    fn translates_chinese_navigation() {
        assert_eq!(Locale::ZhCn.text(MessageKey::Sources), "数据源");
    }
}
