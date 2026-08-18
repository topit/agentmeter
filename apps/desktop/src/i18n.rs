use agentmeter_core::{NanoUsd, SourceHealthState, SourcePermissionState, SourceRemediation};

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
            (Self::En, MessageKey::SourcesLoading) => "Loading sources…",
            (Self::En, MessageKey::SourcesEmptyTitle) => "No sources configured yet",
            (Self::En, MessageKey::SourcesEmptyBody) => {
                "Discovered agent installations and their collection status will appear here."
            }
            (Self::En, MessageKey::SourcesErrorTitle) => "Sources unavailable",
            (Self::En, MessageKey::SourcesAdapter) => "Adapter",
            (Self::En, MessageKey::SourcesSourcePath) => "Path",
            (Self::En, MessageKey::SourcesSourceKind) => "Type",
            (Self::En, MessageKey::SourcesParserVersion) => "Parser version",
            (Self::En, MessageKey::SourcesPermission) => "Permission",
            (Self::En, MessageKey::SourcesLastScan) => "Last scan",
            (Self::En, MessageKey::SourcesLastSuccess) => "Last successful scan",
            (Self::En, MessageKey::SourcesLastEvent) => "Latest event",
            (Self::En, MessageKey::SourcesRecordsChanged) => "Records changed",
            (Self::En, MessageKey::SourcesWarnings) => "Warnings",
            (Self::En, MessageKey::SourcesErrorLabel) => "Error",
            (Self::En, MessageKey::SourcesRemediation) => "Next step",
            (Self::En, MessageKey::PermissionUnknown) => "Unknown",
            (Self::En, MessageKey::PermissionGranted) => "Granted",
            (Self::En, MessageKey::PermissionDenied) => "Denied",
            (Self::En, MessageKey::PermissionMissing) => "Missing",
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
            (Self::ZhCn, MessageKey::SourcesLoading) => "正在加载数据源…",
            (Self::ZhCn, MessageKey::SourcesEmptyTitle) => "暂无已配置的数据源",
            (Self::ZhCn, MessageKey::SourcesEmptyBody) => {
                "发现的 Agent 安装及其采集状态将在这里显示。"
            }
            (Self::ZhCn, MessageKey::SourcesErrorTitle) => "数据源暂不可用",
            (Self::ZhCn, MessageKey::SourcesAdapter) => "适配器",
            (Self::ZhCn, MessageKey::SourcesSourcePath) => "路径",
            (Self::ZhCn, MessageKey::SourcesSourceKind) => "类型",
            (Self::ZhCn, MessageKey::SourcesParserVersion) => "解析器版本",
            (Self::ZhCn, MessageKey::SourcesPermission) => "权限",
            (Self::ZhCn, MessageKey::SourcesLastScan) => "最近扫描",
            (Self::ZhCn, MessageKey::SourcesLastSuccess) => "最近成功扫描",
            (Self::ZhCn, MessageKey::SourcesLastEvent) => "最近事件",
            (Self::ZhCn, MessageKey::SourcesRecordsChanged) => "变更记录数",
            (Self::ZhCn, MessageKey::SourcesWarnings) => "警告",
            (Self::ZhCn, MessageKey::SourcesErrorLabel) => "错误",
            (Self::ZhCn, MessageKey::SourcesRemediation) => "处理建议",
            (Self::ZhCn, MessageKey::PermissionUnknown) => "未知",
            (Self::ZhCn, MessageKey::PermissionGranted) => "已授权",
            (Self::ZhCn, MessageKey::PermissionDenied) => "已拒绝",
            (Self::ZhCn, MessageKey::PermissionMissing) => "缺失",
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

    /// Formats a Unix millisecond instant as an explicitly UTC timestamp so
    /// collection facts never masquerade as local time.
    pub fn format_unix_ms_utc(self, unix_ms: i64) -> String {
        let (year, month, day, hour, minute) = utc_civil(unix_ms);
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
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
    SourcesLoading,
    SourcesEmptyTitle,
    SourcesEmptyBody,
    SourcesErrorTitle,
    SourcesAdapter,
    SourcesSourcePath,
    SourcesSourceKind,
    SourcesParserVersion,
    SourcesPermission,
    SourcesLastScan,
    SourcesLastSuccess,
    SourcesLastEvent,
    SourcesRecordsChanged,
    SourcesWarnings,
    SourcesErrorLabel,
    SourcesRemediation,
    PermissionUnknown,
    PermissionGranted,
    PermissionDenied,
    PermissionMissing,
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

pub fn permission_key(permission: SourcePermissionState) -> MessageKey {
    match permission {
        SourcePermissionState::Unknown => MessageKey::PermissionUnknown,
        SourcePermissionState::Granted => MessageKey::PermissionGranted,
        SourcePermissionState::Denied => MessageKey::PermissionDenied,
        SourcePermissionState::Missing => MessageKey::PermissionMissing,
    }
}

/// Splits a Unix millisecond instant into UTC calendar parts. Uses the
/// floor-division civil-date algorithm so negative instants stay correct.
fn utc_civil(unix_ms: i64) -> (i64, u32, u32, u32, u32) {
    let days = unix_ms.div_euclid(86_400_000);
    let seconds_of_day = unix_ms.rem_euclid(86_400_000) / 1000;
    let hour = (seconds_of_day / 3_600) as u32;
    let minute = (seconds_of_day % 3_600 / 60) as u32;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, hour, minute)
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{NanoUsd, SourceHealthState, SourcePermissionState, SourceRemediation};

    use super::{Locale, MessageKey, health_state_key, permission_key, remediation_key};

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
        for permission in [
            SourcePermissionState::Unknown,
            SourcePermissionState::Granted,
            SourcePermissionState::Denied,
            SourcePermissionState::Missing,
        ] {
            let key = permission_key(permission);
            assert!(!Locale::En.text(key).is_empty());
            assert!(!Locale::ZhCn.text(key).is_empty());
            assert_ne!(Locale::En.text(key), Locale::ZhCn.text(key));
        }
    }

    #[test]
    fn localizes_every_sources_state_label_and_error() {
        for key in [
            MessageKey::SourcesLoading,
            MessageKey::SourcesEmptyTitle,
            MessageKey::SourcesEmptyBody,
            MessageKey::SourcesErrorTitle,
            MessageKey::SourcesAdapter,
            MessageKey::SourcesSourcePath,
            MessageKey::SourcesSourceKind,
            MessageKey::SourcesParserVersion,
            MessageKey::SourcesPermission,
            MessageKey::SourcesLastScan,
            MessageKey::SourcesLastSuccess,
            MessageKey::SourcesLastEvent,
            MessageKey::SourcesRecordsChanged,
            MessageKey::SourcesWarnings,
            MessageKey::SourcesErrorLabel,
            MessageKey::SourcesRemediation,
            MessageKey::OverviewDataDirectoryError,
            MessageKey::OverviewDatabaseError,
        ] {
            assert!(!Locale::En.text(key).is_empty());
            assert!(!Locale::ZhCn.text(key).is_empty());
            assert_ne!(Locale::En.text(key), Locale::ZhCn.text(key));
        }
    }

    #[test]
    fn formats_explicit_utc_timestamps() {
        assert_eq!(Locale::En.format_unix_ms_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(
            Locale::ZhCn.format_unix_ms_utc(1_787_011_200_000),
            "2026-08-18 00:00 UTC"
        );
        assert_eq!(
            Locale::En.format_unix_ms_utc(1_787_046_083_000),
            "2026-08-18 09:41 UTC"
        );
        assert_eq!(
            Locale::En.format_unix_ms_utc(951_827_696_000),
            "2000-02-29 12:34 UTC"
        );
        assert_eq!(Locale::En.format_unix_ms_utc(-1), "1969-12-31 23:59 UTC");
    }
}
