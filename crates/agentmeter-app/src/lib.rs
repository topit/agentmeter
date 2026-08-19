//! Application services that coordinate portable AgentMeter core crates.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use agentmeter_core::{
    AppPreferences, NanoUsd, OverviewSnapshot, SourceHealthSnapshot, TokenBreakdown,
};
use agentmeter_pricing::RateDataset;
use agentmeter_storage::{Database, EstimateFact, StorageError};

/// Why a local data service could not produce a snapshot. The kinds are
/// presentation-safe: they never embed paths or raw database errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalDataErrorKind {
    DataDirectory,
    Database,
}

/// Immutable headline facts for the Overview screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverviewService {
    database_path: PathBuf,
}

impl OverviewService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(&self) -> Result<OverviewSnapshot, LocalDataServiceError> {
        open_local_database(&self.database_path)?
            .overview_snapshot()
            .map_err(LocalDataServiceError::Database)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivityGranularity {
    #[default]
    Daily,
    Weekly,
    Monthly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivityDimension {
    #[default]
    Client,
    Provider,
    Model,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityPoint {
    pub period_start_utc: String,
    /// Empty only when the selected source dimension is absent.
    pub series: String,
    pub tokens: u64,
    pub api_equivalent_estimate_usd: Option<NanoUsd>,
    pub event_count: u64,
    pub unpriced_event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivitySnapshot {
    pub generation: u64,
    pub granularity: ActivityGranularity,
    pub dimension: ActivityDimension,
    pub points: Vec<ActivityPoint>,
}

/// Immutable UTC activity series for the Activity screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityService {
    database_path: PathBuf,
}

impl ActivityService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(
        &self,
        granularity: ActivityGranularity,
        dimension: ActivityDimension,
    ) -> Result<ActivitySnapshot, LocalDataServiceError> {
        let rows = open_local_database(&self.database_path)?
            .activity_daily_rows()
            .map_err(LocalDataServiceError::Database)?;
        let mut points = BTreeMap::<(String, String), ActivityAccumulator>::new();
        for row in rows {
            let period = match granularity {
                ActivityGranularity::Daily => row.day,
                ActivityGranularity::Weekly => row.week_start,
                ActivityGranularity::Monthly => row.month_start,
            };
            let series = match dimension {
                ActivityDimension::Client => row.client,
                ActivityDimension::Provider => row.provider.unwrap_or_default(),
                ActivityDimension::Model => row.model,
            };
            points
                .entry((period, series))
                .or_default()
                .add(
                    row.tokens,
                    row.api_equivalent_estimate_usd,
                    row.event_count,
                    row.unpriced_event_count,
                )
                .map_err(LocalDataServiceError::Database)?;
        }
        let points: Vec<ActivityPoint> = points
            .into_iter()
            .map(|((period_start_utc, series), point)| ActivityPoint {
                period_start_utc,
                series,
                tokens: point.tokens,
                api_equivalent_estimate_usd: point.cost,
                event_count: point.event_count,
                unpriced_event_count: point.unpriced_event_count,
            })
            .collect();
        validate_activity_period_totals(&points).map_err(LocalDataServiceError::Database)?;
        Ok(ActivitySnapshot {
            generation: activity_generation(granularity, dimension, &points),
            granularity,
            dimension,
            points,
        })
    }
}

fn validate_activity_period_totals(points: &[ActivityPoint]) -> Result<(), StorageError> {
    let mut totals = BTreeMap::<&str, (u64, u64)>::new();
    for point in points {
        let total = totals.entry(point.period_start_utc.as_str()).or_default();
        total.0 = total
            .0
            .checked_add(point.tokens)
            .ok_or(StorageError::ActivityOverflow)?;
        total.1 = total
            .1
            .checked_add(
                point
                    .api_equivalent_estimate_usd
                    .map_or(0, NanoUsd::as_nanos),
            )
            .ok_or(StorageError::ActivityOverflow)?;
    }
    Ok(())
}

#[derive(Default)]
struct ActivityAccumulator {
    tokens: u64,
    cost: Option<NanoUsd>,
    event_count: u64,
    unpriced_event_count: u64,
}

impl ActivityAccumulator {
    fn add(
        &mut self,
        tokens: TokenBreakdown,
        cost: Option<NanoUsd>,
        event_count: u64,
        unpriced_event_count: u64,
    ) -> Result<(), StorageError> {
        self.tokens = self
            .tokens
            .checked_add(
                tokens
                    .checked_total()
                    .ok_or(StorageError::ActivityOverflow)?,
            )
            .ok_or(StorageError::ActivityOverflow)?;
        if let Some(cost) = cost {
            self.cost = Some(match self.cost {
                Some(total) => total
                    .checked_add(cost)
                    .ok_or(StorageError::ActivityOverflow)?,
                None => cost,
            });
        }
        self.event_count = self
            .event_count
            .checked_add(event_count)
            .ok_or(StorageError::ActivityOverflow)?;
        self.unpriced_event_count = self
            .unpriced_event_count
            .checked_add(unpriced_event_count)
            .ok_or(StorageError::ActivityOverflow)?;
        Ok(())
    }
}

fn activity_generation(
    granularity: ActivityGranularity,
    dimension: ActivityDimension,
    points: &[ActivityPoint],
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    hash_activity_bytes(&mut hash, &[granularity as u8, dimension as u8]);
    for point in points {
        hash_activity_bytes(&mut hash, point.period_start_utc.as_bytes());
        hash_activity_bytes(&mut hash, point.series.as_bytes());
        for value in [
            point.tokens,
            point
                .api_equivalent_estimate_usd
                .map_or(0, NanoUsd::as_nanos),
            point.event_count,
            point.unpriced_event_count,
        ] {
            hash_activity_bytes(&mut hash, &value.to_le_bytes());
        }
        hash_activity_bytes(
            &mut hash,
            &[point.api_equivalent_estimate_usd.is_some() as u8],
        );
    }
    hash
}

fn hash_activity_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSummary {
    pub source_object_id: String,
    pub session_id: String,
    pub adapter_id: String,
    pub source_kind: String,
    pub parser_version: u32,
    pub client: String,
    pub project: Option<String>,
    pub started_at_unix_ms: i64,
    pub ended_at_unix_ms: i64,
    pub total_tokens: u64,
    pub event_count: u64,
    pub confidence: agentmeter_core::DataConfidence,
    pub providers: Vec<String>,
    pub models: Vec<String>,
    pub provider_reported_usd: Option<NanoUsd>,
    pub api_equivalent_estimate_usd: Option<NanoUsd>,
    pub unpriced_event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionsSnapshot {
    pub generation: u64,
    pub sessions: Vec<SessionSummary>,
}

/// Immutable content-free summaries for the Sessions screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionsService {
    database_path: PathBuf,
}

impl SessionsService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(&self) -> Result<SessionsSnapshot, LocalDataServiceError> {
        let sessions: Vec<SessionSummary> = open_local_database(&self.database_path)?
            .session_summaries()
            .map_err(LocalDataServiceError::Database)?
            .into_iter()
            .map(|row| {
                Ok(SessionSummary {
                    source_object_id: row.source_object_id,
                    session_id: row.session_id,
                    adapter_id: row.adapter_id,
                    source_kind: row.source_kind,
                    parser_version: row.parser_version,
                    client: row.client,
                    project: row.project,
                    started_at_unix_ms: row.started_at_unix_ms,
                    ended_at_unix_ms: row.ended_at_unix_ms,
                    total_tokens: row
                        .tokens
                        .checked_total()
                        .ok_or(StorageError::SessionOverflow)?,
                    event_count: row.event_count,
                    confidence: row.confidence,
                    providers: row.providers,
                    models: row.models,
                    provider_reported_usd: row.provider_reported_usd,
                    api_equivalent_estimate_usd: row.api_equivalent_estimate_usd,
                    unpriced_event_count: row.unpriced_event_count,
                })
            })
            .collect::<Result<_, StorageError>>()
            .map_err(LocalDataServiceError::Database)?;
        Ok(SessionsSnapshot {
            generation: sessions_generation(&sessions),
            sessions,
        })
    }
}

fn sessions_generation(sessions: &[SessionSummary]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for session in sessions {
        for value in [
            &session.source_object_id,
            &session.session_id,
            &session.adapter_id,
            &session.source_kind,
            &session.client,
        ] {
            hash_activity_bytes(&mut hash, value.as_bytes());
        }
        if let Some(project) = &session.project {
            hash_activity_bytes(&mut hash, project.as_bytes());
        }
        for value in [
            session.started_at_unix_ms as u64,
            session.ended_at_unix_ms as u64,
            session.total_tokens,
            session.event_count,
            session.provider_reported_usd.map_or(0, NanoUsd::as_nanos),
            session
                .api_equivalent_estimate_usd
                .map_or(0, NanoUsd::as_nanos),
            session.unpriced_event_count,
            u64::from(session.parser_version),
            session.confidence as u64,
        ] {
            hash_activity_bytes(&mut hash, &value.to_le_bytes());
        }
        for value in session.providers.iter().chain(&session.models) {
            hash_activity_bytes(&mut hash, value.as_bytes());
        }
    }
    hash
}

/// Immutable per-source collection health for the Sources screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcesService {
    database_path: PathBuf,
}

impl SourcesService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(&self) -> Result<SourceHealthSnapshot, LocalDataServiceError> {
        open_local_database(&self.database_path)?
            .source_health_snapshot()
            .map_err(LocalDataServiceError::Database)
    }
}

/// Loads and persists user preferences in the local database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreferencesService {
    database_path: PathBuf,
}

impl PreferencesService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(&self) -> Result<AppPreferences, LocalDataServiceError> {
        open_local_database(&self.database_path)?
            .preferences()
            .map_err(LocalDataServiceError::Database)
    }

    pub fn save(&self, preferences: AppPreferences) -> Result<(), LocalDataServiceError> {
        open_local_database(&self.database_path)?
            .set_preferences(&preferences)
            .map_err(LocalDataServiceError::Database)
    }
}

/// Prices every canonical event from a versioned dataset, replacing the
/// previous estimate run wholesale. Provider-reported facts are never
/// touched and token facts are never re-ingested.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PricingService {
    database_path: PathBuf,
}

impl PricingService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn reprice(
        &self,
        dataset: &RateDataset,
        observed_at_unix_ms: i64,
    ) -> Result<RepriceSummary, LocalDataServiceError> {
        let mut database = open_local_database(&self.database_path)?;
        let snapshot_id = database
            .record_pricing_snapshot(
                &dataset.source,
                &dataset.version,
                observed_at_unix_ms,
                &dataset.content_hash(),
            )
            .map_err(LocalDataServiceError::Database)?;
        let facts: Vec<EstimateFact> = database
            .events_for_pricing()
            .map_err(LocalDataServiceError::Database)?
            .into_iter()
            .map(|event| {
                let estimate =
                    dataset.estimate(event.provider.as_deref(), &event.model, event.tokens);
                EstimateFact {
                    source_object_id: event.source_object_id,
                    event_id: event.event_id,
                    usd: estimate.usd,
                    pricing_key: estimate.pricing_key,
                    pricing_rule: estimate.pricing_rule,
                }
            })
            .collect();
        let report = database
            .replace_estimate_facts(snapshot_id, &facts)
            .map_err(LocalDataServiceError::Database)?;
        Ok(RepriceSummary {
            priced_events: report.priced_events,
            unpriced_events: report.unpriced_events,
            dataset: dataset.provenance(),
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepriceSummary {
    pub priced_events: u64,
    pub unpriced_events: u64,
    pub dataset: String,
}

#[derive(Debug)]
pub enum LocalDataServiceError {
    DataDirectory(std::io::Error),
    Database(StorageError),
}

impl LocalDataServiceError {
    pub const fn kind(&self) -> LocalDataErrorKind {
        match self {
            Self::DataDirectory(_) => LocalDataErrorKind::DataDirectory,
            Self::Database(_) => LocalDataErrorKind::Database,
        }
    }
}

fn local_database_path(data_directory: &Path) -> PathBuf {
    data_directory.join("AgentMeter").join("agentmeter.db")
}

fn open_local_database(database_path: &Path) -> Result<Database, LocalDataServiceError> {
    let parent = database_path
        .parent()
        .expect("AgentMeter database path must have a parent directory");
    std::fs::create_dir_all(parent).map_err(LocalDataServiceError::DataDirectory)?;
    Database::open(database_path).map_err(LocalDataServiceError::Database)
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{
        AppPreferences, AppearancePreference, CostFact, CostKind, DataConfidence, EventProvenance,
        LanguagePreference, NanoUsd, SourceHealthState, SourcePermissionState, SourceRemediation,
        TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
    };
    use agentmeter_storage::{
        Database, IngestRequest, SourceInstallationRegistration, SourceRegistration, WriteMode,
    };
    use tempfile::tempdir;

    use super::{
        ActivityDimension, ActivityGranularity, ActivityService, LocalDataErrorKind,
        OverviewService, PreferencesService, PricingService, SessionsService, SourcesService,
    };
    use agentmeter_pricing::RateDataset;

    #[test]
    fn creates_and_loads_the_local_database_for_overview() {
        let directory = tempdir().unwrap();
        let service = OverviewService::in_data_directory(directory.path());

        let snapshot = service.load().unwrap();

        assert_eq!(snapshot.event_count, 0);
        assert!(directory.path().join("AgentMeter/agentmeter.db").is_file());
    }

    #[test]
    fn loads_an_empty_sources_snapshot_with_a_stable_generation() {
        let directory = tempdir().unwrap();
        let service = SourcesService::in_data_directory(directory.path());

        let first = service.load().unwrap();
        let second = service.load().unwrap();

        assert!(first.sources.is_empty());
        assert_eq!(first, second, "unchanged data must not change the snapshot");
    }

    #[test]
    fn loads_registered_source_health_from_the_local_database() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("AgentMeter/agentmeter.db");
        std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut database = Database::open(&database_path).unwrap();
        database
            .register_installation(&SourceInstallationRegistration {
                installation_id: "installation-synthetic".into(),
                adapter_id: "amp".into(),
                platform: "macos".into(),
                root_path: "/fixture/home/.config/amp".into(),
                discovery_method: "default".into(),
                enabled: true,
                permission: SourcePermissionState::Granted,
            })
            .unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-synthetic".into(),
                source_object_id: "amp-stream-synthetic".into(),
                adapter_id: "amp".into(),
                platform: "macos".into(),
                root_path: "/fixture/home/.config/amp".into(),
                discovery_method: "default".into(),
                native_path: "/fixture/home/.config/amp/agent.jsonl".into(),
                kind: "jsonl".into(),
            })
            .unwrap();

        let snapshot = SourcesService::in_data_directory(directory.path())
            .load()
            .unwrap();

        assert_eq!(snapshot.sources.len(), 1);
        let source = &snapshot.sources[0];
        assert_eq!(source.adapter_id, "amp");
        assert_eq!(
            source.native_path.as_deref(),
            Some("/fixture/home/.config/amp/agent.jsonl")
        );
        assert_eq!(source.state, SourceHealthState::SetupRequired);
        assert_eq!(source.remediation, Some(SourceRemediation::RetryCollection));
    }

    #[test]
    fn loads_default_preferences_and_round_trips_a_change() {
        let directory = tempdir().unwrap();
        let service = PreferencesService::in_data_directory(directory.path());

        assert_eq!(service.load().unwrap(), AppPreferences::default());

        let preferences = AppPreferences {
            language: LanguagePreference::SimplifiedChinese,
            appearance: AppearancePreference::Dark,
        };
        service.save(preferences).unwrap();

        assert_eq!(
            PreferencesService::in_data_directory(directory.path())
                .load()
                .unwrap(),
            preferences,
            "preferences must survive a fresh service and connection"
        );
    }

    #[test]
    fn aggregates_activity_by_utc_period_and_dimension_without_hiding_unpriced_events() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("AgentMeter/agentmeter.db");
        std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut database = Database::open(&database_path).unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-activity".into(),
                source_object_id: "source-pricing".into(),
                adapter_id: "reference-jsonl".into(),
                platform: "test".into(),
                root_path: "/fixture/home/reference".into(),
                discovery_method: "fixture".into(),
                native_path: "/fixture/home/reference/events.jsonl".into(),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        let mut sunday = pricing_record("event-sunday", "model-a");
        sunday.event.occurred_at_unix_ms = 1_704_585_600_000; // 2024-01-07 UTC
        sunday.event.client = "client-a".into();
        sunday.event.provider = Some("provider-a".into());
        sunday.costs.push(CostFact {
            kind: CostKind::ApiEquivalentEstimate,
            usd: Some(NanoUsd::from_nanos(100_000_000)),
            confidence: DataConfidence::Estimated,
        });
        let mut monday = pricing_record("event-monday", "model-b");
        monday.event.occurred_at_unix_ms = 1_704_672_000_000; // 2024-01-08 UTC
        monday.event.client = "client-a".into();
        monday.event.provider = Some("provider-b".into());
        monday.costs.push(CostFact {
            kind: CostKind::Unpriced,
            usd: None,
            confidence: DataConfidence::Estimated,
        });
        database
            .apply_ingest(IngestRequest {
                source_object_id: "source-pricing".into(),
                parser_version: 1,
                mode: WriteMode::Append,
                source_fingerprint: "fingerprint-activity".into(),
                source_len: 256,
                byte_offset: Some(256),
                prefix_fingerprint: Some("prefix-activity".into()),
                parser_state: Vec::new(),
                observed_at_unix_ms: 1_704_672_000_000,
                records: vec![sunday, monday],
                warnings: Vec::new(),
            })
            .unwrap();

        let service = ActivityService::in_data_directory(directory.path());
        let weekly = service
            .load(ActivityGranularity::Weekly, ActivityDimension::Client)
            .unwrap();
        assert_eq!(weekly.points.len(), 2, "Monday starts a new UTC week");
        assert_eq!(weekly.points[0].period_start_utc, "2024-01-01");
        assert_eq!(weekly.points[1].period_start_utc, "2024-01-08");
        assert_eq!(weekly.points[0].tokens, 10);
        assert_eq!(
            weekly.points[0].api_equivalent_estimate_usd,
            Some(NanoUsd::from_nanos(100_000_000))
        );
        assert_eq!(weekly.points[1].api_equivalent_estimate_usd, None);
        assert_eq!(weekly.points[1].unpriced_event_count, 1);

        let by_provider = service
            .load(ActivityGranularity::Monthly, ActivityDimension::Provider)
            .unwrap();
        assert_eq!(by_provider.points.len(), 2);
        assert_eq!(by_provider.points[0].period_start_utc, "2024-01-01");
        assert_eq!(by_provider.points[0].series, "provider-a");
        assert_eq!(by_provider.points[1].series, "provider-b");
        assert_eq!(
            by_provider,
            service
                .load(ActivityGranularity::Monthly, ActivityDimension::Provider)
                .unwrap()
        );
    }

    #[test]
    fn loads_content_free_session_summaries_with_cost_kinds_and_confidence_separated() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("AgentMeter/agentmeter.db");
        std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut database = Database::open(&database_path).unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-sessions".into(),
                source_object_id: "source-pricing".into(),
                adapter_id: "reference-jsonl".into(),
                platform: "test".into(),
                root_path: "/fixture/home/reference".into(),
                discovery_method: "fixture".into(),
                native_path: "/fixture/home/reference/events.jsonl".into(),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        let mut first = pricing_record("event-session-first", "model-a");
        first.event.provider = Some("provider-a".into());
        first.costs.extend([
            CostFact {
                kind: CostKind::ProviderReported,
                usd: Some(NanoUsd::from_nanos(200_000_000)),
                confidence: DataConfidence::Exact,
            },
            CostFact {
                kind: CostKind::ApiEquivalentEstimate,
                usd: Some(NanoUsd::from_nanos(100_000_000)),
                confidence: DataConfidence::Estimated,
            },
        ]);
        let mut second = pricing_record("event-session-second", "model-b");
        second.event.occurred_at_unix_ms += 60_000;
        second.event.provider = Some("provider-b".into());
        second.event.confidence = DataConfidence::Derived;
        second.costs.push(CostFact {
            kind: CostKind::Unpriced,
            usd: None,
            confidence: DataConfidence::Estimated,
        });
        database
            .apply_ingest(IngestRequest {
                source_object_id: "source-pricing".into(),
                parser_version: 3,
                mode: WriteMode::Append,
                source_fingerprint: "fingerprint-sessions".into(),
                source_len: 256,
                byte_offset: Some(256),
                prefix_fingerprint: Some("prefix-sessions".into()),
                parser_state: Vec::new(),
                observed_at_unix_ms: 1_704_067_300_000,
                records: vec![first, second],
                warnings: Vec::new(),
            })
            .unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-sessions-other".into(),
                source_object_id: "source-pricing-other".into(),
                adapter_id: "reference-jsonl".into(),
                platform: "test".into(),
                root_path: "/fixture/home/reference-other".into(),
                discovery_method: "fixture".into(),
                native_path: "/fixture/home/reference-other/events.jsonl".into(),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        let mut same_native_session = pricing_record("event-session-other", "model-c");
        same_native_session.event.source_id = "source-pricing-other".into();
        same_native_session.event.occurred_at_unix_ms -= 60_000;
        database
            .apply_ingest(IngestRequest {
                source_object_id: "source-pricing-other".into(),
                parser_version: 3,
                mode: WriteMode::Append,
                source_fingerprint: "fingerprint-sessions-other".into(),
                source_len: 128,
                byte_offset: Some(128),
                prefix_fingerprint: Some("prefix-sessions-other".into()),
                parser_state: Vec::new(),
                observed_at_unix_ms: 1_704_067_300_000,
                records: vec![same_native_session],
                warnings: Vec::new(),
            })
            .unwrap();

        let service = SessionsService::in_data_directory(directory.path());
        let snapshot = service.load().unwrap();
        let session = &snapshot.sessions[0];

        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(session.source_object_id, "source-pricing");
        assert_eq!(session.session_id, "session-synthetic");
        assert_eq!(snapshot.sessions[1].session_id, "session-synthetic");
        assert_eq!(
            snapshot.sessions[1].source_object_id,
            "source-pricing-other"
        );
        assert_eq!(session.adapter_id, "reference-jsonl");
        assert_eq!(session.parser_version, 3);
        assert_eq!(session.project, None);
        assert_eq!(
            session.ended_at_unix_ms - session.started_at_unix_ms,
            60_000
        );
        assert_eq!(session.total_tokens, 20);
        assert_eq!(session.event_count, 2);
        assert_eq!(session.confidence, DataConfidence::Derived);
        assert_eq!(session.providers, ["provider-a", "provider-b"]);
        assert_eq!(session.models, ["model-a", "model-b"]);
        assert_eq!(
            session.provider_reported_usd,
            Some(NanoUsd::from_nanos(200_000_000))
        );
        assert_eq!(
            session.api_equivalent_estimate_usd,
            Some(NanoUsd::from_nanos(100_000_000))
        );
        assert_eq!(session.unpriced_event_count, 1);
        assert_eq!(snapshot, service.load().unwrap());
    }

    #[test]
    fn classifies_a_preferences_data_directory_failure() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();
        let service = PreferencesService::in_data_directory(directory.path());

        assert_eq!(
            service.load().unwrap_err().kind(),
            LocalDataErrorKind::DataDirectory
        );
        assert_eq!(
            service.save(AppPreferences::default()).unwrap_err().kind(),
            LocalDataErrorKind::DataDirectory
        );
    }

    #[test]
    fn reprices_events_reversibly_while_keeping_provider_reported_facts() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("AgentMeter/agentmeter.db");
        std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut database = Database::open(&database_path).unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-pricing".into(),
                source_object_id: "source-pricing".into(),
                adapter_id: "reference-jsonl".into(),
                platform: "test".into(),
                root_path: "/fixture/home/reference".into(),
                discovery_method: "fixture".into(),
                native_path: "/fixture/home/reference/events.jsonl".into(),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        let mut reported = pricing_record("event-reported", "model-priced");
        reported.costs.push(CostFact {
            kind: CostKind::ProviderReported,
            usd: Some(NanoUsd::from_nanos(1_000_000_000)),
            confidence: DataConfidence::Exact,
        });
        database
            .apply_ingest(IngestRequest {
                source_object_id: "source-pricing".into(),
                parser_version: 1,
                mode: WriteMode::Append,
                source_fingerprint: "fingerprint-synthetic".into(),
                source_len: 128,
                byte_offset: Some(128),
                prefix_fingerprint: Some("prefix-synthetic".into()),
                parser_state: Vec::new(),
                observed_at_unix_ms: 1_704_067_200_000,
                records: vec![reported, pricing_record("event-unpriced", "model-unknown")],
                warnings: Vec::new(),
            })
            .unwrap();

        let service = PricingService::in_data_directory(directory.path());
        let mut dataset = RateDataset::bundled();
        dataset.rates.insert(
            "model-priced".to_owned(),
            agentmeter_pricing::ModelRates {
                input: 1_000,
                ..agentmeter_pricing::ModelRates::default()
            },
        );
        let summary = service.reprice(&dataset, 1_704_067_300_000).unwrap();

        assert_eq!(summary.priced_events, 1);
        assert_eq!(summary.unpriced_events, 1);
        assert!(summary.dataset.starts_with("agentmeter-reviewed@"));

        let snapshot = Database::open(&database_path)
            .unwrap()
            .overview_snapshot()
            .unwrap();
        assert_eq!(
            snapshot.costs.api_equivalent_estimate_usd,
            Some(NanoUsd::from_nanos(10_000)),
            "10 input tokens at 1_000 nano-USD per token"
        );
        assert_eq!(snapshot.data_quality.unpriced_events, 1);
        assert_eq!(
            snapshot.costs.provider_reported_usd,
            Some(NanoUsd::from_nanos(1_000_000_000)),
            "provider-reported cost survives repricing"
        );

        // A changed dataset fully reverses the previous estimate run.
        dataset.version = "2026-08-19.1".to_owned();
        dataset.rates.insert(
            "model-unknown".to_owned(),
            agentmeter_pricing::ModelRates {
                input: 500,
                ..agentmeter_pricing::ModelRates::default()
            },
        );
        let resummarized = service.reprice(&dataset, 1_704_067_400_000).unwrap();
        assert_eq!(resummarized.priced_events, 2);
        assert_eq!(resummarized.unpriced_events, 0);

        let snapshot = Database::open(&database_path)
            .unwrap()
            .overview_snapshot()
            .unwrap();
        assert_eq!(snapshot.data_quality.unpriced_events, 0);
        assert_eq!(
            snapshot.costs.api_equivalent_estimate_usd,
            Some(NanoUsd::from_nanos(15_000)),
            "10×1_000 + 10×500 nano-USD after the second run"
        );
    }

    #[test]
    fn classifies_a_pricing_data_directory_failure() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();

        let error = PricingService::in_data_directory(directory.path())
            .reprice(&RateDataset::bundled(), 0)
            .unwrap_err();

        assert_eq!(error.kind(), LocalDataErrorKind::DataDirectory);
    }

    fn pricing_record(id: &str, model: &str) -> UsageRecord {
        UsageRecord {
            event: UsageEvent {
                id: id.into(),
                source_id: "source-pricing".into(),
                session_id: Some("session-synthetic".into()),
                client: "synthetic".into(),
                provider: None,
                model: model.into(),
                occurred_at_unix_ms: 1_704_067_200_000,
                tokens: TokenBreakdown {
                    input: 10,
                    ..TokenBreakdown::default()
                },
                source_reported_total: Some(10),
                confidence: DataConfidence::Exact,
            },
            costs: Vec::new(),
            provenance: EventProvenance {
                native_id: Some(id.into()),
                record_offset: Some(0),
                schema_variant: "reference-v1".into(),
                timestamp_origin: TimestampOrigin::Source,
                normalization_notes: Vec::new(),
            },
        }
    }

    #[test]
    fn classifies_an_unavailable_data_directory_without_exposing_its_path() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();

        let error = OverviewService::in_data_directory(directory.path())
            .load()
            .unwrap_err();

        assert_eq!(error.kind(), LocalDataErrorKind::DataDirectory);
    }

    #[test]
    fn classifies_sources_data_directory_and_database_failures() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();
        let error = SourcesService::in_data_directory(directory.path())
            .load()
            .unwrap_err();
        assert_eq!(error.kind(), LocalDataErrorKind::DataDirectory);

        let directory = tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("AgentMeter")).unwrap();
        std::fs::write(
            directory.path().join("AgentMeter/agentmeter.db"),
            b"not a sqlite database",
        )
        .unwrap();
        let error = SourcesService::in_data_directory(directory.path())
            .load()
            .unwrap_err();
        assert_eq!(error.kind(), LocalDataErrorKind::Database);
    }
}
