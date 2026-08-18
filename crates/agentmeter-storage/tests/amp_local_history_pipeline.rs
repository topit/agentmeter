use agentmeter_collectors::{
    CollectorAdapter, IngestBatch, IngestMode, IngestStart,
    amp::local_history::AmpLocalHistoryAdapter,
};
use agentmeter_storage::{Database, IngestRequest, SourceRegistration, WriteMode};
use tempfile::TempDir;

const SOURCE_ID: &str = "source-amp-local-pipeline";

#[test]
fn amp_local_snapshot_reconciles_and_replaces_canonical_events() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("T-synthetic-pipeline.json");
    std::fs::write(&path, snapshot(true)).unwrap();

    let adapter = AmpLocalHistoryAdapter::new(directory.path());
    let source = adapter.discover().unwrap().remove(0);
    let mut database = Database::open_in_memory().unwrap();
    database
        .register_source(&SourceRegistration {
            installation_id: "installation-amp-local-pipeline".into(),
            source_object_id: SOURCE_ID.into(),
            adapter_id: adapter.id().into(),
            platform: "test".into(),
            root_path: "/fixture/home/amp/threads".into(),
            discovery_method: "fixture".into(),
            native_path: "/fixture/home/amp/threads/T-synthetic-pipeline.json".into(),
            kind: "mutable_json".into(),
        })
        .unwrap();

    let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
    assert_eq!(first.records.len(), 2);
    database
        .apply_ingest(request(first, adapter.parser_version()))
        .unwrap();
    assert_eq!(database.event_count(SOURCE_ID).unwrap(), 2);

    std::fs::write(&path, snapshot(false)).unwrap();
    let replacement = adapter.ingest(&source, IngestStart::Rebuild).unwrap();
    assert_eq!(replacement.mode, IngestMode::Replace);
    database
        .apply_ingest(request(replacement, adapter.parser_version()))
        .unwrap();

    assert_eq!(database.event_count(SOURCE_ID).unwrap(), 1);
    let daily = database.daily_usage_utc().unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].client, "amp");
    assert_eq!(daily[0].model, "model-a");
    assert_eq!(daily[0].tokens.input, 10);
    assert_eq!(daily[0].tokens.output, 2);
}

fn snapshot(include_second: bool) -> String {
    let second = if include_second {
        r#",{"timestamp":"2024-01-02T00:00:00Z","model":"model-b","tokens":{"input":20,"cacheReadInputTokens":5},"toMessageId":2}"#
    } else {
        ""
    };
    format!(
        r#"{{"id":"T-synthetic-pipeline","created":1704067200000,"usageLedger":{{"events":[{{"timestamp":"2024-01-01T00:00:00Z","model":"model-a","tokens":{{"input":10,"output":2}},"toMessageId":1}}{second}]}},"messages":[{{"role":"assistant","messageId":1,"usage":{{"model":"model-a","inputTokens":10,"outputTokens":2}}}}]}}"#
    )
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
        observed_at_unix_ms: 1_704_153_600_000,
        records: batch.records,
        warnings: batch.warnings,
    }
}
