use agentmeter_core::{
    DataConfidence, EventProvenance, SourceHealthState, SourcePermissionState, SourceRemediation,
    TimestampOrigin, TokenBreakdown, UsageEvent, UsageRecord,
};
use agentmeter_storage::{
    CollectionFailureKind, Database, IngestRequest, SourceInstallationRegistration,
    SourceRegistration, WriteMode,
};

const SOURCE_ID: &str = "source-health-synthetic";
const INSTALLATION_ID: &str = "installation-health-synthetic";

#[test]
fn source_health_snapshot_covers_setup_success_partial_and_failures() {
    let mut database = Database::open_in_memory().unwrap();
    database
        .register_installation(&SourceInstallationRegistration {
            installation_id: "installation-missing-synthetic".into(),
            adapter_id: "adapter-missing-synthetic".into(),
            platform: "test".into(),
            root_path: "/fixture/home/missing".into(),
            discovery_method: "fixture".into(),
            enabled: true,
            permission: SourcePermissionState::Missing,
        })
        .unwrap();
    let registration = SourceRegistration {
        installation_id: INSTALLATION_ID.into(),
        source_object_id: SOURCE_ID.into(),
        adapter_id: "adapter-health-synthetic".into(),
        platform: "test".into(),
        root_path: "/fixture/home/health".into(),
        discovery_method: "fixture".into(),
        native_path: "/fixture/home/health/source.jsonl".into(),
        kind: "append_only_jsonl".into(),
    };
    database.register_source(&registration).unwrap();

    let setup = database.source_health_snapshot().unwrap();
    assert_eq!(setup.sources.len(), 2);
    let missing = setup
        .sources
        .iter()
        .find(|source| source.source_object_id.is_none())
        .unwrap();
    assert_eq!(missing.state, SourceHealthState::SetupRequired);
    assert_eq!(missing.root_path, "/fixture/home/missing");
    assert_eq!(missing.remediation, Some(SourceRemediation::ConfigurePath));
    let pending = source(&setup);
    assert_eq!(pending.state, SourceHealthState::SetupRequired);
    assert_eq!(
        pending.remediation,
        Some(SourceRemediation::RetryCollection)
    );
    assert_eq!(pending.parser_version, None);

    database
        .apply_ingest(request(1_704_067_200_000, Vec::new()))
        .unwrap();
    let healthy = database.source_health_snapshot().unwrap();
    let health = source(&healthy);
    assert_eq!(health.state, SourceHealthState::Healthy);
    assert_eq!(health.parser_version, Some(7));
    assert_eq!(health.last_scan_unix_ms, Some(1_704_067_200_000));
    assert_eq!(health.last_success_unix_ms, Some(1_704_067_200_000));
    assert_eq!(health.last_event_unix_ms, Some(1_704_067_199_000));
    assert_eq!(health.records_changed, 1);
    assert_ne!(setup.generation, healthy.generation);
    assert_eq!(healthy, database.source_health_snapshot().unwrap());

    database
        .apply_ingest(request(
            1_704_067_201_000,
            vec!["synthetic parser warning".into()],
        ))
        .unwrap();
    let partial = database.source_health_snapshot().unwrap();
    let health = source(&partial);
    assert_eq!(health.state, SourceHealthState::Partial);
    assert_eq!(health.remediation, Some(SourceRemediation::ReviewWarnings));
    assert_eq!(health.records_changed, 0);
    assert_ne!(healthy.generation, partial.generation);

    database
        .apply_ingest(request(
            1_704_067_201_500,
            vec!["Pi session schema is newer than version 3".into()],
        ))
        .unwrap();
    let warning_unsupported = database.source_health_snapshot().unwrap();
    assert_eq!(
        source(&warning_unsupported).state,
        SourceHealthState::UnsupportedSchema
    );

    database
        .record_collection_failure(
            SOURCE_ID,
            1_704_067_202_000,
            CollectionFailureKind::UnsupportedSchema,
            "fixture schema version is newer",
        )
        .unwrap();
    let unsupported = database.source_health_snapshot().unwrap();
    let health = source(&unsupported);
    assert_eq!(health.state, SourceHealthState::UnsupportedSchema);
    assert_eq!(
        health.remediation,
        Some(SourceRemediation::UpgradeAgentMeter)
    );
    assert_eq!(
        health.error.as_deref(),
        Some("fixture schema version is newer")
    );
    assert_eq!(health.last_success_unix_ms, Some(1_704_067_201_500));

    database
        .record_collection_failure(
            SOURCE_ID,
            1_704_067_203_000,
            CollectionFailureKind::Collection,
            "synthetic read failure",
        )
        .unwrap();
    let failed = database.source_health_snapshot().unwrap();
    let health = source(&failed);
    assert_eq!(health.state, SourceHealthState::Error);
    assert_eq!(health.remediation, Some(SourceRemediation::RetryCollection));

    database
        .record_collection_failure(
            SOURCE_ID,
            1_704_067_204_000,
            CollectionFailureKind::Permission,
            "fixture permission denied",
        )
        .unwrap();
    let denied = database.source_health_snapshot().unwrap();
    let health = source(&denied);
    assert_eq!(health.state, SourceHealthState::SetupRequired);
    assert_eq!(health.permission, SourcePermissionState::Denied);
    assert_eq!(health.remediation, Some(SourceRemediation::GrantPermission));

    database
        .apply_ingest(request(1_704_067_205_000, Vec::new()))
        .unwrap();
    let recovered = database.source_health_snapshot().unwrap();
    let health = source(&recovered);
    assert_eq!(health.state, SourceHealthState::Healthy);
    assert_eq!(health.permission, SourcePermissionState::Granted);

    database
        .set_installation_state(INSTALLATION_ID, false, SourcePermissionState::Granted)
        .unwrap();
    database.register_source(&registration).unwrap();
    let disabled = database.source_health_snapshot().unwrap();
    let health = source(&disabled);
    assert_eq!(health.state, SourceHealthState::Disabled);
    assert_eq!(health.remediation, None);
}

fn source(snapshot: &agentmeter_core::SourceHealthSnapshot) -> &agentmeter_core::SourceHealth {
    snapshot
        .sources
        .iter()
        .find(|source| source.source_object_id.as_deref() == Some(SOURCE_ID))
        .unwrap()
}

fn request(observed_at_unix_ms: i64, warnings: Vec<String>) -> IngestRequest {
    IngestRequest {
        source_object_id: SOURCE_ID.into(),
        parser_version: 7,
        mode: WriteMode::Append,
        source_fingerprint: format!("fingerprint-{observed_at_unix_ms}"),
        source_len: 10,
        byte_offset: Some(10),
        prefix_fingerprint: Some("prefix-synthetic".into()),
        parser_state: Vec::new(),
        observed_at_unix_ms,
        records: if observed_at_unix_ms == 1_704_067_200_000 {
            vec![record()]
        } else {
            Vec::new()
        },
        warnings,
    }
}

fn record() -> UsageRecord {
    UsageRecord {
        event: UsageEvent {
            id: "event-health-synthetic".into(),
            source_id: String::new(),
            session_id: Some("session-health-synthetic".into()),
            client: "client-synthetic".into(),
            provider: Some("provider-synthetic".into()),
            model: "model-synthetic".into(),
            occurred_at_unix_ms: 1_704_067_199_000,
            tokens: TokenBreakdown {
                input: 10,
                output: 5,
                ..TokenBreakdown::default()
            },
            source_reported_total: Some(15),
            confidence: DataConfidence::Exact,
        },
        costs: Vec::new(),
        provenance: EventProvenance {
            native_id: Some("native-health-synthetic".into()),
            record_offset: Some(0),
            schema_variant: "health-synthetic-v1".into(),
            timestamp_origin: TimestampOrigin::Source,
            normalization_notes: Vec::new(),
        },
    }
}
