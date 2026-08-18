use std::io::Write;

use agentmeter_collectors::{
    CollectorAdapter, IngestBatch, IngestMode, IngestStart, SourceCheckpoint,
    reference::{ReferenceJsonlAdapter, ReferenceSnapshotAdapter},
};
use agentmeter_storage::{
    CheckpointStatus, Database, IngestRequest, SourceRegistration, StoredCheckpoint, WriteMode,
};
use tempfile::NamedTempFile;

const SOURCE_ID: &str = "source-reference-pipeline";
const EVENT_ONE: &str = r#"{"id":"event-001","session_id":"session-synthetic-001","client":"synthetic","provider":"provider-a","model":"model-a","occurred_at_unix_ms":1704067200000,"input":10,"output":2}"#;
const EVENT_TWO: &str = r#"{"id":"event-002","session_id":"session-synthetic-001","client":"synthetic","provider":"provider-a","model":"model-a","occurred_at_unix_ms":1704153600000,"input":20,"cache_read":5}"#;

#[test]
fn jsonl_append_and_rewrite_flow_through_the_ledger() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "{EVENT_ONE}").unwrap();
    file.flush().unwrap();

    let adapter = ReferenceJsonlAdapter::new(file.path());
    let source = adapter.discover().unwrap().remove(0);
    let mut database = registered_database("append_only_jsonl");

    let first = adapter.ingest(&source, IngestStart::Fresh).unwrap();
    database
        .apply_ingest(request(first, adapter.parser_version()))
        .unwrap();

    writeln!(file, "{EVENT_TWO}").unwrap();
    file.flush().unwrap();
    let checkpoint = current_checkpoint(&database, adapter.parser_version());
    let appended = adapter
        .ingest(&source, IngestStart::Resume(&checkpoint))
        .unwrap();
    assert_eq!(appended.mode, IngestMode::Append);
    database
        .apply_ingest(request(appended, adapter.parser_version()))
        .unwrap();
    assert_eq!(
        database.event_ids(SOURCE_ID).unwrap(),
        ["event-001", "event-002"]
    );

    std::fs::write(file.path(), format!("{EVENT_TWO}\n")).unwrap();
    let checkpoint = current_checkpoint(&database, adapter.parser_version());
    let rewritten = adapter
        .ingest(&source, IngestStart::Resume(&checkpoint))
        .unwrap();
    assert_eq!(rewritten.mode, IngestMode::Replace);
    database
        .apply_ingest(request(rewritten, adapter.parser_version()))
        .unwrap();

    assert_eq!(database.event_ids(SOURCE_ID).unwrap(), ["event-002"]);
    assert_eq!(database.daily_usage_utc().unwrap()[0].tokens.input, 20);
}

#[test]
fn mutable_snapshot_replaces_source_owned_events() {
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), format!(r#"{{"events":[{EVENT_ONE}]}}"#)).unwrap();

    let adapter = ReferenceSnapshotAdapter::new(file.path());
    let source = adapter.discover().unwrap().remove(0);
    let mut database = registered_database("mutable_json");
    database
        .apply_ingest(request(
            adapter.ingest(&source, IngestStart::Fresh).unwrap(),
            adapter.parser_version(),
        ))
        .unwrap();

    std::fs::write(file.path(), format!(r#"{{"events":[{EVENT_TWO}]}}"#)).unwrap();
    database
        .apply_ingest(request(
            adapter.ingest(&source, IngestStart::Rebuild).unwrap(),
            adapter.parser_version(),
        ))
        .unwrap();

    assert_eq!(database.event_ids(SOURCE_ID).unwrap(), ["event-002"]);
}

#[test]
fn parser_upgrade_rebuilds_instead_of_appending_stale_events() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "{EVENT_ONE}").unwrap();
    writeln!(file, "{EVENT_TWO}").unwrap();
    file.flush().unwrap();

    let adapter = ReferenceJsonlAdapter::new(file.path());
    let source = adapter.discover().unwrap().remove(0);
    let mut database = registered_database("append_only_jsonl");
    database
        .apply_ingest(request(
            adapter.ingest(&source, IngestStart::Fresh).unwrap(),
            1,
        ))
        .unwrap();

    let upgraded_event = EVENT_ONE.replace("\"input\":10", "\"input\":999");
    std::fs::write(file.path(), format!("{upgraded_event}\n")).unwrap();
    assert_eq!(
        database.checkpoint_status(SOURCE_ID, 2).unwrap(),
        CheckpointStatus::Invalidated {
            stored_parser_version: 1
        }
    );

    let rebuilt = adapter.ingest(&source, IngestStart::Rebuild).unwrap();
    assert_eq!(rebuilt.mode, IngestMode::Replace);
    database.apply_ingest(request(rebuilt, 2)).unwrap();

    assert_eq!(database.event_ids(SOURCE_ID).unwrap(), ["event-001"]);
    assert_eq!(database.daily_usage_utc().unwrap()[0].tokens.input, 999);
    assert!(matches!(
        database.checkpoint_status(SOURCE_ID, 2).unwrap(),
        CheckpointStatus::Current(_)
    ));
}

fn registered_database(kind: &str) -> Database {
    let mut database = Database::open_in_memory().unwrap();
    database
        .register_source(&SourceRegistration {
            installation_id: "installation-reference-pipeline".into(),
            source_object_id: SOURCE_ID.into(),
            adapter_id: "reference".into(),
            platform: "test".into(),
            root_path: "/fixture/home/reference".into(),
            discovery_method: "fixture".into(),
            native_path: "/fixture/home/reference/source".into(),
            kind: kind.into(),
        })
        .unwrap();
    database
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

fn collector_checkpoint(checkpoint: StoredCheckpoint) -> SourceCheckpoint {
    SourceCheckpoint {
        byte_offset: checkpoint.byte_offset,
        source_len: checkpoint.source_len,
        prefix_fingerprint: checkpoint.prefix_fingerprint,
        parser_state: checkpoint.parser_state,
    }
}

fn current_checkpoint(database: &Database, parser_version: u32) -> SourceCheckpoint {
    match database
        .checkpoint_status(SOURCE_ID, parser_version)
        .unwrap()
    {
        CheckpointStatus::Current(checkpoint) => collector_checkpoint(checkpoint),
        other => panic!("expected current checkpoint, got {other:?}"),
    }
}
