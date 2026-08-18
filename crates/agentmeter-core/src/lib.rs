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

/// Why a cost fact exists. Provider-reported amounts and locally calculated
/// API equivalents must never be presented as the same kind of spend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostKind {
    ProviderReported,
    ApiEquivalentEstimate,
    SubscriptionCredit,
    Unpriced,
}

/// An exact USD amount expressed as USD × 10^9. Keeping decimal source facts
/// as integers prevents binary floating point from affecting event identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NanoUsd(u64);

impl NanoUsd {
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// Parses a non-negative JSON decimal without passing through a float.
    /// Values must be exactly representable to nine decimal places.
    pub fn parse_decimal(value: &str) -> Result<Self, NanoUsdParseError> {
        let (negative, unsigned) = value
            .strip_prefix('-')
            .map_or((false, value), |unsigned| (true, unsigned));
        if negative {
            return Err(NanoUsdParseError::Negative);
        }
        let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (
                mantissa,
                exponent
                    .parse::<i32>()
                    .map_err(|_| NanoUsdParseError::Invalid)?,
            ),
            None => (unsigned, 0),
        };
        if mantissa.is_empty() {
            return Err(NanoUsdParseError::Invalid);
        }
        let (whole, fraction) = match mantissa.split_once('.') {
            Some((_, "")) => return Err(NanoUsdParseError::Invalid),
            Some(parts) => parts,
            None => (mantissa, ""),
        };
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(NanoUsdParseError::Invalid);
        }
        let digits = format!("{whole}{fraction}");
        let significant = digits.trim_start_matches('0');
        if significant.is_empty() {
            return Ok(Self(0));
        }
        let power = 9_i64 - i64::try_from(fraction.len()).unwrap_or(i64::MAX) + i64::from(exponent);
        let (significant, power) = if power < 0 {
            let places = usize::try_from(-power).map_err(|_| NanoUsdParseError::TooPrecise)?;
            let split = significant
                .len()
                .checked_sub(places)
                .ok_or(NanoUsdParseError::TooPrecise)?;
            if !significant[split..].bytes().all(|byte| byte == b'0') {
                return Err(NanoUsdParseError::TooPrecise);
            }
            (&significant[..split], 0)
        } else {
            (significant, power)
        };
        let value = significant
            .parse::<u64>()
            .map_err(|_| NanoUsdParseError::OutOfRange)?;
        let nanos = if power == 0 {
            value
        } else {
            let power = u32::try_from(power).map_err(|_| NanoUsdParseError::OutOfRange)?;
            let multiplier = 10_u64
                .checked_pow(power)
                .ok_or(NanoUsdParseError::OutOfRange)?;
            value
                .checked_mul(multiplier)
                .ok_or(NanoUsdParseError::OutOfRange)?
        };
        Ok(Self(nanos))
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NanoUsdParseError {
    Invalid,
    Negative,
    TooPrecise,
    OutOfRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostFact {
    pub kind: CostKind,
    /// `None` is reserved for facts such as `Unpriced` and non-USD credits.
    pub usd: Option<NanoUsd>,
    pub confidence: DataConfidence,
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
    pub costs: Vec<CostFact>,
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

/// Persisted user preferences. Both default to following the system so a
/// fresh installation never overrides platform locale or appearance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LanguagePreference {
    #[default]
    System,
    English,
    SimplifiedChinese,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppearancePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AppPreferences {
    pub language: LanguagePreference,
    pub appearance: AppearancePreference,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverviewCostSummary {
    pub provider_reported_usd: Option<NanoUsd>,
    pub api_equivalent_estimate_usd: Option<NanoUsd>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverviewDataQuality {
    pub exact_events: u64,
    pub derived_events: u64,
    pub estimated_events: u64,
    pub unpriced_events: u64,
}

/// Immutable headline facts for the Overview screen. Storage computes the
/// stable content generation; presentation code uses a separate request token
/// to reject out-of-order asynchronous loads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverviewSnapshot {
    pub generation: u64,
    pub tokens: TokenBreakdown,
    pub event_count: u64,
    pub session_count: u64,
    pub active_days: u64,
    pub model_count: u64,
    pub costs: OverviewCostSummary,
    pub data_quality: OverviewDataQuality,
    pub source_health: SourceHealthSnapshot,
}

#[cfg(test)]
mod tests {
    use super::{
        AppPreferences, AppearancePreference, LanguagePreference, NanoUsd, NanoUsdParseError,
        TokenBreakdown,
    };

    #[test]
    fn fresh_preferences_follow_the_system() {
        assert_eq!(
            AppPreferences::default(),
            AppPreferences {
                language: LanguagePreference::System,
                appearance: AppearancePreference::System,
            }
        );
    }

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

    #[test]
    fn nano_usd_parses_json_decimals_exactly() {
        assert_eq!(NanoUsd::parse_decimal("0.000000001").unwrap().as_nanos(), 1);
        assert_eq!(
            NanoUsd::parse_decimal("1.25").unwrap().as_nanos(),
            1_250_000_000
        );
        assert_eq!(
            NanoUsd::parse_decimal("12e-3").unwrap().as_nanos(),
            12_000_000
        );
        assert_eq!(
            NanoUsd::parse_decimal("1.0000000000").unwrap().as_nanos(),
            1_000_000_000
        );
        assert_eq!(
            NanoUsd::parse_decimal("100000000000000000000e-20")
                .unwrap()
                .as_nanos(),
            1_000_000_000
        );
    }

    #[test]
    fn nano_usd_rejects_lossy_or_invalid_amounts() {
        assert_eq!(
            NanoUsd::parse_decimal("-0.1"),
            Err(NanoUsdParseError::Negative)
        );
        assert_eq!(
            NanoUsd::parse_decimal("0.0000000001"),
            Err(NanoUsdParseError::TooPrecise)
        );
        assert_eq!(
            NanoUsd::parse_decimal("18446744074"),
            Err(NanoUsdParseError::OutOfRange)
        );
    }
}
