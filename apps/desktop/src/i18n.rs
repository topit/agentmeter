use agentmeter_core::{NanoUsd, SourceHealthState, SourceRemediation};

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
            (Self::En, MessageKey::ShellPlaceholder) => {
                "Usage views will appear here as local snapshots become available."
            }
            (Self::En, MessageKey::OverviewLoading) => "Loading local usage…",
            (Self::En, MessageKey::OverviewEmptyTitle) => "No usage recorded yet",
            (Self::En, MessageKey::OverviewEmptyBody) => {
                "Usage will appear after AgentMeter completes a successful local scan."
            }
            (Self::En, MessageKey::OverviewPartial) => {
                "Some sources need attention. Totals include the data currently available."
            }
            (Self::En, MessageKey::OverviewErrorTitle) => "Overview unavailable",
            (Self::En, MessageKey::OverviewDataDirectoryError) => {
                "AgentMeter could not prepare its local data folder."
            }
            (Self::En, MessageKey::OverviewDatabaseError) => {
                "AgentMeter could not read its local usage database."
            }
            (Self::En, MessageKey::TotalTokens) => "Total tokens",
            (Self::En, MessageKey::ActiveDays) => "Active days",
            (Self::En, MessageKey::ProviderReportedCost) => "Provider-reported cost",
            (Self::En, MessageKey::ApiEquivalentCost) => "API-equivalent estimate",
            (Self::En, MessageKey::NotAvailable) => "Not available",
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
            (Self::ZhCn, MessageKey::ShellPlaceholder) => {
                "本地数据快照可用后，用量视图将在这里显示。"
            }
            (Self::ZhCn, MessageKey::OverviewLoading) => "正在加载本地用量…",
            (Self::ZhCn, MessageKey::OverviewEmptyTitle) => "暂无用量记录",
            (Self::ZhCn, MessageKey::OverviewEmptyBody) => {
                "AgentMeter 成功完成本地扫描后，用量数据将在这里显示。"
            }
            (Self::ZhCn, MessageKey::OverviewPartial) => {
                "部分数据源需要处理。当前总计仅包含已有数据。"
            }
            (Self::ZhCn, MessageKey::OverviewErrorTitle) => "概览暂不可用",
            (Self::ZhCn, MessageKey::OverviewDataDirectoryError) => {
                "AgentMeter 无法准备本地数据文件夹。"
            }
            (Self::ZhCn, MessageKey::OverviewDatabaseError) => {
                "AgentMeter 无法读取本地用量数据库。"
            }
            (Self::ZhCn, MessageKey::TotalTokens) => "Token 总量",
            (Self::ZhCn, MessageKey::ActiveDays) => "活跃天数",
            (Self::ZhCn, MessageKey::ProviderReportedCost) => "服务商报告成本",
            (Self::ZhCn, MessageKey::ApiEquivalentCost) => "API 等价估算",
            (Self::ZhCn, MessageKey::NotAvailable) => "暂无数据",
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

    pub fn format_count(self, value: u64) -> String {
        let digits = value.to_string();
        let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
        for (index, character) in digits.chars().enumerate() {
            if index != 0 && (digits.len() - index).is_multiple_of(3) {
                formatted.push(',');
            }
            formatted.push(character);
        }
        formatted
    }

    pub fn format_usd(self, value: NanoUsd) -> String {
        let nanos = value.as_nanos();
        let micros = nanos / 1_000 + u64::from(nanos % 1_000 >= 500);
        let whole = micros / 1_000_000;
        let fraction = format!("{:06}", micros % 1_000_000);
        let mut fraction = fraction.trim_end_matches('0').to_owned();
        while fraction.len() < 2 {
            fraction.push('0');
        }
        let prefix = match self {
            Self::En => "$",
            Self::ZhCn => "US$",
        };
        format!("{prefix}{}.{fraction}", self.format_count(whole))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKey {
    AppSubtitle,
    ShellPlaceholder,
    OverviewLoading,
    OverviewEmptyTitle,
    OverviewEmptyBody,
    OverviewPartial,
    OverviewErrorTitle,
    OverviewDataDirectoryError,
    OverviewDatabaseError,
    TotalTokens,
    ActiveDays,
    ProviderReportedCost,
    ApiEquivalentCost,
    NotAvailable,
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
    use agentmeter_core::{NanoUsd, SourceHealthState, SourceRemediation};

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
    fn formats_counts_and_usd_for_each_locale() {
        assert_eq!(Locale::En.format_count(1_234_567), "1,234,567");
        assert_eq!(
            Locale::En.format_usd(NanoUsd::from_nanos(1_250_000_000)),
            "$1.25"
        );
        assert_eq!(
            Locale::ZhCn.format_usd(NanoUsd::from_nanos(1_000_000)),
            "US$0.001"
        );
    }

    #[test]
    fn localizes_every_overview_state_and_metric() {
        for key in [
            MessageKey::OverviewLoading,
            MessageKey::OverviewEmptyTitle,
            MessageKey::OverviewEmptyBody,
            MessageKey::OverviewPartial,
            MessageKey::OverviewErrorTitle,
            MessageKey::OverviewDataDirectoryError,
            MessageKey::OverviewDatabaseError,
            MessageKey::TotalTokens,
            MessageKey::ActiveDays,
            MessageKey::ProviderReportedCost,
            MessageKey::ApiEquivalentCost,
            MessageKey::NotAvailable,
        ] {
            assert!(!Locale::En.text(key).is_empty());
            assert!(!Locale::ZhCn.text(key).is_empty());
            assert_ne!(Locale::En.text(key), Locale::ZhCn.text(key));
        }
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
