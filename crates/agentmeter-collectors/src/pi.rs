//! Incremental collector for official Pi coding-agent session JSONL files.

use std::{
    collections::{BTreeMap, HashMap},
    env,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use agentmeter_core::{
    CostFact, CostKind, DataConfidence, EventProvenance, NanoUsd, TimestampOrigin, TokenBreakdown,
    UsageEvent, UsageRecord,
};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    CollectorAdapter, CollectorError, IngestBatch, IngestMode, IngestStart, SourceCandidate,
    SourceCheckpoint, SourceKind,
    file_support::{checkpoint_continues, ensure_kind, hash_file, hash_prefix, io_error},
};

const PARSER_VERSION: u32 = 2;
#[derive(Clone, Debug)]
pub struct PiJsonlAdapter {
    sessions_root: PathBuf,
}

impl PiJsonlAdapter {
    pub fn new(sessions_root: impl Into<PathBuf>) -> Self {
        Self {
            sessions_root: sessions_root.into(),
        }
    }

    pub fn default_sessions_root() -> Option<PathBuf> {
        env::var_os("PI_CODING_AGENT_SESSION_DIR")
            .filter(|value| !value.is_empty())
            .map(expand_home)
            .or_else(|| {
                env::var_os("PI_CODING_AGENT_DIR")
                    .filter(|value| !value.is_empty())
                    .map(|root| expand_home(root).join("sessions"))
            })
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".pi/agent/sessions"))
            })
            .or_else(|| {
                env::var_os("USERPROFILE")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".pi/agent/sessions"))
            })
    }

    fn inherited_usage(&self, source: &SourceCandidate) -> (InheritedUsage, Vec<String>) {
        let Some(header) = read_header(&source.path) else {
            return (InheritedUsage::default(), Vec::new());
        };
        let Some(parent_session) = header.parent_session else {
            return (InheritedUsage::default(), Vec::new());
        };
        let parent_name = Path::new(&parent_session).file_name();
        let parent = self.discover().ok().and_then(|sources| {
            sources.into_iter().find(|candidate| {
                candidate.path != source.path && candidate.path.file_name() == parent_name
            })
        });
        let Some(parent) = parent else {
            return (
                InheritedUsage {
                    unresolved: true,
                    ..InheritedUsage::default()
                },
                vec!["Pi parent session is unavailable; copied usage was retained".to_owned()],
            );
        };
        match read_usage_signatures(&parent.path) {
            Ok(entries) => (
                InheritedUsage {
                    entries,
                    unresolved: false,
                },
                Vec::new(),
            ),
            Err(message) => (
                InheritedUsage {
                    unresolved: true,
                    ..InheritedUsage::default()
                },
                vec![format!(
                    "Pi parent session could not be reconciled ({message}); copied usage was retained"
                )],
            ),
        }
    }
}

impl CollectorAdapter for PiJsonlAdapter {
    fn id(&self) -> &'static str {
        "pi-session-jsonl"
    }

    fn parser_version(&self) -> u32 {
        PARSER_VERSION
    }

    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError> {
        if !self.sessions_root.exists() {
            return Ok(Vec::new());
        }
        let mut pending = vec![self.sessions_root.clone()];
        let mut sources = BTreeMap::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let file_type = entry.file_type().map_err(io_error)?;
                if file_type.is_dir() {
                    pending.push(entry.path());
                } else if file_type.is_file()
                    && entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl")
                {
                    let file_name = entry.file_name().to_string_lossy().into_owned();
                    let source_key = read_header(&entry.path())
                        .map(|header| format!("session:{}:{file_name}", header.id))
                        .unwrap_or(file_name);
                    sources.insert(
                        source_key.clone(),
                        SourceCandidate {
                            path: entry.path(),
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
        let modified_unix_ms = modified_unix_ms(&metadata)?;
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
        let (inherited, mut warnings) = self.inherited_usage(source);
        state.lineage_unresolved |= inherited.unresolved;

        let mut reader = BufReader::new(File::open(&source.path).map_err(io_error)?);
        reader
            .seek(SeekFrom::Start(start_offset))
            .map_err(io_error)?;
        let mut committed_offset = start_offset;
        let mut records = Vec::new();
        loop {
            let record_offset = reader.stream_position().map_err(io_error)?;
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).map_err(io_error)?;
            if bytes_read == 0 {
                break;
            }
            if !line.ends_with('\n') {
                warnings.push(format!(
                    "incomplete trailing Pi record at byte {record_offset}; retrying after append"
                ));
                break;
            }
            committed_offset = reader.stream_position().map_err(io_error)?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<PiEntry>(&line) {
                Ok(entry) => {
                    if let Some(record) = process_entry(
                        entry,
                        source,
                        record_offset,
                        modified_unix_ms,
                        &inherited,
                        &mut state,
                        &mut warnings,
                    ) {
                        records.push(record);
                    }
                }
                Err(error) => warnings.push(format!(
                    "malformed Pi record at byte {record_offset}: {error}"
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
#[serde(default, rename_all = "camelCase")]
struct PiEntry {
    #[serde(rename = "type")]
    kind: String,
    version: Option<u64>,
    id: Option<String>,
    timestamp: Option<String>,
    parent_session: Option<String>,
    message: Option<PiMessage>,
    usage: Option<RawUsage>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PiMessage {
    role: String,
    provider: Option<String>,
    model: Option<String>,
    response_model: Option<String>,
    timestamp: Option<i64>,
    usage: Option<RawUsage>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawUsage {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    cache_write_1h: i64,
    reasoning: i64,
    total_tokens: i64,
    cost: Option<RawCost>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawCost {
    input: Option<serde_json::Number>,
    output: Option<serde_json::Number>,
    cache_read: Option<serde_json::Number>,
    cache_write: Option<serde_json::Number>,
    total: Option<serde_json::Number>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsageSignature {
    kind: String,
    usage: RawUsage,
    provider: Option<String>,
    model: Option<String>,
    timestamp: Option<i64>,
}

#[derive(Default)]
struct InheritedUsage {
    entries: HashMap<String, UsageSignature>,
    unresolved: bool,
}

#[derive(Default)]
struct Header {
    id: String,
    parent_session: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ParserState {
    session_id: Option<String>,
    header_seen: bool,
    schema_supported: bool,
    lineage_unresolved: bool,
    schema_version: u64,
}

fn process_entry(
    entry: PiEntry,
    source: &SourceCandidate,
    record_offset: u64,
    modified_unix_ms: i64,
    inherited: &InheritedUsage,
    state: &mut ParserState,
    warnings: &mut Vec<String>,
) -> Option<UsageRecord> {
    if entry.kind == "session" {
        if state.header_seen {
            warnings.push(format!(
                "duplicate Pi session header at byte {record_offset}; ignored"
            ));
            return None;
        }
        state.header_seen = true;
        state.schema_version = entry.version.unwrap_or(1);
        state.schema_supported = state.schema_version <= 3;
        state.session_id = entry.id.filter(|value| !value.is_empty());
        if !state.schema_supported {
            warnings.push(format!(
                "Pi session schema at byte {record_offset} is newer than version 3; usage was skipped"
            ));
        } else if state.session_id.is_none() {
            warnings.push(format!(
                "Pi session header at byte {record_offset} has no id; usage was skipped"
            ));
        }
        return None;
    }
    if !state.header_seen || !state.schema_supported || state.session_id.is_none() {
        if entry_usage(&entry).is_some() {
            warnings.push(format!(
                "Pi usage at byte {record_offset} appears before a supported session header; skipped"
            ));
        }
        return None;
    }

    let (usage, provider, model, message_timestamp) = match entry.kind.as_str() {
        "message" => {
            let message = entry.message.as_ref()?;
            if message.role != "assistant" {
                return None;
            }
            (
                message.usage.clone()?,
                message.provider.clone().filter(|value| !value.is_empty()),
                message
                    .response_model
                    .clone()
                    .filter(|value| !value.is_empty())
                    .or_else(|| message.model.clone().filter(|value| !value.is_empty())),
                message.timestamp,
            )
        }
        "compaction" | "branch_summary" => (entry.usage.clone()?, None, None, None),
        _ => return None,
    };
    let native_id = entry.id.as_deref().filter(|value| !value.is_empty());
    let identity = native_id.map_or_else(
        || {
            warnings.push(format!(
                "legacy Pi usage at byte {record_offset} has no entry id; used offset identity"
            ));
            format!("legacy-offset:{record_offset}")
        },
        str::to_owned,
    );
    let signature = signature(&entry, &usage);
    if let Some(parent_signature) = native_id.and_then(|id| inherited.entries.get(id)) {
        if parent_signature == &signature {
            return None;
        }
        state.lineage_unresolved = true;
        warnings.push(format!(
            "Pi inherited entry at byte {record_offset} changed usage facts; retained as derived"
        ));
    }
    normalize_usage(
        usage,
        provider,
        model,
        message_timestamp,
        entry.timestamp.as_deref(),
        &identity,
        native_id,
        source,
        record_offset,
        modified_unix_ms,
        state,
        warnings,
    )
}

#[allow(clippy::too_many_arguments)]
fn normalize_usage(
    usage: RawUsage,
    provider: Option<String>,
    model: Option<String>,
    message_timestamp: Option<i64>,
    entry_timestamp: Option<&str>,
    identity: &str,
    native_id: Option<&str>,
    source: &SourceCandidate,
    record_offset: u64,
    modified_unix_ms: i64,
    state: &ParserState,
    warnings: &mut Vec<String>,
) -> Option<UsageRecord> {
    let mut derived = state.lineage_unresolved || native_id.is_none();
    let provider_cost = usage
        .cost
        .as_ref()
        .and_then(|cost| normalize_provider_cost(cost, record_offset, warnings));
    let input = nonnegative(usage.input, "input", record_offset, &mut derived, warnings);
    let output_total = nonnegative(
        usage.output,
        "output",
        record_offset,
        &mut derived,
        warnings,
    );
    let cache_read = nonnegative(
        usage.cache_read,
        "cache read",
        record_offset,
        &mut derived,
        warnings,
    );
    let cache_write = nonnegative(
        usage.cache_write,
        "cache write",
        record_offset,
        &mut derived,
        warnings,
    );
    let cache_write_1h = nonnegative(
        usage.cache_write_1h,
        "one-hour cache write",
        record_offset,
        &mut derived,
        warnings,
    );
    if cache_write_1h > cache_write {
        derived = true;
        warnings.push(format!(
            "Pi one-hour cache write exceeds total cache write at byte {record_offset}"
        ));
    }
    let reasoning = nonnegative(
        usage.reasoning,
        "reasoning",
        record_offset,
        &mut derived,
        warnings,
    );
    let output = if reasoning <= output_total {
        output_total - reasoning
    } else {
        derived = true;
        warnings.push(format!(
            "Pi reasoning exceeds output at byte {record_offset}; non-reasoning output was clamped to zero"
        ));
        0
    };
    let tokens = TokenBreakdown {
        input,
        output,
        cache_read,
        cache_write,
        reasoning,
    };
    let Some(canonical_total) = tokens.checked_total() else {
        warnings.push(format!(
            "Pi usage at byte {record_offset} overflows the canonical total; skipped"
        ));
        return None;
    };
    if canonical_total == 0 {
        warnings.push(format!(
            "Pi usage at byte {record_offset} reports no component tokens; skipped"
        ));
        return None;
    }
    let source_total = u64::try_from(usage.total_tokens)
        .ok()
        .filter(|value| *value != 0);
    if source_total.is_some_and(|total| total != canonical_total) {
        derived = true;
        warnings.push(format!(
            "Pi reported total disagrees with component tokens at byte {record_offset}"
        ));
    }
    let (occurred_at_unix_ms, timestamp_origin) = if let Some(timestamp) = message_timestamp {
        (timestamp, TimestampOrigin::Source)
    } else if let Some(timestamp) = entry_timestamp.and_then(parse_rfc3339_millis) {
        (timestamp, TimestampOrigin::Source)
    } else {
        warnings.push(format!(
            "Pi usage at byte {record_offset} has no valid timestamp; used file modification time"
        ));
        (modified_unix_ms, TimestampOrigin::FileModified)
    };
    let session_id = state.session_id.clone();
    Some(UsageRecord {
        event: UsageEvent {
            id: format!("pi-v1:{}:{identity}", source.source_key),
            source_id: String::new(),
            session_id,
            client: "pi".to_owned(),
            provider,
            model: model.unwrap_or_else(|| "unknown".to_owned()),
            occurred_at_unix_ms,
            tokens,
            source_reported_total: source_total,
            confidence: if derived {
                DataConfidence::Derived
            } else {
                DataConfidence::Exact
            },
        },
        costs: provider_cost.into_iter().collect(),
        provenance: EventProvenance {
            native_id: native_id.map(str::to_owned),
            record_offset: Some(record_offset),
            schema_variant: format!("pi-session-jsonl-v{}", state.schema_version),
            timestamp_origin,
            normalization_notes: vec![format!(
                "raw Pi usage: input={}, output={}, cache_read={}, cache_write={}, reasoning={}, total={}",
                usage.input,
                usage.output,
                usage.cache_read,
                usage.cache_write,
                usage.reasoning,
                usage.total_tokens
            )],
        },
    })
}

fn normalize_provider_cost(
    raw: &RawCost,
    record_offset: u64,
    warnings: &mut Vec<String>,
) -> Option<CostFact> {
    let mut components = Vec::new();
    let mut components_valid = true;
    for (name, value) in [
        ("input", raw.input.as_ref()),
        ("output", raw.output.as_ref()),
        ("cache read", raw.cache_read.as_ref()),
        ("cache write", raw.cache_write.as_ref()),
    ] {
        if let Some(value) = value {
            match NanoUsd::parse_decimal(&value.to_string()) {
                Ok(value) => components.push(value),
                Err(error) => {
                    components_valid = false;
                    warnings.push(format!(
                        "Pi provider-reported {name} cost at byte {record_offset} is invalid ({error:?}); cost bucket was not used"
                    ));
                }
            }
        }
    }
    let component_total = components
        .iter()
        .copied()
        .try_fold(NanoUsd::from_nanos(0), NanoUsd::checked_add);
    if components_valid && component_total.is_none() {
        warnings.push(format!(
            "Pi provider-reported cost components overflow at byte {record_offset}; cost was not retained"
        ));
    }

    let usd = if let Some(total) = raw.total.as_ref() {
        let total = match NanoUsd::parse_decimal(&total.to_string()) {
            Ok(total) => total,
            Err(error) => {
                warnings.push(format!(
                    "Pi provider-reported total cost at byte {record_offset} is invalid ({error:?}); cost was not retained"
                ));
                return None;
            }
        };
        if components_valid
            && !components.is_empty()
            && component_total.is_some_and(|component_total| component_total != total)
        {
            warnings.push(format!(
                "Pi provider-reported total cost disagrees with cost components at byte {record_offset}; retained the source total"
            ));
        }
        total
    } else if components_valid && !components.is_empty() {
        component_total?
    } else {
        warnings.push(format!(
            "Pi provider-reported cost at byte {record_offset} has no valid total; cost was not retained"
        ));
        return None;
    };

    Some(CostFact {
        kind: CostKind::ProviderReported,
        usd: Some(usd),
        confidence: DataConfidence::Exact,
    })
}

fn nonnegative(
    value: i64,
    name: &str,
    record_offset: u64,
    derived: &mut bool,
    warnings: &mut Vec<String>,
) -> u64 {
    u64::try_from(value).unwrap_or_else(|_| {
        *derived = true;
        warnings.push(format!(
            "Pi {name} tokens are negative at byte {record_offset}; clamped to zero"
        ));
        0
    })
}

fn signature(entry: &PiEntry, usage: &RawUsage) -> UsageSignature {
    let message = entry.message.as_ref();
    UsageSignature {
        kind: entry.kind.clone(),
        usage: usage.clone(),
        provider: message.and_then(|value| value.provider.clone()),
        model: message
            .and_then(|value| value.response_model.clone().or_else(|| value.model.clone())),
        timestamp: message.and_then(|value| value.timestamp),
    }
}

fn entry_usage(entry: &PiEntry) -> Option<&RawUsage> {
    match entry.kind.as_str() {
        "message" if entry.message.as_ref()?.role == "assistant" => {
            entry.message.as_ref()?.usage.as_ref()
        }
        "compaction" | "branch_summary" => entry.usage.as_ref(),
        _ => None,
    }
}

fn read_header(path: &Path) -> Option<Header> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let entry: PiEntry = serde_json::from_str(&line).ok()?;
        if entry.kind != "session" {
            return None;
        }
        return Some(Header {
            id: entry.id.filter(|value| !value.is_empty())?,
            parent_session: entry.parent_session,
        });
    }
    None
}

fn read_usage_signatures(path: &Path) -> Result<HashMap<String, UsageSignature>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut entries = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: PiEntry = serde_json::from_str(&line)
            .map_err(|error| format!("malformed parent record: {error}"))?;
        if let (Some(id), Some(usage)) = (entry.id.as_ref(), entry_usage(&entry)) {
            entries.insert(id.clone(), signature(&entry, usage));
        }
    }
    Ok(entries)
}

fn decode_state(bytes: &[u8]) -> Result<ParserState, CollectorError> {
    if bytes.is_empty() {
        Ok(ParserState::default())
    } else {
        serde_json::from_slice(bytes)
            .map_err(|error| CollectorError::new(format!("invalid Pi parser state: {error}")))
    }
}

fn expand_home(value: impl AsRef<std::ffi::OsStr>) -> PathBuf {
    let path = PathBuf::from(value.as_ref());
    let text = path.to_string_lossy();
    let suffix = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\"));
    suffix
        .and_then(|suffix| {
            env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(suffix))
        })
        .unwrap_or(path)
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

    use tempfile::TempDir;

    use super::{PiJsonlAdapter, RawCost, normalize_provider_cost};
    use crate::{CollectorAdapter, IngestMode, IngestStart};

    #[test]
    fn discovers_nested_sessions_with_header_identity() {
        let root = TempDir::new().unwrap();
        let directory = root.path().join("--fixture-project--");
        fs::create_dir_all(&directory).unwrap();
        write_lines(
            &directory.join("synthetic.jsonl"),
            &[header("session-synthetic-pi-001", None, 3)],
        );
        fs::write(directory.join("ignored.txt"), "").unwrap();

        let sources = PiJsonlAdapter::new(root.path()).discover().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].source_key,
            "session:session-synthetic-pi-001:synthetic.jsonl"
        );
    }

    #[test]
    fn discovers_retained_flat_and_current_project_nested_sessions() {
        let root = TempDir::new().unwrap();
        let nested = root.path().join("--fixture-project--");
        fs::create_dir_all(&nested).unwrap();
        write_lines(
            &root.path().join("legacy-flat.jsonl"),
            &[header("session-synthetic-flat", None, 1)],
        );
        write_lines(
            &nested.join("current-nested.jsonl"),
            &[header("session-synthetic-nested", None, 3)],
        );

        let sources = PiJsonlAdapter::new(root.path()).discover().unwrap();
        assert_eq!(sources.len(), 2);
        assert!(
            sources
                .iter()
                .any(|source| source.path.parent() == Some(root.path()))
        );
        assert!(
            sources
                .iter()
                .any(|source| source.path.parent() == Some(nested.as_path()))
        );
    }

    #[test]
    fn normalizes_assistant_and_summary_usage_without_content() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("session.jsonl");
        write_lines(
            &path,
            &[
                header("session-synthetic-pi-002", None, 3),
                assistant("entry-a001", 100, 30, 40, 10, 8, 180),
                serde_json::json!({
                    "type":"compaction","id":"entry-c001","parentId":"entry-a001",
                    "timestamp":"2024-01-01T00:01:00Z",
                    "usage":{"input":20,"output":5,"cacheRead":0,"cacheWrite":0,"totalTokens":25}
                }),
                serde_json::json!({
                    "type":"message","id":"entry-user","timestamp":"2024-01-01T00:02:00Z",
                    "message":{"role":"user"}
                }),
            ],
        );
        let adapter = PiJsonlAdapter::new(root.path());
        let source = adapter.discover().unwrap().pop().unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.warnings.is_empty());
        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].costs[0].usd.unwrap().as_nanos(), 0);
        let assistant = &batch.records[0].event;
        assert_eq!(
            assistant.session_id.as_deref(),
            Some("session-synthetic-pi-002")
        );
        assert_eq!(assistant.provider.as_deref(), Some("openrouter"));
        assert_eq!(assistant.model, "model-response-synthetic");
        assert_eq!(assistant.tokens.input, 100);
        assert_eq!(assistant.tokens.output, 22);
        assert_eq!(assistant.tokens.reasoning, 8);
        assert_eq!(assistant.tokens.cache_read, 40);
        assert_eq!(assistant.tokens.cache_write, 10);
        assert_eq!(assistant.occurred_at_unix_ms, 1_704_067_202_000);
        assert_eq!(batch.records[1].event.model, "unknown");
        assert_eq!(batch.records[1].event.tokens.input, 20);
    }

    #[test]
    fn fork_suppresses_copied_usage_and_keeps_child_entries() {
        let root = TempDir::new().unwrap();
        let directory = root.path().join("--fixture-project--");
        fs::create_dir_all(&directory).unwrap();
        let parent_name = "2024-01-01_parent.jsonl";
        let parent = directory.join(parent_name);
        let inherited = assistant("entry-shared", 100, 20, 0, 0, 0, 120);
        write_lines(
            &parent,
            &[
                header("session-synthetic-parent", None, 3),
                inherited.clone(),
            ],
        );
        let child = directory.join("2024-01-02_child.jsonl");
        write_lines(
            &child,
            &[
                header(
                    "session-synthetic-child",
                    Some(&format!("/fixture/home/pi/{parent_name}")),
                    3,
                ),
                inherited,
                assistant("entry-child", 30, 5, 0, 0, 0, 35),
            ],
        );

        let adapter = PiJsonlAdapter::new(root.path());
        let source = adapter
            .discover()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.path == child)
            .unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.warnings.is_empty());
        assert_eq!(batch.records.len(), 1);
        assert_eq!(
            batch.records[0].provenance.native_id.as_deref(),
            Some("entry-child")
        );
        assert_eq!(batch.records[0].event.tokens.checked_total(), Some(35));
        assert_eq!(batch.records[0].costs[0].usd.unwrap().as_nanos(), 0);
    }

    #[test]
    fn provider_cost_prefers_total_and_can_sum_components_exactly() {
        let with_total: RawCost = serde_json::from_value(serde_json::json!({
            "input": 0.001, "output": 0.002, "cacheRead": 0.003,
            "cacheWrite": 0.004, "total": 0.01
        }))
        .unwrap();
        let mut warnings = Vec::new();
        let fact = normalize_provider_cost(&with_total, 10, &mut warnings).unwrap();
        assert_eq!(fact.kind, agentmeter_core::CostKind::ProviderReported);
        assert_eq!(fact.usd.unwrap().as_nanos(), 10_000_000);
        assert!(warnings.is_empty());

        let components_only: RawCost = serde_json::from_value(serde_json::json!({
            "input": 0.000000001, "output": 0.000000002
        }))
        .unwrap();
        let fact = normalize_provider_cost(&components_only, 20, &mut warnings).unwrap();
        assert_eq!(fact.usd.unwrap().as_nanos(), 3);
    }

    #[test]
    fn invalid_provider_cost_is_diagnosed_without_fabricating_a_fact() {
        let invalid: RawCost = serde_json::from_value(serde_json::json!({
            "total": -0.01
        }))
        .unwrap();
        let mut warnings = Vec::new();
        assert!(normalize_provider_cost(&invalid, 10, &mut warnings).is_none());
        assert!(warnings.iter().any(|warning| warning.contains("Negative")));

        let too_precise: RawCost = serde_json::from_value(serde_json::json!({
            "total": 0.0000000001
        }))
        .unwrap();
        assert!(normalize_provider_cost(&too_precise, 20, &mut warnings).is_none());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("TooPrecise"))
        );
    }

    #[test]
    fn resumes_complete_lines_and_rebuilds_after_rewrite() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("session.jsonl");
        write_lines(&path, &[header("session-synthetic-resume", None, 3)]);
        let adapter = PiJsonlAdapter::new(root.path());
        let source = adapter.discover().unwrap().pop().unwrap();
        let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", assistant("entry-a", 10, 5, 0, 0, 0, 15)).unwrap();
        file.flush().unwrap();
        let resumed = adapter
            .ingest(&source, IngestStart::Resume(&first.checkpoint))
            .unwrap();
        assert_eq!(resumed.mode, IngestMode::Append);
        assert_eq!(resumed.records.len(), 1);

        write_lines(
            &path,
            &[
                header("session-synthetic-resume", None, 3),
                assistant("entry-b", 20, 5, 0, 0, 0, 25),
            ],
        );
        let rebuilt = adapter
            .ingest(&source, IngestStart::Resume(&resumed.checkpoint))
            .unwrap();
        assert_eq!(rebuilt.mode, IngestMode::Replace);
        assert_eq!(
            rebuilt.records[0].provenance.native_id.as_deref(),
            Some("entry-b")
        );
    }

    #[test]
    fn diagnoses_schema_drift_malformed_and_incomplete_records() {
        let root = TempDir::new().unwrap();
        let newer = root.path().join("newer.jsonl");
        write_lines(
            &newer,
            &[
                header("session-synthetic-newer", None, 4),
                assistant("entry-a", 10, 5, 0, 0, 0, 15),
            ],
        );
        let adapter = PiJsonlAdapter::new(root.path());
        let source = adapter
            .discover()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.path == newer)
            .unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.records.is_empty());
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("newer"))
        );

        let malformed = root.path().join("malformed.jsonl");
        let mut file = fs::File::create(&malformed).unwrap();
        writeln!(file, "{}", header("session-synthetic-malformed", None, 3)).unwrap();
        writeln!(file, "{{\"type\":}}").unwrap();
        write!(file, "{}", assistant("entry-tail", 10, 5, 0, 0, 0, 15)).unwrap();
        file.flush().unwrap();
        let source = crate::SourceCandidate {
            path: malformed,
            kind: crate::SourceKind::AppendOnlyJsonl,
            source_key: "session:session-synthetic-malformed".to_owned(),
        };
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert!(batch.records.is_empty());
        assert_eq!(batch.warnings.len(), 2);
    }

    #[test]
    fn missing_parent_and_legacy_identity_lower_confidence() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("legacy.jsonl");
        write_lines(
            &path,
            &[
                header(
                    "session-synthetic-legacy",
                    Some("/fixture/home/pi/missing-parent.jsonl"),
                    1,
                ),
                serde_json::json!({
                    "type":"message","timestamp":"invalid",
                    "message":{"role":"assistant","provider":"provider-synthetic","model":"model-synthetic",
                        "usage":{"input":-1,"output":5,"cacheRead":0,"cacheWrite":0,"totalTokens":5}}
                }),
            ],
        );
        let adapter = PiJsonlAdapter::new(root.path());
        let source = adapter.discover().unwrap().pop().unwrap();
        let batch = adapter.ingest(&source, IngestStart::Fresh).unwrap();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(
            batch.records[0].event.confidence,
            agentmeter_core::DataConfidence::Derived
        );
        assert!(batch.records[0].provenance.native_id.is_none());
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("unavailable"))
        );
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("negative"))
        );
        assert!(
            batch
                .warnings
                .iter()
                .any(|warning| warning.contains("modification time"))
        );
    }

    fn header(id: &str, parent_session: Option<&str>, version: u64) -> serde_json::Value {
        serde_json::json!({
            "type":"session","version":version,"id":id,
            "timestamp":"2024-01-01T00:00:00Z","cwd":"/fixture/project",
            "parentSession":parent_session
        })
    }

    fn assistant(
        id: &str,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
        reasoning: i64,
        total: i64,
    ) -> serde_json::Value {
        serde_json::json!({
            "type":"message","id":id,"parentId":null,"timestamp":"2024-01-01T00:00:02Z",
            "message":{
                "role":"assistant","content":[],"provider":"openrouter",
                "model":"model-request-synthetic","responseModel":"model-response-synthetic",
                "timestamp":1704067202000_i64,
                "usage":{
                    "input":input,"output":output,"cacheRead":cache_read,"cacheWrite":cache_write,
                    "reasoning":reasoning,"totalTokens":total,
                    "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}
                }
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
