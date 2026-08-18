use std::{fs, io::Write};

use agentmeter_collectors::{
    CollectorAdapter, IngestBatch, IngestMode, IngestStart, SourceCandidate, SourceCheckpoint,
    SourceKind, codex::CodexJsonlAdapter,
};
use agentmeter_storage::{
    CheckpointStatus, Database, IngestRequest, SourceRegistration, WriteMode,
};
use tempfile::{NamedTempFile, TempDir};

const SOURCE_ID: &str = "source-codex-pipeline";

#[test]
fn codex_cumulative_state_resumes_into_the_canonical_ledger() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, r#"{{"timestamp":"2024-01-01T00:00:00Z","ordinal":0,"type":"session_meta","payload":{{"id":"thread-synthetic-codex","model_provider":"openai"}}}}"#).unwrap();
    writeln!(file, r#"{{"timestamp":"2024-01-01T00:00:01Z","ordinal":1,"type":"turn_context","payload":{{"model":"gpt-synthetic"}}}}"#).unwrap();
    write_total(&mut file, 2, 100, 20);

    let adapter = CodexJsonlAdapter::new("unused");
    let source = SourceCandidate {
        path: file.path().to_owned(),
        kind: SourceKind::AppendOnlyJsonl,
        source_key: "rollout-synthetic-codex.jsonl".into(),
    };
    let mut database = Database::open_in_memory().unwrap();
    database
        .register_source(&SourceRegistration {
            installation_id: "installation-codex-pipeline".into(),
            source_object_id: SOURCE_ID.into(),
            adapter_id: adapter.id().into(),
            platform: "test".into(),
            root_path: "/fixture/home/codex".into(),
            discovery_method: "fixture".into(),
            native_path: "/fixture/home/codex/rollout.jsonl".into(),
            kind: "append_only_jsonl".into(),
        })
        .unwrap();

    let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
    database
        .apply_ingest(request(first, adapter.parser_version()))
        .unwrap();
    write_total(&mut file, 3, 160, 35);

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
    let resumed = adapter
        .ingest(&source, IngestStart::Resume(&checkpoint))
        .unwrap();
    assert_eq!(resumed.mode, IngestMode::Append);
    database
        .apply_ingest(request(resumed, adapter.parser_version()))
        .unwrap();

    assert_eq!(database.event_count(SOURCE_ID).unwrap(), 2);
    let daily = database.daily_usage_utc().unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].client, "codex-cli");
    assert_eq!(daily[0].provider.as_deref(), Some("openai"));
    assert_eq!(daily[0].model, "gpt-synthetic");
    assert_eq!(daily[0].tokens.input, 160);
    assert_eq!(daily[0].tokens.output, 35);
}

#[test]
fn codex_paginated_lineage_counts_parent_and_child_usage_once() {
    let home = TempDir::new().unwrap();
    let directory = home.path().join("sessions/2024/01/01");
    fs::create_dir_all(&directory).unwrap();
    let thread_id = "01900000-0000-7000-8000-000000000101";
    let child_rollout = "01900000-0000-7000-8000-000000000102";
    let parent = directory.join(format!("rollout-2024-01-01T00-00-00-{thread_id}.jsonl"));
    let parent_lines = [
        serde_json::json!({"timestamp":"2024-01-01T00:00:00Z","ordinal":0,"type":"session_meta","payload":{"id":thread_id,"model_provider":"openai","history_mode":"paginated"}}),
        serde_json::json!({"timestamp":"2024-01-01T00:00:01Z","ordinal":1,"type":"turn_context","payload":{"model":"gpt-synthetic"}}),
        total(2, 100, 20),
    ];
    write_lines(&parent, &parent_lines);
    let cutoff = fs::metadata(&parent).unwrap().len();
    let child = directory.join(format!(
        "rollout-2024-01-01T00-00-03-{thread_id}_{child_rollout}.jsonl"
    ));
    write_lines(
        &child,
        &[
            serde_json::json!({"timestamp":"2024-01-01T00:00:03Z","ordinal":0,"type":"session_meta","payload":{"id":thread_id,"model_provider":"openai","history_mode":"paginated","history_base":{"thread_id":thread_id,"end_ordinal_exclusive":3,"end_byte_offset":cutoff}}}),
            serde_json::json!({"timestamp":"2024-01-01T00:00:04Z","ordinal":3,"type":"turn_context","payload":{"model":"gpt-synthetic"}}),
            total(4, 140, 30),
        ],
    );

    let adapter = CodexJsonlAdapter::new(home.path());
    let sources = adapter.discover().unwrap();
    let mut database = Database::open_in_memory().unwrap();
    for (index, source) in sources.iter().enumerate() {
        let source_id = format!("source-codex-lineage-{index}");
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-codex-lineage".into(),
                source_object_id: source_id.clone(),
                adapter_id: adapter.id().into(),
                platform: "test".into(),
                root_path: "/fixture/home/codex".into(),
                discovery_method: "fixture".into(),
                native_path: format!("/fixture/home/codex/{}", source.source_key),
                kind: "append_only_jsonl".into(),
            })
            .unwrap();
        let batch = adapter.ingest(source, IngestStart::Fresh).unwrap();
        database
            .apply_ingest(IngestRequest {
                source_object_id: source_id,
                parser_version: adapter.parser_version(),
                mode: WriteMode::Append,
                source_fingerprint: batch.source_fingerprint,
                source_len: batch.checkpoint.source_len,
                byte_offset: batch.checkpoint.byte_offset,
                prefix_fingerprint: batch.checkpoint.prefix_fingerprint,
                parser_state: batch.checkpoint.parser_state,
                observed_at_unix_ms: 1_704_067_200_000,
                records: batch.records,
                warnings: batch.warnings,
            })
            .unwrap();
    }

    let daily = database.daily_usage_utc().unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].tokens.input, 140);
    assert_eq!(daily[0].tokens.output, 30);
}

fn write_total(file: &mut NamedTempFile, ordinal: u64, input: u64, output: u64) {
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
    file.flush().unwrap();
}

fn total(ordinal: u64, input: u64, output: u64) -> serde_json::Value {
    serde_json::json!({
        "timestamp": "2024-01-01T00:00:02Z",
        "ordinal": ordinal,
        "type": "event_msg",
        "payload": {"type":"token_count","info":{"total_token_usage":{
            "input_tokens": input,
            "output_tokens": output,
            "total_tokens": input + output
        }}}
    })
}

fn write_lines(path: &std::path::Path, lines: &[serde_json::Value]) {
    let contents = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{contents}\n")).unwrap();
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
        observed_at_unix_ms: 1_704_067_200_000,
        records: batch.records,
        warnings: batch.warnings,
    }
}
