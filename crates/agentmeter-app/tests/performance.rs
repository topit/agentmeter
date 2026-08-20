//! Synthetic-corpus performance validation for the targets in
//! `docs/PLAN.md` section 16. These are `#[ignore]`d so routine gates stay
//! deterministic; run explicitly and read the printed table:
//!
//! ```sh
//! cargo test -p agentmeter-app --test performance -- --ignored --nocapture
//! ```

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

use agentmeter_app::{IngestionService, OverviewService, SourcesService};
use agentmeter_collectors::kimi::KimiWireAdapter;
use agentmeter_storage::Database;
use tempfile::TempDir;

const SOURCES: usize = 40;
const EVENTS_PER_SOURCE: usize = 2_500;
const APPENDED_LINES: usize = 100;

#[test]
#[ignore = "synthetic performance corpus; run explicitly with --ignored --nocapture"]
fn meets_warm_dashboard_and_reconciliation_targets() {
    let data_directory = TempDir::new().unwrap();
    let kimi_root = TempDir::new().unwrap();
    let mut wires = Vec::new();
    write_corpus(kimi_root.path(), &mut wires);
    let corpus_bytes: u64 = wires
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();

    let service = IngestionService::with_adapters(
        data_directory.path(),
        vec![(
            kimi_root.path().to_owned(),
            Box::new(KimiWireAdapter::new(kimi_root.path())),
        )],
    );
    let database_path = data_directory.path().join("AgentMeter/agentmeter.db");

    // Cold collection: discovery plus full ingestion of every source.
    let cold_start = Instant::now();
    let cold = service.scan_and_ingest(1_787_011_200_000).unwrap();
    let cold_elapsed = cold_start.elapsed();
    let cold_events = Database::open(&database_path)
        .unwrap()
        .overview_snapshot()
        .unwrap()
        .event_count;
    assert_eq!(cold.runs[0].ingested_sources as usize, SOURCES);
    assert_eq!(cold_events, (SOURCES * EVENTS_PER_SOURCE) as u64);

    // Warm dashboard: the first useful snapshot is the overview load,
    // which already embeds source health. Target: under 200 ms.
    let mut dashboard_elapsed = None;
    for _ in 0..3 {
        let dashboard_start = Instant::now();
        let snapshot = OverviewService::in_data_directory(data_directory.path())
            .load()
            .unwrap();
        let elapsed = dashboard_start.elapsed();
        if dashboard_elapsed.is_none_or(|best| elapsed < best) {
            dashboard_elapsed = Some(elapsed);
            std::mem::forget(snapshot);
        }
    }
    let dashboard_elapsed = dashboard_elapsed.unwrap();
    let overview = OverviewService::in_data_directory(data_directory.path())
        .load()
        .unwrap();
    let sources = SourcesService::in_data_directory(data_directory.path())
        .load()
        .unwrap();
    let health_start = Instant::now();
    let _ = Database::open(&database_path)
        .unwrap()
        .source_health_snapshot()
        .unwrap();
    let health_elapsed = health_start.elapsed();
    println!("sources snapshot alone: {health_elapsed:?}");
    assert_eq!(overview.event_count, cold_events);
    assert_eq!(sources.sources.len(), SOURCES);
    assert!(
        dashboard_elapsed.as_millis() < 200,
        "warm dashboard took {dashboard_elapsed:?}"
    );

    // Unchanged warm reconciliation must stay under one second.
    let warm_start = Instant::now();
    let warm = service.scan_and_ingest(1_787_011_300_000).unwrap();
    let warm_elapsed = warm_start.elapsed();
    assert_eq!(warm.runs[0].failed_sources, 0);
    assert!(
        warm_elapsed.as_millis() < 1_000,
        "unchanged warm reconciliation took {warm_elapsed:?}"
    );
    let warm_events = Database::open(&database_path)
        .unwrap()
        .overview_snapshot()
        .unwrap()
        .event_count;
    assert_eq!(warm_events, cold_events, "warm scan must not double count");

    // Append refresh stays proportional to appended bytes, not history.
    append_lines(&wires[0], APPENDED_LINES, 9_000_000);
    let append_start = Instant::now();
    service.scan_and_ingest(1_787_011_400_000).unwrap();
    let append_elapsed = append_start.elapsed();
    let appended_events = Database::open(&database_path)
        .unwrap()
        .overview_snapshot()
        .unwrap()
        .event_count;
    assert_eq!(
        appended_events,
        cold_events + APPENDED_LINES as u64,
        "appended usage must land exactly once"
    );
    assert!(
        append_elapsed < cold_elapsed,
        "append refresh ({append_elapsed:?}) must stay below cold rebuild ({cold_elapsed:?})"
    );

    // Projections rebuild deterministically from the canonical ledger.
    let mut database = Database::open(&database_path).unwrap();
    let before = database.daily_usage_utc().unwrap();
    database.rebuild_daily_usage().unwrap();
    let after = database.daily_usage_utc().unwrap();
    assert_eq!(before, after, "projection rebuild must be deterministic");

    println!("performance corpus: {SOURCES} sources × {EVENTS_PER_SOURCE} events");
    println!("corpus bytes: {corpus_bytes}");
    println!("cold collection:    {cold_elapsed:?}");
    println!("warm dashboard:     {dashboard_elapsed:?} (target < 200 ms)");
    println!("warm reconciliation:{warm_elapsed:?} (target < 1 s)");
    println!("append refresh:     {append_elapsed:?} (+{APPENDED_LINES} lines)");
}

/// Writes the synthetic Kimi corpus: one agent journal per source with
/// unique timestamps so every usage fingerprint stays distinct.
fn write_corpus(root: &Path, wires: &mut Vec<PathBuf>) {
    for source in 0..SOURCES {
        let session = root
            .join("sessions")
            .join(format!("wd_bench_{source:02}"))
            .join(format!(
                "session_{:08x}-0000-4000-8000-{:012x}",
                source, source
            ))
            .join("agents")
            .join("main");
        fs::create_dir_all(&session).unwrap();
        let wire = session.join("wire.jsonl");
        let mut file = fs::File::create(&wire).unwrap();
        writeln!(file, r#"{{"type":"metadata","protocol_version":"1.5"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"llm.request","kind":"loop","provider":"moonshot","model":"kimi-for-coding","time":1782113000000}}"#
        )
        .unwrap();
        // Spread usage over 90 distinct UTC days like a real quarter of
        // history; per-file uniqueness comes from the varying token buckets.
        for event in 0..EVENTS_PER_SOURCE {
            let time = 1_782_113_000_000_u64
                + (event as u64 % 90) * 86_400_000
                + source as u64 % 900 * 1_000;
            writeln!(
                file,
                r#"{{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{{"inputOther":{input},"output":{output},"inputCacheRead":{cache_read},"inputCacheCreation":0}},"usageScope":"turn","time":{time}}}"#,
                input = 100 + event,
                output = 20 + (event % 7),
                cache_read = 400 + (event % 13),
            )
            .unwrap();
        }
        file.flush().unwrap();
        wires.push(wire);
    }
}

fn append_lines(wire: &Path, lines: usize, base_time_offset: u64) {
    let mut contents = fs::read_to_string(wire).unwrap();
    for line in 0..lines {
        let time = 1_787_011_500_000_u64 + base_time_offset + line as u64;
        contents.push_str(&format!(
            r#"{{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{{"inputOther":7,"output":3,"inputCacheRead":11,"inputCacheCreation":0}},"usageScope":"turn","time":{time}}}"#
        ));
        contents.push('\n');
    }
    fs::write(wire, contents).unwrap();
}
