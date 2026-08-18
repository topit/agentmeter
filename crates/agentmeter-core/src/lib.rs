//! Shared, platform-independent AgentMeter domain types.

/// Canonical token buckets. Adapters must normalize these buckets so they are
/// mutually exclusive; source-reported totals are retained separately.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenBreakdown {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl TokenBreakdown {
    /// Returns the normalized total, or `None` if malformed source data would
    /// overflow a `u64`.
    pub fn checked_total(self) -> Option<u64> {
        [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            self.reasoning,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }
}

/// How trustworthy a normalized event is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataConfidence {
    Exact,
    Derived,
    Estimated,
}

/// Where the event timestamp came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampOrigin {
    Source,
    Derived,
    FileModified,
}

/// One normalized usage event. Client, provider, and model are intentionally
/// separate dimensions: for example, Pi can call a DeepSeek model through
/// OpenRouter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageEvent {
    pub id: String,
    pub source_id: String,
    pub session_id: Option<String>,
    pub client: String,
    pub provider: Option<String>,
    pub model: String,
    pub occurred_at_unix_ms: i64,
    pub tokens: TokenBreakdown,
    pub source_reported_total: Option<u64>,
    pub confidence: DataConfidence,
}

/// Source details needed to audit a parser decision without storing message
/// content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventProvenance {
    pub native_id: Option<String>,
    pub record_offset: Option<u64>,
    pub schema_variant: String,
    pub timestamp_origin: TimestampOrigin,
    pub normalization_notes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageRecord {
    pub event: UsageEvent,
    pub provenance: EventProvenance,
}

/// Collection state for one configured installation or discovered source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceHealthState {
    Healthy,
    Partial,
    SetupRequired,
    UnsupportedSchema,
    Error,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourcePermissionState {
    Unknown,
    Granted,
    Denied,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceRemediation {
    ConfigurePath,
    GrantPermission,
    UpgradeAgentMeter,
    RetryCollection,
    ReviewWarnings,
}

/// Immutable local source facts consumed by presentation code. Paths remain
/// local UI data and must be redacted before any diagnostics export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceHealth {
    pub installation_id: String,
    pub source_object_id: Option<String>,
    pub adapter_id: String,
    pub root_path: String,
    pub native_path: Option<String>,
    pub source_kind: Option<String>,
    pub enabled: bool,
    pub permission: SourcePermissionState,
    pub parser_version: Option<u32>,
    pub last_scan_unix_ms: Option<i64>,
    pub last_success_unix_ms: Option<i64>,
    pub last_event_unix_ms: Option<i64>,
    pub records_changed: u64,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub state: SourceHealthState,
    pub remediation: Option<SourceRemediation>,
}

/// A generation changes whenever any exposed health fact changes. Consumers
/// can reject stale asynchronous responses by comparing this value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceHealthSnapshot {
    pub generation: u64,
    pub sources: Vec<SourceHealth>,
}

#[cfg(test)]
mod tests {
    use super::TokenBreakdown;

    #[test]
    fn normalized_total_sums_exclusive_buckets() {
        let usage = TokenBreakdown {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
            reasoning: 50,
        };

        assert_eq!(usage.checked_total(), Some(150));
    }

    #[test]
    fn normalized_total_detects_overflow() {
        let usage = TokenBreakdown {
            input: u64::MAX,
            output: 1,
            ..TokenBreakdown::default()
        };

        assert_eq!(usage.checked_total(), None);
    }
}
