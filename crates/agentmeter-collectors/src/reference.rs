//! Synthetic reference formats used to prove collector lifecycle behavior.
//!
//! These adapters are not vendor integrations. Their deliberately small
//! schemas exercise append-only and mutable-snapshot ingestion before private
//! agent formats are added.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
};

use agentmeter_core::{
    DataConfidence, EventProvenance, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use serde::Deserialize;

use crate::{
    CollectorAdapter, CollectorError, IngestBatch, IngestMode, IngestStart, SourceCandidate,
    SourceCheckpoint, SourceKind,
    file_support::{
        checkpoint_continues, ensure_kind, hash_bytes, hash_file, hash_prefix, io_error,
    },
};

const SCHEMA_VARIANT: &str = "reference-v1";

#[derive(Clone, Debug)]
pub struct ReferenceJsonlAdapter {
    path: PathBuf,
}

impl ReferenceJsonlAdapter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl CollectorAdapter for ReferenceJsonlAdapter {
    fn id(&self) -> &'static str {
        "reference-jsonl"
    }

    fn parser_version(&self) -> u32 {
        1
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        Ok(self
            .path
            .is_file()
            .then(|| SourceCandidate {
                source_key: self.path.to_string_lossy().into_owned(),
                path: self.path.clone(),
                kind: SourceKind::AppendOnlyJsonl,
            })
            .into_iter()
            .collect())
    }

    fn ingest(
        &self,
        source: &SourceCandidate,
        start: IngestStart<'_>,
    ) -> Result<IngestBatch, CollectorError> {
        ensure_kind(source, SourceKind::AppendOnlyJsonl)?;
        let observed_source_len = source.path.metadata().map_err(io_error)?.len();

        let (mode, start_offset) = match start {
            IngestStart::Resume(checkpoint)
                if checkpoint_continues(&source.path, checkpoint, observed_source_len)? =>
            {
                (
                    IngestMode::Append,
                    checkpoint.byte_offset.unwrap_or_default(),
                )
            }
            IngestStart::Resume(_) | IngestStart::Rebuild => (IngestMode::Replace, 0),
            IngestStart::Fresh => (IngestMode::Append, 0),
        };

        let mut reader = BufReader::new(File::open(&source.path).map_err(io_error)?);
        reader
            .seek(SeekFrom::Start(start_offset))
            .map_err(io_error)?;

        let mut records = Vec::new();
        let mut warnings = Vec::new();
        let mut committed_offset = start_offset;

        loop {
            let record_offset = reader.stream_position().map_err(io_error)?;
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).map_err(io_error)?;
            if bytes_read == 0 {
                break;
            }
            if !line.ends_with('\n') {
                warnings.push(format!(
                    "incomplete trailing record at byte {record_offset}; retrying after append"
                ));
                break;
            }

            committed_offset = reader.stream_position().map_err(io_error)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<ReferenceEvent>(trimmed) {
                Ok(event) => records.push(event.into_record(Some(record_offset))),
                Err(error) => {
                    warnings.push(format!("malformed record at byte {record_offset}: {error}"))
                }
            }
        }

        let source_len = source.path.metadata().map_err(io_error)?.len();
        Ok(IngestBatch {
            mode,
            records,
            checkpoint: SourceCheckpoint {
                byte_offset: Some(committed_offset),
                source_len,
                prefix_fingerprint: Some(hash_prefix(&source.path, committed_offset)?),
                parser_state: Vec::new(),
            },
            source_fingerprint: hash_file(&source.path)?,
            warnings,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ReferenceSnapshotAdapter {
    path: PathBuf,
}

impl ReferenceSnapshotAdapter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl CollectorAdapter for ReferenceSnapshotAdapter {
    fn id(&self) -> &'static str {
        "reference-snapshot"
    }

    fn parser_version(&self) -> u32 {
        1
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        Ok(self
            .path
            .is_file()
            .then(|| SourceCandidate {
                source_key: self.path.to_string_lossy().into_owned(),
                path: self.path.clone(),
                kind: SourceKind::MutableJson,
            })
            .into_iter()
            .collect())
    }

    fn ingest(
        &self,
        source: &SourceCandidate,
        _start: IngestStart<'_>,
    ) -> Result<IngestBatch, CollectorError> {
        ensure_kind(source, SourceKind::MutableJson)?;
        let bytes = std::fs::read(&source.path).map_err(io_error)?;
        let snapshot: ReferenceSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| CollectorError::new(error.to_string()))?;

        Ok(IngestBatch {
            mode: IngestMode::Replace,
            records: snapshot
                .events
                .into_iter()
                .map(|event| event.into_record(None))
                .collect(),
            checkpoint: SourceCheckpoint {
                byte_offset: None,
                source_len: bytes.len() as u64,
                prefix_fingerprint: None,
                parser_state: Vec::new(),
            },
            source_fingerprint: hash_bytes(&bytes),
            warnings: Vec::new(),
        })
    }
}

#[derive(Deserialize)]
struct ReferenceSnapshot {
    events: Vec<ReferenceEvent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceEvent {
    id: String,
    #[serde(default)]
    session_id: Option<String>,
    client: String,
    #[serde(default)]
    provider: Option<String>,
    model: String,
    occurred_at_unix_ms: i64,
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    cache_read: u64,
    #[serde(default)]
    cache_write: u64,
    #[serde(default)]
    reasoning: u64,
    #[serde(default)]
    source_reported_total: Option<u64>,
}

impl ReferenceEvent {
    fn into_record(self, record_offset: Option<u64>) -> UsageRecord {
        let native_id = self.id.clone();
        UsageRecord {
            event: UsageEvent {
                id: self.id,
                source_id: String::new(),
                session_id: self.session_id,
                client: self.client,
                provider: self.provider,
                model: self.model,
                occurred_at_unix_ms: self.occurred_at_unix_ms,
                tokens: TokenBreakdown {
                    input: self.input,
                    output: self.output,
                    cache_read: self.cache_read,
                    cache_write: self.cache_write,
                    reasoning: self.reasoning,
                },
                source_reported_total: self.source_reported_total,
                confidence: DataConfidence::Exact,
            },
            costs: Vec::new(),
            provenance: EventProvenance {
                native_id: Some(native_id),
                record_offset,
                schema_variant: SCHEMA_VARIANT.to_owned(),
                timestamp_origin: TimestampOrigin::Source,
                normalization_notes: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use tempfile::NamedTempFile;

    use super::{ReferenceJsonlAdapter, ReferenceSnapshotAdapter};
    use crate::{CollectorAdapter, IngestMode, IngestStart};

    const EVENT_ONE: &str = r#"{"id":"event-001","client":"synthetic","model":"model-a","occurred_at_unix_ms":1704067200000,"input":10,"output":2}"#;
    const EVENT_TWO: &str = r#"{"id":"event-002","client":"synthetic","model":"model-a","occurred_at_unix_ms":1704153600000,"input":20,"cache_read":5}"#;

    #[test]
    fn jsonl_resumes_from_a_verified_prefix() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{EVENT_ONE}").unwrap();
        file.flush().unwrap();

        let adapter = ReferenceJsonlAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(first.records.len(), 1);

        writeln!(file, "{EVENT_TWO}").unwrap();
        file.flush().unwrap();
        let second = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();

        assert_eq!(second.mode, IngestMode::Append);
        assert_eq!(second.records.len(), 1);
        assert_eq!(second.records[0].event.id, "event-002");
    }

    #[test]
    fn jsonl_replaces_after_truncation_or_rewrite() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{EVENT_ONE}").unwrap();
        writeln!(file, "{EVENT_TWO}").unwrap();
        file.flush().unwrap();

        let adapter = ReferenceJsonlAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        fs::write(file.path(), format!("{EVENT_TWO}\n")).unwrap();
        let rewritten = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();

        assert_eq!(rewritten.mode, IngestMode::Replace);
        assert_eq!(rewritten.records.len(), 1);
        assert_eq!(rewritten.records[0].event.id, "event-002");
    }

    #[test]
    fn jsonl_retries_an_incomplete_trailing_record() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{EVENT_ONE}").unwrap();
        file.flush().unwrap();

        let adapter = ReferenceJsonlAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(first.records.is_empty());
        assert_eq!(first.checkpoint.byte_offset, Some(0));
        assert_eq!(first.warnings.len(), 1);

        writeln!(file).unwrap();
        file.flush().unwrap();
        let complete = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(complete.records.len(), 1);
    }

    #[test]
    fn snapshot_always_replaces_source_owned_events() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), format!(r#"{{"events":[{EVENT_ONE}]}}"#)).unwrap();

        let adapter = ReferenceSnapshotAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(first.mode, IngestMode::Replace);
        assert_eq!(first.records.len(), 1);

        fs::write(file.path(), format!(r#"{{"events":[{EVENT_TWO}]}}"#)).unwrap();
        let rewritten = adapter.ingest(&source, IngestStart::Rebuild).unwrap();
        assert_eq!(rewritten.mode, IngestMode::Replace);
        assert_eq!(rewritten.records[0].event.id, "event-002");
    }

    #[test]
    fn snapshot_accepts_an_empty_event_set() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), r#"{"events":[]}"#).unwrap();

        let adapter = ReferenceSnapshotAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.mode, IngestMode::Replace);
        assert!(batch.records.is_empty());
    }

    #[test]
    fn snapshot_rejects_malformed_or_unknown_event_fields() {
        let file = NamedTempFile::new().unwrap();
        let adapter = ReferenceSnapshotAdapter::new(file.path());
        let source = crate::SourceCandidate {
            source_key: file.path().to_string_lossy().into_owned(),
            path: file.path().to_owned(),
            kind: crate::SourceKind::MutableJson,
        };

        fs::write(file.path(), r#"{"events":[}"#).unwrap();
        assert!(adapter.ingest(&source, IngestStart::Fresh).is_err());

        let with_unknown_field =
            EVENT_ONE.strip_suffix('}').unwrap().to_owned() + ",\"unknown\":true}";
        fs::write(
            file.path(),
            format!(r#"{{"events":[{with_unknown_field}]}}"#),
        )
        .unwrap();
        assert!(adapter.ingest(&source, IngestStart::Fresh).is_err());
    }

    #[test]
    fn jsonl_empty_input_is_a_valid_empty_append() {
        let file = NamedTempFile::new().unwrap();
        let adapter = ReferenceJsonlAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);

        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.mode, IngestMode::Append);
        assert!(batch.records.is_empty());
        assert!(batch.warnings.is_empty());
        assert_eq!(batch.checkpoint.byte_offset, Some(0));
    }

    #[test]
    fn jsonl_reports_complete_malformed_and_unknown_field_records() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{{\"id\":}}").unwrap();
        let with_unknown_field =
            EVENT_ONE.strip_suffix('}').unwrap().to_owned() + ",\"unknown\":true}";
        writeln!(file, "{with_unknown_field}").unwrap();
        file.flush().unwrap();

        let adapter = ReferenceJsonlAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert!(batch.records.is_empty());
        assert_eq!(batch.warnings.len(), 2);
        assert_eq!(
            batch.checkpoint.byte_offset,
            Some(batch.checkpoint.source_len)
        );
    }
}
