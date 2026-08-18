//! Experimental reader for Amp's undocumented local thread snapshots.
//!
//! This shape is corroborated by independent third-party parsers, not
//! guaranteed by Amp. Each thread file is therefore treated as a mutable,
//! source-owned snapshot and schema failures are surfaced as errors.

use std::{
    env,
    path::{Path, PathBuf},
};

use agentmeter_core::{
    DataConfidence, EventProvenance, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::modified_unix_ms;
use crate::{
    CollectorAdapter, CollectorError, IngestBatch, IngestMode, IngestStart, SourceCandidate,
    SourceCheckpoint, SourceKind,
    file_support::{ensure_kind, hash_bytes, io_error},
};

const PARSER_VERSION: u32 = 1;
const SCHEMA_VARIANT: &str = "amp-local-thread-observed-v1";

#[derive(Clone, Debug)]
pub struct AmpLocalHistoryAdapter {
    threads_root: PathBuf,
}

impl AmpLocalHistoryAdapter {
    pub fn new(threads_root: impl Into<PathBuf>) -> Self {
        Self {
            threads_root: threads_root.into(),
        }
    }

    /// Resolves Amp's observed data root without inspecting thread files.
    pub fn default_threads_root() -> Option<PathBuf> {
        let data_root = env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".local").join("share"))
            })
            .or_else(|| {
                env::var_os("USERPROFILE")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".local").join("share"))
            })?;
        Some(Self::threads_root_from_data_root(data_root))
    }

    pub fn threads_root_from_data_root(data_root: impl AsRef<Path>) -> PathBuf {
        data_root.as_ref().join("amp").join("threads")
    }
}

impl CollectorAdapter for AmpLocalHistoryAdapter {
    fn id(&self) -> &'static str {
        "amp-local-history-experimental"
    }

    fn parser_version(&self) -> u32 {
        PARSER_VERSION
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        if !self.threads_root.exists() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(&self.threads_root).map_err(io_error)?;
        let mut sources = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            if !entry.file_type().map_err(io_error)?.is_file()
                || !is_amp_thread_name(&entry.file_name())
            {
                continue;
            }
            sources.push(SourceCandidate {
                source_key: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                kind: SourceKind::MutableJson,
            });
        }
        sources.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(sources)
    }

    fn ingest(
        &self,
        source: &SourceCandidate,
        _start: IngestStart<'_>,
    ) -> Result<IngestBatch, CollectorError> {
        ensure_kind(source, SourceKind::MutableJson)?;
        let file_modified_unix_ms = modified_unix_ms(&source.path.metadata().map_err(io_error)?)?;
        let bytes = std::fs::read(&source.path).map_err(io_error)?;
        let thread: AmpThread = serde_json::from_slice(&bytes).map_err(|error| {
            CollectorError::new(format!("unsupported Amp thread schema: {error}"))
        })?;
        let fallback_id = source
            .path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown-amp-thread");
        let thread_id = thread
            .id
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| fallback_id.to_owned());
        let thread_created_unix_ms = thread.created.filter(|value| *value > 0);

        let mut warnings = Vec::new();
        let mut credits_observed = 0_usize;
        let mut ledger_records = parse_ledger_records(
            thread.usage_ledger,
            &thread_id,
            thread_created_unix_ms,
            file_modified_unix_ms,
            &mut credits_observed,
            &mut warnings,
        );
        let message_records = parse_message_records(
            thread.messages,
            &thread_id,
            thread_created_unix_ms,
            file_modified_unix_ms,
            &mut credits_observed,
            &mut warnings,
        );
        reconcile_messages(&mut ledger_records, message_records, &mut warnings);

        if credits_observed > 0 {
            warnings.push(format!(
                "observed {credits_observed} Amp credit value(s); credit semantics are undocumented and are not yet persisted as USD cost"
            ));
        }

        ledger_records.sort_by(|left, right| {
            left.occurred_at_unix_ms
                .cmp(&right.occurred_at_unix_ms)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(IngestBatch {
            mode: IngestMode::Replace,
            records: ledger_records
                .into_iter()
                .map(LocalRecord::finish)
                .collect(),
            checkpoint: SourceCheckpoint {
                byte_offset: None,
                source_len: bytes.len() as u64,
                prefix_fingerprint: None,
                parser_state: Vec::new(),
            },
            source_fingerprint: hash_bytes(&bytes),
            warnings,
        })
    }
}

fn is_amp_thread_name(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with("T-") && name.ends_with(".json"))
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AmpThread {
    id: Option<String>,
    created: Option<i64>,
    messages: Vec<AmpMessage>,
    #[serde(rename = "usageLedger")]
    usage_ledger: Option<AmpUsageLedger>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AmpUsageLedger {
    events: Vec<AmpUsageEvent>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AmpUsageEvent {
    timestamp: Option<String>,
    model: Option<String>,
    credits: Option<f64>,
    tokens: Option<AmpLedgerTokens>,
    #[serde(rename = "toMessageId")]
    to_message_id: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AmpLedgerTokens {
    input: i64,
    output: i64,
    #[serde(rename = "cacheReadInputTokens")]
    cache_read_input_tokens: i64,
    #[serde(rename = "cacheCreationInputTokens")]
    cache_creation_input_tokens: i64,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AmpMessage {
    role: Option<String>,
    #[serde(rename = "messageId")]
    message_id: Option<i64>,
    usage: Option<AmpMessageUsage>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AmpMessageUsage {
    model: Option<String>,
    #[serde(rename = "inputTokens")]
    input_tokens: i64,
    #[serde(rename = "outputTokens")]
    output_tokens: i64,
    #[serde(rename = "cacheReadInputTokens")]
    cache_read_input_tokens: i64,
    #[serde(rename = "cacheCreationInputTokens")]
    cache_creation_input_tokens: i64,
    credits: Option<f64>,
}

#[derive(Clone)]
struct LocalRecord {
    event_id: String,
    native_id: Option<String>,
    session_id: String,
    model: String,
    occurred_at_unix_ms: i64,
    timestamp_origin: TimestampOrigin,
    message_id: Option<i64>,
    tokens: TokenBreakdown,
    confidence: DataConfidence,
    normalization_notes: Vec<String>,
}

impl LocalRecord {
    fn finish(self) -> UsageRecord {
        UsageRecord {
            event: UsageEvent {
                id: self.event_id,
                source_id: String::new(),
                session_id: Some(self.session_id),
                client: "amp".to_owned(),
                provider: None,
                model: self.model,
                occurred_at_unix_ms: self.occurred_at_unix_ms,
                tokens: self.tokens,
                source_reported_total: None,
                confidence: self.confidence,
            },
            costs: Vec::new(),
            provenance: EventProvenance {
                native_id: self.native_id,
                record_offset: None,
                schema_variant: SCHEMA_VARIANT.to_owned(),
                timestamp_origin: self.timestamp_origin,
                normalization_notes: self.normalization_notes,
            },
        }
    }
}

fn parse_ledger_records(
    ledger: Option<AmpUsageLedger>,
    thread_id: &str,
    thread_created_unix_ms: Option<i64>,
    file_modified_unix_ms: i64,
    credits_observed: &mut usize,
    warnings: &mut Vec<String>,
) -> Vec<LocalRecord> {
    let Some(ledger) = ledger else {
        return Vec::new();
    };
    ledger
        .events
        .into_iter()
        .enumerate()
        .filter_map(|(index, event)| {
            *credits_observed += usize::from(event.credits.is_some());
            let (tokens, derived) =
                ledger_tokens(event.tokens.unwrap_or_default(), index, warnings);
            if !has_usable_tokens(&tokens, &format!("Amp ledger event {index}"), warnings) {
                return None;
            }
            let explicit_timestamp = event.timestamp.as_deref().and_then(|value| {
                let parsed = parse_rfc3339_millis(value);
                if parsed.is_none() {
                    warnings.push(format!(
                        "Amp ledger event {index} has an invalid timestamp; used fallback time"
                    ));
                }
                parsed
            });
            let (occurred_at_unix_ms, timestamp_origin, timestamp_note) = explicit_timestamp
                .map(|value| (value, TimestampOrigin::Source, None))
                .unwrap_or_else(|| {
                    timestamp_fallback(thread_created_unix_ms, file_modified_unix_ms)
                });
            let model = event
                .model
                .filter(|model| !model.is_empty())
                .unwrap_or_else(|| {
                    warnings.push(format!(
                        "Amp ledger event {index} has no model; using unknown"
                    ));
                    "unknown".to_owned()
                });
            let positive_message_id = event.to_message_id.filter(|id| *id > 0);
            Some(LocalRecord {
                event_id: local_event_id(thread_id, "ledger", index, positive_message_id),
                native_id: positive_message_id.map(|id| format!("toMessageId:{id}")),
                session_id: thread_id.to_owned(),
                model,
                occurred_at_unix_ms,
                timestamp_origin,
                message_id: positive_message_id,
                tokens,
                confidence: if derived {
                    DataConfidence::Derived
                } else {
                    DataConfidence::Exact
                },
                normalization_notes: timestamp_note.into_iter().collect(),
            })
        })
        .collect()
}

fn parse_message_records(
    messages: Vec<AmpMessage>,
    thread_id: &str,
    thread_created_unix_ms: Option<i64>,
    file_modified_unix_ms: i64,
    credits_observed: &mut usize,
    warnings: &mut Vec<String>,
) -> Vec<LocalRecord> {
    messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| {
            if message.role.as_deref() != Some("assistant") {
                return None;
            }
            let usage = message.usage?;
            *credits_observed += usize::from(usage.credits.is_some());
            let (tokens, derived) = message_tokens(&usage, index, warnings);
            if !has_usable_tokens(&tokens, &format!("Amp assistant usage {index}"), warnings) {
                return None;
            }
            let model = usage
                .model
                .filter(|model| !model.is_empty())
                .unwrap_or_else(|| {
                    warnings.push(format!(
                        "Amp assistant usage {index} has no model; using unknown"
                    ));
                    "unknown".to_owned()
                });
            let (occurred_at_unix_ms, timestamp_origin, timestamp_note) =
                timestamp_fallback(thread_created_unix_ms, file_modified_unix_ms);
            let positive_message_id = message.message_id.filter(|id| *id > 0);
            Some(LocalRecord {
                event_id: local_event_id(thread_id, "message", index, positive_message_id),
                native_id: positive_message_id.map(|id| format!("messageId:{id}")),
                session_id: thread_id.to_owned(),
                model,
                occurred_at_unix_ms,
                timestamp_origin,
                message_id: positive_message_id,
                tokens,
                confidence: if derived {
                    DataConfidence::Derived
                } else {
                    DataConfidence::Exact
                },
                normalization_notes: timestamp_note.into_iter().collect(),
            })
        })
        .collect()
}

fn reconcile_messages(
    ledger_records: &mut Vec<LocalRecord>,
    message_records: Vec<LocalRecord>,
    warnings: &mut Vec<String>,
) {
    if ledger_records.is_empty() {
        *ledger_records = message_records;
        return;
    }
    let mut consumed = vec![false; ledger_records.len()];
    for message in message_records {
        let id_match = message.message_id.and_then(|message_id| {
            ledger_records
                .iter()
                .enumerate()
                .position(|(index, ledger)| {
                    !consumed[index] && ledger.message_id == Some(message_id)
                })
        });
        let match_index = id_match.or_else(|| {
            ledger_records
                .iter()
                .enumerate()
                .position(|(index, ledger)| {
                    !consumed[index]
                        && ledger.model == message.model
                        && ledger.tokens == message.tokens
                })
        });

        if let Some(index) = match_index {
            consumed[index] = true;
            if ledger_records[index].tokens != message.tokens {
                warnings.push(format!(
                    "Amp usageLedger and assistant usage disagree for message {}; kept usageLedger facts",
                    message.message_id.unwrap_or_default()
                ));
            }
            if ledger_records[index].model == "unknown" && message.model != "unknown" {
                ledger_records[index].model = message.model;
            }
        } else {
            ledger_records.push(message);
            consumed.push(true);
        }
    }
}

fn ledger_tokens(
    tokens: AmpLedgerTokens,
    index: usize,
    warnings: &mut Vec<String>,
) -> (TokenBreakdown, bool) {
    signed_tokens(
        tokens.input,
        tokens.output,
        tokens.cache_read_input_tokens,
        tokens.cache_creation_input_tokens,
        &format!("Amp ledger event {index}"),
        warnings,
    )
}

fn message_tokens(
    usage: &AmpMessageUsage,
    index: usize,
    warnings: &mut Vec<String>,
) -> (TokenBreakdown, bool) {
    signed_tokens(
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
        &format!("Amp assistant usage {index}"),
        warnings,
    )
}

fn signed_tokens(
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    label: &str,
    warnings: &mut Vec<String>,
) -> (TokenBreakdown, bool) {
    let derived = [input, output, cache_read, cache_write]
        .into_iter()
        .any(|value| value < 0);
    if derived {
        warnings.push(format!(
            "{label} contains negative token values; clamped them to zero"
        ));
    }
    (
        TokenBreakdown {
            input: input.max(0) as u64,
            output: output.max(0) as u64,
            cache_read: cache_read.max(0) as u64,
            cache_write: cache_write.max(0) as u64,
            reasoning: 0,
        },
        derived,
    )
}

fn has_usable_tokens(tokens: &TokenBreakdown, label: &str, warnings: &mut Vec<String>) -> bool {
    match tokens.checked_total() {
        Some(0) => {
            warnings.push(format!("{label} reports no tokens; skipped"));
            false
        }
        Some(_) => true,
        None => {
            warnings.push(format!("{label} token total overflows u64; skipped"));
            false
        }
    }
}

fn parse_rfc3339_millis(value: &str) -> Option<i64> {
    let nanos = OffsetDateTime::parse(value, &Rfc3339)
        .ok()?
        .unix_timestamp_nanos();
    i64::try_from(nanos / 1_000_000)
        .ok()
        .filter(|value| *value != 0)
}

fn timestamp_fallback(
    thread_created_unix_ms: Option<i64>,
    file_modified_unix_ms: i64,
) -> (i64, TimestampOrigin, Option<String>) {
    if let Some(created) = thread_created_unix_ms {
        (
            created,
            TimestampOrigin::Derived,
            Some("usage timestamp absent; used thread creation time".to_owned()),
        )
    } else {
        (
            file_modified_unix_ms,
            TimestampOrigin::FileModified,
            Some(
                "usage and thread timestamps absent; used source file modification time".to_owned(),
            ),
        )
    }
}

fn local_event_id(thread_id: &str, kind: &str, index: usize, message_id: Option<i64>) -> String {
    let material = format!(
        "amp-local-v1\0{thread_id}\0{kind}\0{index}\0{}",
        message_id.unwrap_or_default()
    );
    format!("amp-local-v1:{}", hash_bytes(material.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::AmpLocalHistoryAdapter;
    use crate::{CollectorAdapter, IngestMode, IngestStart};

    #[test]
    fn discovers_only_amp_thread_snapshots() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("T-synthetic-002.json"), "{}").unwrap();
        fs::write(directory.path().join("T-synthetic-001.json"), "{}").unwrap();
        fs::write(directory.path().join("other.json"), "{}").unwrap();
        fs::create_dir(directory.path().join("T-directory.json")).unwrap();

        let sources = AmpLocalHistoryAdapter::new(directory.path())
            .discover()
            .unwrap();
        assert_eq!(sources.len(), 2);
        assert!(sources[0].path.ends_with("T-synthetic-001.json"));
        assert!(sources[1].path.ends_with("T-synthetic-002.json"));
    }

    #[test]
    fn reconciles_ledger_and_messages_without_double_counting() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("T-synthetic-full.json");
        fs::write(
            &path,
            r#"{
                "id":"T-synthetic-full","created":1704067200000,
                "usageLedger":{"events":[
                    {"timestamp":"2024-01-02T00:00:00Z","model":"model-a","tokens":{"input":20,"output":5},"toMessageId":2},
                    {"timestamp":"2024-01-03T00:00:00Z","model":"model-a","tokens":{"input":10,"output":2},"toMessageId":1}]},
                "messages":[
                    {"role":"assistant","messageId":1,"usage":{"model":"model-a","inputTokens":10,"outputTokens":2}},
                    {"role":"assistant","messageId":2,"usage":{"model":"model-a","inputTokens":20,"outputTokens":5}}]
            }"#,
        )
        .unwrap();

        let adapter = AmpLocalHistoryAdapter::new(directory.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.mode, IngestMode::Replace);
        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].event.tokens.input, 20);
        assert_eq!(
            batch.records[0].event.occurred_at_unix_ms,
            1_704_153_600_000
        );
        assert_eq!(batch.records[1].event.tokens.input, 10);
        assert_eq!(
            batch.records[1].event.occurred_at_unix_ms,
            1_704_240_000_000
        );
    }

    #[test]
    fn preserves_unmatched_messages_and_reports_credit_semantics() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("T-synthetic-partial.json");
        fs::write(
            &path,
            r#"{
                "id":"T-synthetic-partial","created":1704067200000,
                "usageLedger":{"events":[{"model":"model-a","credits":0.25,"tokens":{"input":20,"output":5},"toMessageId":1}]},
                "messages":[
                    {"role":"assistant","messageId":1,"usage":{"model":"model-a","inputTokens":20,"outputTokens":5,"credits":0.25}},
                    {"role":"assistant","messageId":2,"usage":{"model":"model-b","inputTokens":30,"cacheReadInputTokens":7}}]
            }"#,
        )
        .unwrap();

        let adapter = AmpLocalHistoryAdapter::new(directory.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 2);
        let unmatched = batch
            .records
            .iter()
            .find(|record| record.event.model == "model-b")
            .unwrap();
        assert_eq!(unmatched.event.tokens.cache_read, 7);
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("credit"))
        );
    }

    #[test]
    fn message_id_match_keeps_ledger_facts_and_reports_disagreement() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("T-synthetic-disagreement.json");
        fs::write(
            &path,
            r#"{
                "id":"T-synthetic-disagreement","created":1704067200000,
                "usageLedger":{"events":[{"model":"model-a","tokens":{"input":20,"output":5},"toMessageId":7}]},
                "messages":[{"role":"assistant","messageId":7,"usage":{"model":"model-a","inputTokens":999,"outputTokens":1}}]
            }"#,
        )
        .unwrap();

        let adapter = AmpLocalHistoryAdapter::new(directory.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.tokens.input, 20);
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("disagree"))
        );
    }

    #[test]
    fn surfaces_schema_and_invalid_usage_diagnostics() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("T-synthetic-invalid.json");
        fs::write(&path, "{").unwrap();
        let adapter = AmpLocalHistoryAdapter::new(directory.path());
        let source = adapter.discover().unwrap().remove(0);
        assert!(adapter.ingest(&source, IngestStart::Fresh).is_err());

        fs::write(
            &path,
            r#"{"id":"T-synthetic-invalid","unknownFutureField":true,"usageLedger":{"events":[
                {"timestamp":"not-a-timestamp","model":"model-a","tokens":{"input":-10,"output":5}},
                {"model":"model-b","tokens":{}}]}}"#,
        )
        .unwrap();
        let batch = adapter.ingest(&source, IngestStart::Rebuild).unwrap();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.tokens.input, 0);
        assert_eq!(batch.records[0].event.tokens.output, 5);
        assert_eq!(
            batch.records[0].event.confidence,
            agentmeter_core::DataConfidence::Derived
        );
        assert_eq!(batch.warnings.len(), 3);
    }

    #[test]
    fn builds_threads_root_portably_from_a_data_root() {
        assert_eq!(
            AmpLocalHistoryAdapter::threads_root_from_data_root("fixture-data"),
            std::path::Path::new("fixture-data")
                .join("amp")
                .join("threads")
        );
    }
}
