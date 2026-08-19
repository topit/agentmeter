use std::{fs, io::Write};

use agentmeter_collectors::{
    CollectorAdapter, IngestBatch, IngestMode, IngestStart, SourceCheckpoint, kimi::KimiWireAdapter,
};
use agentmeter_storage::{
    CheckpointStatus, CrossCheckStatus, Database, IngestRequest, ReferenceExpectation,
    ReferenceKind, SourceRegistration, SourceReportedStatus, WriteMode,
};
use tempfile::tempdir;

const SOURCE_ID: &str = "source-kimi-pipeline";

const METADATA: &str = r#"{"type":"metadata","protocol_version":"1.5","created_at":1782113170000}"#;
const LLM_REQUEST: &str = r#"{"type":"llm.request","kind":"loop","provider":"moonshot","model":"kimi-for-coding","modelAlias":"kimi-code/kimi-for-coding","messageCount":12,"time":1782113170000}"#;
const USAGE_TURN: &str = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":3064,"output":76,"inputCacheRead":14848,"inputCacheCreation":0},"usageScope":"turn","time":1782113184943}"#;
const USAGE_SESSION_SCOPE: &str = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":1000,"output":20,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"session","time":1782113194943}"#;
const STEP_END_DUPLICATE: &str = r#"{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"synthetic-uuid","turnId":"1","step":1,"finishReason":"tool_calls","usage":{"inputOther":3064,"output":76,"inputCacheRead":14848,"inputCacheCreation":0},"messageId":"chatcmpl-synthetic"},"time":1782113184950}"#;
const USAGE_TURN_TWO: &str = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":500,"output":10,"inputCacheRead":2000,"inputCacheCreation":0},"usageScope":"turn","time":1782113294943}"#;

#[test]
fn kimi_wire_appends_into_the_canonical_ledger_exactly_once() {
    let directory = tempdir().unwrap();
    let session_dir = directory
        .path()
        .join("sessions")
        .join("wd_project_ab12cd34ef56")
        .join("session_1a2b3c4d-0506-0708-090a-0b0c0d0e0f10")
        .join("agents")
        .join("main");
    fs::create_dir_all(&session_dir).unwrap();
    let wire = session_dir.join("wire.jsonl");
    fs::write(
        &wire,
        format!(
            "{METADATA}\n{LLM_REQUEST}\n{USAGE_TURN}\n{STEP_END_DUPLICATE}\n{USAGE_SESSION_SCOPE}\n"
        ),
    )
    .unwrap();

    let adapter = KimiWireAdapter::new(directory.path());
    let mut sources = adapter.discover().unwrap();
    assert_eq!(sources.len(), 1);
    let source = sources.remove(0);

    let mut database = Database::open_in_memory().unwrap();
    database
        .register_source(&SourceRegistration {
            installation_id: "installation-kimi-pipeline".into(),
            source_object_id: SOURCE_ID.into(),
            adapter_id: adapter.id().into(),
            platform: "test".into(),
            root_path: "/fixture/home/.kimi-code".into(),
            discovery_method: "fixture".into(),
            native_path: "/fixture/home/.kimi-code/sessions/wd_project_ab12cd34ef56/session_1a2b3c4d-0506-0708-090a-0b0c0d0e0f10/agents/main/wire.jsonl".into(),
            kind: "append_only_jsonl".into(),
        })
        .unwrap();

    let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
    assert_eq!(first.records.len(), 2, "step.end duplicates must not count");
    database
        .apply_ingest(request(first, adapter.parser_version()))
        .unwrap();

    // A crash-truncated tail stays uncommitted and recovers on append.
    {
        let mut file = fs::OpenOptions::new().append(true).open(&wire).unwrap();
        writeln!(file, "{USAGE_TURN_TWO}").unwrap();
    }
    let checkpoint = match database
        .checkpoint_status(SOURCE_ID, adapter.parser_version())
        .unwrap()
    {
        CheckpointStatus::Current(checkpoint) => SourceCheckpoint {
            byte_offset: checkpoint.byte_offset,
            source_len: checkpoint.source_len,
            prefix_fingerprint: checkpoint.prefix_fingerprint,
            parser_state: checkpoint.parser_state,
        },
        other => panic!("expected current checkpoint, got {other:?}"),
    };
    let appended = adapter
        .ingest(&source, IngestStart::Resume(&checkpoint))
        .unwrap();
    assert_eq!(appended.mode, IngestMode::Append);
    assert_eq!(appended.records.len(), 1);
    database
        .apply_ingest(request(appended, adapter.parser_version()))
        .unwrap();

    assert_eq!(database.event_count(SOURCE_ID).unwrap(), 3);
    let daily = database.daily_usage_utc().unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].client, "kimi");
    assert_eq!(daily[0].model, "kimi-for-coding");
    assert_eq!(daily[0].tokens.input, 3064 + 1000 + 500);
    assert_eq!(daily[0].tokens.output, 76 + 20 + 10);
    assert_eq!(daily[0].tokens.cache_read, 14848 + 2000);

    let rebuilt = adapter.ingest(&source, IngestStart::Rebuild).unwrap();
    assert_eq!(rebuilt.mode, IngestMode::Replace);
    database
        .apply_ingest(request(rebuilt, adapter.parser_version()))
        .unwrap();
    assert_eq!(database.event_count(SOURCE_ID).unwrap(), 3);

    // Fixture total: every counted delta summed exactly once.
    let expected_total = 4_564 + 106 + 16_848;
    let report = database
        .reconciliation_report(
            1_782_113_195_000,
            &[ReferenceExpectation {
                adapter_id: adapter.id().into(),
                reference: ReferenceKind::Fixture,
                expected_total_tokens: expected_total,
            }],
        )
        .unwrap();
    assert_eq!(
        report.sources[0].source_reported.status,
        SourceReportedStatus::Unavailable
    );
    assert_eq!(report.reference_checks[0].status, CrossCheckStatus::Match);
}

fn request(batch: IngestBatch, parser_version: u32) -> IngestRequest {
    IngestRequest {
        source_object_id: SOURCE_ID.into(),
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
        observed_at_unix_ms: 1_782_113_195_000,
        records: batch.records,
        warnings: batch.warnings,
    }
}
