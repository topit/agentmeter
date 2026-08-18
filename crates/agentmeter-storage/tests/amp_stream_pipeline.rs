use std::io::Write;

use agentmeter_collectors::{
    CollectorAdapter, IngestBatch, IngestMode, IngestStart, SourceCheckpoint,
    amp::AmpStreamJsonAdapter,
};
use agentmeter_storage::{
    CheckpointStatus, Database, IngestRequest, SourceRegistration, WriteMode,
};
use tempfile::NamedTempFile;

const SOURCE_ID: &str = "source-amp-stream-pipeline";
const SESSION_ID: &str = "T-00000000-0000-0000-0000-000000000001";

#[test]
fn amp_stream_appends_into_the_canonical_ledger() {
    let mut file = NamedTempFile::new().unwrap();
    write_assistant(&mut file, 10, 2);

    let adapter = AmpStreamJsonAdapter::new(file.path());
    let source = adapter.discover().unwrap().remove(0);
    let mut database = Database::open_in_memory().unwrap();
    database
        .register_source(&SourceRegistration {
            installation_id: "installation-amp-stream-pipeline".into(),
            source_object_id: SOURCE_ID.into(),
            adapter_id: adapter.id().into(),
            platform: "test".into(),
            root_path: "/fixture/home/amp-stream".into(),
            discovery_method: "fixture".into(),
            native_path: "/fixture/home/amp-stream/events.jsonl".into(),
            kind: "append_only_jsonl".into(),
        })
        .unwrap();

    let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
    database
        .apply_ingest(request(first, adapter.parser_version()))
        .unwrap();

    write_assistant(&mut file, 20, 4);
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
    database
        .apply_ingest(request(appended, adapter.parser_version()))
        .unwrap();

    assert_eq!(database.event_count(SOURCE_ID).unwrap(), 2);
    let daily = database.daily_usage_utc().unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].client, "amp");
    assert_eq!(daily[0].model, "unknown");
    assert_eq!(daily[0].tokens.input, 30);
    assert_eq!(daily[0].tokens.output, 6);
}

fn write_assistant(file: &mut NamedTempFile, input: u64, output: u64) {
    writeln!(
        file,
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[],"usage":{{"input_tokens":{input},"output_tokens":{output}}}}},"parent_tool_use_id":null,"session_id":"{SESSION_ID}"}}"#
    )
    .unwrap();
    file.flush().unwrap();
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
