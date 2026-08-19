//! Collector for Kimi Code `wire.jsonl` session journals.
//!
//! Two upstream products share this adapter. The frozen Python CLI
//! (`kimi-cli`, `~/.kimi`) wraps every record as
//! `{"timestamp": <unix seconds>, "message": {"type": ..., "payload": ...}}`
//! and reports usage on `StatusUpdate` messages. The current TypeScript CLI
//! (`kimi-code`, `~/.kimi-code`) writes flat
//! `{"type": "<dotted.type>", ..., "time": <unix ms>}` records and reports
//! usage on `usage.record`. Both layouts report one delta per completed LLM
//! request, so records are summed directly without cumulative-to-delta
//! conversion. `step.end` carries a copy of the same per-step usage and is
//! deliberately ignored, as are goal-budget and local context-length
//! estimates. Session- and turn-scoped records are both real spend and are
//! both counted; ccusage and tokscale skip `usageScope: "session"` records,
//! which undercounts compaction and title-generation requests.

use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use agentmeter_core::{
    DataConfidence, EventProvenance, TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use serde::{Deserialize, Serialize};

use crate::{
    CollectorAdapter, CollectorError, IngestBatch, IngestMode, IngestStart, SourceCandidate,
    SourceCheckpoint, SourceKind,
    amp::modified_unix_ms,
    file_support::{
        checkpoint_continues, ensure_kind, expand_home, hash_bytes, hash_file, hash_prefix,
        io_error,
    },
};

const CLIENT: &str = "kimi";
const PARSER_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct KimiWireAdapter {
    root: PathBuf,
}

impl KimiWireAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// One adapter per Kimi data root: `KIMI_CODE_HOME`/`~/.kimi-code` for
    /// the current CLI and `KIMI_SHARE_DIR`/`~/.kimi` for the frozen Python
    /// CLI.
    pub fn default_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let code_root = env::var_os("KIMI_CODE_HOME")
            .filter(|value| !value.is_empty())
            .map(expand_home)
            .or_else(|| home_dir().map(|home| home.join(".kimi-code")));
        if let Some(root) = code_root {
            roots.push(root);
        }
        let share_root = env::var_os("KIMI_SHARE_DIR")
            .filter(|value| !value.is_empty())
            .map(expand_home)
            .or_else(|| home_dir().map(|home| home.join(".kimi")));
        if let Some(root) = share_root {
            roots.push(root);
        }
        roots
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

impl CollectorAdapter for KimiWireAdapter {
    fn id(&self) -> &'static str {
        "kimi-wire"
    }

    fn parser_version(&self) -> u32 {
        PARSER_VERSION
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        let sessions = self.root.join("sessions");
        if !sessions.exists() {
            return Ok(Vec::new());
        }
        let mut pending = vec![sessions];
        let mut sources = BTreeMap::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let file_type = entry.file_type().map_err(io_error)?;
                if file_type.is_dir() {
                    pending.push(entry.path());
                } else if file_type.is_file() && entry.file_name() == "wire.jsonl" {
                    let path = entry.path();
                    let source_key = path
                        .strip_prefix(&self.root)
                        .map(|relative| relative.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
                    sources.insert(
                        source_key.clone(),
                        SourceCandidate {
                            path,
                            kind: SourceKind::AppendOnlyJsonl,
                            source_key,
                        },
                    );
                }
            }
        }
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
                let state = decode_state(&checkpoint.parser_state)?;
                (
                    IngestMode::Append,
                    checkpoint.byte_offset.unwrap_or_default(),
                    state,
                )
            }
            IngestStart::Resume(_) | IngestStart::Rebuild => {
                (IngestMode::Replace, 0, ParserState::default())
            }
            IngestStart::Fresh => (IngestMode::Append, 0, ParserState::default()),
        };

        let session_id = session_identity(&source.path);

        let mut reader = BufReader::new(File::open(&source.path).map_err(io_error)?);
        reader
            .seek(SeekFrom::Start(start_offset))
            .map_err(io_error)?;

        let mut records: Vec<UsageRecord> = Vec::new();
        let mut native_totals: BTreeMap<String, u128> = BTreeMap::new();
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
                    "incomplete trailing Kimi wire record at byte {record_offset}; retrying after append"
                ));
                break;
            }
            committed_offset = reader.stream_position().map_err(io_error)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let parsed = match serde_json::from_str::<WireLine>(trimmed) {
                Ok(WireLine::Legacy(record)) => record.into_usage_record(
                    &source.source_key,
                    &session_id,
                    file_modified_unix_ms,
                    record_offset,
                    &mut warnings,
                ),
                Ok(WireLine::Flat(record)) => {
                    record.update_parser_state(&mut state);
                    record.into_usage_record(
                        &source.source_key,
                        &session_id,
                        file_modified_unix_ms,
                        record_offset,
                        &state,
                        &mut warnings,
                    )
                }
                Err(error) => {
                    warnings.push(format!(
                        "malformed Kimi wire record at byte {record_offset}: {error}"
                    ));
                    None
                }
            };
            if let Some(record) = parsed {
                reconcile_native_duplicate(&mut records, &mut native_totals, record, &mut warnings);
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

/// Kimi may repeat a provider completion id across `StatusUpdate` records;
/// within one batch the record with the largest token total wins and every
/// repeat is reported. Equal repeats are skipped: storage-side identity
/// remains the safety net for repeats that span batches.
fn reconcile_native_duplicate(
    records: &mut Vec<UsageRecord>,
    native_totals: &mut BTreeMap<String, u128>,
    record: UsageRecord,
    warnings: &mut Vec<String>,
) {
    let Some(native_id) = record.provenance.native_id.clone() else {
        records.push(record);
        return;
    };
    let total = u128::from(record.event.tokens.checked_total().unwrap_or(u64::MAX));
    match native_totals.get(&native_id).copied() {
        Some(previous) if previous >= total => warnings.push(format!(
            "Kimi usage for native id {native_id} repeated without larger totals; kept the earlier record"
        )),
        Some(_) => {
            let position = records
                .iter()
                .position(|existing| existing.provenance.native_id.as_deref() == Some(&native_id))
                .expect("a tracked native id must have an in-batch record");
            warnings.push(format!(
                "Kimi usage for native id {native_id} was superseded by a larger total; kept the larger record"
            ));
            records[position] = record;
            native_totals.insert(native_id, total);
        }
        None => {
            native_totals.insert(native_id, total);
            records.push(record);
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireLine {
    Legacy(LegacyRecord),
    Flat(FlatRecord),
}

/// `{"timestamp": <unix seconds>, "message": {"type": "StatusUpdate",
/// "payload": {"token_usage": {...}, "message_id": ...}}}`
#[derive(Deserialize)]
struct LegacyRecord {
    timestamp: f64,
    message: LegacyMessage,
}

#[derive(Deserialize)]
struct LegacyMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Option<StatusUpdatePayload>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StatusUpdatePayload {
    token_usage: Option<SnakeUsage>,
    message_id: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SnakeUsage {
    input_other: u64,
    output: u64,
    input_cache_read: u64,
    input_cache_creation: u64,
}

impl SnakeUsage {
    fn tokens(&self) -> TokenBreakdown {
        TokenBreakdown {
            input: self.input_other,
            output: self.output,
            cache_read: self.input_cache_read,
            cache_write: self.input_cache_creation,
            reasoning: 0,
        }
    }
}

impl LegacyRecord {
    fn into_usage_record(
        self,
        source_key: &str,
        session_id: &Option<String>,
        file_modified_unix_ms: i64,
        record_offset: u64,
        warnings: &mut Vec<String>,
    ) -> Option<UsageRecord> {
        if self.message.kind != "StatusUpdate" {
            return None;
        }
        let payload = self.message.payload.unwrap_or_default();
        let usage = payload.token_usage?;
        let tokens = usage.tokens();
        let total = tokens.checked_total()?;
        if total == 0 {
            warnings.push(format!(
                "Kimi StatusUpdate at byte {record_offset} reports no tokens; skipped"
            ));
            return None;
        }

        let (occurred_at_unix_ms, timestamp_origin, mut notes) =
            seconds_to_unix_ms(self.timestamp, file_modified_unix_ms);
        notes.push("Kimi reports reasoning tokens within output".to_owned());
        notes.push(
            "Kimi legacy wire files carry no model identifier; the event stays unattributed"
                .to_owned(),
        );
        let native_id = payload.message_id.clone().filter(|value| !value.is_empty());
        let identity_material = native_id
            .as_deref()
            .map(|message_id| format!("kimi-wire-v1\0{message_id}"))
            .unwrap_or_else(|| {
                format!(
                    "kimi-wire-fp1\0{source_key}\0{}\0{}\0{}\0{}\0{}",
                    usage.input_other,
                    usage.output,
                    usage.input_cache_read,
                    usage.input_cache_creation,
                    self.timestamp.to_bits(),
                )
            });
        Some(UsageRecord {
            event: UsageEvent {
                id: format!("kimi-wire-v1:{}", hash_bytes(identity_material.as_bytes())),
                source_id: String::new(),
                session_id: session_id.clone(),
                client: CLIENT.to_owned(),
                provider: None,
                model: "unknown".to_owned(),
                occurred_at_unix_ms,
                tokens,
                source_reported_total: None,
                confidence: DataConfidence::Exact,
            },
            costs: Vec::new(),
            provenance: EventProvenance {
                native_id,
                record_offset: Some(record_offset),
                schema_variant: "kimi-wire-v1-status-update".to_owned(),
                timestamp_origin,
                normalization_notes: notes,
            },
        })
    }
}

/// Flat `{"type": "usage.record", "model": ..., "usage": {...},
/// "usageScope": ..., "time": <unix ms>}` records from the current CLI.
#[derive(Deserialize)]
struct FlatRecord {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    time: Option<i64>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<CamelUsage>,
    #[serde(rename = "usageScope", default)]
    usage_scope: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CamelUsage {
    input_other: Option<u64>,
    output: u64,
    input_cache_read: Option<u64>,
    input_cache_creation: Option<u64>,
}

impl CamelUsage {
    fn tokens(&self) -> TokenBreakdown {
        TokenBreakdown {
            input: self.input_other.unwrap_or_default(),
            output: self.output,
            cache_read: self.input_cache_read.unwrap_or_default(),
            cache_write: self.input_cache_creation.unwrap_or_default(),
            reasoning: 0,
        }
    }
}

impl FlatRecord {
    fn update_parser_state(&self, state: &mut ParserState) {
        if self.kind == "llm.request"
            && let Some(model) = self.model.as_deref()
            && let Some(resolved) = resolve_model(model)
        {
            state.last_model = Some(resolved.to_owned());
        }
    }

    fn into_usage_record(
        self,
        source_key: &str,
        session_id: &Option<String>,
        file_modified_unix_ms: i64,
        record_offset: u64,
        state: &ParserState,
        warnings: &mut Vec<String>,
    ) -> Option<UsageRecord> {
        if self.kind != "usage.record" {
            return None;
        }
        let Some(usage) = self.usage else {
            warnings.push(format!(
                "Kimi usage.record at byte {record_offset} has no usage payload; skipped"
            ));
            return None;
        };
        let tokens = usage.tokens();
        let total = tokens.checked_total()?;
        if total == 0 {
            warnings.push(format!(
                "Kimi usage.record at byte {record_offset} reports no tokens; skipped"
            ));
            return None;
        }

        let raw_model = self.model.clone().unwrap_or_default();
        let (model, mut notes) = match resolve_model(&raw_model) {
            Some(model) => (model.to_owned(), Vec::new()),
            None => match &state.last_model {
                Some(last) => (
                    last.clone(),
                    vec![format!(
                        "Kimi usage.record carried symbolic model {raw_model}; used the last concrete llm.request model"
                    )],
                ),
                None => (
                    "unknown".to_owned(),
                    vec![format!(
                        "Kimi usage.record carried symbolic model {raw_model} with no earlier llm.request model"
                    )],
                ),
            },
        };

        let (occurred_at_unix_ms, timestamp_origin, timestamp_notes) = match self.time {
            Some(time) if time > 0 => (time, TimestampOrigin::Source, Vec::new()),
            _ => (
                file_modified_unix_ms,
                TimestampOrigin::FileModified,
                vec![
                    "Kimi usage.record had no usable time; used source file modification time"
                        .to_owned(),
                ],
            ),
        };
        notes.extend(timestamp_notes);
        notes.push("Kimi reports reasoning tokens within output".to_owned());
        let scope = self
            .usage_scope
            .clone()
            .unwrap_or_else(|| "turn".to_owned());
        notes.push(format!("Kimi usage scope: {scope}"));
        if scope != "turn" {
            notes.push(
                "session-scoped Kimi usage covers compaction and title generation and is counted as spend"
                    .to_owned(),
            );
        }

        let identity_material = format!(
            "kimi-wire-fp2\0{source_key}\0{}\0{raw_model}\0{}\0{}\0{}\0{}\0{scope}",
            self.time.unwrap_or_default(),
            usage.input_other.unwrap_or_default(),
            usage.output,
            usage.input_cache_read.unwrap_or_default(),
            usage.input_cache_creation.unwrap_or_default(),
        );
        Some(UsageRecord {
            event: UsageEvent {
                id: format!("kimi-wire-v2:{}", hash_bytes(identity_material.as_bytes())),
                source_id: String::new(),
                session_id: session_id.clone(),
                client: CLIENT.to_owned(),
                provider: None,
                model,
                occurred_at_unix_ms,
                tokens,
                source_reported_total: None,
                confidence: DataConfidence::Exact,
            },
            costs: Vec::new(),
            provenance: EventProvenance {
                native_id: None,
                record_offset: Some(record_offset),
                schema_variant: "kimi-wire-v2-usage-record".to_owned(),
                timestamp_origin,
                normalization_notes: notes,
            },
        })
    }
}

/// Strips the `kimi-code/` alias prefix and rejects symbolic placeholder
/// models such as `__kimi_env_model__`.
fn resolve_model(model: &str) -> Option<&str> {
    let stripped = model.strip_prefix("kimi-code/").unwrap_or(model);
    if stripped.is_empty() || (stripped.starts_with("__") && stripped.ends_with("__")) {
        None
    } else {
        Some(stripped)
    }
}

#[derive(Default, Deserialize, Serialize)]
struct ParserState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_model: Option<String>,
}

fn decode_state(bytes: &[u8]) -> Result<ParserState, CollectorError> {
    if bytes.is_empty() {
        return Ok(ParserState::default());
    }
    serde_json::from_slice(bytes)
        .map_err(|error| CollectorError::new(format!("invalid Kimi parser state: {error}")))
}

fn seconds_to_unix_ms(
    timestamp: f64,
    file_modified_unix_ms: i64,
) -> (i64, TimestampOrigin, Vec<String>) {
    if timestamp.is_finite() && timestamp > 0.0 {
        let millis = timestamp * 1_000.0;
        if (0.0..=i64::MAX as f64).contains(&millis) {
            return (millis.round() as i64, TimestampOrigin::Source, Vec::new());
        }
    }
    (
        file_modified_unix_ms,
        TimestampOrigin::FileModified,
        vec![
            "Kimi StatusUpdate had no usable timestamp; used source file modification time"
                .to_owned(),
        ],
    )
}

/// Session identity from the journal path: `session_<uuid>` directories in
/// the current CLI and bare `<uuid>` directories (including subagents) in
/// the frozen Python CLI.
fn session_identity(path: &Path) -> Option<String> {
    path.ancestors()
        .skip(1)
        .filter_map(|ancestor| ancestor.file_name())
        .find_map(|name| {
            let name = name.to_string_lossy();
            if let Some(uuid) = name.strip_prefix("session_") {
                Some(uuid.to_owned())
            } else if is_uuid_like(&name) {
                Some(name.into_owned())
            } else {
                None
            }
        })
}

fn is_uuid_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
    };

    use agentmeter_core::TimestampOrigin;
    use tempfile::tempdir;

    use super::KimiWireAdapter;
    use crate::{CollectorAdapter, IngestMode, IngestStart};

    const WORKDIR_KEY: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
    const LEGACY_SESSION: &str = "0a1b2c3d-0405-0607-0809-0a0b0c0d0e0f";
    const CODE_SESSION: &str = "1a2b3c4d-0506-0708-090a-0b0c0d0e0f10";
    const LEGACY_STATUS: &str = r#"{"timestamp":1770983410.123,"message":{"type":"StatusUpdate","payload":{"context_usage":0.024,"context_tokens":8000,"max_context_tokens":262144,"token_usage":{"input_other":1508,"output":205,"input_cache_read":4864,"input_cache_creation":0},"message_id":"chatcmpl-2tNw2mhUNfdPMP0Jyie7gDhD"}}}"#;
    const FLAT_METADATA: &str =
        r#"{"type":"metadata","protocol_version":"1.5","created_at":1780319376954}"#;
    const FLAT_LLM_REQUEST: &str = r#"{"type":"llm.request","kind":"loop","provider":"moonshot","model":"kimi-for-coding","modelAlias":"kimi-code/kimi-for-coding","messageCount":12,"time":1782113170000}"#;
    const FLAT_USAGE_TURN: &str = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":3064,"output":76,"inputCacheRead":14848,"inputCacheCreation":0},"usageScope":"turn","time":1782113184943}"#;
    const FLAT_USAGE_SESSION: &str = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":1000,"output":20,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"session","time":1782113194943}"#;
    const FLAT_STEP_END_DUPLICATE: &str = r#"{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"synthetic-uuid","turnId":"3","step":1,"finishReason":"tool_calls","usage":{"inputOther":3064,"output":76,"inputCacheRead":14848,"inputCacheCreation":0},"messageId":"chatcmpl-synthetic"},"time":1782113184950}"#;

    fn adapter_with_legacy_session(content: &str) -> (KimiWireAdapter, tempfile::TempDir) {
        adapter_with_journal(
            &["sessions", WORKDIR_KEY, LEGACY_SESSION],
            "wire.jsonl",
            content,
        )
    }

    fn adapter_with_flat_session(content: &str) -> (KimiWireAdapter, tempfile::TempDir) {
        adapter_with_journal(
            &[
                "sessions",
                "wd_project_ab12cd34ef56",
                &format!("session_{CODE_SESSION}"),
                "agents",
                "main",
            ],
            "wire.jsonl",
            content,
        )
    }

    fn adapter_with_journal(
        directory_parts: &[&str],
        file_name: &str,
        content: &str,
    ) -> (KimiWireAdapter, tempfile::TempDir) {
        let directory = tempdir().unwrap();
        let parts: Vec<&str> = directory
            .path()
            .to_str()
            .into_iter()
            .chain(directory_parts.iter().copied())
            .collect();
        let directory_path: PathBuf = parts.iter().collect();
        let journal = directory_path.join(file_name);
        fs::create_dir_all(&directory_path).unwrap();
        fs::write(&journal, content).unwrap();
        (KimiWireAdapter::new(directory.path()), directory)
    }

    fn write_in_root(root: &Path, relative: &[&str], content: &str) {
        let parts: Vec<&str> = root
            .to_str()
            .into_iter()
            .chain(relative.iter().copied())
            .collect();
        let path: PathBuf = parts.iter().collect();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn single_source(adapter: &KimiWireAdapter) -> crate::SourceCandidate {
        let mut sources = adapter.discover().unwrap();
        assert_eq!(sources.len(), 1);
        sources.remove(0)
    }

    #[test]
    fn parses_legacy_status_update_usage_with_native_identity() {
        let (adapter, _directory) = adapter_with_legacy_session(&format!(
            "{{\"type\":\"metadata\",\"protocol_version\":\"1.10\"}}\n{LEGACY_STATUS}\n"
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        let record = &batch.records[0];
        assert_eq!(record.event.client, "kimi");
        assert_eq!(record.event.model, "unknown");
        assert_eq!(
            record.event.session_id.as_deref(),
            Some(LEGACY_SESSION),
            "legacy session identity comes from the session directory"
        );
        assert_eq!(record.event.tokens.input, 1508);
        assert_eq!(record.event.tokens.output, 205);
        assert_eq!(record.event.tokens.cache_read, 4864);
        assert_eq!(record.event.tokens.cache_write, 0);
        assert_eq!(record.event.tokens.reasoning, 0);
        assert_eq!(record.event.occurred_at_unix_ms, 1_770_983_410_123);
        assert_eq!(record.provenance.timestamp_origin, TimestampOrigin::Source);
        assert_eq!(
            record.provenance.native_id.as_deref(),
            Some("chatcmpl-2tNw2mhUNfdPMP0Jyie7gDhD")
        );
        assert_eq!(
            record.provenance.schema_variant,
            "kimi-wire-v1-status-update"
        );
        assert!(batch.warnings.is_empty());
    }

    #[test]
    fn ignores_legacy_events_without_usage() {
        let (adapter, _directory) = adapter_with_legacy_session(&format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"type":"metadata","protocol_version":"1.10"}"#,
            r#"{"timestamp":1770983400.0,"message":{"type":"TurnBegin","payload":{}}}"#,
            r#"{"timestamp":1770983410.0,"message":{"type":"StatusUpdate","payload":{"context_usage":0.5}}}"#,
            r#"{"timestamp":1770983420.0,"message":{"type":"TurnEnd","payload":{}}}"#
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert!(batch.records.is_empty());
        assert!(batch.warnings.is_empty());
    }

    #[test]
    fn parses_flat_usage_records_and_skips_duplicates() {
        let (adapter, _directory) = adapter_with_flat_session(&format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            FLAT_METADATA,
            FLAT_LLM_REQUEST,
            FLAT_USAGE_TURN,
            FLAT_STEP_END_DUPLICATE,
            FLAT_USAGE_SESSION,
            r#"{"type":"turn.ended","turnId":3,"reason":"completed","durationMs":45320,"time":1782113231000}"#,
            r#"{"type":"goal.update","tokensUsed":12345,"time":1782113231001}"#,
            r#"{"type":"token_counting.measured","estimate":8000,"time":1782113231002}"#
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(
            batch.records.len(),
            2,
            "step.end, goal, and token_counting must not count"
        );
        let turn = &batch.records[0];
        assert_eq!(turn.event.model, "kimi-for-coding");
        assert_eq!(turn.event.tokens.input, 3064);
        assert_eq!(turn.event.tokens.output, 76);
        assert_eq!(turn.event.tokens.cache_read, 14848);
        assert_eq!(turn.event.occurred_at_unix_ms, 1_782_113_184_943);
        assert!(
            turn.provenance
                .normalization_notes
                .iter()
                .any(|note| note.contains("usage scope: turn"))
        );
        let session = &batch.records[1];
        assert_eq!(session.event.tokens.input, 1000);
        assert!(
            session
                .provenance
                .normalization_notes
                .iter()
                .any(|note| note.contains("counted as spend"))
        );
        assert_eq!(
            session.event.session_id.as_deref(),
            Some(CODE_SESSION),
            "session_ prefix is stripped for session identity"
        );
        assert!(batch.warnings.is_empty());
    }

    #[test]
    fn symbolic_models_fall_back_to_the_last_concrete_request_model() {
        let (adapter, _directory) = adapter_with_flat_session(&format!(
            "{FLAT_LLM_REQUEST}\n{}\n",
            r#"{"type":"usage.record","model":"__kimi_env_model__","usage":{"inputOther":10,"output":2,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1782113184943}"#
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.model, "kimi-for-coding");
        assert!(
            batch.records[0]
                .provenance
                .normalization_notes
                .iter()
                .any(|note| note.contains("symbolic model"))
        );
    }

    #[test]
    fn unusable_timestamps_fall_back_to_file_modification_time() {
        let (adapter, _directory) = adapter_with_flat_session(&format!(
            "{}\n",
            r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":5,"output":1,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":-1}"#
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        assert_eq!(
            batch.records[0].provenance.timestamp_origin,
            TimestampOrigin::FileModified
        );
        assert!(
            batch.records[0]
                .provenance
                .normalization_notes
                .iter()
                .any(|note| note.contains("file modification time"))
        );
    }

    #[test]
    fn duplicate_native_ids_keep_the_larger_total() {
        let (adapter, _directory) = adapter_with_legacy_session(&format!(
            "{LEGACY_STATUS}\n{}\n{}\n",
            r#"{"timestamp":1770983410.5,"message":{"type":"StatusUpdate","payload":{"token_usage":{"input_other":3000,"output":400,"input_cache_read":9000,"input_cache_creation":0},"message_id":"chatcmpl-2tNw2mhUNfdPMP0Jyie7gDhD"}}}"#,
            r#"{"timestamp":1770983411.0,"message":{"type":"StatusUpdate","payload":{"token_usage":{"input_other":10,"output":1,"input_cache_read":2,"input_cache_creation":0},"message_id":"chatcmpl-2tNw2mhUNfdPMP0Jyie7gDhD"}}}"#
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].event.tokens.input, 3000);
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("superseded by a larger total"))
        );
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("kept the earlier record"))
        );
    }

    #[test]
    fn equal_repeats_are_skipped_with_a_warning() {
        let (adapter, _directory) =
            adapter_with_legacy_session(&format!("{LEGACY_STATUS}\n{LEGACY_STATUS}\n"));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert_eq!(batch.records.len(), 1);
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("kept the earlier record"))
        );
    }

    #[test]
    fn skips_zero_usage_and_reports_malformed_lines() {
        let (adapter, _directory) = adapter_with_flat_session(&format!(
            "{{\"not json\"\n{}\n{}\n",
            r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":0,"output":0,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1782113184943}"#,
            r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usageScope":"turn","time":1782113184944}"#
        ));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert!(batch.records.is_empty());
        assert_eq!(batch.warnings.len(), 3);
        assert!(batch.warnings[0].contains("malformed Kimi wire record"));
        assert!(batch.warnings[1].contains("reports no tokens"));
        assert!(batch.warnings[2].contains("has no usage payload"));
    }

    #[test]
    fn retries_incomplete_tails_and_checkpoints_newline_boundaries() {
        let (adapter, _directory) =
            adapter_with_flat_session(&format!("{FLAT_LLM_REQUEST}\n{FLAT_USAGE_TURN}"));

        let source = single_source(&adapter);
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        assert!(batch.records.is_empty());
        assert_eq!(batch.warnings.len(), 1);
        assert!(batch.warnings[0].contains("incomplete trailing"));
        assert!(batch.checkpoint.byte_offset.unwrap() < batch.checkpoint.source_len);
    }

    #[test]
    fn resumes_append_after_new_data_and_replaces_after_rewrite() {
        let (adapter, _directory) = adapter_with_flat_session(&format!(
            "{FLAT_METADATA}\n{FLAT_LLM_REQUEST}\n{FLAT_USAGE_TURN}\n"
        ));
        let source = single_source(&adapter);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(first.records.len(), 1);

        let wire = source.path.clone();
        let flat_extra = FLAT_USAGE_TURN.replace("1782113184943", "1782113284943");
        {
            let mut file = fs::OpenOptions::new().append(true).open(&wire).unwrap();
            writeln!(file, "{flat_extra}").unwrap();
        }
        let appended = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(appended.mode, IngestMode::Append);
        assert_eq!(appended.records.len(), 1);
        assert_ne!(first.records[0].event.id, appended.records[0].event.id);

        // A protocol-migration rewrite changes earlier records, so the
        // checkpointed prefix no longer matches and the source rebuilds.
        let migrated_metadata =
            r#"{"type":"metadata","protocol_version":"1.6","created_at":1780319376954}"#;
        let rewritten =
            format!("{migrated_metadata}\n{FLAT_LLM_REQUEST}\n{FLAT_USAGE_TURN}\n{flat_extra}\n");
        fs::write(&wire, rewritten).unwrap();
        let replaced = adapter
            .ingest(&source, IngestStart::Resume(&appended.checkpoint))
            .unwrap();
        assert_eq!(replaced.mode, IngestMode::Replace);
        assert_eq!(replaced.records.len(), 2);
    }

    #[test]
    fn resumes_parser_state_across_appends() {
        let (adapter, _directory) = adapter_with_flat_session(&format!("{FLAT_LLM_REQUEST}\n"));
        let source = single_source(&adapter);
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();

        let wire = source.path.clone();
        {
            let mut file = fs::OpenOptions::new().append(true).open(&wire).unwrap();
            writeln!(
                file,
                r#"{{"type":"usage.record","model":"__kimi_env_model__","usage":{{"inputOther":10,"output":2,"inputCacheRead":0,"inputCacheCreation":0}},"usageScope":"turn","time":1782113184943}}"#
            )
            .unwrap();
        }
        let appended = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();

        assert_eq!(appended.records.len(), 1);
        assert_eq!(
            appended.records[0].event.model, "kimi-for-coding",
            "the checkpointed llm.request model must survive the append boundary"
        );
    }

    #[test]
    fn discovers_legacy_subagent_journals_with_session_identity() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        write_in_root(
            root,
            &[
                "sessions",
                WORKDIR_KEY,
                LEGACY_SESSION,
                "subagents",
                "agent-synthetic-001",
                "wire.jsonl",
            ],
            &format!("{LEGACY_STATUS}\n"),
        );
        write_in_root(
            root,
            &["sessions", WORKDIR_KEY, LEGACY_SESSION, "context.jsonl"],
            "synthetic\n",
        );

        let adapter = KimiWireAdapter::new(root);
        let mut sources = adapter.discover().unwrap();

        assert_eq!(
            sources.len(),
            1,
            "only wire.jsonl journals are sources; context.jsonl is not"
        );
        let source = sources.remove(0);
        assert_eq!(
            source.source_key,
            format!(
                "sessions/{WORKDIR_KEY}/{LEGACY_SESSION}/subagents/agent-synthetic-001/wire.jsonl"
            )
        );
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(
            batch.records[0].event.session_id.as_deref(),
            Some(LEGACY_SESSION)
        );
    }
}
