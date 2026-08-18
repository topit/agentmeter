//! Collector for Amp's documented `--stream-json` output.
//!
//! Amp does not publish a compatibility contract for its local thread files.
//! This adapter therefore consumes an explicitly captured NDJSON stream and
//! does not inspect prompts, responses, or tool payloads.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
    time::UNIX_EPOCH,
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

const CLIENT: &str = "amp";
const PARSER_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct AmpStreamJsonAdapter {
    path: PathBuf,
}

impl AmpStreamJsonAdapter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl CollectorAdapter for AmpStreamJsonAdapter {
    fn id(&self) -> &'static str {
        "amp-stream-json"
    }

    fn parser_version(&self) -> u32 {
        PARSER_VERSION
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        Ok(self
            .path
            .is_file()
            .then(|| SourceCandidate {
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
        let metadata = source.path.metadata().map_err(io_error)?;
        let observed_source_len = metadata.len();
        let occurred_at_unix_ms = modified_unix_ms(&metadata)?;

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
                    "incomplete trailing Amp stream record at byte {record_offset}; retrying after append"
                ));
                break;
            }

            committed_offset = reader.stream_position().map_err(io_error)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<StreamEvent>(trimmed) {
                Ok(event) => {
                    if let Some(record) =
                        event.into_record(record_offset, occurred_at_unix_ms, &mut warnings)
                    {
                        records.push(record);
                    }
                }
                Err(error) => warnings.push(format!(
                    "malformed Amp stream record at byte {record_offset}: {error}"
                )),
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

#[derive(Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parent_tool_use_id: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    message: Option<AssistantMessage>,
}

impl StreamEvent {
    fn into_record(
        self,
        record_offset: u64,
        occurred_at_unix_ms: i64,
        warnings: &mut Vec<String>,
    ) -> Option<UsageRecord> {
        if self.kind != "assistant" || self.parent_tool_use_id.is_some() {
            return None;
        }

        let Some(session_id) = self.session_id else {
            warnings.push(format!(
                "Amp assistant record at byte {record_offset} has no session_id; skipped"
            ));
            return None;
        };
        let Some(message) = self.message else {
            warnings.push(format!(
                "Amp assistant record at byte {record_offset} has no message; skipped"
            ));
            return None;
        };
        let usage = message.usage?;

        let (tokens, usage_model, schema_variant, selected_iteration) =
            if let Some(iteration) = usage.iterations.last() {
                (
                    iteration.tokens(),
                    iteration.model.as_deref(),
                    "amp-stream-json-v1-iterations",
                    true,
                )
            } else {
                (
                    usage.tokens.tokens(),
                    usage.tokens.model.as_deref(),
                    "amp-stream-json-v1",
                    false,
                )
            };

        let Some(total) = tokens.checked_total() else {
            warnings.push(format!(
                "Amp usage at byte {record_offset} overflows the canonical token total; skipped"
            ));
            return None;
        };
        if total == 0 {
            warnings.push(format!(
                "Amp usage at byte {record_offset} reports no tokens; skipped"
            ));
            return None;
        }

        let model = usage_model
            .or(message.model.as_deref())
            .unwrap_or("unknown");
        let mut normalization_notes = vec![
            "Amp Stream JSON has no event timestamp; used source file modification time".to_owned(),
        ];
        if selected_iteration {
            normalization_notes.push(
                "selected the final observed usage iteration to avoid cumulative double counting"
                    .to_owned(),
            );
        }
        if model == "unknown" {
            normalization_notes
                .push("Amp Stream JSON did not report a model identifier".to_owned());
        }

        let identity_material = format!("amp-stream-v1\0{session_id}\0{record_offset}");
        Some(UsageRecord {
            event: UsageEvent {
                id: format!("amp-stream-v1:{}", hash_bytes(identity_material.as_bytes())),
                source_id: String::new(),
                session_id: Some(session_id),
                client: CLIENT.to_owned(),
                provider: None,
                model: model.to_owned(),
                occurred_at_unix_ms,
                tokens,
                source_reported_total: None,
                confidence: DataConfidence::Exact,
            },
            provenance: EventProvenance {
                native_id: None,
                record_offset: Some(record_offset),
                schema_variant: schema_variant.to_owned(),
                timestamp_origin: TimestampOrigin::FileModified,
                normalization_notes,
            },
        })
    }
}

#[derive(Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<UsageEnvelope>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct UsageEnvelope {
    #[serde(flatten)]
    tokens: UsageTokens,
    iterations: Vec<UsageTokens>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct UsageTokens {
    model: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
}

impl UsageTokens {
    fn tokens(&self) -> TokenBreakdown {
        TokenBreakdown {
            input: self.input_tokens,
            output: self.output_tokens,
            cache_read: self.cache_read_input_tokens,
            cache_write: self.cache_creation_input_tokens,
            reasoning: 0,
        }
    }
}

fn modified_unix_ms(metadata: &std::fs::Metadata) -> Result<i64, CollectorError> {
    let duration = metadata
        .modified()
        .map_err(io_error)?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CollectorError::new(error.to_string()))?;
    duration
        .as_millis()
        .try_into()
        .map_err(|_| CollectorError::new("source modification time does not fit i64 milliseconds"))
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use tempfile::NamedTempFile;

    use super::AmpStreamJsonAdapter;
    use crate::{CollectorAdapter, IngestMode, IngestStart};

    const SESSION: &str = "T-00000000-0000-0000-0000-000000000001";
    const OFFICIAL_ASSISTANT: &str = r#"{"type":"assistant","message":{"type":"message","role":"assistant","content":[],"stop_reason":"end_turn","usage":{"input_tokens":120,"cache_creation_input_tokens":10,"cache_read_input_tokens":30,"output_tokens":8,"max_tokens":224000,"service_tier":"standard"}},"parent_tool_use_id":null,"session_id":"T-00000000-0000-0000-0000-000000000001"}"#;

    #[test]
    fn parses_documented_usage_without_reading_message_content() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{OFFICIAL_ASSISTANT}").unwrap();
        file.flush().unwrap();

        let adapter = AmpStreamJsonAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        let record = &batch.records[0];
        assert_eq!(record.event.session_id.as_deref(), Some(SESSION));
        assert_eq!(record.event.client, "amp");
        assert_eq!(record.event.model, "unknown");
        assert_eq!(record.event.tokens.input, 120);
        assert_eq!(record.event.tokens.output, 8);
        assert_eq!(record.event.tokens.cache_read, 30);
        assert_eq!(record.event.tokens.cache_write, 10);
        assert_eq!(record.provenance.record_offset, Some(0));
        assert_eq!(record.provenance.schema_variant, "amp-stream-json-v1");
    }

    #[test]
    fn ignores_non_assistant_and_child_events() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"system","subtype":"init","session_id":"{SESSION}"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":50}}}},"parent_tool_use_id":"tool-synthetic-001","session_id":"{SESSION}"}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let adapter = AmpStreamJsonAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert!(batch.records.is_empty());
        assert!(batch.warnings.is_empty());
    }

    #[test]
    fn selects_the_last_observed_usage_iteration() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"model":"outer-model","usage":{{"input_tokens":999,"iterations":[{{"model":"model-a","input_tokens":10,"output_tokens":2}},{{"model":"model-b","input_tokens":20,"output_tokens":4}}]}}}},"session_id":"{SESSION}"}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let adapter = AmpStreamJsonAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.model, "model-b");
        assert_eq!(batch.records[0].event.tokens.input, 20);
        assert_eq!(batch.records[0].event.tokens.output, 4);
        assert_eq!(
            batch.records[0].provenance.schema_variant,
            "amp-stream-json-v1-iterations"
        );
    }

    #[test]
    fn resumes_and_recovers_from_rewrite() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{OFFICIAL_ASSISTANT}").unwrap();
        file.flush().unwrap();

        let adapter = AmpStreamJsonAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        writeln!(file, "{OFFICIAL_ASSISTANT}").unwrap();
        file.flush().unwrap();

        let appended = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(appended.mode, IngestMode::Append);
        assert_eq!(appended.records.len(), 1);
        assert_ne!(first.records[0].event.id, appended.records[0].event.id);

        fs::write(file.path(), format!("{OFFICIAL_ASSISTANT}\n")).unwrap();
        let replaced = adapter
            .ingest(&source, IngestStart::Resume(&appended.checkpoint))
            .unwrap();
        assert_eq!(replaced.mode, IngestMode::Replace);
        assert_eq!(replaced.records.len(), 1);
    }

    #[test]
    fn retries_partial_records_and_reports_invalid_usage() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{{\"type\":}}").unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"usage":{{}}}},"session_id":"{SESSION}"}}"#
        )
        .unwrap();
        write!(file, "{OFFICIAL_ASSISTANT}").unwrap();
        file.flush().unwrap();

        let adapter = AmpStreamJsonAdapter::new(file.path());
        let source = adapter.discover().unwrap().remove(0);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert!(batch.records.is_empty());
        assert_eq!(batch.warnings.len(), 3);
        assert!(batch.checkpoint.byte_offset.unwrap() < batch.checkpoint.source_len);
    }
}
