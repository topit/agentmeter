//! Incremental collector for official Codex CLI rollout JSONL files.

use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use agentmeter_core::{
    DataConfidence, EventProvenance, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    CollectorAdapter, CollectorError, IngestBatch, IngestMode, IngestStart, SourceCandidate,
    SourceCheckpoint, SourceKind,
    file_support::{
        checkpoint_continues, ensure_kind, hash_bytes, hash_file, hash_prefix, io_error,
    },
};

const PARSER_VERSION: u32 = 1;
const SCHEMA_VARIANT: &str = "codex-rollout-jsonl-v1";

#[derive(Clone, Debug)]
pub struct CodexJsonlAdapter {
    codex_home: PathBuf,
}

impl CodexJsonlAdapter {
    pub fn new(codex_home: impl Into<PathBuf>) -> Self {
        Self {
            codex_home: codex_home.into(),
        }
    }

    pub fn default_codex_home() -> Option<PathBuf> {
        env::var_os("CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".codex"))
            })
            .or_else(|| {
                env::var_os("USERPROFILE")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".codex"))
            })
    }

    fn discover_root(
        root: &Path,
        sources: &mut BTreeMap<String, SourceCandidate>,
        replace_existing: bool,
    ) -> Result<(), CollectorError> {
        if !root.exists() {
            return Ok(());
        }
        let mut pending = vec![root.to_owned()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let file_type = entry.file_type().map_err(io_error)?;
                if file_type.is_dir() {
                    pending.push(entry.path());
                    continue;
                }
                if !file_type.is_file()
                    || entry.path().extension().and_then(|ext| ext.to_str()) != Some("jsonl")
                {
                    continue;
                }
                let source_key = entry.file_name().to_string_lossy().into_owned();
                let candidate = SourceCandidate {
                    path: entry.path(),
                    kind: SourceKind::AppendOnlyJsonl,
                    source_key: source_key.clone(),
                };
                if replace_existing {
                    sources.insert(source_key, candidate);
                } else {
                    sources.entry(source_key).or_insert(candidate);
                }
            }
        }
        Ok(())
    }
}

impl CollectorAdapter for CodexJsonlAdapter {
    fn id(&self) -> &'static str {
        "codex-cli-jsonl"
    }

    fn parser_version(&self) -> u32 {
        PARSER_VERSION
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        let mut sources = BTreeMap::new();
        Self::discover_root(
            &self.codex_home.join("archived_sessions"),
            &mut sources,
            false,
        )?;
        // During an interrupted archive operation both copies may exist. The
        // active source wins, while source_key remains stable after movement.
        Self::discover_root(&self.codex_home.join("sessions"), &mut sources, true)?;
        Ok(sources.into_values().collect())
    }

    fn ingest(
        &self,
        source: &SourceCandidate,
        start: IngestStart<'_>,
    ) -> Result<IngestBatch, CollectorError> {
        ensure_kind(source, SourceKind::AppendOnlyJsonl)?;
        let metadata = source.path.metadata().map_err(io_error)?;
        let observed_source_len = metadata.len();
        let file_modified_unix_ms = modified_unix_ms(&metadata)?;

        let (mode, start_offset, mut state) = match start {
            IngestStart::Resume(checkpoint)
                if checkpoint_continues(&source.path, checkpoint, observed_source_len)? =>
            {
                (
                    IngestMode::Append,
                    checkpoint.byte_offset.unwrap_or_default(),
                    decode_state(&checkpoint.parser_state)?,
                )
            }
            IngestStart::Resume(_) | IngestStart::Rebuild => {
                (IngestMode::Replace, 0, ParserState::default())
            }
            IngestStart::Fresh => (IngestMode::Append, 0, ParserState::default()),
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
                    "incomplete trailing Codex record at byte {record_offset}; retrying after append"
                ));
                break;
            }
            committed_offset = reader.stream_position().map_err(io_error)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<RolloutEnvelope>(trimmed) {
                Ok(envelope) => {
                    if let Some(record) = process_envelope(
                        envelope,
                        &source.source_key,
                        record_offset,
                        trimmed.as_bytes(),
                        file_modified_unix_ms,
                        &mut state,
                        &mut warnings,
                    ) {
                        records.push(record);
                    }
                }
                Err(error) => warnings.push(format!(
                    "malformed Codex record at byte {record_offset}: {error}"
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
                parser_state: serde_json::to_vec(&state)
                    .map_err(|error| CollectorError::new(error.to_string()))?,
            },
            source_fingerprint: hash_file(&source.path)?,
            warnings,
        })
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RolloutEnvelope {
    timestamp: Option<String>,
    ordinal: Option<u64>,
    #[serde(rename = "type")]
    kind: String,
    payload: RolloutPayload,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RolloutPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    session_id: Option<String>,
    forked_from_id: Option<String>,
    model_provider: Option<String>,
    model: Option<String>,
    info: Option<TokenUsageInfo>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TokenUsageInfo {
    total_token_usage: Option<RawUsage>,
    last_token_usage: Option<RawUsage>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
struct RawUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

impl RawUsage {
    fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.cached_input_tokens == 0
            && self.cache_write_input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_output_tokens == 0
    }

    fn same_counters(&self, other: &Self) -> bool {
        self.input_tokens == other.input_tokens
            && self.cached_input_tokens == other.cached_input_tokens
            && self.cache_write_input_tokens == other.cache_write_input_tokens
            && self.output_tokens == other.output_tokens
            && self.reasoning_output_tokens == other.reasoning_output_tokens
    }

    fn componentwise_at_least(&self, previous: &Self) -> bool {
        self.input_tokens >= previous.input_tokens
            && self.cached_input_tokens >= previous.cached_input_tokens
            && self.cache_write_input_tokens >= previous.cache_write_input_tokens
            && self.output_tokens >= previous.output_tokens
            && self.reasoning_output_tokens >= previous.reasoning_output_tokens
    }

    fn subtract(&self, previous: &Self) -> Self {
        Self {
            input_tokens: self.input_tokens - previous.input_tokens,
            cached_input_tokens: self.cached_input_tokens - previous.cached_input_tokens,
            cache_write_input_tokens: self.cache_write_input_tokens
                - previous.cache_write_input_tokens,
            output_tokens: self.output_tokens - previous.output_tokens,
            reasoning_output_tokens: self.reasoning_output_tokens
                - previous.reasoning_output_tokens,
            total_tokens: if self.total_tokens >= previous.total_tokens
                && self.total_tokens != 0
                && previous.total_tokens != 0
            {
                self.total_tokens - previous.total_tokens
            } else {
                0
            },
        }
    }

    fn saturating_add(&self, increment: &Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_add(increment.input_tokens),
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_add(increment.cached_input_tokens),
            cache_write_input_tokens: self
                .cache_write_input_tokens
                .saturating_add(increment.cache_write_input_tokens),
            output_tokens: self.output_tokens.saturating_add(increment.output_tokens),
            reasoning_output_tokens: self
                .reasoning_output_tokens
                .saturating_add(increment.reasoning_output_tokens),
            total_tokens: self.total_tokens.saturating_add(increment.total_tokens),
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ParserState {
    thread_id: Option<String>,
    forked_from_id: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    previous_total: Option<RawUsage>,
}

fn process_envelope(
    envelope: RolloutEnvelope,
    source_key: &str,
    record_offset: u64,
    raw_line: &[u8],
    file_modified_unix_ms: i64,
    state: &mut ParserState,
    warnings: &mut Vec<String>,
) -> Option<UsageRecord> {
    match envelope.kind.as_str() {
        "session_meta" => {
            if state.thread_id.is_none() {
                state.thread_id = envelope
                    .payload
                    .id
                    .or(envelope.payload.session_id)
                    .filter(|id| !id.is_empty());
                state.forked_from_id = envelope.payload.forked_from_id;
            }
            if let Some(provider) = envelope
                .payload
                .model_provider
                .filter(|value| !value.is_empty())
            {
                state.provider = Some(provider);
            }
            None
        }
        "turn_context" => {
            if let Some(model) = envelope.payload.model.filter(|value| !value.is_empty()) {
                state.model = Some(model);
            }
            None
        }
        "event_msg" if envelope.payload.kind.as_deref() == Some("token_count") => {
            let Some(info) = envelope.payload.info else {
                warnings.push(format!(
                    "Codex token_count at byte {record_offset} has no usage info; skipped"
                ));
                return None;
            };
            let previous = state.previous_total.as_ref();
            if info
                .total_token_usage
                .as_ref()
                .is_some_and(|total| previous.is_some_and(|value| total.same_counters(value)))
            {
                return None;
            }

            let mut reset = false;
            let delta = if let Some(last) = info.last_token_usage.filter(|usage| !usage.is_zero()) {
                if let (Some(total), Some(previous)) = (&info.total_token_usage, previous)
                    && !total.componentwise_at_least(previous)
                {
                    reset = true;
                }
                last
            } else if let Some(total) = info
                .total_token_usage
                .clone()
                .filter(|usage| !usage.is_zero())
            {
                match previous {
                    Some(previous) if total.componentwise_at_least(previous) => {
                        total.subtract(previous)
                    }
                    Some(_) => {
                        reset = true;
                        total.clone()
                    }
                    None => total.clone(),
                }
            } else {
                warnings.push(format!(
                    "Codex token_count at byte {record_offset} reports no tokens; skipped"
                ));
                return None;
            };

            if reset {
                warnings.push(format!(
                    "Codex cumulative usage regressed at byte {record_offset}; treated it as a reset boundary"
                ));
            }
            state.previous_total = match info.total_token_usage {
                Some(total) => Some(total),
                None => Some(
                    state
                        .previous_total
                        .as_ref()
                        .map_or_else(|| delta.clone(), |total| total.saturating_add(&delta)),
                ),
            };
            Some(finish_usage(
                delta,
                envelope.timestamp.as_deref(),
                envelope.ordinal,
                source_key,
                record_offset,
                raw_line,
                file_modified_unix_ms,
                state,
                warnings,
            ))
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_usage(
    raw: RawUsage,
    timestamp: Option<&str>,
    ordinal: Option<u64>,
    source_key: &str,
    record_offset: u64,
    raw_line: &[u8],
    file_modified_unix_ms: i64,
    state: &ParserState,
    warnings: &mut Vec<String>,
) -> UsageRecord {
    let mut derived = false;
    let cached_total = raw
        .cached_input_tokens
        .saturating_add(raw.cache_write_input_tokens);
    let uncached_input = if cached_total <= raw.input_tokens {
        raw.input_tokens - cached_total
    } else {
        derived = true;
        warnings.push(format!(
            "Codex cached input exceeds total input at byte {record_offset}; clamped uncached input to zero"
        ));
        0
    };
    let non_reasoning_output = if raw.reasoning_output_tokens <= raw.output_tokens {
        raw.output_tokens - raw.reasoning_output_tokens
    } else {
        derived = true;
        warnings.push(format!(
            "Codex reasoning output exceeds total output at byte {record_offset}; clamped non-reasoning output to zero"
        ));
        0
    };
    let expected_total = raw.input_tokens.checked_add(raw.output_tokens);
    if raw.total_tokens != 0 && expected_total != Some(raw.total_tokens) {
        derived = true;
        warnings.push(format!(
            "Codex reported total disagrees with input plus output at byte {record_offset}"
        ));
    }

    let (occurred_at_unix_ms, timestamp_origin, mut normalization_notes) = match timestamp
        .and_then(parse_rfc3339_millis)
    {
        Some(value) => (value, TimestampOrigin::Source, Vec::new()),
        None => {
            warnings.push(format!(
                    "Codex usage at byte {record_offset} has no valid timestamp; used file modification time"
                ));
            (
                file_modified_unix_ms,
                TimestampOrigin::FileModified,
                vec![
                    "event timestamp absent or invalid; used source file modification time"
                        .to_owned(),
                ],
            )
        }
    };
    normalization_notes.push(format!(
        "raw Codex usage: input={}, cached_input={}, cache_write_input={}, output={}, reasoning_output={}, total={}",
        raw.input_tokens,
        raw.cached_input_tokens,
        raw.cache_write_input_tokens,
        raw.output_tokens,
        raw.reasoning_output_tokens,
        raw.total_tokens
    ));
    if state.forked_from_id.is_some() {
        normalization_notes.push(
            "fork lineage observed; cross-file inherited-history deduplication is pending"
                .to_owned(),
        );
    }

    let thread_id = state.thread_id.as_deref().unwrap_or(source_key);
    let identity = ordinal.map_or_else(
        || {
            let material = [thread_id.as_bytes(), &record_offset.to_le_bytes(), raw_line].concat();
            format!("legacy:{}", hash_bytes(&material))
        },
        |ordinal| format!("ordinal:{ordinal}"),
    );
    UsageRecord {
        event: UsageEvent {
            id: format!(
                "codex-v1:{}",
                hash_bytes(format!("{thread_id}\0{identity}").as_bytes())
            ),
            source_id: String::new(),
            session_id: Some(thread_id.to_owned()),
            client: "codex-cli".to_owned(),
            provider: state.provider.clone(),
            model: state.model.clone().unwrap_or_else(|| "unknown".to_owned()),
            occurred_at_unix_ms,
            tokens: TokenBreakdown {
                input: uncached_input,
                output: non_reasoning_output,
                cache_read: raw.cached_input_tokens,
                cache_write: raw.cache_write_input_tokens,
                reasoning: raw.reasoning_output_tokens,
            },
            source_reported_total: (raw.total_tokens != 0).then_some(raw.total_tokens),
            confidence: if derived {
                DataConfidence::Derived
            } else {
                DataConfidence::Exact
            },
        },
        provenance: EventProvenance {
            native_id: ordinal.map(|value| format!("ordinal:{value}")),
            record_offset: Some(record_offset),
            schema_variant: SCHEMA_VARIANT.to_owned(),
            timestamp_origin,
            normalization_notes,
        },
    }
}

fn decode_state(bytes: &[u8]) -> Result<ParserState, CollectorError> {
    if bytes.is_empty() {
        Ok(ParserState::default())
    } else {
        serde_json::from_slice(bytes)
            .map_err(|error| CollectorError::new(format!("invalid Codex parser state: {error}")))
    }
}

fn parse_rfc3339_millis(value: &str) -> Option<i64> {
    let nanos = OffsetDateTime::parse(value, &Rfc3339)
        .ok()?
        .unix_timestamp_nanos();
    i64::try_from(nanos / 1_000_000).ok()
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

    use tempfile::{NamedTempFile, TempDir};

    use super::CodexJsonlAdapter;
    use crate::{CollectorAdapter, IngestMode, IngestStart};

    const META: &str = r#"{"timestamp":"2024-01-01T00:00:00Z","ordinal":0,"type":"session_meta","payload":{"id":"thread-synthetic-001","session_id":"session-synthetic-001","model_provider":"openai"}}"#;
    const TURN: &str = r#"{"timestamp":"2024-01-01T00:00:01Z","ordinal":1,"type":"turn_context","payload":{"model":"gpt-synthetic"}}"#;

    fn token(ordinal: u64, total_input: u64, last_input: u64) -> String {
        serde_json::json!({
            "timestamp": "2024-01-01T00:00:02Z",
            "ordinal": ordinal,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": total_input,
                        "cached_input_tokens": 20,
                        "cache_write_input_tokens": 5,
                        "output_tokens": 40,
                        "reasoning_output_tokens": 10,
                        "total_tokens": total_input + 40
                    },
                    "last_token_usage": {
                        "input_tokens": last_input,
                        "cached_input_tokens": 10,
                        "cache_write_input_tokens": 2,
                        "output_tokens": 20,
                        "reasoning_output_tokens": 5,
                        "total_tokens": last_input + 20
                    }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn discovers_active_and_archived_with_active_precedence() {
        let home = TempDir::new().unwrap();
        let active = home.path().join("sessions/2024/01/01");
        let archived = home.path().join("archived_sessions");
        fs::create_dir_all(&active).unwrap();
        fs::create_dir_all(&archived).unwrap();
        fs::write(active.join("rollout-a.jsonl"), "").unwrap();
        fs::write(archived.join("rollout-a.jsonl"), "archived").unwrap();
        fs::write(archived.join("rollout-b.jsonl"), "").unwrap();

        let sources = CodexJsonlAdapter::new(home.path()).discover().unwrap();
        assert_eq!(sources.len(), 2);
        let rollout_a = sources
            .iter()
            .find(|source| source.source_key == "rollout-a.jsonl")
            .unwrap();
        assert!(rollout_a.path.starts_with(&active));
    }

    #[test]
    fn parses_last_usage_into_exclusive_canonical_buckets() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{META}").unwrap();
        writeln!(file, "{TURN}").unwrap();
        writeln!(file, "{}", token(2, 100, 50)).unwrap();
        file.flush().unwrap();

        let adapter = CodexJsonlAdapter::new("unused");
        let source = crate::SourceCandidate {
            path: file.path().to_owned(),
            kind: crate::SourceKind::AppendOnlyJsonl,
            source_key: "rollout-synthetic.jsonl".into(),
        };
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 1);
        let event = &batch.records[0].event;
        assert_eq!(event.session_id.as_deref(), Some("thread-synthetic-001"));
        assert_eq!(event.provider.as_deref(), Some("openai"));
        assert_eq!(event.model, "gpt-synthetic");
        assert_eq!(event.tokens.input, 38);
        assert_eq!(event.tokens.cache_read, 10);
        assert_eq!(event.tokens.cache_write, 2);
        assert_eq!(event.tokens.output, 15);
        assert_eq!(event.tokens.reasoning, 5);
        assert_eq!(event.tokens.checked_total(), Some(70));
    }

    #[test]
    fn total_only_usage_deltas_and_equal_snapshots_do_not_double_count() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{META}\n{TURN}").unwrap();
        for (ordinal, input, output) in [(2, 100, 40), (3, 100, 40), (4, 160, 55)] {
            let line = serde_json::json!({
                "timestamp": "2024-01-01T00:00:02Z",
                "ordinal": ordinal,
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {
                            "input_tokens": input,
                            "output_tokens": output,
                            "total_tokens": input + output
                        }
                    }
                }
            });
            writeln!(file, "{line}").unwrap();
        }
        file.flush().unwrap();
        let adapter = CodexJsonlAdapter::new("unused");
        let source = crate::SourceCandidate {
            path: file.path().to_owned(),
            kind: crate::SourceKind::AppendOnlyJsonl,
            source_key: "rollout-synthetic.jsonl".into(),
        };
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].event.tokens.input, 100);
        assert_eq!(batch.records[1].event.tokens.input, 60);
        assert_eq!(batch.records[1].event.tokens.output, 15);
    }

    #[test]
    fn last_only_usage_advances_the_synthetic_cumulative_baseline() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{META}\n{TURN}").unwrap();
        for line in [
            serde_json::json!({
                "timestamp":"2024-01-01T00:00:02Z","ordinal":2,"type":"event_msg",
                "payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}
            }),
            serde_json::json!({
                "timestamp":"2024-01-01T00:00:03Z","ordinal":3,"type":"event_msg",
                "payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}
            }),
            serde_json::json!({
                "timestamp":"2024-01-01T00:00:04Z","ordinal":4,"type":"event_msg",
                "payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":130,"output_tokens":30,"total_tokens":160}}}
            }),
        ] {
            writeln!(file, "{line}").unwrap();
        }
        file.flush().unwrap();
        let adapter = CodexJsonlAdapter::new("unused");
        let source = crate::SourceCandidate {
            path: file.path().to_owned(),
            kind: crate::SourceKind::AppendOnlyJsonl,
            source_key: "rollout-synthetic.jsonl".into(),
        };

        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 3);
        assert_eq!(batch.records[1].event.tokens.input, 10);
        assert_eq!(batch.records[2].event.tokens.input, 20);
        assert_eq!(batch.records[2].event.tokens.output, 5);
    }

    #[test]
    fn resumes_with_parser_state_and_recovers_after_rewrite() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{META}\n{TURN}").unwrap();
        writeln!(file, "{}", token(2, 100, 50)).unwrap();
        file.flush().unwrap();
        let adapter = CodexJsonlAdapter::new("unused");
        let source = crate::SourceCandidate {
            path: file.path().to_owned(),
            kind: crate::SourceKind::AppendOnlyJsonl,
            source_key: "rollout-synthetic.jsonl".into(),
        };
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        writeln!(file, "{}", token(3, 150, 50)).unwrap();
        file.flush().unwrap();
        let resumed = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(resumed.mode, IngestMode::Append);
        assert_eq!(resumed.records.len(), 1);

        fs::write(
            file.path(),
            format!("{META}\n{TURN}\n{}\n", token(2, 80, 40)),
        )
        .unwrap();
        let rebuilt = adapter
            .ingest(&source, IngestStart::Resume(&resumed.checkpoint))
            .unwrap();
        assert_eq!(rebuilt.mode, IngestMode::Replace);
        assert_eq!(rebuilt.records.len(), 1);
    }

    #[test]
    fn reports_malformed_middle_null_info_and_incomplete_tail() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{{\"type\":}}").unwrap();
        writeln!(file, r#"{{"timestamp":"2024-01-01T00:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":null}}}}"#).unwrap();
        write!(file, "{}", token(2, 100, 50)).unwrap();
        file.flush().unwrap();
        let adapter = CodexJsonlAdapter::new("unused");
        let source = crate::SourceCandidate {
            path: file.path().to_owned(),
            kind: crate::SourceKind::AppendOnlyJsonl,
            source_key: "rollout-synthetic.jsonl".into(),
        };
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.records.is_empty());
        assert_eq!(batch.warnings.len(), 3);
        assert!(batch.checkpoint.byte_offset.unwrap() < batch.checkpoint.source_len);
    }
}
