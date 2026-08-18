use agentmeter_core::{SourceHealthState, SourceRemediation};

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
            (Self::En, MessageKey::HealthHealthy) => "Healthy",
            (Self::En, MessageKey::HealthPartial) => "Partial data",
            (Self::En, MessageKey::HealthSetupRequired) => "Setup required",
            (Self::En, MessageKey::HealthUnsupportedSchema) => "Unsupported source format",
            (Self::En, MessageKey::HealthError) => "Collection error",
            (Self::En, MessageKey::HealthDisabled) => "Disabled",
            (Self::En, MessageKey::RemediationConfigurePath) => "Configure the source path.",
            (Self::En, MessageKey::RemediationGrantPermission) => {
                "Grant permission to read this source."
            }
            (Self::En, MessageKey::RemediationUpgradeAgentMeter) => {
                "Update AgentMeter to support this source format."
            }
            (Self::En, MessageKey::RemediationRetryCollection) => "Retry collection.",
            (Self::En, MessageKey::RemediationReviewWarnings) => "Review the collection warnings.",
            (Self::ZhCn, MessageKey::AppSubtitle) => "本地 Agent 用量",
            (Self::ZhCn, MessageKey::Overview) => "概览",
            (Self::ZhCn, MessageKey::Sessions) => "会话",
            (Self::ZhCn, MessageKey::Sources) => "数据源",
            (Self::ZhCn, MessageKey::Models) => "模型",
            (Self::ZhCn, MessageKey::Pricing) => "计价",
            (Self::ZhCn, MessageKey::Settings) => "设置",
            (Self::ZhCn, MessageKey::CollectionHealth) => "采集健康度",
            (Self::ZhCn, MessageKey::NeedsAttention) => "需要处理",
            (Self::ZhCn, MessageKey::HealthHealthy) => "健康",
            (Self::ZhCn, MessageKey::HealthPartial) => "数据不完整",
            (Self::ZhCn, MessageKey::HealthSetupRequired) => "需要设置",
            (Self::ZhCn, MessageKey::HealthUnsupportedSchema) => "不支持的数据格式",
            (Self::ZhCn, MessageKey::HealthError) => "采集错误",
            (Self::ZhCn, MessageKey::HealthDisabled) => "已停用",
            (Self::ZhCn, MessageKey::RemediationConfigurePath) => "请配置数据源路径。",
            (Self::ZhCn, MessageKey::RemediationGrantPermission) => "请授予此数据源的读取权限。",
            (Self::ZhCn, MessageKey::RemediationUpgradeAgentMeter) => {
                "请更新 AgentMeter 以支持此数据格式。"
            }
            (Self::ZhCn, MessageKey::RemediationRetryCollection) => "请重试采集。",
            (Self::ZhCn, MessageKey::RemediationReviewWarnings) => "请查看采集警告。",
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
    HealthHealthy,
    HealthPartial,
    HealthSetupRequired,
    HealthUnsupportedSchema,
    HealthError,
    HealthDisabled,
    RemediationConfigurePath,
    RemediationGrantPermission,
    RemediationUpgradeAgentMeter,
    RemediationRetryCollection,
    RemediationReviewWarnings,
}

pub fn health_state_key(state: SourceHealthState) -> MessageKey {
    match state {
        SourceHealthState::Healthy => MessageKey::HealthHealthy,
        SourceHealthState::Partial => MessageKey::HealthPartial,
        SourceHealthState::SetupRequired => MessageKey::HealthSetupRequired,
        SourceHealthState::UnsupportedSchema => MessageKey::HealthUnsupportedSchema,
        SourceHealthState::Error => MessageKey::HealthError,
        SourceHealthState::Disabled => MessageKey::HealthDisabled,
    }
}

pub fn remediation_key(remediation: SourceRemediation) -> MessageKey {
    match remediation {
        SourceRemediation::ConfigurePath => MessageKey::RemediationConfigurePath,
        SourceRemediation::GrantPermission => MessageKey::RemediationGrantPermission,
        SourceRemediation::UpgradeAgentMeter => MessageKey::RemediationUpgradeAgentMeter,
        SourceRemediation::RetryCollection => MessageKey::RemediationRetryCollection,
        SourceRemediation::ReviewWarnings => MessageKey::RemediationReviewWarnings,
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{SourceHealthState, SourceRemediation};

    use super::{Locale, MessageKey, health_state_key, remediation_key};

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

    #[test]
    fn localizes_every_health_state_and_remediation() {
        for state in [
            SourceHealthState::Healthy,
            SourceHealthState::Partial,
            SourceHealthState::SetupRequired,
            SourceHealthState::UnsupportedSchema,
            SourceHealthState::Error,
            SourceHealthState::Disabled,
        ] {
            let key = health_state_key(state);
            assert!(!Locale::En.text(key).is_empty());
            assert!(!Locale::ZhCn.text(key).is_empty());
        }
        for remediation in [
            SourceRemediation::ConfigurePath,
            SourceRemediation::GrantPermission,
            SourceRemediation::UpgradeAgentMeter,
            SourceRemediation::RetryCollection,
            SourceRemediation::ReviewWarnings,
        ] {
            let key = remediation_key(remediation);
            assert!(!Locale::En.text(key).is_empty());
            assert!(!Locale::ZhCn.text(key).is_empty());
        }
    }
}
