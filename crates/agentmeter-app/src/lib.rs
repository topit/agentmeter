//! Application services that coordinate portable AgentMeter core crates.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use agentmeter_collectors::{
    CollectorAdapter, IngestBatch, IngestMode, IngestStart, SourceCandidate, SourceCheckpoint,
    SourceKind, codex::CodexJsonlAdapter, kimi::KimiWireAdapter, pi::PiJsonlAdapter,
};
use agentmeter_core::{
    AppPreferences, NanoUsd, OverviewSnapshot, SourceHealthSnapshot, TokenBreakdown,
};
use agentmeter_pricing::{ModelRates, RateDataset};
use agentmeter_storage::{
    CheckpointStatus, CollectionFailureKind, Database, EstimateFact, IngestRequest, StorageError,
    WriteMode,
};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelUsageSummary {
    pub provider: Option<String>,
    pub model: String,
    pub clients: Vec<String>,
    pub tokens: TokenBreakdown,
    pub total_tokens: u64,
    pub event_count: u64,
    pub confidence: agentmeter_core::DataConfidence,
    pub provider_reported_usd: Option<NanoUsd>,
    pub api_equivalent_estimate_usd: Option<NanoUsd>,
    pub unpriced_event_count: u64,
    pub pricing_keys: Vec<String>,
    pub pricing_rules: Vec<String>,
    pub pricing_confidence: Option<agentmeter_core::DataConfidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelRateSummary {
    pub key: String,
    pub aliases: Vec<String>,
    pub input_per_million: NanoUsd,
    pub output_per_million: NanoUsd,
    pub cache_read_per_million: NanoUsd,
    pub cache_write_per_million: NanoUsd,
    pub reasoning_per_million: NanoUsd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PricingApplicationSummary {
    pub source: String,
    pub version: String,
    pub content_hash: String,
    pub dataset_updated_at_unix_ms: i64,
    pub priced_event_count: u64,
    pub unpriced_event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelsPricingSnapshot {
    pub generation: u64,
    pub dataset_source: String,
    pub dataset_version: String,
    pub rates: Vec<ModelRateSummary>,
    pub models: Vec<ModelUsageSummary>,
    pub applied: Option<PricingApplicationSummary>,
}

/// Immutable lifetime model totals plus the reviewed pricing catalog and its
/// latest local application status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelsPricingService {
    database_path: PathBuf,
}

impl ModelsPricingService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(
        &self,
        dataset: &RateDataset,
    ) -> Result<ModelsPricingSnapshot, LocalDataServiceError> {
        let database = open_local_database(&self.database_path)?;
        let models = database
            .model_usage_rows()
            .map_err(LocalDataServiceError::Database)?
            .into_iter()
            .map(|row| {
                Ok(ModelUsageSummary {
                    provider: row.provider,
                    model: row.model,
                    clients: row.clients,
                    total_tokens: row
                        .tokens
                        .checked_total()
                        .ok_or(StorageError::ModelsOverflow)?,
                    tokens: row.tokens,
                    event_count: row.event_count,
                    confidence: row.confidence,
                    provider_reported_usd: row.provider_reported_usd,
                    api_equivalent_estimate_usd: row.api_equivalent_estimate_usd,
                    unpriced_event_count: row.unpriced_event_count,
                    pricing_keys: row.pricing_keys,
                    pricing_rules: row.pricing_rules,
                    pricing_confidence: row.pricing_confidence,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()
            .map_err(LocalDataServiceError::Database)?;
        let applied = database
            .latest_pricing_status()
            .map_err(LocalDataServiceError::Database)?
            .map(|row| PricingApplicationSummary {
                source: row.source,
                version: row.dataset_version,
                content_hash: row.content_hash,
                dataset_updated_at_unix_ms: row.fetched_at_unix_ms,
                priced_event_count: row.priced_event_count,
                unpriced_event_count: row.unpriced_event_count,
            });
        let rates = dataset
            .rates
            .iter()
            .map(|(key, rates)| model_rate_summary(dataset, key, *rates))
            .collect::<Result<Vec<_>, StorageError>>()
            .map_err(LocalDataServiceError::Database)?;
        let mut snapshot = ModelsPricingSnapshot {
            generation: 0,
            dataset_source: dataset.source.clone(),
            dataset_version: dataset.version.clone(),
            rates,
            models,
            applied,
        };
        snapshot.generation = models_pricing_generation(&snapshot);
        Ok(snapshot)
    }

    pub fn load_bundled(&self) -> Result<ModelsPricingSnapshot, LocalDataServiceError> {
        self.load(&RateDataset::bundled())
    }

    /// Applies the bundled dataset when it is stale or new events have no
    /// estimate fact, then returns the resulting immutable read snapshot.
    pub fn load_or_apply_bundled(
        &self,
        observed_at_unix_ms: i64,
    ) -> Result<ModelsPricingSnapshot, LocalDataServiceError> {
        let dataset = RateDataset::bundled();
        let snapshot = self.load(&dataset)?;
        let event_count = snapshot
            .models
            .iter()
            .try_fold(0_u64, |total, model| total.checked_add(model.event_count));
        let is_current = event_count.is_some_and(|event_count| {
            snapshot.applied.as_ref().is_some_and(|applied| {
                applied.source == dataset.source
                    && applied.version == dataset.version
                    && applied.content_hash == dataset.content_hash()
                    && applied
                        .priced_event_count
                        .checked_add(applied.unpriced_event_count)
                        == Some(event_count)
            })
        });
        if is_current {
            return Ok(snapshot);
        }
        PricingService {
            database_path: self.database_path.clone(),
        }
        .reprice(&dataset, observed_at_unix_ms)?;
        self.load(&dataset)
    }
}

fn model_rate_summary(
    dataset: &RateDataset,
    key: &str,
    rates: ModelRates,
) -> Result<ModelRateSummary, StorageError> {
    let per_million = |rate: u64| {
        rate.checked_mul(1_000_000)
            .map(NanoUsd::from_nanos)
            .ok_or(StorageError::ModelsOverflow)
    };
    Ok(ModelRateSummary {
        key: key.to_owned(),
        aliases: dataset
            .aliases
            .iter()
            .filter(|(_, canonical)| canonical.as_str() == key)
            .map(|(alias, _)| alias.clone())
            .collect(),
        input_per_million: per_million(rates.input)?,
        output_per_million: per_million(rates.output)?,
        cache_read_per_million: per_million(rates.cache_read)?,
        cache_write_per_million: per_million(rates.cache_write)?,
        reasoning_per_million: per_million(rates.reasoning)?,
    })
}

fn models_pricing_generation(snapshot: &ModelsPricingSnapshot) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [&snapshot.dataset_source, &snapshot.dataset_version] {
        hash_activity_bytes(&mut hash, value.as_bytes());
    }
    for rate in &snapshot.rates {
        hash_activity_bytes(&mut hash, rate.key.as_bytes());
        for alias in &rate.aliases {
            hash_activity_bytes(&mut hash, alias.as_bytes());
        }
        for value in [
            rate.input_per_million,
            rate.output_per_million,
            rate.cache_read_per_million,
            rate.cache_write_per_million,
            rate.reasoning_per_million,
        ] {
            hash_activity_bytes(&mut hash, &value.as_nanos().to_le_bytes());
        }
    }
    for model in &snapshot.models {
        hash_activity_bytes(&mut hash, model.model.as_bytes());
        if let Some(provider) = &model.provider {
            hash_activity_bytes(&mut hash, provider.as_bytes());
        }
        for value in model
            .clients
            .iter()
            .chain(&model.pricing_keys)
            .chain(&model.pricing_rules)
        {
            hash_activity_bytes(&mut hash, value.as_bytes());
        }
        for value in [
            model.tokens.input,
            model.tokens.output,
            model.tokens.cache_read,
            model.tokens.cache_write,
            model.tokens.reasoning,
            model.event_count,
            model.unpriced_event_count,
            model.provider_reported_usd.map_or(0, NanoUsd::as_nanos),
            model
                .api_equivalent_estimate_usd
                .map_or(0, NanoUsd::as_nanos),
            model.confidence as u64,
            model
                .pricing_confidence
                .map_or(u64::MAX, |value| value as u64),
        ] {
            hash_activity_bytes(&mut hash, &value.to_le_bytes());
        }
    }
    if let Some(applied) = &snapshot.applied {
        for value in [&applied.source, &applied.version, &applied.content_hash] {
            hash_activity_bytes(&mut hash, value.as_bytes());
        }
        for value in [
            applied.dataset_updated_at_unix_ms as u64,
            applied.priced_event_count,
            applied.unpriced_event_count,
        ] {
            hash_activity_bytes(&mut hash, &value.to_le_bytes());
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

/// Orchestrates local collection: discovery, registration, checkpointed
/// ingestion, due full reconciliation, and visible failure diagnostics.
/// This is the write path that feeds every read service; vendor files are
/// only ever read.
pub struct IngestionService {
    data_directory: PathBuf,
    adapters: Vec<LocalAdapter>,
}

struct LocalAdapter {
    root: PathBuf,
    adapter: Box<dyn CollectorAdapter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionSummary {
    pub runs: Vec<AdapterRunSummary>,
    /// True when the scan stopped early; every started source either fully
    /// committed or never ran, so a later scan resumes cleanly.
    pub cancelled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterRunSummary {
    pub adapter_id: String,
    pub discovered_sources: u64,
    pub ingested_sources: u64,
    pub failed_sources: u64,
    pub reconciled_sources: u64,
    /// Discovery-level failure; without discovered sources there is no
    /// registered object to persist it against, so it stays in the summary.
    pub discovery_error: Option<String>,
}

/// Cooperative cancellation for long collection passes. Cancellation is
/// checked between source-owned transactions, so abandoning a scan never
/// corrupts a checkpoint or half-applies a source.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The default scan interval for periodic full reconciliation.
pub const RECONCILIATION_INTERVAL_MS: u64 = 86_400_000;

impl IngestionService {
    pub fn with_adapters(
        data_directory: impl AsRef<Path>,
        adapters: Vec<(PathBuf, Box<dyn CollectorAdapter>)>,
    ) -> Self {
        Self {
            data_directory: data_directory.as_ref().to_owned(),
            adapters: adapters
                .into_iter()
                .map(|(root, adapter)| LocalAdapter { root, adapter })
                .collect(),
        }
    }

    /// The enabled local adapters for 1.0: the Codex home, Pi sessions, and
    /// Kimi wire roots. Amp's local-history parsing stays experimental and
    /// opt-in, and stream-json capture is an explicit user action, so
    /// neither runs by default.
    pub fn with_default_local_adapters(data_directory: impl AsRef<Path>) -> Self {
        let mut adapters: Vec<(PathBuf, Box<dyn CollectorAdapter>)> = Vec::new();
        if let Some(home) = CodexJsonlAdapter::default_codex_home() {
            let adapter = CodexJsonlAdapter::new(home.clone());
            adapters.push((home, Box::new(adapter)));
        }
        if let Some(root) = PiJsonlAdapter::default_sessions_root() {
            let adapter = PiJsonlAdapter::new(root.clone());
            adapters.push((root, Box::new(adapter)));
        }
        for root in KimiWireAdapter::default_roots() {
            let adapter = KimiWireAdapter::new(root.clone());
            adapters.push((root, Box::new(adapter)));
        }
        Self::with_adapters(data_directory, adapters)
    }

    pub fn scan_and_ingest(
        &self,
        observed_at_unix_ms: i64,
    ) -> Result<IngestionSummary, LocalDataServiceError> {
        self.scan_and_ingest_cancellable(observed_at_unix_ms, &CancellationToken::new())
    }

    /// Runs the collection pass, checking `token` between source-owned
    /// transactions. A cancelled scan returns the partial summary with
    /// `cancelled` set instead of an error.
    pub fn scan_and_ingest_cancellable(
        &self,
        observed_at_unix_ms: i64,
        token: &CancellationToken,
    ) -> Result<IngestionSummary, LocalDataServiceError> {
        self.scan_with(observed_at_unix_ms, token, || false)
    }

    fn scan_with(
        &self,
        observed_at_unix_ms: i64,
        token: &CancellationToken,
        mut extra_cancel_check: impl FnMut() -> bool,
    ) -> Result<IngestionSummary, LocalDataServiceError> {
        let is_cancelled = |extra: &mut dyn FnMut() -> bool| token.is_cancelled() || extra();
        let mut database = open_local_database(&local_database_path(&self.data_directory))?;
        let mut runs = Vec::new();
        let mut candidates: BTreeMap<String, (usize, SourceCandidate)> = BTreeMap::new();
        let mut cancelled = false;

        'adapters: for (index, local) in self.adapters.iter().enumerate() {
            if is_cancelled(&mut extra_cancel_check) {
                cancelled = true;
                break;
            }
            let mut run = AdapterRunSummary {
                adapter_id: local.adapter.id().to_owned(),
                discovered_sources: 0,
                ingested_sources: 0,
                failed_sources: 0,
                reconciled_sources: 0,
                discovery_error: None,
            };
            let discovered = match local.adapter.discover() {
                Ok(discovered) => discovered,
                Err(error) => {
                    run.discovery_error = Some(error.message);
                    runs.push(run);
                    continue;
                }
            };
            run.discovered_sources = discovered.len() as u64;
            for candidate in discovered {
                if is_cancelled(&mut extra_cancel_check) {
                    cancelled = true;
                    runs.push(run);
                    break 'adapters;
                }
                let source_object_id = format!("{}:{}", local.adapter.id(), candidate.source_key);
                candidates.insert(source_object_id.clone(), (index, candidate.clone()));
                if let Err(error) =
                    database.register_source(&registration(local, &candidate, &source_object_id))
                {
                    let _ = database.record_collection_failure(
                        &source_object_id,
                        observed_at_unix_ms,
                        CollectionFailureKind::Collection,
                        &error.to_string(),
                    );
                    run.failed_sources += 1;
                    continue;
                }
                let start = match database
                    .checkpoint_status(&source_object_id, local.adapter.parser_version())
                {
                    Ok(CheckpointStatus::NeverIngested) => IngestStart::Fresh,
                    Ok(CheckpointStatus::Current(checkpoint)) => {
                        IngestStart::Resume(&SourceCheckpoint {
                            byte_offset: checkpoint.byte_offset,
                            source_len: checkpoint.source_len,
                            prefix_fingerprint: checkpoint.prefix_fingerprint,
                            parser_state: checkpoint.parser_state,
                        })
                    }
                    Ok(CheckpointStatus::Invalidated { .. }) => IngestStart::Rebuild,
                    Err(error) => {
                        let _ = database.record_collection_failure(
                            &source_object_id,
                            observed_at_unix_ms,
                            CollectionFailureKind::Collection,
                            &error.to_string(),
                        );
                        run.failed_sources += 1;
                        continue;
                    }
                };
                if apply_batch(
                    &mut database,
                    local,
                    &candidate,
                    &source_object_id,
                    start,
                    observed_at_unix_ms,
                ) {
                    run.ingested_sources += 1;
                } else {
                    run.failed_sources += 1;
                }
            }
            runs.push(run);
        }

        // Periodic full reconciliation: due sources rebuild through the
        // adapter that still owns them.
        if !cancelled
            && !is_cancelled(&mut extra_cancel_check)
            && let Ok(due) = database
                .sources_due_for_reconciliation(observed_at_unix_ms, RECONCILIATION_INTERVAL_MS)
        {
            for target in due {
                if is_cancelled(&mut extra_cancel_check) {
                    cancelled = true;
                    break;
                }
                let Some((index, candidate)) = candidates.get(&target.source_object_id) else {
                    continue;
                };
                let local = &self.adapters[*index];
                if apply_batch(
                    &mut database,
                    local,
                    candidate,
                    &target.source_object_id,
                    IngestStart::Rebuild,
                    observed_at_unix_ms,
                ) && let Some(run) = runs
                    .iter_mut()
                    .find(|run| run.adapter_id == target.adapter_id)
                {
                    run.reconciled_sources += 1;
                }
            }
        }

        Ok(IngestionSummary { runs, cancelled })
    }
}

fn registration(
    local: &LocalAdapter,
    candidate: &SourceCandidate,
    source_object_id: &str,
) -> agentmeter_storage::SourceRegistration {
    agentmeter_storage::SourceRegistration {
        installation_id: format!("{}:{}", local.adapter.id(), local.root.to_string_lossy()),
        source_object_id: source_object_id.to_owned(),
        adapter_id: local.adapter.id().to_owned(),
        platform: std::env::consts::OS.to_owned(),
        root_path: local.root.to_string_lossy().into_owned(),
        discovery_method: "scan".to_owned(),
        native_path: candidate.path.to_string_lossy().into_owned(),
        kind: source_kind_string(candidate.kind).to_owned(),
    }
}

fn source_kind_string(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::AppendOnlyJsonl => "append_only_jsonl",
        SourceKind::MutableJson => "mutable_json",
        SourceKind::Sqlite => "sqlite",
        SourceKind::Api => "api",
    }
}

/// Runs one adapter batch into the ledger transactionally; adapter errors
/// become persisted collection-failure diagnostics instead of silent zeroes.
fn apply_batch(
    database: &mut Database,
    local: &LocalAdapter,
    candidate: &SourceCandidate,
    source_object_id: &str,
    start: IngestStart<'_>,
    observed_at_unix_ms: i64,
) -> bool {
    match local.adapter.ingest(candidate, start) {
        Ok(batch) => {
            let request = ingest_request(
                source_object_id,
                local.adapter.parser_version(),
                batch,
                observed_at_unix_ms,
            );
            database.apply_ingest(request).is_ok()
        }
        Err(error) => {
            let _ = database.record_collection_failure(
                source_object_id,
                observed_at_unix_ms,
                CollectionFailureKind::Collection,
                &error.message,
            );
            false
        }
    }
}

fn ingest_request(
    source_object_id: &str,
    parser_version: u32,
    batch: IngestBatch,
    observed_at_unix_ms: i64,
) -> IngestRequest {
    IngestRequest {
        source_object_id: source_object_id.to_owned(),
        parser_version,
        mode: match batch.mode {
            IngestMode::Append => WriteMode::Append,
            IngestMode::Replace => WriteMode::Replace,
        },
        source_fingerprint: batch.source_fingerprint,
        source_len: batch.checkpoint.source_len,
        byte_offset: batch.checkpoint.byte_offset,
        prefix_fingerprint: batch.checkpoint.prefix_fingerprint,
        parser_state: batch.checkpoint.parser_state,
        observed_at_unix_ms,
        records: batch.records,
        warnings: batch.warnings,
    }
}

/// Writes a privacy-reviewed export of the canonical event ledger to the
/// local exports directory. Payloads contain normalized usage facts only —
/// never source paths, warnings, provenance text, or message content — and
/// every export is an explicit, user-triggered action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportService {
    data_directory: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportSummary {
    pub format: ExportFormat,
    pub file_name: String,
    pub event_count: u64,
}

impl ExportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }
}

impl ExportService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            data_directory: data_directory.as_ref().to_owned(),
        }
    }

    pub fn export_to_file(
        &self,
        format: ExportFormat,
        observed_at_unix_ms: i64,
    ) -> Result<ExportSummary, LocalDataServiceError> {
        let database = open_local_database(&local_database_path(&self.data_directory))?;
        let rows = database
            .export_event_rows()
            .map_err(LocalDataServiceError::Database)?;
        let content = match format {
            ExportFormat::Json => render_json(&rows, observed_at_unix_ms),
            ExportFormat::Csv => render_csv(&rows),
        };
        let exports_directory = self.data_directory.join("AgentMeter").join("exports");
        std::fs::create_dir_all(&exports_directory)
            .map_err(LocalDataServiceError::DataDirectory)?;
        let file_name = format!(
            "agentmeter-events-{}-{}.{}",
            compact_timestamp(observed_at_unix_ms),
            deterministic_suffix(&content),
            format.extension(),
        );
        std::fs::write(exports_directory.join(&file_name), content)
            .map_err(LocalDataServiceError::DataDirectory)?;
        Ok(ExportSummary {
            format,
            file_name,
            event_count: rows.len() as u64,
        })
    }
}

/// Exact minimal decimal for a nano-USD amount, without locale formatting:
/// `0.12` stays `0.12` and whole dollars stay integral.
pub fn usd_decimal(value: NanoUsd) -> String {
    let nanos = value.as_nanos();
    let whole = nanos / 1_000_000_000;
    let fraction = nanos % 1_000_000_000;
    if fraction == 0 {
        return format!("{whole}");
    }
    let mut fraction = format!("{fraction:09}");
    while fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{whole}.{fraction}")
}

fn compact_timestamp(observed_at_unix_ms: i64) -> String {
    let seconds = observed_at_unix_ms.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        seconds_of_day / 3_600,
        seconds_of_day % 3_600 / 60,
        seconds_of_day % 60,
    )
}

/// Floor-division civil-date conversion so the UTC file-name stamp stays
/// correct without a calendar dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn deterministic_suffix(content: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn render_json(rows: &[agentmeter_storage::ExportEventRow], generated_at_unix_ms: i64) -> String {
    let events: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "eventId": row.event_id,
                "sessionId": row.session_id,
                "client": row.client,
                "provider": row.provider,
                "model": row.model,
                "occurredAtUnixMs": row.occurred_at_unix_ms,
                "tokens": {
                    "input": row.tokens.input,
                    "output": row.tokens.output,
                    "cacheRead": row.tokens.cache_read,
                    "cacheWrite": row.tokens.cache_write,
                    "reasoning": row.tokens.reasoning,
                },
                "totalTokens": row.tokens.checked_total(),
                "sourceReportedTotal": row.source_reported_total,
                "confidence": confidence_str(row.confidence),
                "costs": {
                    "providerReportedUsd": row
                        .provider_reported_usd
                        .map(usd_decimal),
                    "apiEquivalentEstimateUsd": row
                        .api_equivalent_estimate_usd
                        .map(usd_decimal),
                    "unpriced": row.unpriced,
                },
                "pricing": {
                    "key": row.pricing_key,
                    "rule": row.pricing_rule,
                },
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "format": "agentmeter-events-v1",
        "generatedAtUnixMs": generated_at_unix_ms,
        "eventCount": rows.len(),
        "events": events,
    }))
    .expect("export JSON is always serializable")
}

fn confidence_str(confidence: agentmeter_core::DataConfidence) -> &'static str {
    match confidence {
        agentmeter_core::DataConfidence::Exact => "exact",
        agentmeter_core::DataConfidence::Derived => "derived",
        agentmeter_core::DataConfidence::Estimated => "estimated",
    }
}

const CSV_HEADER: &str = "event_id,session_id,occurred_at_unix_ms,client,provider,model,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,total_tokens,source_reported_total,confidence,provider_reported_usd,api_equivalent_estimate_usd,unpriced,pricing_key,pricing_rule";

fn render_csv(rows: &[agentmeter_storage::ExportEventRow]) -> String {
    let mut lines = vec![CSV_HEADER.to_owned()];
    for row in rows {
        let fields = [
            row.event_id.clone(),
            row.session_id.clone().unwrap_or_default(),
            row.occurred_at_unix_ms.to_string(),
            row.client.clone(),
            row.provider.clone().unwrap_or_default(),
            row.model.clone(),
            row.tokens.input.to_string(),
            row.tokens.output.to_string(),
            row.tokens.cache_read.to_string(),
            row.tokens.cache_write.to_string(),
            row.tokens.reasoning.to_string(),
            row.tokens
                .checked_total()
                .map(|total| total.to_string())
                .unwrap_or_default(),
            row.source_reported_total
                .map(|total| total.to_string())
                .unwrap_or_default(),
            confidence_str(row.confidence).to_owned(),
            row.provider_reported_usd
                .map(usd_decimal)
                .unwrap_or_default(),
            row.api_equivalent_estimate_usd
                .map(usd_decimal)
                .unwrap_or_default(),
            row.unpriced.to_string(),
            row.pricing_key.clone().unwrap_or_default(),
            row.pricing_rule.clone().unwrap_or_default(),
        ];
        lines.push(
            fields
                .into_iter()
                .map(csv_field)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

fn csv_field(value: String) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
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
        ActivityDimension, ActivityGranularity, ActivityService, CancellationToken, ExportFormat,
        ExportService, IngestionService, LocalDataErrorKind, ModelsPricingService, OverviewService,
        PreferencesService, PricingService, SessionsService, SourcesService,
    };
    use agentmeter_collectors::{codex::CodexJsonlAdapter, kimi::KimiWireAdapter};
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
        Database::open(&database_path)
            .unwrap()
            .record_pricing_snapshot(
                "unapplied-synthetic",
                "future",
                1_704_067_500_000,
                "unapplied-content",
            )
            .unwrap();

        let models_pricing = ModelsPricingService::in_data_directory(directory.path())
            .load(&dataset)
            .unwrap();
        assert_eq!(models_pricing.models.len(), 2);
        let priced = models_pricing
            .models
            .iter()
            .find(|model| model.model == "model-priced")
            .unwrap();
        assert_eq!(priced.total_tokens, 10);
        assert_eq!(priced.event_count, 1);
        assert_eq!(
            priced.provider_reported_usd,
            Some(NanoUsd::from_nanos(1_000_000_000))
        );
        assert_eq!(
            priced.api_equivalent_estimate_usd,
            Some(NanoUsd::from_nanos(10_000))
        );
        assert_eq!(priced.pricing_keys, ["model-priced"]);
        assert_eq!(priced.pricing_rules, ["exact"]);
        assert_eq!(priced.pricing_confidence, Some(DataConfidence::Estimated));
        let applied = models_pricing.applied.as_ref().unwrap();
        assert_eq!(applied.source, dataset.source);
        assert_eq!(applied.version, dataset.version);
        assert_eq!(applied.dataset_updated_at_unix_ms, 1_704_067_400_000);
        assert_eq!(applied.priced_event_count, 2);
        assert_eq!(applied.unpriced_event_count, 0);
        let kimi = models_pricing
            .rates
            .iter()
            .find(|rate| rate.key == "kimi-k2.7-code")
            .unwrap();
        assert_eq!(kimi.aliases, ["kimi-for-coding"]);
        assert_eq!(kimi.input_per_million, NanoUsd::from_nanos(950_000_000));
        assert_eq!(
            models_pricing,
            ModelsPricingService::in_data_directory(directory.path())
                .load(&dataset)
                .unwrap()
        );

        let bundled_service = ModelsPricingService::in_data_directory(directory.path());
        let bundled = bundled_service
            .load_or_apply_bundled(1_704_067_600_000)
            .unwrap();
        assert_eq!(
            bundled.applied.as_ref().unwrap().content_hash,
            RateDataset::bundled().content_hash()
        );
        assert_eq!(bundled.applied.as_ref().unwrap().priced_event_count, 0);
        assert_eq!(bundled.applied.as_ref().unwrap().unpriced_event_count, 2);
        assert_eq!(
            bundled,
            bundled_service
                .load_or_apply_bundled(1_704_067_700_000)
                .unwrap(),
            "an unchanged complete bundled run is not rewritten"
        );
    }

    #[test]
    fn exports_privacy_reviewed_json_and_csv_payloads() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("AgentMeter/agentmeter.db");
        std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut database = Database::open(&database_path).unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-export".into(),
                source_object_id: "source-export".into(),
                adapter_id: "reference-jsonl".into(),
                platform: "test".into(),
                root_path: "/fixture/home/reference".into(),
                discovery_method: "fixture".into(),
                native_path: "/fixture/home/reference/events.jsonl".into(),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        let mut record = pricing_record("event-export", "model-priced");
        record.event.client = "kimi".into();
        record.event.provider = Some("moonshot".into());
        record.event.tokens = TokenBreakdown {
            input: 10,
            output: 2,
            ..TokenBreakdown::default()
        };
        record.costs.push(CostFact {
            kind: CostKind::ProviderReported,
            usd: Some(NanoUsd::from_nanos(120_000_000)),
            confidence: DataConfidence::Exact,
        });
        database
            .apply_ingest(IngestRequest {
                source_object_id: "source-export".into(),
                parser_version: 1,
                mode: WriteMode::Append,
                source_fingerprint: "fingerprint-synthetic".into(),
                source_len: 128,
                byte_offset: Some(128),
                prefix_fingerprint: Some("prefix-synthetic".into()),
                parser_state: Vec::new(),
                observed_at_unix_ms: 1_704_067_200_000,
                records: vec![record],
                warnings: vec!["diagnostic text must never leak".into()],
            })
            .unwrap();

        let service = ExportService::in_data_directory(directory.path());
        let summary = service
            .export_to_file(ExportFormat::Json, 1_787_011_200_000)
            .unwrap();
        assert_eq!(summary.event_count, 1);
        assert!(summary.file_name.starts_with("agentmeter-events-20260818"));
        assert!(summary.file_name.ends_with(".json"));
        let json = std::fs::read_to_string(
            directory
                .path()
                .join("AgentMeter/exports")
                .join(&summary.file_name),
        )
        .unwrap();
        assert!(json.contains("\"format\": \"agentmeter-events-v1\""));
        assert!(json.contains("\"eventId\": \"event-export\""));
        assert!(json.contains("\"providerReportedUsd\": \"0.12\""));
        assert!(
            !json.contains("/fixture"),
            "paths must never appear in exports"
        );
        assert!(
            !json.contains("diagnostic text"),
            "warnings must never appear"
        );
        assert!(!json.contains("prefix-synthetic"));

        let summary = service
            .export_to_file(ExportFormat::Csv, 1_787_011_200_000)
            .unwrap();
        let csv = std::fs::read_to_string(
            directory
                .path()
                .join("AgentMeter/exports")
                .join(&summary.file_name),
        )
        .unwrap();
        assert!(csv.starts_with("event_id,session_id,occurred_at_unix_ms"));
        assert!(csv.contains("event-export"));
        assert!(
            csv.contains("moonshot,model-priced,10,2,0,0,0,12,10,exact,0.12"),
            "unexpected CSV rows:\n{csv}"
        );
        assert!(!csv.contains("/fixture"));
        assert!(!csv.contains("diagnostic text"));
    }

    #[test]
    fn collects_from_real_adapters_end_to_end() {
        let data_directory = tempdir().unwrap();
        let kimi_root = tempdir().unwrap();
        let kimi_session = kimi_root
            .path()
            .join("sessions")
            .join("wd_project_ab12cd34ef56")
            .join("session_1a2b3c4d-0506-0708-090a-0b0c0d0e0f10")
            .join("agents")
            .join("main");
        std::fs::create_dir_all(&kimi_session).unwrap();
        let kimi_wire = kimi_session.join("wire.jsonl");
        std::fs::write(
            &kimi_wire,
            concat!(
                r#"{"type":"metadata","protocol_version":"1.5"}"#,
                '\n',
                r#"{"type":"llm.request","kind":"loop","provider":"moonshot","model":"kimi-for-coding","time":1782113170000}"#,
                '\n',
                r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":3064,"output":76,"inputCacheRead":14848,"inputCacheCreation":0},"usageScope":"turn","time":1782113184943}"#,
                '\n',
            ),
        )
        .unwrap();
        let codex_home = tempdir().unwrap();
        let codex_sessions = codex_home.path().join("sessions");
        std::fs::create_dir_all(&codex_sessions).unwrap();
        std::fs::write(
            codex_sessions.join("rollout-synthetic.jsonl"),
            concat!(
                r#"{"timestamp":"2024-01-01T00:00:00Z","ordinal":0,"type":"session_meta","payload":{"id":"thread-synthetic","source":"cli","model_provider":"openai"}}"#,
                '\n',
                r#"{"timestamp":"2024-01-01T00:00:01Z","ordinal":1,"type":"turn_context","payload":{"model":"gpt-5.3-codex"}}"#,
                '\n',
                r#"{"timestamp":"2024-01-01T00:00:02Z","ordinal":2,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}}"#,
                '\n',
            ),
        )
        .unwrap();

        let service = IngestionService::with_adapters(
            data_directory.path(),
            vec![
                (
                    kimi_root.path().to_owned(),
                    Box::new(KimiWireAdapter::new(kimi_root.path())),
                ),
                (
                    codex_home.path().to_owned(),
                    Box::new(CodexJsonlAdapter::new(codex_home.path())),
                ),
            ],
        );
        let summary = service.scan_and_ingest(1_787_011_200_000).unwrap();

        assert_eq!(summary.runs.len(), 2);
        let kimi_run = summary
            .runs
            .iter()
            .find(|run| run.adapter_id == "kimi-wire")
            .unwrap();
        assert_eq!(kimi_run.discovered_sources, 1);
        assert_eq!(kimi_run.ingested_sources, 1);
        assert_eq!(kimi_run.failed_sources, 0);
        let codex_run = summary
            .runs
            .iter()
            .find(|run| run.adapter_id == "codex-cli-jsonl")
            .unwrap();
        assert_eq!(codex_run.ingested_sources, 1);

        let database =
            Database::open(data_directory.path().join("AgentMeter/agentmeter.db")).unwrap();
        let overview = database.overview_snapshot().unwrap();
        assert_eq!(
            overview.event_count, 2,
            "one Kimi delta and one Codex delta"
        );
        let health = database.source_health_snapshot().unwrap();
        assert_eq!(health.sources.len(), 2);
        assert!(
            health
                .sources
                .iter()
                .all(|source| source.last_success_unix_ms.is_some())
        );

        // A repeated scan is idempotent and appends only new bytes.
        let repeated = service.scan_and_ingest(1_787_011_300_000).unwrap();
        assert!(repeated.runs.iter().all(|run| run.failed_sources == 0));
        let database =
            Database::open(data_directory.path().join("AgentMeter/agentmeter.db")).unwrap();
        assert_eq!(
            database.overview_snapshot().unwrap().event_count,
            2,
            "unchanged sources must not double count"
        );

        std::fs::write(
            &kimi_wire,
            std::fs::read_to_string(&kimi_wire)
                .unwrap()
                + r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":10,"output":2,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1782113284943}"#
                + "\n",
        )
        .unwrap();
        service.scan_and_ingest(1_787_011_400_000).unwrap();
        let database =
            Database::open(data_directory.path().join("AgentMeter/agentmeter.db")).unwrap();
        assert_eq!(
            database.overview_snapshot().unwrap().event_count,
            3,
            "appended Kimi usage resumes from the checkpoint"
        );
    }

    #[test]
    fn cancelled_scans_commit_whole_sources_and_resume_cleanly() {
        let data_directory = tempdir().unwrap();
        let kimi_root = tempdir().unwrap();
        write_kimi_usage(
            kimi_root.path(),
            "aa000000-0000-4000-8000-00000000000a",
            1_782_113_000_000,
        );
        write_kimi_usage(
            kimi_root.path(),
            "bb000000-0000-4000-8000-00000000000b",
            1_782_114_000_000,
        );

        let service = IngestionService::with_adapters(
            data_directory.path(),
            vec![(
                kimi_root.path().to_owned(),
                Box::new(KimiWireAdapter::new(kimi_root.path())),
            )],
        );

        // Cancel at the third cancellation check: the adapter check and the
        // first source pass, the second source never starts.
        let mut checks = 0_u32;
        let partial = service
            .scan_with(1_787_011_200_000, &CancellationToken::new(), || {
                checks += 1;
                checks >= 3
            })
            .unwrap();

        assert!(partial.cancelled);
        assert_eq!(partial.runs[0].discovered_sources, 2);
        assert_eq!(partial.runs[0].ingested_sources, 1);
        let database_path = data_directory.path().join("AgentMeter/agentmeter.db");
        assert_eq!(
            Database::open(&database_path)
                .unwrap()
                .overview_snapshot()
                .unwrap()
                .event_count,
            1,
            "the completed source stays committed"
        );

        // A later uncancellable scan resumes and finishes the rest.
        let completed = service.scan_and_ingest(1_787_011_300_000).unwrap();
        assert!(!completed.cancelled);
        assert_eq!(completed.runs[0].ingested_sources, 2);
        assert_eq!(
            Database::open(&database_path)
                .unwrap()
                .overview_snapshot()
                .unwrap()
                .event_count,
            2,
            "the skipped source ingests exactly once on the resumed scan"
        );
    }

    #[test]
    fn pre_cancelled_scans_do_nothing() {
        let data_directory = tempdir().unwrap();
        let kimi_root = tempdir().unwrap();
        write_kimi_usage(
            kimi_root.path(),
            "aa000000-0000-4000-8000-00000000000a",
            1_782_113_000_000,
        );

        let service = IngestionService::with_adapters(
            data_directory.path(),
            vec![(
                kimi_root.path().to_owned(),
                Box::new(KimiWireAdapter::new(kimi_root.path())),
            )],
        );
        let token = CancellationToken::new();
        token.cancel();

        let summary = service
            .scan_and_ingest_cancellable(1_787_011_200_000, &token)
            .unwrap();

        assert!(summary.cancelled);
        assert!(summary.runs.is_empty());
        assert_eq!(
            Database::open(data_directory.path().join("AgentMeter/agentmeter.db"))
                .unwrap()
                .overview_snapshot()
                .unwrap()
                .event_count,
            0
        );
    }

    /// One Kimi agent journal holding a single usage delta.
    fn write_kimi_usage(root: &std::path::Path, session: &str, time: u64) {
        let directory = root
            .join("sessions")
            .join(format!("wd_{session}"))
            .join(format!("session_{session}"))
            .join("agents")
            .join("main");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("wire.jsonl"),
            format!(
                "{{\"type\":\"metadata\",\"protocol_version\":\"1.5\"}}\n{{\"type\":\"usage.record\",\"model\":\"kimi-code/kimi-for-coding\",\"usage\":{{\"inputOther\":100,\"output\":20,\"inputCacheRead\":0,\"inputCacheCreation\":0}},\"usageScope\":\"turn\",\"time\":{time}}}\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn records_adapter_failures_as_health_diagnostics() {
        let data_directory = tempdir().unwrap();
        let codex_home = tempdir().unwrap();
        let codex_sessions = codex_home.path().join("sessions");
        std::fs::create_dir_all(&codex_sessions).unwrap();
        std::fs::write(codex_sessions.join("rollout-corrupt.jsonl.zst"), "not-zstd").unwrap();

        let service = IngestionService::with_adapters(
            data_directory.path(),
            vec![(
                codex_home.path().to_owned(),
                Box::new(CodexJsonlAdapter::new(codex_home.path())),
            )],
        );
        let summary = service.scan_and_ingest(1_787_011_200_000).unwrap();

        assert_eq!(summary.runs[0].discovered_sources, 1);
        assert_eq!(summary.runs[0].failed_sources, 1);
        assert_eq!(summary.runs[0].ingested_sources, 0);

        let database =
            Database::open(data_directory.path().join("AgentMeter/agentmeter.db")).unwrap();
        let health = database.source_health_snapshot().unwrap();
        assert_eq!(health.sources.len(), 1);
        assert_eq!(
            health.sources[0].state,
            agentmeter_core::SourceHealthState::Error
        );
        assert!(health.sources[0].error.is_some());
        assert_eq!(
            health.sources[0].remediation,
            Some(agentmeter_core::SourceRemediation::RetryCollection)
        );
    }

    #[test]
    fn classifies_an_export_data_directory_failure() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();

        let error = ExportService::in_data_directory(directory.path())
            .export_to_file(ExportFormat::Csv, 0)
            .unwrap_err();

        assert_eq!(error.kind(), LocalDataErrorKind::DataDirectory);
    }

    #[test]
    fn decimal_and_timestamp_helpers_stay_exact() {
        use super::{compact_timestamp, usd_decimal};
        assert_eq!(usd_decimal(NanoUsd::from_nanos(120_000_000)), "0.12");
        assert_eq!(usd_decimal(NanoUsd::from_nanos(3_000_000_000)), "3");
        assert_eq!(usd_decimal(NanoUsd::from_nanos(1)), "0.000000001");
        assert_eq!(compact_timestamp(1_787_011_200_000), "20260818-000000");
        assert_eq!(compact_timestamp(1_787_046_083_000), "20260818-094123");
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
