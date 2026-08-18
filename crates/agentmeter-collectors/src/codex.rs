//! Incremental collector for official Codex CLI rollout JSONL files.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use agentmeter_core::{
    DataConfidence, EventProvenance, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use serde::{Deserialize, Deserializer, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    CollectorAdapter, CollectorError, IngestBatch, IngestMode, IngestStart, SourceCandidate,
    SourceCheckpoint, SourceKind,
    file_support::{
        checkpoint_continues, ensure_kind, hash_bytes, hash_file, hash_prefix, io_error,
    },
};

const PARSER_VERSION: u32 = 3;
const SCHEMA_VARIANT: &str = "codex-rollout-jsonl-v2";

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
        sources: &mut BTreeMap<String, (u8, SourceCandidate)>,
        root_priority: u8,
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
                if !file_type.is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy().into_owned();
                let (source_key, format_priority) = if file_name.ends_with(".jsonl") {
                    (file_name, 1)
                } else if let Some(source_key) = file_name.strip_suffix(".jsonl.zst") {
                    (format!("{source_key}.jsonl"), 0)
                } else {
                    continue;
                };
                let candidate = SourceCandidate {
                    path: entry.path(),
                    kind: SourceKind::AppendOnlyJsonl,
                    source_key: source_key.clone(),
                };
                let priority = root_priority.saturating_add(format_priority);
                if sources
                    .get(&source_key)
                    .is_none_or(|(existing_priority, _)| priority > *existing_priority)
                {
                    sources.insert(source_key, (priority, candidate));
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
        Self::discover_root(&self.codex_home.join("archived_sessions"), &mut sources, 0)?;
        // During an interrupted archive operation both copies may exist. The
        // active source wins, while source_key remains stable after movement.
        Self::discover_root(&self.codex_home.join("sessions"), &mut sources, 2)?;
        Ok(sources
            .into_values()
            .map(|(_, candidate)| candidate)
            .collect())
    }

    fn ingest(
        &self,
        source: &SourceCandidate,
        start: IngestStart<'_>,
    ) -> Result<IngestBatch, CollectorError> {
        ensure_kind(source, SourceKind::AppendOnlyJsonl)?;
        let compressed = is_compressed(&source.path);
        let metadata = source.path.metadata().map_err(io_error)?;
        let observed_source_len = metadata.len();
        let file_modified_unix_ms = modified_unix_ms(&metadata)?;

        let (mode, start_offset, mut state) = match start {
            _ if compressed => (IngestMode::Replace, 0, ParserState::default()),
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

        let mut reader = open_rollout(&source.path, start_offset)?;
        let mut records = Vec::new();
        let (mut replay, mut warnings) = if start_offset == 0 {
            self.lineage_plan(source, &mut state)
        } else if state.legacy_replay_matching {
            let mut lineage_state = ParserState::default();
            let (mut replay, warnings) = self.lineage_plan(source, &mut lineage_state);
            if replay.expected.len() < state.legacy_replay_cursor {
                state.legacy_replay_matching = false;
                state.lineage_unresolved = true;
                (
                    LegacyReplay::default(),
                    vec![
                        "Codex legacy parent prefix changed during resume; remaining records were retained"
                            .to_owned(),
                    ],
                )
            } else {
                replay.cursor = state.legacy_replay_cursor;
                replay.matching = true;
                (replay, warnings)
            }
        } else {
            (LegacyReplay::default(), Vec::new())
        };
        let mut committed_offset = start_offset;

        loop {
            let record_offset = committed_offset;
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
            committed_offset = committed_offset.saturating_add(bytes_read as u64);
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
                        &mut replay,
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
                byte_offset: (!compressed).then_some(committed_offset),
                source_len,
                prefix_fingerprint: (!compressed)
                    .then(|| hash_prefix(&source.path, committed_offset))
                    .transpose()?,
                parser_state: serde_json::to_vec(&state)
                    .map_err(|error| CollectorError::new(error.to_string()))?,
            },
            source_fingerprint: hash_file(&source.path)?,
            warnings,
        })
    }
}

impl CodexJsonlAdapter {
    fn lineage_plan(
        &self,
        source: &SourceCandidate,
        state: &mut ParserState,
    ) -> (LegacyReplay, Vec<String>) {
        let Some(metadata) = read_first_metadata(&source.path) else {
            return (LegacyReplay::default(), Vec::new());
        };
        if let Some(base) = metadata.history_base.as_ref() {
            let mut seen = BTreeSet::new();
            if let Some(rollout_id) = rollout_id(&source.source_key) {
                seen.insert(rollout_id);
            }
            match self.paginated_baseline(&metadata, base, &mut seen) {
                Ok(baseline) => state.previous_total = baseline,
                Err(message) => {
                    state.lineage_unresolved = true;
                    return (
                        LegacyReplay::default(),
                        vec![format!("Codex paginated lineage unresolved: {message}")],
                    );
                }
            }
            return (LegacyReplay::default(), Vec::new());
        }

        let Some(parent_id) = metadata.forked_from_id.filter(|value| !value.is_empty()) else {
            return (LegacyReplay::default(), Vec::new());
        };
        if metadata.id.as_deref() == Some(parent_id.as_str()) {
            state.lineage_unresolved = true;
            return (
                LegacyReplay::default(),
                vec![
                    "Codex legacy fork has a self-referencing parent; replay was not deduplicated"
                        .to_owned(),
                ],
            );
        }
        let Ok(sources) = self.discover() else {
            state.lineage_unresolved = true;
            return (
                LegacyReplay::default(),
                vec![
                    "Codex legacy parent discovery failed; replay was not deduplicated".to_owned(),
                ],
            );
        };
        let parent = sources
            .iter()
            .filter(|candidate| candidate.path != source.path)
            .filter_map(|candidate| {
                let candidate_meta = read_first_metadata(&candidate.path)?;
                let same_parent = candidate_meta.id.as_deref() == Some(parent_id.as_str());
                let existed_at_fork = match (
                    candidate_meta
                        .timestamp
                        .as_deref()
                        .and_then(parse_rfc3339_millis),
                    metadata.timestamp.as_deref().and_then(parse_rfc3339_millis),
                ) {
                    (Some(candidate_time), Some(fork_time)) => candidate_time <= fork_time,
                    _ => false,
                };
                (same_parent && existed_at_fork).then_some(candidate)
            })
            .max_by(|left, right| left.source_key.cmp(&right.source_key));
        let Some(parent) = parent else {
            state.lineage_unresolved = true;
            return (
                LegacyReplay::default(),
                vec![format!(
                    "Codex legacy parent {parent_id} is missing; replay was not deduplicated"
                )],
            );
        };
        let signatures = read_token_signatures(&parent.path, metadata.timestamp.as_deref());
        if signatures.is_empty() {
            state.lineage_unresolved = true;
            return (
                LegacyReplay::default(),
                vec![format!(
                    "Codex legacy parent {parent_id} has no matchable usage prefix; replay was not deduplicated"
                )],
            );
        }
        state.legacy_replay_matching = true;
        state.legacy_replay_cursor = 0;
        (
            LegacyReplay {
                expected: signatures,
                cursor: 0,
                matching: true,
            },
            Vec::new(),
        )
    }

    fn paginated_baseline(
        &self,
        child: &SessionMetadata,
        base: &HistoryBase,
        seen: &mut BTreeSet<String>,
    ) -> Result<Option<RawUsage>, String> {
        if child.history_mode.as_deref() != Some("paginated") {
            return Err("history_base appears on a non-paginated rollout".to_owned());
        }
        if base.end_ordinal_exclusive == 0 {
            return Err("parent cutoff ordinal is zero".to_owned());
        }
        if !seen.insert(base.thread_id.clone()) {
            return Err(format!("lineage cycle at rollout {}", base.thread_id));
        }
        let sources = self.discover().map_err(|error| error.message)?;
        let parent = sources
            .iter()
            .find(|candidate| rollout_id(&candidate.source_key).as_deref() == Some(&base.thread_id))
            .ok_or_else(|| format!("parent rollout {} is missing", base.thread_id))?;
        let parent_meta = read_first_metadata(&parent.path).ok_or_else(|| {
            format!(
                "parent rollout {} has no canonical metadata",
                base.thread_id
            )
        })?;
        if parent_meta.history_mode.as_deref() != Some("paginated") {
            return Err(format!(
                "parent rollout {} is not paginated",
                base.thread_id
            ));
        }
        let inherited = match parent_meta.history_base.as_ref() {
            Some(parent_base) => self.paginated_baseline(&parent_meta, parent_base, seen),
            None => Ok(None),
        }?;
        Ok(read_baseline_at(&parent.path, base)?.or(inherited))
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
    history_mode: Option<String>,
    history_base: Option<HistoryBase>,
    #[serde(deserialize_with = "deserialize_source_tag")]
    source: Option<String>,
    model_provider: Option<String>,
    model: Option<String>,
    info: Option<TokenUsageInfo>,
}

#[derive(Clone, Default, Deserialize, Eq, PartialEq)]
#[serde(default)]
struct TokenUsageInfo {
    total_token_usage: Option<RawUsage>,
    last_token_usage: Option<RawUsage>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct HistoryBase {
    thread_id: String,
    end_ordinal_exclusive: u64,
    end_byte_offset: u64,
}

#[derive(Default)]
struct SessionMetadata {
    id: Option<String>,
    timestamp: Option<String>,
    forked_from_id: Option<String>,
    history_mode: Option<String>,
    history_base: Option<HistoryBase>,
}

#[derive(Clone, Eq, PartialEq)]
struct TokenSignature {
    timestamp: Option<String>,
    info: TokenUsageInfo,
}

#[derive(Default)]
struct LegacyReplay {
    expected: Vec<TokenSignature>,
    cursor: usize,
    matching: bool,
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
    seen_session_meta: bool,
    unsupported_source: bool,
    lineage_unresolved: bool,
    legacy_replay_cursor: usize,
    legacy_replay_matching: bool,
}

#[allow(clippy::too_many_arguments)]
fn process_envelope(
    envelope: RolloutEnvelope,
    source_key: &str,
    record_offset: u64,
    raw_line: &[u8],
    file_modified_unix_ms: i64,
    state: &mut ParserState,
    replay: &mut LegacyReplay,
    warnings: &mut Vec<String>,
) -> Option<UsageRecord> {
    match envelope.kind.as_str() {
        "session_meta" => {
            if !state.seen_session_meta {
                state.seen_session_meta = true;
                if envelope
                    .payload
                    .source
                    .as_deref()
                    .is_some_and(|source| !matches!(source, "cli" | "exec"))
                {
                    state.unsupported_source = true;
                    warnings.push(
                        "Codex rollout belongs to a non-CLI session source; usage was skipped"
                            .to_owned(),
                    );
                }
                state.thread_id = envelope
                    .payload
                    .id
                    .or(envelope.payload.session_id)
                    .filter(|id| !id.is_empty());
                state.forked_from_id = envelope.payload.forked_from_id;
                if let Some(provider) = envelope
                    .payload
                    .model_provider
                    .filter(|value| !value.is_empty())
                {
                    state.provider = Some(provider);
                }
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
            if state.unsupported_source {
                return None;
            }
            let Some(info) = envelope.payload.info else {
                warnings.push(format!(
                    "Codex token_count at byte {record_offset} has no usage info; skipped"
                ));
                return None;
            };
            let suppress_replay = if replay.matching {
                let signature = TokenSignature {
                    timestamp: envelope.timestamp.clone(),
                    info: info.clone(),
                };
                if replay.expected.get(replay.cursor) == Some(&signature) {
                    replay.cursor += 1;
                    state.legacy_replay_cursor = replay.cursor;
                    if replay.cursor == replay.expected.len() {
                        replay.matching = false;
                        state.legacy_replay_matching = false;
                    }
                    true
                } else {
                    replay.matching = false;
                    state.legacy_replay_matching = false;
                    state.lineage_unresolved = true;
                    warnings.push(format!(
                        "Codex legacy replay diverged after {} matched usage records; remaining records were retained",
                        replay.cursor
                    ));
                    false
                }
            } else {
                false
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
            if suppress_replay {
                return None;
            }
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
    if state.lineage_unresolved {
        derived = true;
        normalization_notes
            .push("lineage could not be resolved exactly; usage was retained".to_owned());
    }

    let thread_id = state.thread_id.as_deref().unwrap_or(source_key);
    let identity = ordinal.map_or_else(
        || {
            let material = [
                source_key.as_bytes(),
                &record_offset.to_le_bytes(),
                raw_line,
            ]
            .concat();
            format!("legacy:{}", hash_bytes(&material))
        },
        |ordinal| format!("ordinal:{ordinal}"),
    );
    UsageRecord {
        event: UsageEvent {
            id: format!(
                "codex-v2:{}",
                hash_bytes(format!("{source_key}\0{identity}").as_bytes())
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
        costs: Vec::new(),
        provenance: EventProvenance {
            native_id: ordinal.map(|value| format!("ordinal:{value}")),
            record_offset: Some(record_offset),
            schema_variant: SCHEMA_VARIANT.to_owned(),
            timestamp_origin,
            normalization_notes,
        },
    }
}

fn deserialize_source_tag<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            None => None,
            Some(serde_json::Value::String(source)) => Some(source),
            Some(_) => Some("non_cli".to_owned()),
        },
    )
}

fn read_first_metadata(path: &Path) -> Option<SessionMetadata> {
    let reader = open_rollout(path, 0).ok()?;
    for line in reader.lines() {
        let line = line.ok()?;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: RolloutEnvelope = serde_json::from_str(&line).ok()?;
        if envelope.kind != "session_meta" {
            return None;
        }
        return Some(SessionMetadata {
            id: envelope
                .payload
                .id
                .or(envelope.payload.session_id)
                .filter(|value| !value.is_empty()),
            timestamp: envelope.timestamp,
            forked_from_id: envelope.payload.forked_from_id,
            history_mode: envelope.payload.history_mode,
            history_base: envelope.payload.history_base,
        });
    }
    None
}

fn rollout_id(source_key: &str) -> Option<String> {
    let stem = source_key.strip_suffix(".jsonl")?;
    let rest = stem.strip_prefix("rollout-")?;
    if rest.len() < 20 || rest.as_bytes().get(19) != Some(&b'-') {
        return None;
    }
    let ids = &rest[20..];
    let id = ids.rsplit_once('_').map_or(ids, |(_, rollout)| rollout);
    is_uuid(id).then(|| id.to_owned())
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn read_token_signatures(path: &Path, end_timestamp: Option<&str>) -> Vec<TokenSignature> {
    let Some(end_millis) = end_timestamp.and_then(parse_rfc3339_millis) else {
        return Vec::new();
    };
    let Ok(reader) = open_rollout(path, 0) else {
        return Vec::new();
    };
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<RolloutEnvelope>(&line).ok())
        .filter(|envelope| {
            envelope.kind == "event_msg"
                && envelope.payload.kind.as_deref() == Some("token_count")
                && envelope
                    .timestamp
                    .as_deref()
                    .and_then(parse_rfc3339_millis)
                    .is_some_and(|timestamp| timestamp <= end_millis)
        })
        .filter_map(|envelope| {
            envelope.payload.info.map(|info| TokenSignature {
                timestamp: envelope.timestamp,
                info,
            })
        })
        .collect()
}

fn read_baseline_at(path: &Path, base: &HistoryBase) -> Result<Option<RawUsage>, String> {
    let mut reader = open_rollout(path, 0).map_err(|error| error.message)?;
    let mut position = 0_u64;
    let mut baseline: Option<RawUsage> = None;
    while position < base.end_byte_offset {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        let next = position.saturating_add(read as u64);
        if next > base.end_byte_offset {
            return Err(format!(
                "parent cutoff byte {} is not a JSONL record boundary",
                base.end_byte_offset
            ));
        }
        position = next;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: RolloutEnvelope = serde_json::from_str(&line)
            .map_err(|error| format!("parent record before cutoff is malformed: {error}"))?;
        let ordinal = envelope
            .ordinal
            .ok_or_else(|| "parent record before cutoff has no ordinal".to_owned())?;
        if ordinal >= base.end_ordinal_exclusive {
            return Err(format!(
                "parent ordinal {ordinal} crosses exclusive cutoff {}",
                base.end_ordinal_exclusive
            ));
        }
        if envelope.kind == "event_msg"
            && envelope.payload.kind.as_deref() == Some("token_count")
            && let Some(info) = envelope.payload.info
        {
            baseline = match info.total_token_usage {
                Some(total) => Some(total),
                None => info.last_token_usage.map(|last| {
                    baseline
                        .as_ref()
                        .map_or_else(|| last.clone(), |total| total.saturating_add(&last))
                }),
            };
        }
    }
    if position != base.end_byte_offset {
        return Err(format!(
            "parent cutoff byte {} was not reached",
            base.end_byte_offset
        ));
    }
    Ok(baseline)
}

fn is_compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

fn open_rollout(path: &Path, offset: u64) -> Result<Box<dyn BufRead>, CollectorError> {
    let file = File::open(path).map_err(io_error)?;
    if is_compressed(path) {
        if offset != 0 {
            return Err(CollectorError::new(
                "compressed Codex rollouts cannot resume from a byte offset",
            ));
        }
        let decoder = zstd::stream::read::Decoder::new(file).map_err(io_error)?;
        Ok(Box::new(BufReader::new(decoder)))
    } else {
        let mut file = file;
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        Ok(Box::new(BufReader::new(file)))
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
    use std::{fs, io::Write, path::Path};

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
        fs::write(archived.join("rollout-c.jsonl"), "archived").unwrap();
        fs::write(active.join("rollout-c.jsonl.zst"), "compressed").unwrap();
        fs::write(active.join("rollout-d.jsonl.zst"), "compressed").unwrap();
        fs::write(active.join("rollout-d.jsonl"), "plain").unwrap();

        let sources = CodexJsonlAdapter::new(home.path()).discover().unwrap();
        assert_eq!(sources.len(), 4);
        let rollout_a = sources
            .iter()
            .find(|source| source.source_key == "rollout-a.jsonl")
            .unwrap();
        assert!(rollout_a.path.starts_with(&active));
        let rollout_c = sources
            .iter()
            .find(|source| source.source_key == "rollout-c.jsonl")
            .unwrap();
        assert_eq!(rollout_c.path.file_name().unwrap(), "rollout-c.jsonl.zst");
        let rollout_d = sources
            .iter()
            .find(|source| source.source_key == "rollout-d.jsonl")
            .unwrap();
        assert_eq!(rollout_d.path.file_name().unwrap(), "rollout-d.jsonl");
    }

    #[test]
    fn compressed_rollout_reuses_jsonl_parser_and_always_rebuilds() {
        let home = TempDir::new().unwrap();
        let active = home.path().join("sessions/2024/01/01");
        fs::create_dir_all(&active).unwrap();
        let contents = format!("{META}\n{TURN}\n{}\n", token(2, 100, 50));
        let path = active.join("rollout-synthetic.jsonl.zst");
        fs::write(
            &path,
            zstd::stream::encode_all(contents.as_bytes(), 3).unwrap(),
        )
        .unwrap();

        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter.discover().unwrap().pop().unwrap();
        assert_eq!(source.source_key, "rollout-synthetic.jsonl");
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(first.mode, IngestMode::Replace);
        assert_eq!(first.records.len(), 1);
        assert_eq!(first.records[0].event.tokens.checked_total(), Some(70));
        assert_eq!(first.checkpoint.byte_offset, None);
        assert_eq!(first.checkpoint.prefix_fingerprint, None);

        let repeated = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(repeated.mode, IngestMode::Replace);
        assert_eq!(repeated.records, first.records);
    }

    #[test]
    fn corrupt_compressed_rollout_fails_instead_of_reporting_zero() {
        let home = TempDir::new().unwrap();
        let active = home.path().join("sessions");
        fs::create_dir_all(&active).unwrap();
        fs::write(active.join("rollout-corrupt.jsonl.zst"), "not-zstd").unwrap();
        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter.discover().unwrap().pop().unwrap();

        assert!(adapter.ingest(&source, IngestStart::Fresh).is_err());
    }

    #[test]
    fn exec_uses_cli_contract_while_non_cli_rollouts_are_deferred() {
        for (source_tag, expected_records) in [("exec", 1), ("vscode", 0)] {
            let mut file = NamedTempFile::new().unwrap();
            writeln!(
                file,
                "{}",
                serde_json::json!({
                    "timestamp":"2024-01-01T00:00:00Z","ordinal":0,
                    "type":"session_meta","payload":{
                        "id":"thread-synthetic-source","source":source_tag,
                        "model_provider":"openai"
                    }
                })
            )
            .unwrap();
            writeln!(file, "{TURN}").unwrap();
            writeln!(file, "{}", token(2, 100, 50)).unwrap();
            file.flush().unwrap();
            let adapter = CodexJsonlAdapter::new("unused");
            let source = crate::SourceCandidate {
                path: file.path().to_owned(),
                kind: crate::SourceKind::AppendOnlyJsonl,
                source_key: format!("rollout-{source_tag}.jsonl"),
            };

            let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
            assert_eq!(batch.records.len(), expected_records);
            assert_eq!(batch.warnings.is_empty(), source_tag == "exec");
        }
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

    #[test]
    fn paginated_child_starts_from_parent_cutoff_baseline() {
        let home = TempDir::new().unwrap();
        let directory = home.path().join("sessions/2024/01/01");
        fs::create_dir_all(&directory).unwrap();
        let parent_id = "01900000-0000-7000-8000-000000000001";
        let child_rollout_id = "01900000-0000-7000-8000-000000000002";
        let parent = directory.join(format!("rollout-2024-01-01T00-00-00-{parent_id}.jsonl"));
        let parent_lines = vec![
            meta("thread-lineage", "2024-01-01T00:00:00Z", None, true, None),
            serde_json::json!({"timestamp":"2024-01-01T00:00:01Z","ordinal":1,"type":"turn_context","payload":{"model":"gpt-lineage"}}),
            total_json(2, "2024-01-01T00:00:02Z", 100, 20),
        ];
        write_lines(&parent, &parent_lines);
        let cutoff = fs::metadata(&parent).unwrap().len();
        let archive = home.path().join("archived_sessions");
        fs::create_dir_all(&archive).unwrap();
        fs::rename(&parent, archive.join(parent.file_name().unwrap())).unwrap();
        let child = directory.join(format!(
            "rollout-2024-01-01T00-00-03-{parent_id}_{child_rollout_id}.jsonl"
        ));
        write_lines(
            &child,
            &[
                meta(
                    "thread-lineage",
                    "2024-01-01T00:00:03Z",
                    None,
                    true,
                    Some((parent_id, 3, cutoff)),
                ),
                serde_json::json!({"timestamp":"2024-01-01T00:00:04Z","ordinal":3,"type":"turn_context","payload":{"model":"gpt-lineage"}}),
                total_json(4, "2024-01-01T00:00:05Z", 140, 30),
            ],
        );

        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter
            .discover()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.path == child)
            .unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.warnings.is_empty());
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.tokens.input, 40);
        assert_eq!(batch.records[0].event.tokens.output, 10);
    }

    #[test]
    fn legacy_fork_suppresses_only_exact_parent_usage_prefix() {
        let home = TempDir::new().unwrap();
        let directory = home.path().join("sessions/2024/01/01");
        fs::create_dir_all(&directory).unwrap();
        let parent_id = "01900000-0000-7000-8000-000000000011";
        let child_id = "01900000-0000-7000-8000-000000000012";
        let inherited = total_json(2, "2024-01-01T00:00:02Z", 100, 20);
        let parent = directory.join(format!("rollout-2024-01-01T00-00-00-{parent_id}.jsonl"));
        write_lines(
            &parent,
            &[
                meta(parent_id, "2024-01-01T00:00:00Z", None, false, None),
                inherited.clone(),
            ],
        );
        let child = directory.join(format!("rollout-2024-01-01T00-00-03-{child_id}.jsonl"));
        write_lines(
            &child,
            &[
                meta(
                    child_id,
                    "2024-01-01T00:00:03Z",
                    Some(parent_id),
                    false,
                    None,
                ),
                inherited,
                serde_json::json!({"timestamp":"2024-01-01T00:00:04Z","ordinal":3,"type":"turn_context","payload":{"model":"gpt-lineage"}}),
                total_json(4, "2024-01-01T00:00:05Z", 130, 25),
            ],
        );

        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter
            .discover()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.path == child)
            .unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.warnings.is_empty());
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.tokens.input, 30);
        assert_eq!(batch.records[0].event.tokens.output, 5);
    }

    #[test]
    fn legacy_replay_gate_survives_an_incomplete_tail_resume() {
        let home = TempDir::new().unwrap();
        let directory = home.path().join("sessions/2024/01/01");
        fs::create_dir_all(&directory).unwrap();
        let parent_id = "01900000-0000-7000-8000-000000000015";
        let child_id = "01900000-0000-7000-8000-000000000016";
        let first_inherited = total_json(2, "2024-01-01T00:00:02Z", 100, 20);
        let second_inherited = total_json(3, "2024-01-01T00:00:03Z", 120, 25);
        let parent = directory.join(format!("rollout-2024-01-01T00-00-00-{parent_id}.jsonl"));
        write_lines(
            &parent,
            &[
                meta(parent_id, "2024-01-01T00:00:00Z", None, false, None),
                first_inherited.clone(),
                second_inherited.clone(),
            ],
        );
        let child = directory.join(format!("rollout-2024-01-01T00-00-04-{child_id}.jsonl"));
        write_lines(
            &child,
            &[
                meta(
                    child_id,
                    "2024-01-01T00:00:04Z",
                    Some(parent_id),
                    false,
                    None,
                ),
                first_inherited,
            ],
        );
        let mut child_file = fs::OpenOptions::new().append(true).open(&child).unwrap();
        write!(child_file, "{second_inherited}").unwrap();
        child_file.flush().unwrap();

        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter
            .discover()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.path == child)
            .unwrap();
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(first.records.is_empty());
        writeln!(child_file).unwrap();
        writeln!(
            child_file,
            "{}",
            total_json(4, "2024-01-01T00:00:05Z", 140, 30)
        )
        .unwrap();
        child_file.flush().unwrap();

        let resumed = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(resumed.mode, IngestMode::Append);
        assert_eq!(resumed.records.len(), 1);
        assert_eq!(resumed.records[0].event.tokens.input, 20);
        assert_eq!(resumed.records[0].event.tokens.output, 5);
    }

    #[test]
    fn unresolved_lineage_is_visible_and_retained_as_derived_usage() {
        let home = TempDir::new().unwrap();
        let directory = home.path().join("sessions/2024/01/01");
        fs::create_dir_all(&directory).unwrap();
        let child_id = "01900000-0000-7000-8000-000000000022";
        let child = directory.join(format!("rollout-2024-01-01T00-00-03-{child_id}.jsonl"));
        write_lines(
            &child,
            &[
                meta(
                    child_id,
                    "2024-01-01T00:00:03Z",
                    Some("01900000-0000-7000-8000-000000000021"),
                    false,
                    None,
                ),
                total_json(2, "2024-01-01T00:00:05Z", 30, 5),
            ],
        );
        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter.discover().unwrap().pop().unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(
            batch.records[0].event.confidence,
            agentmeter_core::DataConfidence::Derived
        );
        assert!(batch.warnings[0].contains("is missing"));
    }

    #[test]
    fn paginated_lineage_cycle_is_reported() {
        let home = TempDir::new().unwrap();
        let directory = home.path().join("sessions/2024/01/01");
        fs::create_dir_all(&directory).unwrap();
        let first_id = "01900000-0000-7000-8000-000000000031";
        let second_id = "01900000-0000-7000-8000-000000000032";
        let first = directory.join(format!("rollout-2024-01-01T00-00-00-{first_id}.jsonl"));
        let second = directory.join(format!(
            "rollout-2024-01-01T00-00-01-{first_id}_{second_id}.jsonl"
        ));
        let mut first_len = 1;
        let mut second_len = 1;
        loop {
            write_lines(
                &first,
                &[meta(
                    first_id,
                    "2024-01-01T00:00:00Z",
                    None,
                    true,
                    Some((second_id, 1, second_len)),
                )],
            );
            write_lines(
                &second,
                &[meta(
                    first_id,
                    "2024-01-01T00:00:01Z",
                    None,
                    true,
                    Some((first_id, 1, first_len)),
                )],
            );
            let next_first_len = fs::metadata(&first).unwrap().len();
            let next_second_len = fs::metadata(&second).unwrap().len();
            if (next_first_len, next_second_len) == (first_len, second_len) {
                break;
            }
            first_len = next_first_len;
            second_len = next_second_len;
        }

        let adapter = CodexJsonlAdapter::new(home.path());
        let source = adapter
            .discover()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.path == second)
            .unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.records.is_empty());
        assert!(batch.warnings[0].contains("cycle"));
    }

    #[test]
    fn rollout_identity_distinguishes_revert_branches_with_the_same_ordinal() {
        let mut first = NamedTempFile::new().unwrap();
        let mut second = NamedTempFile::new().unwrap();
        for file in [&mut first, &mut second] {
            writeln!(file, "{META}\n{TURN}").unwrap();
            writeln!(file, "{}", token(2, 100, 50)).unwrap();
            file.flush().unwrap();
        }
        let adapter = CodexJsonlAdapter::new("unused");
        let ingest = |file: &NamedTempFile, source_key: &str| {
            adapter
                .ingest(
                    &crate::SourceCandidate {
                        path: file.path().to_owned(),
                        kind: crate::SourceKind::AppendOnlyJsonl,
                        source_key: source_key.to_owned(),
                    },
                    IngestStart::Fresh,
                )
                .unwrap()
        };
        let first_batch = ingest(&first, "rollout-branch-a.jsonl");
        let second_batch = ingest(&second, "rollout-branch-b.jsonl");
        assert_ne!(
            first_batch.records[0].event.id,
            second_batch.records[0].event.id
        );
    }

    fn meta(
        id: &str,
        timestamp: &str,
        forked_from_id: Option<&str>,
        paginated: bool,
        history_base: Option<(&str, u64, u64)>,
    ) -> serde_json::Value {
        let history_base = history_base.map(|(thread_id, ordinal, byte_offset)| {
            serde_json::json!({
                "thread_id": thread_id,
                "end_ordinal_exclusive": ordinal,
                "end_byte_offset": byte_offset
            })
        });
        serde_json::json!({
            "timestamp": timestamp,
            "ordinal": 0,
            "type": "session_meta",
            "payload": {
                "id": id,
                "session_id": id,
                "forked_from_id": forked_from_id,
                "model_provider": "openai",
                "history_mode": if paginated { "paginated" } else { "legacy" },
                "history_base": history_base
            }
        })
    }

    fn total_json(ordinal: u64, timestamp: &str, input: u64, output: u64) -> serde_json::Value {
        serde_json::json!({
            "timestamp": timestamp,
            "ordinal": ordinal,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {"total_token_usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "total_tokens": input + output
                }}
            }
        })
    }

    fn write_lines(path: &Path, lines: &[serde_json::Value]) {
        let contents = lines
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path, format!("{contents}\n")).unwrap();
    }
}
