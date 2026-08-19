//! Durable local storage for AgentMeter events.

use std::{collections::BTreeMap, path::Path};

use agentmeter_core::{
    AppPreferences, AppearancePreference, CostFact, CostKind, DataConfidence, LanguagePreference,
    NanoUsd, OverviewCostSummary, OverviewDataQuality, OverviewSnapshot, SourceHealth,
    SourceHealthSnapshot, SourceHealthState, SourcePermissionState, SourceRemediation,
    TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;

const MIGRATION_V1: &str = r#"
CREATE TABLE source_installations (
    id                  TEXT PRIMARY KEY,
    adapter_id          TEXT NOT NULL,
    platform            TEXT NOT NULL,
    root_path           TEXT NOT NULL,
    discovery_method    TEXT NOT NULL,
    enabled             INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    permission_state    TEXT NOT NULL DEFAULT 'unknown',
    UNIQUE (adapter_id, root_path)
);

CREATE TABLE source_objects (
    id                      TEXT PRIMARY KEY,
    installation_id         TEXT NOT NULL REFERENCES source_installations(id) ON DELETE CASCADE,
    native_path             TEXT NOT NULL,
    kind                    TEXT NOT NULL,
    fingerprint             TEXT,
    parser_version          INTEGER NOT NULL DEFAULT 0 CHECK (parser_version >= 0),
    byte_offset             INTEGER CHECK (byte_offset IS NULL OR byte_offset >= 0),
    source_len              INTEGER NOT NULL DEFAULT 0 CHECK (source_len >= 0),
    prefix_fingerprint      TEXT,
    parser_state            BLOB NOT NULL DEFAULT X'',
    last_scan_unix_ms       INTEGER,
    last_success_unix_ms    INTEGER,
    last_error              TEXT,
    UNIQUE (installation_id, native_path)
);

CREATE TABLE sessions (
    source_object_id        TEXT NOT NULL REFERENCES source_objects(id) ON DELETE CASCADE,
    session_id              TEXT NOT NULL,
    client                  TEXT NOT NULL,
    project                 TEXT,
    title                   TEXT,
    started_at_unix_ms      INTEGER NOT NULL,
    ended_at_unix_ms        INTEGER NOT NULL,
    PRIMARY KEY (source_object_id, session_id)
);

CREATE TABLE usage_events (
    source_object_id        TEXT NOT NULL REFERENCES source_objects(id) ON DELETE CASCADE,
    event_id                TEXT NOT NULL,
    session_id              TEXT,
    occurred_at_unix_ms     INTEGER NOT NULL,
    client                  TEXT NOT NULL,
    provider                TEXT NOT NULL DEFAULT '',
    model                   TEXT NOT NULL,
    input_tokens            INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens           INTEGER NOT NULL CHECK (output_tokens >= 0),
    cache_read_tokens       INTEGER NOT NULL CHECK (cache_read_tokens >= 0),
    cache_write_tokens      INTEGER NOT NULL CHECK (cache_write_tokens >= 0),
    reasoning_tokens        INTEGER NOT NULL CHECK (reasoning_tokens >= 0),
    source_reported_total   INTEGER CHECK (source_reported_total IS NULL OR source_reported_total >= 0),
    confidence              TEXT NOT NULL CHECK (confidence IN ('exact', 'derived', 'estimated')),
    PRIMARY KEY (source_object_id, event_id),
    FOREIGN KEY (source_object_id, session_id)
        REFERENCES sessions(source_object_id, session_id) ON DELETE SET NULL
);

CREATE INDEX usage_events_occurred_at_idx ON usage_events(occurred_at_unix_ms);
CREATE INDEX usage_events_client_model_idx ON usage_events(client, model);

CREATE TABLE event_provenance (
    source_object_id        TEXT NOT NULL,
    event_id                TEXT NOT NULL,
    native_id               TEXT,
    record_offset           INTEGER CHECK (record_offset IS NULL OR record_offset >= 0),
    schema_variant          TEXT NOT NULL,
    timestamp_origin        TEXT NOT NULL CHECK (timestamp_origin IN ('source', 'derived', 'file_modified')),
    normalization_notes_json TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (source_object_id, event_id),
    FOREIGN KEY (source_object_id, event_id)
        REFERENCES usage_events(source_object_id, event_id) ON DELETE CASCADE
);

CREATE TABLE ingest_runs (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    source_object_id    TEXT NOT NULL REFERENCES source_objects(id) ON DELETE CASCADE,
    started_at_unix_ms  INTEGER NOT NULL,
    finished_at_unix_ms INTEGER,
    status              TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed')),
    mode                TEXT NOT NULL CHECK (mode IN ('append', 'replace')),
    parsed_records      INTEGER NOT NULL DEFAULT 0,
    inserted_records    INTEGER NOT NULL DEFAULT 0,
    duplicate_records   INTEGER NOT NULL DEFAULT 0,
    deleted_records     INTEGER NOT NULL DEFAULT 0,
    warnings_json       TEXT NOT NULL DEFAULT '[]',
    error               TEXT
);

CREATE TABLE pricing_snapshots (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    source              TEXT NOT NULL,
    dataset_version     TEXT NOT NULL,
    fetched_at_unix_ms  INTEGER NOT NULL,
    expires_at_unix_ms  INTEGER,
    content_hash        TEXT NOT NULL,
    UNIQUE (source, dataset_version, content_hash)
);

CREATE TABLE event_costs (
    source_object_id    TEXT NOT NULL,
    event_id            TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK (kind IN ('provider_reported', 'api_equivalent_estimate', 'subscription_credit', 'unpriced')),
    usd                 REAL,
    pricing_snapshot_id INTEGER REFERENCES pricing_snapshots(id) ON DELETE SET NULL,
    pricing_key         TEXT,
    pricing_rule        TEXT,
    confidence          TEXT NOT NULL,
    PRIMARY KEY (source_object_id, event_id, kind),
    FOREIGN KEY (source_object_id, event_id)
        REFERENCES usage_events(source_object_id, event_id) ON DELETE CASCADE
);

CREATE TABLE preferences (
    key                 TEXT PRIMARY KEY,
    value_json          TEXT NOT NULL
);

CREATE TABLE daily_usage_utc (
    day                 TEXT NOT NULL,
    client              TEXT NOT NULL,
    provider            TEXT NOT NULL,
    model               TEXT NOT NULL,
    input_tokens        INTEGER NOT NULL,
    output_tokens       INTEGER NOT NULL,
    cache_read_tokens   INTEGER NOT NULL,
    cache_write_tokens  INTEGER NOT NULL,
    reasoning_tokens    INTEGER NOT NULL,
    event_count         INTEGER NOT NULL,
    PRIMARY KEY (day, client, provider, model)
);

PRAGMA user_version = 1;
"#;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than supported version {supported}")]
    NewerSchema { found: u32, supported: u32 },
    #[error("source object is not registered: {0}")]
    SourceNotRegistered(String),
    #[error("event identity {event_id} was reused with different usage facts")]
    EventIdentityConflict { event_id: String },
    #[error("event {event_id} has an invalid {kind} cost fact")]
    InvalidCostFact {
        event_id: String,
        kind: &'static str,
    },
    #[error("{field} value does not fit SQLite INTEGER")]
    IntegerOutOfRange { field: &'static str },
    #[error("token totals overflowed while building a reconciliation report")]
    ReconciliationOverflow,
    #[error("usage or cost totals overflowed while building an overview snapshot")]
    OverviewOverflow,
    #[error("usage or cost totals overflowed while building an activity snapshot")]
    ActivityOverflow,
    #[error("usage or cost totals overflowed while building a session snapshot")]
    SessionOverflow,
    #[error("stored {field} preference has an unsupported value: {value}")]
    InvalidPreference { field: &'static str, value: String },
    #[error("failed to serialize diagnostics: {0}")]
    Diagnostics(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StorageError>;

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        if !connection.is_autocommit() {
            return Err(rusqlite::Error::ExecuteReturnedResults.into());
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;

        let mut database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&mut self) -> Result<()> {
        let found = self.schema_version()?;
        if found > SCHEMA_VERSION {
            return Err(StorageError::NewerSchema {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if found == 0 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(MIGRATION_V1)?;
            transaction.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn register_source(&mut self, registration: &SourceRegistration) -> Result<()> {
        let transaction = self.connection.transaction()?;
        upsert_installation(
            &transaction,
            &SourceInstallationRegistration {
                installation_id: registration.installation_id.clone(),
                adapter_id: registration.adapter_id.clone(),
                platform: registration.platform.clone(),
                root_path: registration.root_path.clone(),
                discovery_method: registration.discovery_method.clone(),
                enabled: true,
                permission: SourcePermissionState::Granted,
            },
            false,
        )?;
        transaction.execute(
            "INSERT INTO source_objects (id, installation_id, native_path, kind)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                installation_id = excluded.installation_id,
                native_path = excluded.native_path,
                kind = excluded.kind",
            params![
                registration.source_object_id,
                registration.installation_id,
                registration.native_path,
                registration.kind,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn register_installation(
        &mut self,
        registration: &SourceInstallationRegistration,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        upsert_installation(&transaction, registration, true)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_installation_state(
        &mut self,
        installation_id: &str,
        enabled: bool,
        permission: SourcePermissionState,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE source_installations
             SET enabled = ?2, permission_state = ?3
             WHERE id = ?1",
            params![installation_id, enabled, permission_state_str(permission)],
        )?;
        Ok(())
    }

    pub fn record_collection_failure(
        &mut self,
        source_object_id: &str,
        observed_at_unix_ms: i64,
        kind: CollectionFailureKind,
        message: &str,
    ) -> Result<()> {
        if !self.source_exists(source_object_id)? {
            return Err(StorageError::SourceNotRegistered(
                source_object_id.to_owned(),
            ));
        }
        let stored_error = encode_collection_error(kind, message);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO ingest_runs (
                source_object_id, started_at_unix_ms, finished_at_unix_ms,
                status, mode, warnings_json, error
             ) VALUES (?1, ?2, ?2, 'failed', 'append', '[]', ?3)",
            params![source_object_id, observed_at_unix_ms, stored_error],
        )?;
        transaction.execute(
            "UPDATE source_objects
             SET last_scan_unix_ms = ?2, last_error = ?3
             WHERE id = ?1",
            params![source_object_id, observed_at_unix_ms, stored_error],
        )?;
        if kind == CollectionFailureKind::Permission {
            transaction.execute(
                "UPDATE source_installations
                 SET permission_state = 'denied'
                 WHERE id = (SELECT installation_id FROM source_objects WHERE id = ?1)",
                params![source_object_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn checkpoint_status(
        &self,
        source_object_id: &str,
        parser_version: u32,
    ) -> Result<CheckpointStatus> {
        let stored = self
            .connection
            .query_row(
                "SELECT parser_version, byte_offset, source_len,
                        prefix_fingerprint, parser_state, fingerprint
                 FROM source_objects
                 WHERE id = ?1 AND last_success_unix_ms IS NOT NULL",
                params![source_object_id],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        StoredCheckpoint {
                            byte_offset: row.get::<_, Option<i64>>(1)?.map(|value| value as u64),
                            source_len: row.get::<_, i64>(2)? as u64,
                            prefix_fingerprint: row.get(3)?,
                            parser_state: row.get(4)?,
                            source_fingerprint: row.get(5)?,
                        },
                    ))
                },
            )
            .optional()
            .map_err(StorageError::from)?;

        Ok(match stored {
            None => CheckpointStatus::NeverIngested,
            Some((stored_parser_version, checkpoint))
                if stored_parser_version == parser_version =>
            {
                CheckpointStatus::Current(checkpoint)
            }
            Some((stored_parser_version, _)) => CheckpointStatus::Invalidated {
                stored_parser_version,
            },
        })
    }

    pub fn apply_ingest(&mut self, request: IngestRequest) -> Result<IngestReport> {
        if !self.source_exists(&request.source_object_id)? {
            return Err(StorageError::SourceNotRegistered(request.source_object_id));
        }

        let mode = request.mode.as_str();
        let warnings_json = serde_json::to_string(&request.warnings)?;
        self.connection.execute(
            "INSERT INTO ingest_runs (
                source_object_id, started_at_unix_ms, status, mode, parsed_records, warnings_json
             ) VALUES (?1, ?2, 'running', ?3, ?4, ?5)",
            params![
                request.source_object_id,
                request.observed_at_unix_ms,
                mode,
                to_sql_integer(request.records.len() as u64, "parsed_records")?,
                warnings_json,
            ],
        )?;
        let run_id = self.connection.last_insert_rowid();

        match self.apply_ingest_transaction(run_id, &request) {
            Ok(report) => Ok(report),
            Err(error) => {
                let transaction = self.connection.transaction()?;
                let error_message = error.to_string();
                transaction.execute(
                    "UPDATE ingest_runs
                     SET status = 'failed', finished_at_unix_ms = ?2, error = ?3
                     WHERE id = ?1",
                    params![run_id, request.observed_at_unix_ms, error_message],
                )?;
                transaction.execute(
                    "UPDATE source_objects
                     SET last_scan_unix_ms = ?2, last_error = ?3
                     WHERE id = ?1",
                    params![
                        request.source_object_id,
                        request.observed_at_unix_ms,
                        error_message,
                    ],
                )?;
                transaction.commit()?;
                Err(error)
            }
        }
    }

    fn apply_ingest_transaction(
        &mut self,
        run_id: i64,
        request: &IngestRequest,
    ) -> Result<IngestReport> {
        let transaction = self.connection.transaction()?;
        let deleted_records = if request.mode == WriteMode::Replace {
            let deleted = transaction.execute(
                "DELETE FROM usage_events WHERE source_object_id = ?1",
                params![request.source_object_id],
            )?;
            transaction.execute(
                "DELETE FROM sessions WHERE source_object_id = ?1",
                params![request.source_object_id],
            )?;
            deleted
        } else {
            0
        };

        let mut inserted_records = 0_usize;
        let mut duplicate_records = 0_usize;
        for record in &request.records {
            upsert_session(&transaction, &request.source_object_id, &record.event)?;
            let inserted = insert_record(&transaction, &request.source_object_id, record)?;
            if inserted {
                inserted_records += 1;
            } else {
                duplicate_records += 1;
            }
        }

        transaction.execute(
            "UPDATE source_objects SET
                fingerprint = ?2,
                parser_version = ?3,
                byte_offset = ?4,
                source_len = ?5,
                prefix_fingerprint = ?6,
                parser_state = ?7,
                last_scan_unix_ms = ?8,
                last_success_unix_ms = ?8,
                last_error = NULL
             WHERE id = ?1",
            params![
                request.source_object_id,
                request.source_fingerprint,
                i64::from(request.parser_version),
                request
                    .byte_offset
                    .map(|value| to_sql_integer(value, "byte_offset"))
                    .transpose()?,
                to_sql_integer(request.source_len, "source_len")?,
                request.prefix_fingerprint,
                request.parser_state,
                request.observed_at_unix_ms,
            ],
        )?;
        transaction.execute(
            "UPDATE source_installations
             SET permission_state = 'granted'
             WHERE id = (SELECT installation_id FROM source_objects WHERE id = ?1)",
            params![request.source_object_id],
        )?;

        rebuild_daily_usage_in(&transaction)?;
        transaction.execute(
            "UPDATE ingest_runs SET
                status = 'completed',
                finished_at_unix_ms = ?2,
                inserted_records = ?3,
                duplicate_records = ?4,
                deleted_records = ?5
             WHERE id = ?1",
            params![
                run_id,
                request.observed_at_unix_ms,
                inserted_records as i64,
                duplicate_records as i64,
                deleted_records as i64,
            ],
        )?;
        transaction.commit()?;

        Ok(IngestReport {
            run_id,
            source_object_id: request.source_object_id.clone(),
            mode: request.mode,
            parsed_records: request.records.len(),
            inserted_records,
            duplicate_records,
            deleted_records,
            warnings: request.warnings.clone(),
        })
    }

    fn source_exists(&self, source_object_id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_objects WHERE id = ?1)",
            params![source_object_id],
            |row| row.get(0),
        )?)
    }

    pub fn event_count(&self, source_object_id: &str) -> Result<u64> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE source_object_id = ?1",
            params![source_object_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    pub fn event_ids(&self, source_object_id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT event_id FROM usage_events
             WHERE source_object_id = ?1 ORDER BY event_id",
        )?;
        let rows = statement.query_map(params![source_object_id], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn event_costs(&self, source_object_id: &str, event_id: &str) -> Result<Vec<CostFact>> {
        let mut statement = self.connection.prepare(
            "SELECT kind, usd, confidence FROM event_costs
             WHERE source_object_id = ?1 AND event_id = ?2 ORDER BY kind",
        )?;
        let rows = statement.query_map(params![source_object_id, event_id], |row| {
            let kind: String = row.get(0)?;
            let usd: Option<f64> = row.get(1)?;
            let confidence: String = row.get(2)?;
            Ok((kind, usd, confidence))
        })?;
        rows.map(|row| {
            let (kind, usd, confidence) = row?;
            Ok(CostFact {
                kind: parse_cost_kind(&kind),
                usd: usd.map(sql_usd_to_nano),
                confidence: parse_confidence(&confidence),
            })
        })
        .collect()
    }

    /// Sources whose most recent successful full replacement is older than
    /// the requested interval. Incremental appends do not postpone this gate.
    pub fn sources_due_for_reconciliation(
        &self,
        now_unix_ms: i64,
        interval_ms: u64,
    ) -> Result<Vec<ReconciliationTarget>> {
        let interval_ms = to_sql_integer(interval_ms, "reconciliation_interval_ms")?;
        let due_before = now_unix_ms.saturating_sub(interval_ms);
        let mut statement = self.connection.prepare(
            "SELECT s.id, i.adapter_id, s.parser_version, r.last_reconciled_unix_ms
             FROM source_objects AS s
             JOIN source_installations AS i ON i.id = s.installation_id
             LEFT JOIN (
                 SELECT source_object_id, MAX(finished_at_unix_ms) AS last_reconciled_unix_ms
                 FROM ingest_runs
                 WHERE status = 'completed' AND mode = 'replace'
                 GROUP BY source_object_id
             ) AS r ON r.source_object_id = s.id
             WHERE i.enabled = 1 AND i.permission_state = 'granted'
               AND (r.last_reconciled_unix_ms IS NULL OR r.last_reconciled_unix_ms <= ?1)
             ORDER BY i.adapter_id, s.id",
        )?;
        let rows = statement.query_map(params![due_before], |row| {
            Ok(ReconciliationTarget {
                source_object_id: row.get(0)?,
                adapter_id: row.get(1)?,
                parser_version: row.get(2)?,
                last_reconciled_unix_ms: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Builds a deterministic, content-free cross-check report. Callers pass
    /// only reviewed aggregate totals from source UIs/CLIs or reference
    /// parsers; arbitrary labels and source excerpts cannot enter the export.
    pub fn reconciliation_report(
        &self,
        generated_at_unix_ms: i64,
        expectations: &[ReferenceExpectation],
    ) -> Result<ReconciliationReport> {
        let mut statement = self.connection.prepare(
            "SELECT s.id, i.adapter_id, s.parser_version, s.last_success_unix_ms
             FROM source_objects AS s
             JOIN source_installations AS i ON i.id = s.installation_id
             ORDER BY i.adapter_id, s.id",
        )?;
        let source_rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut sources = Vec::new();
        let mut adapter_totals = BTreeMap::<String, ReconciliationTokenTotals>::new();
        for source in source_rows {
            let (source_object_id, adapter_id, parser_version, last_success_unix_ms) = source?;
            let source_report = self.source_reconciliation(
                source_object_id,
                adapter_id.clone(),
                parser_version,
                last_success_unix_ms,
            )?;
            adapter_totals
                .entry(adapter_id)
                .or_default()
                .checked_add_assign(source_report.canonical_tokens)?;
            sources.push(source_report);
        }

        let mut expectations = expectations.to_vec();
        expectations.sort_by(|left, right| {
            (&left.adapter_id, left.reference).cmp(&(&right.adapter_id, right.reference))
        });
        let reference_checks = expectations
            .into_iter()
            .map(|expectation| {
                let actual_total_tokens = adapter_totals
                    .get(&expectation.adapter_id)
                    .map(ReconciliationTokenTotals::checked_total)
                    .transpose()?;
                let status = match actual_total_tokens {
                    None => CrossCheckStatus::Unavailable,
                    Some(actual) if actual == expectation.expected_total_tokens => {
                        CrossCheckStatus::Match
                    }
                    Some(_) => CrossCheckStatus::Mismatch,
                };
                Ok(ReferenceCrossCheck {
                    adapter_id: expectation.adapter_id,
                    reference: expectation.reference,
                    expected_total_tokens: expectation.expected_total_tokens,
                    actual_total_tokens,
                    status,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(ReconciliationReport {
            format_version: 1,
            generated_at_unix_ms,
            sources,
            reference_checks,
        })
    }

    fn source_reconciliation(
        &self,
        source_object_id: String,
        adapter_id: String,
        parser_version: u32,
        last_success_unix_ms: Option<i64>,
    ) -> Result<SourceReconciliation> {
        let mut statement = self.connection.prepare(
            "SELECT input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, reasoning_tokens, source_reported_total,
                    confidence
             FROM usage_events WHERE source_object_id = ?1 ORDER BY event_id",
        )?;
        let rows = statement.query_map(params![source_object_id], |row| {
            Ok((
                ReconciliationTokenTotals {
                    input: row.get::<_, i64>(0)? as u64,
                    output: row.get::<_, i64>(1)? as u64,
                    cache_read: row.get::<_, i64>(2)? as u64,
                    cache_write: row.get::<_, i64>(3)? as u64,
                    reasoning: row.get::<_, i64>(4)? as u64,
                },
                row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut canonical_tokens = ReconciliationTokenTotals::default();
        let mut reported_canonical_total = 0_u64;
        let mut source_reported_total = 0_u64;
        let mut event_count = 0_u64;
        let mut reported_event_count = 0_u64;
        let mut derived_event_count = 0_u64;
        let mut estimated_event_count = 0_u64;
        for row in rows {
            let (tokens, source_total, confidence) = row?;
            canonical_tokens.checked_add_assign(tokens)?;
            event_count = event_count
                .checked_add(1)
                .ok_or(StorageError::ReconciliationOverflow)?;
            match confidence.as_str() {
                "derived" => {
                    derived_event_count = derived_event_count
                        .checked_add(1)
                        .ok_or(StorageError::ReconciliationOverflow)?;
                }
                "estimated" => {
                    estimated_event_count = estimated_event_count
                        .checked_add(1)
                        .ok_or(StorageError::ReconciliationOverflow)?;
                }
                _ => {}
            }
            if let Some(source_total) = source_total {
                reported_event_count = reported_event_count
                    .checked_add(1)
                    .ok_or(StorageError::ReconciliationOverflow)?;
                reported_canonical_total = reported_canonical_total
                    .checked_add(tokens.checked_total()?)
                    .ok_or(StorageError::ReconciliationOverflow)?;
                source_reported_total = source_reported_total
                    .checked_add(source_total)
                    .ok_or(StorageError::ReconciliationOverflow)?;
            }
        }
        let source_reported_status = if reported_event_count == 0 {
            SourceReportedStatus::Unavailable
        } else {
            match (
                reported_event_count == event_count,
                reported_canonical_total == source_reported_total,
            ) {
                (true, true) => SourceReportedStatus::Match,
                (true, false) => SourceReportedStatus::Mismatch,
                (false, true) => SourceReportedStatus::PartialMatch,
                (false, false) => SourceReportedStatus::PartialMismatch,
            }
        };
        Ok(SourceReconciliation {
            source_object_id,
            adapter_id,
            parser_version,
            last_success_unix_ms,
            event_count,
            canonical_tokens,
            derived_event_count,
            estimated_event_count,
            source_reported: SourceReportedCrossCheck {
                status: source_reported_status,
                checked_event_count: reported_event_count,
                missing_event_count: event_count - reported_event_count,
                canonical_total_tokens: (reported_event_count != 0)
                    .then_some(reported_canonical_total),
                source_total_tokens: (reported_event_count != 0).then_some(source_reported_total),
            },
        })
    }

    pub fn daily_usage_utc(&self) -> Result<Vec<DailyUsage>> {
        let mut statement = self.connection.prepare(
            "SELECT day, client, provider, model,
                    input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, reasoning_tokens, event_count
             FROM daily_usage_utc
             ORDER BY day, client, provider, model",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DailyUsage {
                day: row.get(0)?,
                client: row.get(1)?,
                provider: nonempty(row.get(2)?),
                model: row.get(3)?,
                tokens: TokenBreakdown {
                    input: row.get::<_, i64>(4)? as u64,
                    output: row.get::<_, i64>(5)? as u64,
                    cache_read: row.get::<_, i64>(6)? as u64,
                    cache_write: row.get::<_, i64>(7)? as u64,
                    reasoning: row.get::<_, i64>(8)? as u64,
                },
                event_count: row.get::<_, i64>(9)? as u64,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Daily UTC facts used by higher-level activity queries. Cost remains an
    /// explicit API-equivalent estimate; events without a reviewed rate are
    /// counted separately instead of becoming a valid zero.
    pub fn activity_daily_rows(&self) -> Result<Vec<ActivityDailyRow>> {
        let mut statement = self.connection.prepare(
            "SELECT date(event.occurred_at_unix_ms / 1000, 'unixepoch'),
                    date(event.occurred_at_unix_ms / 1000, 'unixepoch',
                         printf('-%d days', (strftime('%w', event.occurred_at_unix_ms / 1000,
                         'unixepoch') + 6) % 7)),
                    strftime('%Y-%m-01', event.occurred_at_unix_ms / 1000, 'unixepoch'),
                    event.client, event.provider, event.model,
                    SUM(event.input_tokens), SUM(event.output_tokens),
                    SUM(event.cache_read_tokens), SUM(event.cache_write_tokens),
                    SUM(event.reasoning_tokens), COUNT(*),
                    SUM(CASE WHEN cost.kind = 'api_equivalent_estimate'
                        THEN CAST(ROUND(cost.usd * 1000000000) AS INTEGER) END),
                    COALESCE(SUM(CASE WHEN cost.kind = 'unpriced' THEN 1 ELSE 0 END), 0)
             FROM usage_events AS event
             LEFT JOIN event_costs AS cost
               ON cost.source_object_id = event.source_object_id
              AND cost.event_id = event.event_id
              AND cost.kind IN ('api_equivalent_estimate', 'unpriced')
             GROUP BY date(event.occurred_at_unix_ms / 1000, 'unixepoch'),
                      event.client, event.provider, event.model
             ORDER BY 1, 2, 3, 4",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ActivityDailyRow {
                day: row.get(0)?,
                week_start: row.get(1)?,
                month_start: row.get(2)?,
                client: row.get(3)?,
                provider: nonempty(row.get(4)?),
                model: row.get(5)?,
                tokens: TokenBreakdown {
                    input: row.get::<_, i64>(6)? as u64,
                    output: row.get::<_, i64>(7)? as u64,
                    cache_read: row.get::<_, i64>(8)? as u64,
                    cache_write: row.get::<_, i64>(9)? as u64,
                    reasoning: row.get::<_, i64>(10)? as u64,
                },
                event_count: row.get::<_, i64>(11)? as u64,
                api_equivalent_estimate_usd: row
                    .get::<_, Option<i64>>(12)?
                    .map(|nanos| NanoUsd::from_nanos(nanos as u64)),
                unpriced_event_count: row.get::<_, i64>(13)? as u64,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Immutable, content-free session summaries ordered by most recent
    /// activity. Prompt, response, title, and path content are never joined.
    pub fn session_summaries(&self) -> Result<Vec<SessionSummaryRow>> {
        let mut statement = self.connection.prepare(
            "SELECT session.source_object_id, session.session_id,
                    installation.adapter_id, source.kind, source.parser_version,
                    session.client, session.project,
                    session.started_at_unix_ms, session.ended_at_unix_ms,
                    SUM(event.input_tokens), SUM(event.output_tokens),
                    SUM(event.cache_read_tokens), SUM(event.cache_write_tokens),
                    SUM(event.reasoning_tokens), COUNT(*),
                    CASE MAX(CASE event.confidence
                        WHEN 'estimated' THEN 2 WHEN 'derived' THEN 1 ELSE 0 END)
                        WHEN 2 THEN 'estimated' WHEN 1 THEN 'derived' ELSE 'exact' END,
                    (SELECT GROUP_CONCAT(value, char(31)) FROM (
                        SELECT DISTINCT nested.provider AS value
                        FROM usage_events AS nested
                        WHERE nested.source_object_id = session.source_object_id
                          AND nested.session_id = session.session_id
                          AND nested.provider != ''
                        ORDER BY value
                    )),
                    (SELECT GROUP_CONCAT(value, char(31)) FROM (
                        SELECT DISTINCT nested.model AS value
                        FROM usage_events AS nested
                        WHERE nested.source_object_id = session.source_object_id
                          AND nested.session_id = session.session_id
                        ORDER BY value
                    )),
                    (SELECT SUM(CAST(ROUND(cost.usd * 1000000000) AS INTEGER))
                     FROM event_costs AS cost
                     JOIN usage_events AS cost_event
                       ON cost_event.source_object_id = cost.source_object_id
                      AND cost_event.event_id = cost.event_id
                     WHERE cost_event.source_object_id = session.source_object_id
                       AND cost_event.session_id = session.session_id
                       AND cost.kind = 'provider_reported'),
                    (SELECT SUM(CAST(ROUND(cost.usd * 1000000000) AS INTEGER))
                     FROM event_costs AS cost
                     JOIN usage_events AS cost_event
                       ON cost_event.source_object_id = cost.source_object_id
                      AND cost_event.event_id = cost.event_id
                     WHERE cost_event.source_object_id = session.source_object_id
                       AND cost_event.session_id = session.session_id
                       AND cost.kind = 'api_equivalent_estimate'),
                    (SELECT COUNT(*)
                     FROM event_costs AS cost
                     JOIN usage_events AS cost_event
                       ON cost_event.source_object_id = cost.source_object_id
                      AND cost_event.event_id = cost.event_id
                     WHERE cost_event.source_object_id = session.source_object_id
                       AND cost_event.session_id = session.session_id
                       AND cost.kind = 'unpriced')
             FROM sessions AS session
             JOIN source_objects AS source ON source.id = session.source_object_id
             JOIN source_installations AS installation ON installation.id = source.installation_id
             JOIN usage_events AS event
               ON event.source_object_id = session.source_object_id
              AND event.session_id = session.session_id
             GROUP BY session.source_object_id, session.session_id
             ORDER BY session.ended_at_unix_ms DESC,
                      session.source_object_id, session.session_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SessionSummaryRow {
                source_object_id: row.get(0)?,
                session_id: row.get(1)?,
                adapter_id: row.get(2)?,
                source_kind: row.get(3)?,
                parser_version: row.get::<_, i64>(4)? as u32,
                client: row.get(5)?,
                project: row.get::<_, Option<String>>(6)?.and_then(nonempty),
                started_at_unix_ms: row.get(7)?,
                ended_at_unix_ms: row.get(8)?,
                tokens: TokenBreakdown {
                    input: row.get::<_, i64>(9)? as u64,
                    output: row.get::<_, i64>(10)? as u64,
                    cache_read: row.get::<_, i64>(11)? as u64,
                    cache_write: row.get::<_, i64>(12)? as u64,
                    reasoning: row.get::<_, i64>(13)? as u64,
                },
                event_count: row.get::<_, i64>(14)? as u64,
                confidence: parse_confidence(&row.get::<_, String>(15)?),
                providers: split_grouped_values(row.get(16)?),
                models: split_grouped_values(row.get(17)?),
                provider_reported_usd: row
                    .get::<_, Option<i64>>(18)?
                    .map(|nanos| NanoUsd::from_nanos(nanos as u64)),
                api_equivalent_estimate_usd: row
                    .get::<_, Option<i64>>(19)?
                    .map(|nanos| NanoUsd::from_nanos(nanos as u64)),
                unpriced_event_count: row.get::<_, i64>(20)? as u64,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn overview_snapshot(&self) -> Result<OverviewSnapshot> {
        let (
            input,
            output,
            cache_read,
            cache_write,
            reasoning,
            event_count,
            session_count,
            active_days,
            model_count,
            exact_events,
            derived_events,
            estimated_events,
        ) = self.connection.query_row(
            "SELECT
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_write_tokens), 0),
                COALESCE(SUM(reasoning_tokens), 0),
                COUNT(*),
                (SELECT COUNT(*) FROM sessions),
                COUNT(DISTINCT date(occurred_at_unix_ms / 1000, 'unixepoch')),
                (SELECT COUNT(*) FROM (
                    SELECT provider, model FROM usage_events GROUP BY provider, model
                )),
                COALESCE(SUM(CASE WHEN confidence = 'exact' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN confidence = 'derived' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN confidence = 'estimated' THEN 1 ELSE 0 END), 0)
             FROM usage_events",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                    row.get::<_, i64>(7)? as u64,
                    row.get::<_, i64>(8)? as u64,
                    row.get::<_, i64>(9)? as u64,
                    row.get::<_, i64>(10)? as u64,
                    row.get::<_, i64>(11)? as u64,
                ))
            },
        )?;

        let mut costs = OverviewCostSummary::default();
        let mut unpriced_events = 0_u64;
        let mut statement = self.connection.prepare(
            "SELECT kind, usd FROM event_costs
             WHERE kind IN ('provider_reported', 'api_equivalent_estimate', 'unpriced')
             ORDER BY source_object_id, event_id, kind",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?))
        })?;
        for row in rows {
            let (kind, usd) = row?;
            match (kind.as_str(), usd) {
                ("provider_reported", Some(usd)) => {
                    add_overview_cost(&mut costs.provider_reported_usd, sql_usd_to_nano(usd))?;
                }
                ("api_equivalent_estimate", Some(usd)) => {
                    add_overview_cost(
                        &mut costs.api_equivalent_estimate_usd,
                        sql_usd_to_nano(usd),
                    )?;
                }
                ("unpriced", None) => {
                    unpriced_events = unpriced_events
                        .checked_add(1)
                        .ok_or(StorageError::OverviewOverflow)?;
                }
                _ => {}
            }
        }

        let mut snapshot = OverviewSnapshot {
            generation: 0,
            tokens: TokenBreakdown {
                input,
                output,
                cache_read,
                cache_write,
                reasoning,
            },
            event_count,
            session_count,
            active_days,
            model_count,
            costs,
            data_quality: OverviewDataQuality {
                exact_events,
                derived_events,
                estimated_events,
                unpriced_events,
            },
            source_health: self.source_health_snapshot()?,
        };
        snapshot.generation = overview_generation(&snapshot);
        Ok(snapshot)
    }

    pub fn rebuild_daily_usage(&mut self) -> Result<()> {
        let transaction = self.connection.transaction()?;
        rebuild_daily_usage_in(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn last_ingest_status(&self, source_object_id: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT status FROM ingest_runs
                 WHERE source_object_id = ?1 ORDER BY id DESC LIMIT 1",
                params![source_object_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn source_health_snapshot(&self) -> Result<SourceHealthSnapshot> {
        let mut statement = self.connection.prepare(
            "SELECT
                i.id, i.adapter_id, i.root_path, i.enabled, i.permission_state,
                s.id, s.native_path, s.kind, s.parser_version,
                s.last_scan_unix_ms, s.last_success_unix_ms, s.last_error,
                (SELECT MAX(e.occurred_at_unix_ms)
                   FROM usage_events AS e WHERE e.source_object_id = s.id),
                COALESCE((SELECT r.inserted_records + r.deleted_records
                   FROM ingest_runs AS r WHERE r.source_object_id = s.id
                   ORDER BY r.id DESC LIMIT 1), 0),
                (SELECT r.status FROM ingest_runs AS r
                   WHERE r.source_object_id = s.id ORDER BY r.id DESC LIMIT 1),
                (SELECT r.warnings_json FROM ingest_runs AS r
                   WHERE r.source_object_id = s.id ORDER BY r.id DESC LIMIT 1),
                (SELECT r.error FROM ingest_runs AS r
                   WHERE r.source_object_id = s.id ORDER BY r.id DESC LIMIT 1)
             FROM source_installations AS i
             LEFT JOIN source_objects AS s ON s.installation_id = i.id
             ORDER BY i.adapter_id, i.id, s.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RawSourceHealth {
                installation_id: row.get(0)?,
                adapter_id: row.get(1)?,
                root_path: row.get(2)?,
                enabled: row.get(3)?,
                permission: row.get(4)?,
                source_object_id: row.get(5)?,
                native_path: row.get(6)?,
                source_kind: row.get(7)?,
                parser_version: row.get(8)?,
                last_scan_unix_ms: row.get(9)?,
                last_success_unix_ms: row.get(10)?,
                last_error: row.get(11)?,
                last_event_unix_ms: row.get(12)?,
                records_changed: row.get(13)?,
                last_run_status: row.get(14)?,
                warnings_json: row.get(15)?,
                run_error: row.get(16)?,
            })
        })?;
        let mut sources = Vec::new();
        for row in rows {
            sources.push(row?.into_health()?);
        }
        Ok(SourceHealthSnapshot {
            generation: health_generation(&sources),
            sources,
        })
    }

    #[cfg(test)]
    fn journal_mode(&self) -> Result<String> {
        Ok(self
            .connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?)
    }

    /// Reads persisted user preferences. A database that has never stored a
    /// preference row reports the system-following defaults rather than an
    /// error; an unrecognized stored value is surfaced as one.
    pub fn preferences(&self) -> Result<AppPreferences> {
        let Some(value_json) = self
            .connection
            .query_row(
                "SELECT value_json FROM preferences WHERE key = ?1",
                params![PREFERENCES_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            return Ok(AppPreferences::default());
        };
        let stored: StoredPreferences =
            serde_json::from_str(&value_json).map_err(|_| StorageError::InvalidPreference {
                field: "app_preferences",
                value: value_json.clone(),
            })?;
        Ok(AppPreferences {
            language: parse_language_preference(&stored.language)?,
            appearance: parse_appearance_preference(&stored.appearance)?,
        })
    }

    pub fn set_preferences(&self, preferences: &AppPreferences) -> Result<()> {
        let stored = StoredPreferences {
            language: language_preference_str(preferences.language).to_owned(),
            appearance: appearance_preference_str(preferences.appearance).to_owned(),
        };
        self.connection.execute(
            "INSERT INTO preferences (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![PREFERENCES_KEY, serde_json::to_string(&stored)?],
        )?;
        Ok(())
    }

    /// Records a dataset in the pricing-snapshot ledger and returns its id.
    /// Re-recording identical content is idempotent.
    pub fn record_pricing_snapshot(
        &self,
        source: &str,
        dataset_version: &str,
        fetched_at_unix_ms: i64,
        content_hash: &str,
    ) -> Result<i64> {
        self.connection.execute(
            "INSERT OR IGNORE INTO pricing_snapshots (
                source, dataset_version, fetched_at_unix_ms, content_hash
             ) VALUES (?1, ?2, ?3, ?4)",
            params![source, dataset_version, fetched_at_unix_ms, content_hash],
        )?;
        self.connection
            .query_row(
                "SELECT id FROM pricing_snapshots
                 WHERE source = ?1 AND dataset_version = ?2 AND content_hash = ?3",
                params![source, dataset_version, content_hash],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Every canonical event projected for pricing, in deterministic order.
    pub fn events_for_pricing(&self) -> Result<Vec<PricingEventRow>> {
        let mut statement = self.connection.prepare(
            "SELECT source_object_id, event_id, provider, model,
                    input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, reasoning_tokens
             FROM usage_events
             ORDER BY source_object_id, event_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(PricingEventRow {
                source_object_id: row.get(0)?,
                event_id: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                tokens: TokenBreakdown {
                    input: row.get::<_, i64>(4)? as u64,
                    output: row.get::<_, i64>(5)? as u64,
                    cache_read: row.get::<_, i64>(6)? as u64,
                    cache_write: row.get::<_, i64>(7)? as u64,
                    reasoning: row.get::<_, i64>(8)? as u64,
                },
            })
        })?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Replaces every API-equivalent estimate and unpriced marker in one
    /// transaction. Provider-reported and subscription-credit facts are
    /// never touched: estimates are disposable projections of immutable
    /// token facts, so re-running with a new dataset fully reverses the
    /// previous run without re-ingesting source logs.
    pub fn replace_estimate_facts(
        &mut self,
        pricing_snapshot_id: i64,
        facts: &[EstimateFact],
    ) -> Result<RepriceReport> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM event_costs
             WHERE kind IN ('api_equivalent_estimate', 'unpriced')",
            [],
        )?;
        let mut report = RepriceReport::default();
        for fact in facts {
            let kind = if fact.usd.is_some() {
                CostKind::ApiEquivalentEstimate
            } else {
                CostKind::Unpriced
            };
            transaction.execute(
                "INSERT INTO event_costs (
                    source_object_id, event_id, kind, usd,
                    pricing_snapshot_id, pricing_key, pricing_rule, confidence
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'estimated')",
                params![
                    fact.source_object_id,
                    fact.event_id,
                    cost_kind_str(kind),
                    fact.usd.map(nano_usd_to_sql).transpose()?,
                    pricing_snapshot_id,
                    fact.pricing_key,
                    fact.pricing_rule,
                ],
            )?;
            if fact.usd.is_some() {
                report.priced_events += 1;
            } else {
                report.unpriced_events += 1;
            }
        }
        transaction.commit()?;
        Ok(report)
    }
}

/// One canonical event projected for pricing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PricingEventRow {
    pub source_object_id: String,
    pub event_id: String,
    pub provider: Option<String>,
    pub model: String,
    pub tokens: TokenBreakdown,
}

/// A repricing fact: a priced API-equivalent estimate when `usd` is present,
/// otherwise an explicit unpriced marker so absence never reads as a valid
/// zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EstimateFact {
    pub source_object_id: String,
    pub event_id: String,
    pub usd: Option<NanoUsd>,
    pub pricing_key: Option<String>,
    pub pricing_rule: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RepriceReport {
    pub priced_events: u64,
    pub unpriced_events: u64,
}

const PREFERENCES_KEY: &str = "app_preferences";

#[derive(Deserialize, Serialize)]
struct StoredPreferences {
    language: String,
    appearance: String,
}

fn language_preference_str(language: LanguagePreference) -> &'static str {
    match language {
        LanguagePreference::System => "system",
        LanguagePreference::English => "en",
        LanguagePreference::SimplifiedChinese => "zh-cn",
    }
}

fn parse_language_preference(value: &str) -> Result<LanguagePreference> {
    match value {
        "system" => Ok(LanguagePreference::System),
        "en" => Ok(LanguagePreference::English),
        "zh-cn" => Ok(LanguagePreference::SimplifiedChinese),
        _ => Err(StorageError::InvalidPreference {
            field: "language",
            value: value.to_owned(),
        }),
    }
}

fn appearance_preference_str(appearance: AppearancePreference) -> &'static str {
    match appearance {
        AppearancePreference::System => "system",
        AppearancePreference::Light => "light",
        AppearancePreference::Dark => "dark",
    }
}

fn parse_appearance_preference(value: &str) -> Result<AppearancePreference> {
    match value {
        "system" => Ok(AppearancePreference::System),
        "light" => Ok(AppearancePreference::Light),
        "dark" => Ok(AppearancePreference::Dark),
        _ => Err(StorageError::InvalidPreference {
            field: "appearance",
            value: value.to_owned(),
        }),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRegistration {
    pub installation_id: String,
    pub source_object_id: String,
    pub adapter_id: String,
    pub platform: String,
    pub root_path: String,
    pub discovery_method: String,
    pub native_path: String,
    pub kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInstallationRegistration {
    pub installation_id: String,
    pub adapter_id: String,
    pub platform: String,
    pub root_path: String,
    pub discovery_method: String,
    pub enabled: bool,
    pub permission: SourcePermissionState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionFailureKind {
    Collection,
    Permission,
    UnsupportedSchema,
}

struct RawSourceHealth {
    installation_id: String,
    adapter_id: String,
    root_path: String,
    enabled: bool,
    permission: String,
    source_object_id: Option<String>,
    native_path: Option<String>,
    source_kind: Option<String>,
    parser_version: Option<u32>,
    last_scan_unix_ms: Option<i64>,
    last_success_unix_ms: Option<i64>,
    last_error: Option<String>,
    last_event_unix_ms: Option<i64>,
    records_changed: i64,
    last_run_status: Option<String>,
    warnings_json: Option<String>,
    run_error: Option<String>,
}

impl RawSourceHealth {
    fn into_health(self) -> Result<SourceHealth> {
        let permission = parse_permission_state(&self.permission);
        let warnings: Vec<String> = self
            .warnings_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        let encoded_error = self.run_error.or(self.last_error);
        let (failure_kind, error) = encoded_error
            .as_deref()
            .map(decode_collection_error)
            .map_or((None, None), |(kind, message)| {
                (Some(kind), Some(message.to_owned()))
            });
        let unsupported = failure_kind == Some(CollectionFailureKind::UnsupportedSchema)
            || warnings
                .iter()
                .any(|warning| warning_is_unsupported(warning));
        let failed = self.last_run_status.as_deref() == Some("failed") || error.is_some();
        let (state, remediation) = if !self.enabled {
            (SourceHealthState::Disabled, None)
        } else if permission != SourcePermissionState::Granted {
            (
                SourceHealthState::SetupRequired,
                Some(match permission {
                    SourcePermissionState::Denied => SourceRemediation::GrantPermission,
                    SourcePermissionState::Missing => SourceRemediation::ConfigurePath,
                    SourcePermissionState::Unknown => SourceRemediation::RetryCollection,
                    SourcePermissionState::Granted => unreachable!(),
                }),
            )
        } else if self.source_object_id.is_none() {
            (
                SourceHealthState::SetupRequired,
                Some(SourceRemediation::ConfigurePath),
            )
        } else if self.last_success_unix_ms.is_none() && !failed {
            (
                SourceHealthState::SetupRequired,
                Some(SourceRemediation::RetryCollection),
            )
        } else if unsupported {
            (
                SourceHealthState::UnsupportedSchema,
                Some(SourceRemediation::UpgradeAgentMeter),
            )
        } else if failed {
            (
                SourceHealthState::Error,
                Some(SourceRemediation::RetryCollection),
            )
        } else if !warnings.is_empty() {
            (
                SourceHealthState::Partial,
                Some(SourceRemediation::ReviewWarnings),
            )
        } else {
            (SourceHealthState::Healthy, None)
        };
        Ok(SourceHealth {
            installation_id: self.installation_id,
            source_object_id: self.source_object_id,
            adapter_id: self.adapter_id,
            root_path: self.root_path,
            native_path: self.native_path,
            source_kind: self.source_kind,
            enabled: self.enabled,
            permission,
            parser_version: self.parser_version.filter(|version| *version != 0),
            last_scan_unix_ms: self.last_scan_unix_ms,
            last_success_unix_ms: self.last_success_unix_ms,
            last_event_unix_ms: self.last_event_unix_ms,
            records_changed: self.records_changed.max(0) as u64,
            warnings,
            error,
            state,
            remediation,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredCheckpoint {
    pub byte_offset: Option<u64>,
    pub source_len: u64,
    pub prefix_fingerprint: Option<String>,
    pub parser_state: Vec<u8>,
    pub source_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckpointStatus {
    NeverIngested,
    Current(StoredCheckpoint),
    Invalidated { stored_parser_version: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    Append,
    Replace,
}

impl WriteMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Replace => "replace",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestRequest {
    pub source_object_id: String,
    pub parser_version: u32,
    pub mode: WriteMode,
    pub source_fingerprint: String,
    pub source_len: u64,
    pub byte_offset: Option<u64>,
    pub prefix_fingerprint: Option<String>,
    pub parser_state: Vec<u8>,
    pub observed_at_unix_ms: i64,
    pub records: Vec<UsageRecord>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestReport {
    pub run_id: i64,
    pub source_object_id: String,
    pub mode: WriteMode,
    pub parsed_records: usize,
    pub inserted_records: usize,
    pub duplicate_records: usize,
    pub deleted_records: usize,
    pub warnings: Vec<String>,
}

impl IngestReport {
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationTarget {
    pub source_object_id: String,
    pub adapter_id: String,
    pub parser_version: u32,
    pub last_reconciled_unix_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    SourceUi,
    SourceCli,
    Waku,
    Tokens,
    Tokscale,
    Ccusage,
    Fixture,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceExpectation {
    pub adapter_id: String,
    pub reference: ReferenceKind,
    pub expected_total_tokens: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossCheckStatus {
    Match,
    Mismatch,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceReportedStatus {
    Match,
    Mismatch,
    PartialMatch,
    PartialMismatch,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ReconciliationTokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl ReconciliationTokenTotals {
    pub fn checked_total(&self) -> Result<u64> {
        [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            self.reasoning,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
        .ok_or(StorageError::ReconciliationOverflow)
    }

    fn checked_add_assign(&mut self, other: Self) -> Result<()> {
        self.input = self
            .input
            .checked_add(other.input)
            .ok_or(StorageError::ReconciliationOverflow)?;
        self.output = self
            .output
            .checked_add(other.output)
            .ok_or(StorageError::ReconciliationOverflow)?;
        self.cache_read = self
            .cache_read
            .checked_add(other.cache_read)
            .ok_or(StorageError::ReconciliationOverflow)?;
        self.cache_write = self
            .cache_write
            .checked_add(other.cache_write)
            .ok_or(StorageError::ReconciliationOverflow)?;
        self.reasoning = self
            .reasoning
            .checked_add(other.reasoning)
            .ok_or(StorageError::ReconciliationOverflow)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceReportedCrossCheck {
    pub status: SourceReportedStatus,
    pub checked_event_count: u64,
    pub missing_event_count: u64,
    pub canonical_total_tokens: Option<u64>,
    pub source_total_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceReconciliation {
    pub source_object_id: String,
    pub adapter_id: String,
    pub parser_version: u32,
    pub last_success_unix_ms: Option<i64>,
    pub event_count: u64,
    pub canonical_tokens: ReconciliationTokenTotals,
    pub derived_event_count: u64,
    pub estimated_event_count: u64,
    pub source_reported: SourceReportedCrossCheck,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReferenceCrossCheck {
    pub adapter_id: String,
    pub reference: ReferenceKind,
    pub expected_total_tokens: u64,
    pub actual_total_tokens: Option<u64>,
    pub status: CrossCheckStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReconciliationReport {
    pub format_version: u32,
    pub generated_at_unix_ms: i64,
    pub sources: Vec<SourceReconciliation>,
    pub reference_checks: Vec<ReferenceCrossCheck>,
}

impl ReconciliationReport {
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DailyUsage {
    pub day: String,
    pub client: String,
    pub provider: Option<String>,
    pub model: String,
    pub tokens: TokenBreakdown,
    pub event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityDailyRow {
    pub day: String,
    pub week_start: String,
    pub month_start: String,
    pub client: String,
    pub provider: Option<String>,
    pub model: String,
    pub tokens: TokenBreakdown,
    pub event_count: u64,
    pub api_equivalent_estimate_usd: Option<NanoUsd>,
    pub unpriced_event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSummaryRow {
    pub source_object_id: String,
    pub session_id: String,
    pub adapter_id: String,
    pub source_kind: String,
    pub parser_version: u32,
    pub client: String,
    pub project: Option<String>,
    pub started_at_unix_ms: i64,
    pub ended_at_unix_ms: i64,
    pub tokens: TokenBreakdown,
    pub event_count: u64,
    pub confidence: DataConfidence,
    pub providers: Vec<String>,
    pub models: Vec<String>,
    pub provider_reported_usd: Option<NanoUsd>,
    pub api_equivalent_estimate_usd: Option<NanoUsd>,
    pub unpriced_event_count: u64,
}

fn split_grouped_values(values: Option<String>) -> Vec<String> {
    values
        .map(|values| values.split('\u{1f}').map(str::to_owned).collect())
        .unwrap_or_default()
}

fn upsert_installation(
    transaction: &Transaction<'_>,
    registration: &SourceInstallationRegistration,
    update_state: bool,
) -> Result<()> {
    let conflict_clause = if update_state {
        "enabled = excluded.enabled,
         permission_state = excluded.permission_state"
    } else {
        "enabled = source_installations.enabled,
         permission_state = source_installations.permission_state"
    };
    transaction.execute(
        &format!(
            "INSERT INTO source_installations (
            id, adapter_id, platform, root_path, discovery_method, enabled, permission_state
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            adapter_id = excluded.adapter_id,
            platform = excluded.platform,
            root_path = excluded.root_path,
            discovery_method = excluded.discovery_method,
            {conflict_clause}"
        ),
        params![
            registration.installation_id,
            registration.adapter_id,
            registration.platform,
            registration.root_path,
            registration.discovery_method,
            registration.enabled,
            permission_state_str(registration.permission),
        ],
    )?;
    Ok(())
}

fn permission_state_str(state: SourcePermissionState) -> &'static str {
    match state {
        SourcePermissionState::Unknown => "unknown",
        SourcePermissionState::Granted => "granted",
        SourcePermissionState::Denied => "denied",
        SourcePermissionState::Missing => "missing",
    }
}

fn parse_permission_state(value: &str) -> SourcePermissionState {
    match value {
        "granted" => SourcePermissionState::Granted,
        "denied" => SourcePermissionState::Denied,
        "missing" => SourcePermissionState::Missing,
        _ => SourcePermissionState::Unknown,
    }
}

fn encode_collection_error(kind: CollectionFailureKind, message: &str) -> String {
    let prefix = match kind {
        CollectionFailureKind::Collection => "collection",
        CollectionFailureKind::Permission => "permission",
        CollectionFailureKind::UnsupportedSchema => "unsupported_schema",
    };
    format!("[{prefix}] {message}")
}

fn decode_collection_error(value: &str) -> (CollectionFailureKind, &str) {
    for (prefix, kind) in [
        (
            "[unsupported_schema] ",
            CollectionFailureKind::UnsupportedSchema,
        ),
        ("[permission] ", CollectionFailureKind::Permission),
        ("[collection] ", CollectionFailureKind::Collection),
    ] {
        if let Some(message) = value.strip_prefix(prefix) {
            return (kind, message);
        }
    }
    (CollectionFailureKind::Collection, value)
}

fn warning_is_unsupported(warning: &str) -> bool {
    let normalized = warning.to_ascii_lowercase();
    normalized.contains("unsupported schema")
        || normalized.contains("unsupported") && normalized.contains("schema")
        || normalized.contains("newer than version")
}

fn overview_generation(snapshot: &OverviewSnapshot) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [
        snapshot.tokens.input,
        snapshot.tokens.output,
        snapshot.tokens.cache_read,
        snapshot.tokens.cache_write,
        snapshot.tokens.reasoning,
        snapshot.event_count,
        snapshot.session_count,
        snapshot.active_days,
        snapshot.model_count,
        snapshot.data_quality.exact_events,
        snapshot.data_quality.derived_events,
        snapshot.data_quality.estimated_events,
        snapshot.data_quality.unpriced_events,
        snapshot.source_health.generation,
    ] {
        hash_health_bytes(&mut hash, &value.to_le_bytes());
    }
    for cost in [
        snapshot.costs.provider_reported_usd,
        snapshot.costs.api_equivalent_estimate_usd,
    ] {
        hash_health_bytes(&mut hash, &[cost.is_some() as u8]);
        hash_health_bytes(&mut hash, &cost.map_or(0, NanoUsd::as_nanos).to_le_bytes());
    }
    hash
}

fn health_generation(sources: &[SourceHealth]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for source in sources {
        hash_health_string(&mut hash, &source.installation_id);
        hash_health_option(&mut hash, source.source_object_id.as_deref());
        hash_health_string(&mut hash, &source.adapter_id);
        hash_health_string(&mut hash, &source.root_path);
        hash_health_option(&mut hash, source.native_path.as_deref());
        hash_health_option(&mut hash, source.source_kind.as_deref());
        hash_health_bytes(&mut hash, &[source.enabled as u8]);
        hash_health_bytes(&mut hash, &[source.permission as u8]);
        hash_health_bytes(
            &mut hash,
            &source.parser_version.unwrap_or_default().to_le_bytes(),
        );
        for timestamp in [
            source.last_scan_unix_ms,
            source.last_success_unix_ms,
            source.last_event_unix_ms,
        ] {
            hash_health_bytes(&mut hash, &timestamp.unwrap_or(i64::MIN).to_le_bytes());
        }
        hash_health_bytes(&mut hash, &source.records_changed.to_le_bytes());
        for warning in &source.warnings {
            hash_health_string(&mut hash, warning);
        }
        hash_health_option(&mut hash, source.error.as_deref());
        hash_health_bytes(&mut hash, &[source.state as u8]);
        hash_health_bytes(
            &mut hash,
            &[source.remediation.map_or(u8::MAX, |value| value as u8)],
        );
    }
    hash
}

fn hash_health_option(hash: &mut u64, value: Option<&str>) {
    hash_health_bytes(hash, &[value.is_some() as u8]);
    if let Some(value) = value {
        hash_health_string(hash, value);
    }
}

fn hash_health_string(hash: &mut u64, value: &str) {
    hash_health_bytes(hash, &(value.len() as u64).to_le_bytes());
    hash_health_bytes(hash, value.as_bytes());
}

fn hash_health_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn upsert_session(
    transaction: &Transaction<'_>,
    source_object_id: &str,
    event: &UsageEvent,
) -> Result<()> {
    let Some(session_id) = event.session_id.as_deref() else {
        return Ok(());
    };
    transaction.execute(
        "INSERT INTO sessions (
            source_object_id, session_id, client, started_at_unix_ms, ended_at_unix_ms
         ) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(source_object_id, session_id) DO UPDATE SET
            client = excluded.client,
            started_at_unix_ms = MIN(started_at_unix_ms, excluded.started_at_unix_ms),
            ended_at_unix_ms = MAX(ended_at_unix_ms, excluded.ended_at_unix_ms)",
        params![
            source_object_id,
            session_id,
            event.client,
            event.occurred_at_unix_ms,
        ],
    )?;
    Ok(())
}

fn insert_record(
    transaction: &Transaction<'_>,
    source_object_id: &str,
    record: &UsageRecord,
) -> Result<bool> {
    let event = &record.event;
    let inserted = transaction.execute(
        "INSERT INTO usage_events (
            source_object_id, event_id, session_id, occurred_at_unix_ms,
            client, provider, model, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens,
            source_reported_total, confidence
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
         ) ON CONFLICT(source_object_id, event_id) DO NOTHING",
        params![
            source_object_id,
            event.id,
            event.session_id,
            event.occurred_at_unix_ms,
            event.client,
            event.provider.as_deref().unwrap_or_default(),
            event.model,
            to_sql_integer(event.tokens.input, "input_tokens")?,
            to_sql_integer(event.tokens.output, "output_tokens")?,
            to_sql_integer(event.tokens.cache_read, "cache_read_tokens")?,
            to_sql_integer(event.tokens.cache_write, "cache_write_tokens")?,
            to_sql_integer(event.tokens.reasoning, "reasoning_tokens")?,
            event
                .source_reported_total
                .map(|value| to_sql_integer(value, "source_reported_total"))
                .transpose()?,
            confidence_str(event.confidence),
        ],
    )? == 1;

    if inserted {
        let provenance = &record.provenance;
        transaction.execute(
            "INSERT INTO event_provenance (
                source_object_id, event_id, native_id, record_offset,
                schema_variant, timestamp_origin, normalization_notes_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                source_object_id,
                event.id,
                provenance.native_id,
                provenance
                    .record_offset
                    .map(|value| to_sql_integer(value, "record_offset"))
                    .transpose()?,
                provenance.schema_variant,
                timestamp_origin_str(provenance.timestamp_origin),
                serde_json::to_string(&provenance.normalization_notes)?,
            ],
        )?;
        insert_costs(transaction, source_object_id, record)?;
    } else if !stored_record_matches(transaction, source_object_id, record)? {
        return Err(StorageError::EventIdentityConflict {
            event_id: event.id.clone(),
        });
    }
    Ok(inserted)
}

/// Record offsets are physical locations and may differ when a source repeats
/// an otherwise identical native event. Semantic provenance must remain equal.
fn stored_record_matches(
    transaction: &Transaction<'_>,
    source_object_id: &str,
    record: &UsageRecord,
) -> Result<bool> {
    let event = &record.event;
    let provenance = &record.provenance;
    let usage_matches = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM usage_events AS e
            JOIN event_provenance AS p
              ON p.source_object_id = e.source_object_id
             AND p.event_id = e.event_id
            WHERE e.source_object_id = ?1
              AND e.event_id = ?2
              AND e.session_id IS ?3
              AND e.occurred_at_unix_ms = ?4
              AND e.client = ?5
              AND e.provider = ?6
              AND e.model = ?7
              AND e.input_tokens = ?8
              AND e.output_tokens = ?9
              AND e.cache_read_tokens = ?10
              AND e.cache_write_tokens = ?11
              AND e.reasoning_tokens = ?12
              AND e.source_reported_total IS ?13
              AND e.confidence = ?14
              AND p.native_id IS ?15
              AND p.schema_variant = ?16
              AND p.timestamp_origin = ?17
              AND p.normalization_notes_json = ?18
         )",
        params![
            source_object_id,
            event.id,
            event.session_id,
            event.occurred_at_unix_ms,
            event.client,
            event.provider.as_deref().unwrap_or_default(),
            event.model,
            to_sql_integer(event.tokens.input, "input_tokens")?,
            to_sql_integer(event.tokens.output, "output_tokens")?,
            to_sql_integer(event.tokens.cache_read, "cache_read_tokens")?,
            to_sql_integer(event.tokens.cache_write, "cache_write_tokens")?,
            to_sql_integer(event.tokens.reasoning, "reasoning_tokens")?,
            event
                .source_reported_total
                .map(|value| to_sql_integer(value, "source_reported_total"))
                .transpose()?,
            confidence_str(event.confidence),
            provenance.native_id,
            provenance.schema_variant,
            timestamp_origin_str(provenance.timestamp_origin),
            serde_json::to_string(&provenance.normalization_notes)?,
        ],
        |row| row.get(0),
    )?;
    Ok(usage_matches && stored_costs_match(transaction, source_object_id, record)?)
}

fn insert_costs(
    transaction: &Transaction<'_>,
    source_object_id: &str,
    record: &UsageRecord,
) -> Result<()> {
    for cost in &record.costs {
        validate_cost(&record.event.id, cost)?;
        transaction.execute(
            "INSERT INTO event_costs (
                source_object_id, event_id, kind, usd, confidence
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                source_object_id,
                record.event.id,
                cost_kind_str(cost.kind),
                cost.usd.map(nano_usd_to_sql).transpose()?,
                confidence_str(cost.confidence),
            ],
        )?;
    }
    Ok(())
}

fn stored_costs_match(
    transaction: &Transaction<'_>,
    source_object_id: &str,
    record: &UsageRecord,
) -> Result<bool> {
    let stored_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM event_costs WHERE source_object_id = ?1 AND event_id = ?2",
        params![source_object_id, record.event.id],
        |row| row.get(0),
    )?;
    if stored_count != record.costs.len() as i64 {
        return Ok(false);
    }
    for cost in &record.costs {
        validate_cost(&record.event.id, cost)?;
        let matches: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM event_costs
                WHERE source_object_id = ?1 AND event_id = ?2 AND kind = ?3
                  AND usd IS ?4 AND confidence = ?5
             )",
            params![
                source_object_id,
                record.event.id,
                cost_kind_str(cost.kind),
                cost.usd.map(nano_usd_to_sql).transpose()?,
                confidence_str(cost.confidence),
            ],
            |row| row.get(0),
        )?;
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_cost(event_id: &str, cost: &CostFact) -> Result<()> {
    let valid = match cost.kind {
        CostKind::ProviderReported | CostKind::ApiEquivalentEstimate => cost.usd.is_some(),
        CostKind::SubscriptionCredit | CostKind::Unpriced => cost.usd.is_none(),
    };
    if valid {
        Ok(())
    } else {
        Err(StorageError::InvalidCostFact {
            event_id: event_id.to_owned(),
            kind: cost_kind_str(cost.kind),
        })
    }
}

fn nano_usd_to_sql(value: NanoUsd) -> Result<f64> {
    let nanos = to_sql_integer(value.as_nanos(), "cost_usd_nanos")?;
    Ok(nanos as f64 / 1_000_000_000.0)
}

fn sql_usd_to_nano(value: f64) -> NanoUsd {
    NanoUsd::from_nanos((value * 1_000_000_000.0).round() as u64)
}

fn add_overview_cost(total: &mut Option<NanoUsd>, value: NanoUsd) -> Result<()> {
    *total = Some(match *total {
        Some(total) => total
            .checked_add(value)
            .ok_or(StorageError::OverviewOverflow)?,
        None => value,
    });
    Ok(())
}

fn rebuild_daily_usage_in(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM daily_usage_utc", [])?;
    transaction.execute(
        "INSERT INTO daily_usage_utc (
            day, client, provider, model, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens, event_count
         )
         SELECT
            date(occurred_at_unix_ms / 1000, 'unixepoch'),
            client, provider, model,
            SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens),
            SUM(cache_write_tokens), SUM(reasoning_tokens), COUNT(*)
         FROM usage_events
         GROUP BY 1, client, provider, model",
        [],
    )?;
    Ok(())
}

fn to_sql_integer(value: u64, field: &'static str) -> Result<i64> {
    i64::try_from(value).map_err(|_| StorageError::IntegerOutOfRange { field })
}

fn confidence_str(confidence: DataConfidence) -> &'static str {
    match confidence {
        DataConfidence::Exact => "exact",
        DataConfidence::Derived => "derived",
        DataConfidence::Estimated => "estimated",
    }
}

fn parse_confidence(confidence: &str) -> DataConfidence {
    match confidence {
        "exact" => DataConfidence::Exact,
        "derived" => DataConfidence::Derived,
        _ => DataConfidence::Estimated,
    }
}

fn cost_kind_str(kind: CostKind) -> &'static str {
    match kind {
        CostKind::ProviderReported => "provider_reported",
        CostKind::ApiEquivalentEstimate => "api_equivalent_estimate",
        CostKind::SubscriptionCredit => "subscription_credit",
        CostKind::Unpriced => "unpriced",
    }
}

fn parse_cost_kind(kind: &str) -> CostKind {
    match kind {
        "provider_reported" => CostKind::ProviderReported,
        "api_equivalent_estimate" => CostKind::ApiEquivalentEstimate,
        "subscription_credit" => CostKind::SubscriptionCredit,
        _ => CostKind::Unpriced,
    }
}

fn timestamp_origin_str(origin: TimestampOrigin) -> &'static str {
    match origin {
        TimestampOrigin::Source => "source",
        TimestampOrigin::Derived => "derived",
        TimestampOrigin::FileModified => "file_modified",
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{
        AppPreferences, AppearancePreference, CostFact, CostKind, DataConfidence, EventProvenance,
        LanguagePreference, NanoUsd, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
    };
    use rusqlite::params;
    use tempfile::tempdir;

    use super::{
        CheckpointStatus, CrossCheckStatus, Database, EstimateFact, IngestRequest,
        ReferenceExpectation, ReferenceKind, SCHEMA_VERSION, SourceRegistration,
        SourceReportedStatus, StorageError, WriteMode,
    };

    const SOURCE_ID: &str = "source-synthetic-001";

    #[test]
    fn creates_schema_and_enables_wal_for_file_databases() {
        let directory = tempdir().unwrap();
        let database = Database::open(directory.path().join("agentmeter.db")).unwrap();

        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(database.journal_mode().unwrap(), "wal");
    }

    #[test]
    fn rejects_a_database_from_a_newer_application() {
        let mut database = Database::open_in_memory().unwrap();
        database
            .connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();

        let error = database.migrate().unwrap_err();
        assert!(matches!(error, StorageError::NewerSchema { .. }));
    }

    #[test]
    fn fresh_database_reports_system_default_preferences() {
        let database = Database::open_in_memory().unwrap();

        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(database.preferences().unwrap(), AppPreferences::default());
    }

    #[test]
    fn preferences_round_trip_across_connections() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("agentmeter.db");
        let preferences = AppPreferences {
            language: LanguagePreference::SimplifiedChinese,
            appearance: AppearancePreference::Dark,
        };

        Database::open(&path)
            .unwrap()
            .set_preferences(&preferences)
            .unwrap();
        let reloaded = Database::open(&path).unwrap().preferences().unwrap();

        assert_eq!(reloaded, preferences);
    }

    #[test]
    fn rejects_unknown_or_malformed_stored_preferences() {
        let database = Database::open_in_memory().unwrap();
        database
            .connection
            .execute(
                "INSERT INTO preferences (key, value_json)
                 VALUES ('app_preferences', '{\"language\":\"klingon\",\"appearance\":\"dark\"}')",
                [],
            )
            .unwrap();
        assert!(matches!(
            database.preferences().unwrap_err(),
            StorageError::InvalidPreference {
                field: "language",
                ..
            }
        ));

        let database = Database::open_in_memory().unwrap();
        database
            .connection
            .execute(
                "INSERT INTO preferences (key, value_json)
                 VALUES ('app_preferences', 'not json')",
                [],
            )
            .unwrap();
        assert!(matches!(
            database.preferences().unwrap_err(),
            StorageError::InvalidPreference {
                field: "app_preferences",
                ..
            }
        ));
    }

    #[test]
    fn pricing_snapshots_are_idempotent_by_content() {
        let database = Database::open_in_memory().unwrap();

        let first = database
            .record_pricing_snapshot("reviewed", "1.0", 1_704_067_200_000, "hash-a")
            .unwrap();
        let repeated = database
            .record_pricing_snapshot("reviewed", "1.0", 1_704_999_999_999, "hash-a")
            .unwrap();
        assert_eq!(first, repeated);

        let changed = database
            .record_pricing_snapshot("reviewed", "1.0", 1_704_999_999_999, "hash-b")
            .unwrap();
        assert_ne!(first, changed);
    }

    #[test]
    fn repricing_replaces_estimates_and_preserves_provider_reported_facts() {
        let mut database = registered_database();
        let mut reported = record("event-reported", 1_704_067_200_000, 10);
        reported.costs.push(provider_cost(1_000_000_000));
        let plain = record("event-plain", 1_704_067_200_000, 10);
        database
            .apply_ingest(ingest_request(WriteMode::Append, vec![reported, plain]))
            .unwrap();

        let events = database.events_for_pricing().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].model, "model-a");
        assert_eq!(events[0].tokens.input, 10);

        let snapshot = database
            .record_pricing_snapshot("reviewed", "1.0", 1_704_067_300_000, "hash-a")
            .unwrap();
        let report = database
            .replace_estimate_facts(
                snapshot,
                &[
                    EstimateFact {
                        source_object_id: SOURCE_ID.into(),
                        event_id: "event-reported".into(),
                        usd: Some(NanoUsd::from_nanos(500)),
                        pricing_key: Some("model-a".into()),
                        pricing_rule: Some("exact".into()),
                    },
                    EstimateFact {
                        source_object_id: SOURCE_ID.into(),
                        event_id: "event-plain".into(),
                        usd: None,
                        pricing_key: None,
                        pricing_rule: Some("unpriced:no-match".into()),
                    },
                ],
            )
            .unwrap();
        assert_eq!(report.priced_events, 1);
        assert_eq!(report.unpriced_events, 1);

        let reported_costs = database.event_costs(SOURCE_ID, "event-reported").unwrap();
        assert_eq!(reported_costs.len(), 2);
        assert!(
            reported_costs
                .iter()
                .any(|cost| cost.kind == CostKind::ProviderReported)
        );
        assert!(
            reported_costs
                .iter()
                .any(|cost| cost.kind == CostKind::ApiEquivalentEstimate
                    && cost.usd == Some(NanoUsd::from_nanos(500)))
        );
        assert_eq!(
            database
                .event_costs(SOURCE_ID, "event-plain")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            database
                .overview_snapshot()
                .unwrap()
                .data_quality
                .unpriced_events,
            1,
            "an unpriced marker must stay visible in data quality"
        );

        // Repricing with a new dataset fully reverses the previous run.
        let snapshot_two = database
            .record_pricing_snapshot("reviewed", "2.0", 1_704_067_400_000, "hash-b")
            .unwrap();
        let report_two = database
            .replace_estimate_facts(
                snapshot_two,
                &[
                    EstimateFact {
                        source_object_id: SOURCE_ID.into(),
                        event_id: "event-reported".into(),
                        usd: Some(NanoUsd::from_nanos(900)),
                        pricing_key: Some("model-a".into()),
                        pricing_rule: Some("exact".into()),
                    },
                    EstimateFact {
                        source_object_id: SOURCE_ID.into(),
                        event_id: "event-plain".into(),
                        usd: Some(NanoUsd::from_nanos(700)),
                        pricing_key: Some("model-a".into()),
                        pricing_rule: Some("exact".into()),
                    },
                ],
            )
            .unwrap();
        assert_eq!(report_two.priced_events, 2);
        assert_eq!(report_two.unpriced_events, 0);

        let reported_costs = database.event_costs(SOURCE_ID, "event-reported").unwrap();
        assert_eq!(
            reported_costs.len(),
            2,
            "the provider-reported fact survives"
        );
        assert!(
            reported_costs
                .iter()
                .any(|cost| cost.kind == CostKind::ApiEquivalentEstimate
                    && cost.usd == Some(NanoUsd::from_nanos(900)))
        );
        let plain_costs = database.event_costs(SOURCE_ID, "event-plain").unwrap();
        assert_eq!(plain_costs.len(), 1);
        assert_eq!(
            database
                .overview_snapshot()
                .unwrap()
                .data_quality
                .unpriced_events,
            0
        );
    }

    #[test]
    fn append_is_idempotent_and_builds_daily_projection() {
        let mut database = registered_database();
        let request = ingest_request(
            WriteMode::Append,
            vec![record("event-001", 1_704_067_200_000, 10)],
        );

        let first = database.apply_ingest(request.clone()).unwrap();
        let duplicate = database.apply_ingest(request).unwrap();

        assert_eq!(first.inserted_records, 1);
        assert_eq!(duplicate.duplicate_records, 1);
        assert_eq!(database.event_count(SOURCE_ID).unwrap(), 1);
        let daily = database.daily_usage_utc().unwrap();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].day, "2024-01-01");
        assert_eq!(daily[0].tokens.input, 10);
        assert_eq!(daily[0].event_count, 1);
        assert!(
            first
                .to_json_pretty()
                .unwrap()
                .contains("\"inserted_records\": 1")
        );
    }

    #[test]
    fn overview_snapshot_aggregates_headlines_and_changes_generation() {
        let mut database = registered_database();
        let empty = database.overview_snapshot().unwrap();
        assert_eq!(empty.event_count, 0);
        assert_eq!(empty.costs.provider_reported_usd, None);

        let mut reported = record("event-overview-1", 1_704_067_200_000, 10);
        reported.costs.push(provider_cost(1_250_000_000));

        let mut estimated = record("event-overview-2", 1_704_153_600_000, 20);
        estimated.event.session_id = Some("session-synthetic-002".into());
        estimated.event.provider = Some("provider-b".into());
        estimated.event.model = "model-b".into();
        estimated.event.confidence = DataConfidence::Derived;
        estimated.costs.push(CostFact {
            kind: CostKind::ApiEquivalentEstimate,
            usd: Some(NanoUsd::from_nanos(750_000_000)),
            confidence: DataConfidence::Exact,
        });

        let mut unpriced = record("event-overview-3", 1_704_153_600_001, 30);
        unpriced.event.session_id = None;
        unpriced.event.provider = Some("provider-b".into());
        unpriced.event.model = "model-b".into();
        unpriced.event.confidence = DataConfidence::Estimated;
        unpriced.costs.push(CostFact {
            kind: CostKind::Unpriced,
            usd: None,
            confidence: DataConfidence::Exact,
        });

        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![reported, estimated, unpriced],
            ))
            .unwrap();
        let snapshot = database.overview_snapshot().unwrap();

        assert_ne!(snapshot.generation, empty.generation);
        assert_eq!(snapshot.tokens.input, 60);
        assert_eq!(snapshot.tokens.output, 6);
        assert_eq!(snapshot.event_count, 3);
        assert_eq!(snapshot.session_count, 2);
        assert_eq!(snapshot.active_days, 2);
        assert_eq!(snapshot.model_count, 2);
        assert_eq!(
            snapshot.costs.provider_reported_usd,
            Some(NanoUsd::from_nanos(1_250_000_000))
        );
        assert_eq!(
            snapshot.costs.api_equivalent_estimate_usd,
            Some(NanoUsd::from_nanos(750_000_000))
        );
        assert_eq!(snapshot.data_quality.exact_events, 1);
        assert_eq!(snapshot.data_quality.derived_events, 1);
        assert_eq!(snapshot.data_quality.estimated_events, 1);
        assert_eq!(snapshot.data_quality.unpriced_events, 1);
        assert_eq!(snapshot.source_health.sources.len(), 1);
        assert_eq!(snapshot, database.overview_snapshot().unwrap());
    }

    #[test]
    fn duplicate_identity_with_changed_facts_fails_without_mutation() {
        let mut database = registered_database();
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![record("event-001", 1_704_067_200_000, 10)],
            ))
            .unwrap();

        let error = database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![record("event-001", 1_704_067_200_000, 999)],
            ))
            .unwrap_err();

        assert!(matches!(error, StorageError::EventIdentityConflict { .. }));
        assert_eq!(database.daily_usage_utc().unwrap()[0].tokens.input, 10);
        assert_eq!(
            database.last_ingest_status(SOURCE_ID).unwrap().as_deref(),
            Some("failed")
        );
    }

    #[test]
    fn provider_cost_is_atomic_idempotent_and_part_of_event_identity() {
        let mut database = registered_database();
        let mut original = record("event-cost", 1_704_067_200_000, 10);
        original.costs.push(provider_cost(1_234_567));
        let request = ingest_request(WriteMode::Append, vec![original.clone()]);

        database.apply_ingest(request.clone()).unwrap();
        let duplicate = database.apply_ingest(request).unwrap();
        assert_eq!(duplicate.duplicate_records, 1);
        assert_eq!(
            database.event_costs(SOURCE_ID, "event-cost").unwrap(),
            [provider_cost(1_234_567)]
        );

        original.costs = vec![provider_cost(9_999_999)];
        let error = database
            .apply_ingest(ingest_request(WriteMode::Append, vec![original]))
            .unwrap_err();
        assert!(matches!(error, StorageError::EventIdentityConflict { .. }));
        assert_eq!(
            database.event_costs(SOURCE_ID, "event-cost").unwrap(),
            [provider_cost(1_234_567)]
        );
    }

    #[test]
    fn duplicate_identity_requires_semantic_provenance_but_not_the_same_offset() {
        let mut database = registered_database();
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![record("event-001", 1_704_067_200_000, 10)],
            ))
            .unwrap();

        let mut repeated = record("event-001", 1_704_067_200_000, 10);
        repeated.provenance.record_offset = Some(999);
        let report = database
            .apply_ingest(ingest_request(WriteMode::Append, vec![repeated]))
            .unwrap();
        assert_eq!(report.duplicate_records, 1);

        let mut conflicting = record("event-001", 1_704_067_200_000, 10);
        conflicting.provenance.schema_variant = "reference-v2".into();
        let error = database
            .apply_ingest(ingest_request(WriteMode::Append, vec![conflicting]))
            .unwrap_err();
        assert!(matches!(error, StorageError::EventIdentityConflict { .. }));
    }

    #[test]
    fn replace_deletes_only_events_owned_by_the_source() {
        let mut database = registered_database();
        let mut costed = record("event-001", 1_704_067_200_000, 10);
        costed.costs.push(provider_cost(1_000_000));
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![costed, record("event-002", 1_704_153_600_000, 20)],
            ))
            .unwrap();

        let report = database
            .apply_ingest(ingest_request(
                WriteMode::Replace,
                vec![record("event-003", 1_704_240_000_000, 30)],
            ))
            .unwrap();

        assert_eq!(report.deleted_records, 2);
        assert_eq!(database.event_ids(SOURCE_ID).unwrap(), ["event-003"]);
        assert!(
            database
                .event_costs(SOURCE_ID, "event-001")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn invalid_cost_rolls_back_its_usage_event() {
        let mut database = registered_database();
        let mut invalid = record("event-invalid-cost", 1_704_067_200_000, 10);
        invalid.costs.push(CostFact {
            kind: CostKind::SubscriptionCredit,
            usd: Some(NanoUsd::from_nanos(1)),
            confidence: DataConfidence::Exact,
        });

        let error = database
            .apply_ingest(ingest_request(WriteMode::Append, vec![invalid]))
            .unwrap_err();
        assert!(matches!(error, StorageError::InvalidCostFact { .. }));
        assert_eq!(database.event_count(SOURCE_ID).unwrap(), 0);
    }

    #[test]
    fn parser_upgrade_invalidates_the_checkpoint() {
        let mut database = registered_database();
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![record("event-001", 1_704_067_200_000, 10)],
            ))
            .unwrap();

        assert!(matches!(
            database.checkpoint_status(SOURCE_ID, 1).unwrap(),
            CheckpointStatus::Current(_)
        ));
        assert_eq!(
            database.checkpoint_status(SOURCE_ID, 2).unwrap(),
            CheckpointStatus::Invalidated {
                stored_parser_version: 1
            }
        );
    }

    #[test]
    fn failed_replace_rolls_back_events_and_checkpoint() {
        let mut database = registered_database();
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![record("event-original", 1_704_067_200_000, 10)],
            ))
            .unwrap();
        let checkpoint_before = database.checkpoint_status(SOURCE_ID, 1).unwrap();

        let mut invalid = record("event-overflow", 1_704_153_600_000, 20);
        invalid.event.tokens.input = u64::MAX;
        let error = database
            .apply_ingest(ingest_request(WriteMode::Replace, vec![invalid]))
            .unwrap_err();

        assert!(matches!(error, StorageError::IntegerOutOfRange { .. }));
        assert_eq!(database.event_ids(SOURCE_ID).unwrap(), ["event-original"]);
        assert_eq!(
            database.checkpoint_status(SOURCE_ID, 1).unwrap(),
            checkpoint_before
        );
        assert_eq!(
            database.last_ingest_status(SOURCE_ID).unwrap().as_deref(),
            Some("failed")
        );
    }

    #[test]
    fn projections_rebuild_from_canonical_events() {
        let mut database = registered_database();
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![record("event-001", 1_704_067_200_000, 10)],
            ))
            .unwrap();
        let expected = database.daily_usage_utc().unwrap();

        database
            .connection
            .execute("DELETE FROM daily_usage_utc", [])
            .unwrap();
        assert!(database.daily_usage_utc().unwrap().is_empty());
        database.rebuild_daily_usage().unwrap();

        assert_eq!(database.daily_usage_utc().unwrap(), expected);
    }

    #[test]
    fn periodic_reconciliation_is_not_postponed_by_incremental_appends() {
        let mut database = registered_database();
        let due = database
            .sources_due_for_reconciliation(1_000, 1_000)
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].last_reconciled_unix_ms, None);

        let mut append = ingest_request(WriteMode::Append, vec![record("event-append", 1_000, 10)]);
        append.observed_at_unix_ms = 1_000;
        database.apply_ingest(append).unwrap();
        assert_eq!(
            database
                .sources_due_for_reconciliation(1_500, 1_000)
                .unwrap()
                .len(),
            1
        );

        let mut rebuild =
            ingest_request(WriteMode::Replace, vec![record("event-append", 1_000, 10)]);
        rebuild.observed_at_unix_ms = 2_000;
        database.apply_ingest(rebuild).unwrap();
        assert!(
            database
                .sources_due_for_reconciliation(2_999, 1_000)
                .unwrap()
                .is_empty()
        );
        let due = database
            .sources_due_for_reconciliation(3_000, 1_000)
            .unwrap();
        assert_eq!(due[0].last_reconciled_unix_ms, Some(2_000));
        database
            .connection
            .execute(
                "UPDATE source_installations SET enabled = 0 WHERE id = ?1",
                params!["installation-synthetic-001"],
            )
            .unwrap();
        assert!(
            database
                .sources_due_for_reconciliation(3_000, 1_000)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn reconciliation_report_cross_checks_without_exporting_content_fields() {
        let mut database = registered_database();
        let matching = record("event-match", 1_704_067_200_000, 10);
        let mut mismatching = record("event-mismatch", 1_704_067_201_000, 20);
        mismatching.event.source_reported_total = Some(23);
        mismatching.event.confidence = DataConfidence::Derived;
        let mut missing = record("event-missing", 1_704_067_202_000, 30);
        missing.event.source_reported_total = None;
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![matching, mismatching, missing],
            ))
            .unwrap();

        let report = database
            .reconciliation_report(
                1_704_067_300_000,
                &[
                    ReferenceExpectation {
                        adapter_id: "reference-jsonl".into(),
                        reference: ReferenceKind::Fixture,
                        expected_total_tokens: 66,
                    },
                    ReferenceExpectation {
                        adapter_id: "reference-jsonl".into(),
                        reference: ReferenceKind::Tokens,
                        expected_total_tokens: 67,
                    },
                    ReferenceExpectation {
                        adapter_id: "missing-adapter".into(),
                        reference: ReferenceKind::SourceCli,
                        expected_total_tokens: 1,
                    },
                ],
            )
            .unwrap();

        assert_eq!(report.sources.len(), 1);
        let source = &report.sources[0];
        assert_eq!(source.event_count, 3);
        assert_eq!(source.canonical_tokens.checked_total().unwrap(), 66);
        assert_eq!(source.derived_event_count, 1);
        assert_eq!(
            source.source_reported.status,
            SourceReportedStatus::PartialMismatch
        );
        assert_eq!(source.source_reported.checked_event_count, 2);
        assert_eq!(source.source_reported.missing_event_count, 1);
        assert_eq!(source.source_reported.canonical_total_tokens, Some(34));
        assert_eq!(source.source_reported.source_total_tokens, Some(35));
        assert!(report.reference_checks.iter().any(|check| {
            check.reference == ReferenceKind::Fixture && check.status == CrossCheckStatus::Match
        }));
        assert!(report.reference_checks.iter().any(|check| {
            check.reference == ReferenceKind::Tokens && check.status == CrossCheckStatus::Mismatch
        }));
        assert!(report.reference_checks.iter().any(|check| {
            check.adapter_id == "missing-adapter" && check.status == CrossCheckStatus::Unavailable
        }));

        let json = report.to_json_pretty().unwrap();
        for forbidden in [
            "root_path",
            "native_path",
            "event_id",
            "session_id",
            "model",
            "warnings",
            "normalization_notes",
        ] {
            assert!(!json.contains(forbidden), "report leaked {forbidden}");
        }
        assert!(json.contains("\"status\": \"partial_mismatch\""));
    }

    fn registered_database() -> Database {
        let mut database = Database::open_in_memory().unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-synthetic-001".into(),
                source_object_id: SOURCE_ID.into(),
                adapter_id: "reference-jsonl".into(),
                platform: "test".into(),
                root_path: "/fixture/home/reference".into(),
                discovery_method: "fixture".into(),
                native_path: "/fixture/home/reference/events.jsonl".into(),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        database
    }

    fn ingest_request(mode: WriteMode, records: Vec<UsageRecord>) -> IngestRequest {
        IngestRequest {
            source_object_id: SOURCE_ID.into(),
            parser_version: 1,
            mode,
            source_fingerprint: "fingerprint-synthetic".into(),
            source_len: 128,
            byte_offset: Some(128),
            prefix_fingerprint: Some("prefix-synthetic".into()),
            parser_state: Vec::new(),
            observed_at_unix_ms: 1_704_067_200_000,
            records,
            warnings: Vec::new(),
        }
    }

    fn record(id: &str, occurred_at_unix_ms: i64, input: u64) -> UsageRecord {
        UsageRecord {
            event: UsageEvent {
                id: id.into(),
                source_id: SOURCE_ID.into(),
                session_id: Some("session-synthetic-001".into()),
                client: "synthetic".into(),
                provider: None,
                model: "model-a".into(),
                occurred_at_unix_ms,
                tokens: TokenBreakdown {
                    input,
                    output: 2,
                    ..TokenBreakdown::default()
                },
                source_reported_total: Some(input + 2),
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

    fn provider_cost(nanos: u64) -> CostFact {
        CostFact {
            kind: CostKind::ProviderReported,
            usd: Some(NanoUsd::from_nanos(nanos)),
            confidence: DataConfidence::Exact,
        }
    }
}
