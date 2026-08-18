//! Durable local storage for AgentMeter events.

use std::path::Path;

use agentmeter_core::{DataConfidence, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
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
    #[error("{field} value does not fit SQLite INTEGER")]
    IntegerOutOfRange { field: &'static str },
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
        transaction.execute(
            "INSERT INTO source_installations (
                id, adapter_id, platform, root_path, discovery_method, enabled, permission_state
             ) VALUES (?1, ?2, ?3, ?4, ?5, 1, 'granted')
             ON CONFLICT(id) DO UPDATE SET
                adapter_id = excluded.adapter_id,
                platform = excluded.platform,
                root_path = excluded.root_path,
                discovery_method = excluded.discovery_method",
            params![
                registration.installation_id,
                registration.adapter_id,
                registration.platform,
                registration.root_path,
                registration.discovery_method,
            ],
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

    #[cfg(test)]
    fn journal_mode(&self) -> Result<String> {
        Ok(self
            .connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?)
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
pub struct DailyUsage {
    pub day: String,
    pub client: String,
    pub provider: Option<String>,
    pub model: String,
    pub tokens: TokenBreakdown,
    pub event_count: u64,
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
    Ok(transaction.query_row(
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
    )?)
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
        DataConfidence, EventProvenance, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
    };
    use tempfile::tempdir;

    use super::{
        CheckpointStatus, Database, IngestRequest, SCHEMA_VERSION, SourceRegistration,
        StorageError, WriteMode,
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
        database
            .apply_ingest(ingest_request(
                WriteMode::Append,
                vec![
                    record("event-001", 1_704_067_200_000, 10),
                    record("event-002", 1_704_153_600_000, 20),
                ],
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
            provenance: EventProvenance {
                native_id: Some(id.into()),
                record_offset: Some(0),
                schema_variant: "reference-v1".into(),
                timestamp_origin: TimestampOrigin::Source,
                normalization_notes: Vec::new(),
            },
        }
    }
}
