use std::fs;

use agentmeter_collectors::{CollectorAdapter, IngestStart, pi::PiJsonlAdapter};
use agentmeter_storage::{Database, IngestRequest, SourceRegistration, WriteMode};
use tempfile::TempDir;

#[test]
fn pi_fork_and_summary_usage_flow_into_the_ledger_once() {
    let root = TempDir::new().unwrap();
    let directory = root.path().join("--fixture-project--");
    fs::create_dir_all(&directory).unwrap();
    let parent_name = "2024-01-01_parent.jsonl";
    let inherited = assistant("entry-shared", 100, 20);
    write_lines(
        &directory.join(parent_name),
        &[header("session-synthetic-parent", None), inherited.clone()],
    );
    write_lines(
        &directory.join("2024-01-02_child.jsonl"),
        &[
            header(
                "session-synthetic-child",
                Some(&format!("/fixture/home/pi/{parent_name}")),
            ),
            inherited,
            assistant("entry-child", 30, 5),
            serde_json::json!({
                "type":"branch_summary","id":"entry-summary","parentId":"entry-child",
                "timestamp":"2024-01-01T00:01:00Z",
                "usage":{"input":20,"output":5,"cacheRead":0,"cacheWrite":0,"totalTokens":25}
            }),
        ],
    );

    let adapter = PiJsonlAdapter::new(root.path());
    let mut database = Database::open_in_memory().unwrap();
    for (index, source) in adapter.discover().unwrap().iter().enumerate() {
        let source_id = format!("source-pi-pipeline-{index}");
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-pi-pipeline".into(),
                source_object_id: source_id.clone(),
                adapter_id: adapter.id().into(),
                platform: "test".into(),
                root_path: "/fixture/home/pi".into(),
                discovery_method: "fixture".into(),
                native_path: format!("/fixture/home/pi/{}", source.source_key),
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
    assert_eq!(daily.len(), 2);
    let known = daily
        .iter()
        .find(|row| row.model == "model-synthetic")
        .unwrap();
    assert_eq!(known.tokens.input, 130);
    assert_eq!(known.tokens.output, 25);
    let summaries = daily.iter().find(|row| row.model == "unknown").unwrap();
    assert_eq!(summaries.tokens.input, 20);
    assert_eq!(summaries.tokens.output, 5);
}

fn header(id: &str, parent_session: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "type":"session","version":3,"id":id,
        "timestamp":"2024-01-01T00:00:00Z","cwd":"/fixture/project",
        "parentSession":parent_session
    })
}

fn assistant(id: &str, input: i64, output: i64) -> serde_json::Value {
    serde_json::json!({
        "type":"message","id":id,"parentId":null,"timestamp":"2024-01-01T00:00:02Z",
        "message":{
            "role":"assistant","content":[],"provider":"provider-synthetic",
            "model":"model-synthetic","timestamp":1704067202000_i64,
            "usage":{"input":input,"output":output,"cacheRead":0,"cacheWrite":0,"totalTokens":input+output}
        }
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
