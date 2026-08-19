//! Application services that coordinate portable AgentMeter core crates.

use std::path::{Path, PathBuf};

use agentmeter_core::{AppPreferences, OverviewSnapshot, SourceHealthSnapshot};
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
        LocalDataErrorKind, OverviewService, PreferencesService, PricingService, SourcesService,
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
