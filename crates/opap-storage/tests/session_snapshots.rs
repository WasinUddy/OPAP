use opap_storage::{
    APPLICATION_ID, Database, Error as StorageError, LATEST_SCHEMA_VERSION, NewMachine, NewProfile,
    NewSession, SessionDataKind, SessionDataReplacement, SessionEventInput, SessionSettingInput,
    SessionSettingValue, SessionSliceInput, SessionSliceState, SessionSnapshotReplacement,
    SessionSummaryInput, SessionWaveformChunkInput, SessionWaveformInput, SummaryMetricInput,
};
use std::error::Error;
use std::path::Path;

type TestResult = Result<(), Box<dyn Error>>;

const START_MS: i64 = 1_700_000_000_000;
const END_MS: i64 = START_MS + 3_600_000;
const SOURCE_DIGEST: &str = "191f86388cf82898383ae7449a767deb620445e897a9ba8dfccf7c15eb2b0f9a";
const CONTENT_DIGEST: &str = "42e0a0142ef0d61dc16963eff0e1da37e622e3c72563b23c1f163cb4b534f457";
const UPDATED_DIGEST: &str = "0442fdcd640f5125d8bf60dcaf2b3249912440148ebde22910a2135600e15a8e";
const WAVEFORM_PAYLOAD: [u8; 8] = [1, 0, 2, 0, 3, 0, 4, 0];

const EVENTS: [SessionEventInput<'static>; 2] = [
    SessionEventInput {
        source_key: "event:oa:1",
        channel_key: "respiratory_events",
        event_type: "obstructive_apnea",
        starts_at_ms: START_MS + 60_000,
        duration_ms: Some(12_000),
        value: Some(1.0),
        unit: None,
        created_at_ms: END_MS + 1,
    },
    SessionEventInput {
        source_key: "event:h:2",
        channel_key: "respiratory_events",
        event_type: "hypopnea",
        starts_at_ms: START_MS + 120_000,
        duration_ms: Some(10_000),
        value: None,
        unit: None,
        created_at_ms: END_MS + 1,
    },
];
const CHUNKS: [SessionWaveformChunkInput<'static>; 1] = [SessionWaveformChunkInput {
    chunk_index: 0,
    start_sample: 0,
    sample_count: 4,
    payload: &WAVEFORM_PAYLOAD,
    min_value: Some(1.0),
    max_value: Some(4.0),
}];
const WAVEFORMS: [SessionWaveformInput<'static>; 1] = [SessionWaveformInput {
    source_key: "waveform:flow:1",
    channel_key: "flow_rate",
    unit: Some("L/min"),
    started_at_ms: START_MS,
    sample_interval_us: 1_000_000,
    sample_count: 4,
    encoding: "i16-le",
    min_value: Some(1.0),
    max_value: Some(4.0),
    created_at_ms: END_MS + 1,
    chunks: &CHUNKS,
}];
const SLICES: [SessionSliceInput<'static>; 2] = [
    SessionSliceInput {
        sequence: 0,
        source_key: "slice:mask-on:1",
        state: SessionSliceState::MaskOn,
        started_at_ms: START_MS,
        ended_at_ms: START_MS + 1_800_000,
    },
    SessionSliceInput {
        sequence: 1,
        source_key: "slice:mask-off:2",
        state: SessionSliceState::MaskOff,
        started_at_ms: START_MS + 1_800_000,
        ended_at_ms: END_MS,
    },
];
const METRICS: [SummaryMetricInput<'static>; 2] = [
    SummaryMetricInput {
        key: "ahi",
        value: 2.5,
        unit: Some("1/h"),
    },
    SummaryMetricInput {
        key: "pressure_p95",
        value: 11.2,
        unit: Some("cmH2O"),
    },
];
const SETTINGS: [SessionSettingInput<'static>; 4] = [
    SessionSettingInput {
        key: "mode",
        integer_value: None,
        real_value: None,
        text_value: Some("autoset"),
        boolean_value: None,
        unit: None,
        origin: "device_reported",
    },
    SessionSettingInput {
        key: "minimum_pressure",
        integer_value: None,
        real_value: Some(6.4),
        text_value: None,
        boolean_value: None,
        unit: Some("cmH2O"),
        origin: "device_reported",
    },
    SessionSettingInput {
        key: "ramp_minutes",
        integer_value: Some(20),
        real_value: None,
        text_value: None,
        boolean_value: None,
        unit: Some("min"),
        origin: "device_reported",
    },
    SessionSettingInput {
        key: "smart_start",
        integer_value: None,
        real_value: None,
        text_value: None,
        boolean_value: Some(true),
        unit: None,
        origin: "device_reported",
    },
];

fn session(machine_id: i64, ended_at_ms: i64, now_ms: i64) -> NewSession<'static> {
    NewSession {
        machine_id,
        source_key: "session:2023-11-14:1",
        started_at_ms: START_MS,
        ended_at_ms: Some(ended_at_ms),
        timezone_offset_minutes: Some(0),
        now_ms,
    }
}

fn snapshot<'a>(
    content_digest: &'a str,
    data: SessionDataReplacement<'a>,
    slices: &'a [SessionSliceInput<'a>],
    metrics: &'a [SummaryMetricInput<'a>],
    settings: &'a [SessionSettingInput<'a>],
) -> SessionSnapshotReplacement<'a> {
    SessionSnapshotReplacement {
        data,
        provenance: opap_storage::SessionProvenanceInput {
            therapy_day: "2023-11-14",
            start_local_wall: "2023-11-14T22:13:20.000",
            end_local_wall: "2023-11-14T23:13:20.000",
            start_utc_offset_seconds: Some(0),
            end_utc_offset_seconds: Some(0),
            start_clock_correction_ms: 0,
            end_clock_correction_ms: 0,
            data_kind: SessionDataKind::Detailed,
            importer_name: "resmed",
            importer_schema: "opap-import/v4",
            id_algorithm: "resmed-session-id/v1",
            source_digest: SOURCE_DIGEST,
            content_digest,
        },
        slices,
        summary: SessionSummaryInput {
            usage_ms: 1_800_000,
            metrics,
        },
        settings,
    }
}

fn database_with_machine() -> Result<(Database, i64), Box<dyn Error>> {
    let mut database = Database::open_in_memory()?;
    let transaction = database.transaction()?;
    let profile = opap_storage::repository::Profiles::new(&transaction).insert(&NewProfile {
        display_name: "Snapshot fixture",
        now_ms: START_MS,
    })?;
    let machine = opap_storage::repository::Machines::new(&transaction).upsert(&NewMachine {
        profile_id: profile.id,
        source_key: "machine:opaque:1",
        device_type: "positive_airway_pressure",
        manufacturer: "ResMed",
        model: "AirSense 11",
        model_number: "39421",
        serial_number: "test-only",
        seen_at_ms: START_MS,
    })?;
    transaction.commit()?;
    Ok((database, machine.id))
}

fn create_legacy_database(path: &Path, version: usize) -> Result<(), Box<dyn Error>> {
    let connection = rusqlite::Connection::open(path)?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE schema_migrations (
             version INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             applied_at_ms INTEGER NOT NULL
         ) STRICT;",
    )?;
    let migrations = [
        (
            1_i64,
            "initial_storage",
            include_str!("../migrations/0001_initial_storage.sql"),
        ),
        (
            2_i64,
            "query_indexes",
            include_str!("../migrations/0002_query_indexes.sql"),
        ),
        (
            3_i64,
            "storage_integrity",
            include_str!("../migrations/0003_storage_integrity.sql"),
        ),
        (
            4_i64,
            "import_job_states",
            include_str!("../migrations/0004_import_job_states.sql"),
        ),
        (
            5_i64,
            "opaque_import_sources",
            include_str!("../migrations/0005_opaque_import_sources.sql"),
        ),
        (
            6_i64,
            "import_history_link_integrity",
            include_str!("../migrations/0006_import_history_link_integrity.sql"),
        ),
        (
            7_i64,
            "opaque_import_keys",
            include_str!("../migrations/0007_opaque_import_keys.sql"),
        ),
        (
            8_i64,
            "session_snapshots",
            include_str!("../migrations/0008_session_snapshots.sql"),
        ),
    ];
    for (migration_version, name, sql) in migrations.into_iter().take(version) {
        connection.execute_batch(sql)?;
        connection.execute(
            "INSERT INTO schema_migrations (version, name, applied_at_ms)
             VALUES (?1, ?2, ?1)",
            rusqlite::params![migration_version, name],
        )?;
    }
    connection.execute(
        "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
         VALUES (1, 'Legacy snapshot', 1, 1)",
        [],
    )?;
    connection.execute(
        "INSERT INTO machines (
             id, profile_id, source_key, device_type, manufacturer, model,
             model_number, serial_number, first_seen_at_ms, last_seen_at_ms
         ) VALUES (1, 1, 'legacy-machine', 'pap', 'Test', 'Test', '', '', 1, 1)",
        [],
    )?;
    connection.execute(
        "INSERT INTO sessions (
             id, machine_id, source_key, started_at_ms, ended_at_ms,
             timezone_offset_minutes, created_at_ms, updated_at_ms
         ) VALUES (1, 1, 'legacy-session', 10, 20, 0, 1, 1)",
        [],
    )?;
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", version as i64)?;
    Ok(())
}

#[test]
fn fresh_schema_has_all_strict_snapshot_tables() -> TestResult {
    let database = Database::open_in_memory()?;
    assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
    for table in [
        "session_provenance",
        "session_slices",
        "session_summary",
        "summary_metrics",
        "session_settings",
    ] {
        let strict: i64 = database.connection().query_row(
            "SELECT strict FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        assert_eq!(strict, 1, "{table} must be STRICT");
    }
    assert_eq!(
        database
            .applied_migrations()?
            .last()
            .map(|migration| (migration.version, migration.name.as_str())),
        Some((9, "atomic_import_commits"))
    );
    Ok(())
}

#[test]
fn upgrades_every_v1_through_v8_database_without_losing_sessions() -> TestResult {
    for version in 1_usize..=8 {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join(format!("legacy-v{version}.sqlite3"));
        create_legacy_database(&path, version)?;

        let database = Database::open(&path)?;
        assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
        assert!(database.sessions().get(1)?.is_some());
        assert!(database.session_snapshots().get(1)?.is_none());
        assert_eq!(
            database.applied_migrations()?.len(),
            LATEST_SCHEMA_VERSION as usize
        );
    }
    Ok(())
}

#[test]
fn failed_v8_migration_rolls_back_all_prior_statements() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v8-rollback.sqlite3");
    create_legacy_database(&path, 7)?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "CREATE TABLE session_summary (
             incompatible TEXT NOT NULL
         ) STRICT;",
    )?;
    drop(connection);

    assert!(Database::open(&path).is_err());
    let unchanged = rusqlite::Connection::open(&path)?;
    let user_version: i64 = unchanged.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let history_version: i64 =
        unchanged.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    assert_eq!((user_version, history_version), (7, 7));
    for rolled_back in ["session_provenance", "session_slices", "session_settings"] {
        let count: i64 = unchanged.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [rolled_back],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0, "{rolled_back} must have rolled back");
    }
    let incompatible_column: String = unchanged.query_row(
        "SELECT name FROM pragma_table_info('session_summary')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(incompatible_column, "incompatible");
    Ok(())
}

#[test]
fn v9_migration_gives_legacy_jobs_safe_empty_execution_defaults() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v8-execution-defaults.sqlite3");
    create_legacy_database(&path, 8)?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms
         ) VALUES (
             1, 1,
             'opap-request:00000000000000000000000000000001',
             'opap-source:00000000000000000000000000000001',
             'resmed', 1, 'blocked', 1, 1
         )",
        [],
    )?;
    drop(connection);

    let database = Database::open(&path)?;
    let history = database.imports().get(1)?.expect("migrated import job");
    assert_eq!(history.source_fingerprint, "");
    assert_eq!(history.input_digest, "");
    assert_eq!(history.options_digest, "");
    assert_eq!(history.execution_generation, 0);
    assert_eq!(history.execution_token, None);
    assert!(
        database
            .imports()
            .list_session_results(history.id)?
            .is_empty()
    );
    Ok(())
}

#[test]
fn failed_v9_migration_rolls_back_added_columns_and_tables() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v9-rollback.sqlite3");
    create_legacy_database(&path, 8)?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "CREATE TABLE import_session_results (
             incompatible TEXT NOT NULL
         ) STRICT;",
    )?;
    drop(connection);

    assert!(Database::open(&path).is_err());
    let unchanged = rusqlite::Connection::open(&path)?;
    let user_version: i64 = unchanged.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let history_version: i64 =
        unchanged.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    assert_eq!((user_version, history_version), (8, 8));
    let added_columns: i64 = unchanged.query_row(
        "SELECT COUNT(*)
         FROM pragma_table_info('import_history')
         WHERE name IN (
             'source_fingerprint', 'input_digest', 'options_digest',
             'execution_generation', 'execution_token'
         )",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(added_columns, 0);
    let incompatible_column: String = unchanged.query_row(
        "SELECT name FROM pragma_table_info('import_session_results')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(incompatible_column, "incompatible");
    Ok(())
}

#[test]
fn snapshot_round_trips_provenance_slices_summary_metrics_and_all_setting_types() -> TestResult {
    let (mut database, machine_id) = database_with_machine()?;
    let replacement = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &METRICS,
        &SETTINGS,
    );
    let result = database
        .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &replacement)?;

    assert_eq!(result.stats.session_data.events_written, 2);
    assert_eq!(result.stats.session_data.waveforms_written, 1);
    assert_eq!(result.stats.slices_written, 2);
    assert_eq!(result.stats.summary_metrics_written, 2);
    assert_eq!(result.stats.settings_written, 4);

    let stored = database
        .session_snapshots()
        .get(result.session.id)?
        .expect("stored snapshot");
    assert_eq!(stored.provenance.therapy_day, "2023-11-14");
    assert_eq!(stored.provenance.data_kind, SessionDataKind::Detailed);
    assert_eq!(stored.provenance.source_digest, SOURCE_DIGEST);
    assert_eq!(stored.provenance.content_digest, CONTENT_DIGEST);
    assert_eq!(stored.slices.len(), 2);
    assert_eq!(stored.slices[0].state, SessionSliceState::MaskOn);
    assert_eq!(stored.summary.usage_ms, 1_800_000);
    assert_eq!(
        stored
            .summary
            .metrics
            .iter()
            .map(|metric| metric.key.as_str())
            .collect::<Vec<_>>(),
        vec!["ahi", "pressure_p95"]
    );
    assert_eq!(stored.settings.len(), 4);
    assert!(stored.settings.iter().any(|setting| {
        setting.key == "ramp_minutes"
            && setting.value == SessionSettingValue::Integer(20)
            && setting.unit.as_deref() == Some("min")
    }));
    assert!(stored.settings.iter().any(|setting| {
        setting.key == "minimum_pressure" && setting.value == SessionSettingValue::Real(6.4)
    }));
    assert!(stored.settings.iter().any(|setting| {
        setting.key == "mode" && setting.value == SessionSettingValue::Text("autoset".to_owned())
    }));
    assert!(stored.settings.iter().any(|setting| {
        setting.key == "smart_start" && setting.value == SessionSettingValue::Boolean(true)
    }));
    assert_eq!(
        database.events().list_by_session(result.session.id)?.len(),
        2
    );
    assert_eq!(
        database
            .waveforms()
            .list_metadata_by_session(result.session.id)?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn replacing_the_same_snapshot_is_idempotent() -> TestResult {
    let (mut database, machine_id) = database_with_machine()?;
    let replacement = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &METRICS,
        &SETTINGS,
    );
    let first = database
        .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &replacement)?;
    let second = database
        .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 2), &replacement)?;
    assert_eq!(first.session.id, second.session.id);
    assert_eq!(second.stats.slices_pruned, 0);
    assert_eq!(second.stats.summary_metrics_pruned, 0);
    assert_eq!(second.stats.settings_pruned, 0);

    for (table, expected) in [
        ("sessions", 1_i64),
        ("events", 2),
        ("waveforms", 1),
        ("waveform_chunks", 1),
        ("session_provenance", 1),
        ("session_slices", 2),
        ("session_summary", 1),
        ("summary_metrics", 2),
        ("session_settings", 4),
    ] {
        let count: i64 = database.connection().query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, expected, "{table} must remain idempotent");
    }
    Ok(())
}

#[test]
fn snapshot_replacement_prunes_every_omitted_child_kind() -> TestResult {
    let (mut database, machine_id) = database_with_machine()?;
    let initial = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &METRICS,
        &SETTINGS,
    );
    let created =
        database.replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &initial)?;

    let retained_slice = [SessionSliceInput {
        sequence: 0,
        source_key: "slice:mask-on:1",
        state: SessionSliceState::MaskOn,
        started_at_ms: START_MS,
        ended_at_ms: END_MS,
    }];
    let retained_metric = [METRICS[0]];
    let retained_setting = [SETTINGS[3]];
    let reduced = snapshot(
        UPDATED_DIGEST,
        SessionDataReplacement {
            events: &[],
            waveforms: &[],
        },
        &retained_slice,
        &retained_metric,
        &retained_setting,
    );
    let updated =
        database.replace_session_snapshot(&session(machine_id, END_MS, END_MS + 2), &reduced)?;
    assert_eq!(updated.session.id, created.session.id);
    assert_eq!(updated.stats.session_data.events_pruned, 2);
    assert_eq!(updated.stats.session_data.waveforms_pruned, 1);
    assert_eq!(updated.stats.slices_pruned, 1);
    assert_eq!(updated.stats.summary_metrics_pruned, 1);
    assert_eq!(updated.stats.settings_pruned, 3);

    let stored = database
        .session_snapshots()
        .get(updated.session.id)?
        .expect("reduced snapshot");
    assert_eq!(stored.provenance.content_digest, UPDATED_DIGEST);
    assert_eq!(stored.slices.len(), 1);
    assert_eq!(stored.summary.metrics.len(), 1);
    assert_eq!(stored.settings.len(), 1);
    assert!(
        database
            .events()
            .list_by_session(updated.session.id)?
            .is_empty()
    );
    assert!(
        database
            .waveforms()
            .list_metadata_by_session(updated.session.id)?
            .is_empty()
    );
    Ok(())
}

#[test]
fn prevalidation_rejects_invalid_values_without_creating_a_session() -> TestResult {
    let (mut database, machine_id) = database_with_machine()?;
    let invalid_setting = [SessionSettingInput {
        key: "ambiguous",
        integer_value: Some(1),
        real_value: Some(1.0),
        text_value: None,
        boolean_value: None,
        unit: None,
        origin: "device_reported",
    }];
    let replacement = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &METRICS,
        &invalid_setting,
    );
    let error = database
        .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &replacement)
        .expect_err("multiple typed setting values must fail");
    assert!(matches!(error, StorageError::Integrity(_)));
    assert!(database.sessions().list_by_machine(machine_id)?.is_empty());

    let non_finite_metric = [SummaryMetricInput {
        key: "ahi",
        value: f64::INFINITY,
        unit: Some("1/h"),
    }];
    let replacement = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &non_finite_metric,
        &SETTINGS,
    );
    assert!(
        database
            .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &replacement)
            .is_err()
    );

    let excessive_usage = SessionSnapshotReplacement {
        summary: SessionSummaryInput {
            usage_ms: 3_600_001,
            metrics: &METRICS,
        },
        ..snapshot(
            CONTENT_DIGEST,
            SessionDataReplacement {
                events: &EVENTS,
                waveforms: &WAVEFORMS,
            },
            &SLICES,
            &METRICS,
            &SETTINGS,
        )
    };
    assert!(
        database
            .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &excessive_usage)
            .is_err()
    );
    assert!(database.sessions().list_by_machine(machine_id)?.is_empty());
    Ok(())
}

#[test]
fn database_failure_rolls_back_session_and_all_snapshot_children() -> TestResult {
    let (mut database, machine_id) = database_with_machine()?;
    let initial = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &METRICS,
        &SETTINGS,
    );
    let created =
        database.replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &initial)?;
    database.connection().execute_batch(
        "CREATE TRIGGER test_reject_summary_metric
         BEFORE INSERT ON summary_metrics
         WHEN NEW.metric_key = 'reject'
         BEGIN
             SELECT RAISE(ABORT, 'injected snapshot failure');
         END;",
    )?;

    let rejected_metric = [SummaryMetricInput {
        key: "reject",
        value: 1.0,
        unit: None,
    }];
    let changed_slice = [SessionSliceInput {
        sequence: 0,
        source_key: "slice:changed",
        state: SessionSliceState::EquipmentOff,
        started_at_ms: START_MS,
        ended_at_ms: END_MS,
    }];
    let changed = snapshot(
        UPDATED_DIGEST,
        SessionDataReplacement {
            events: &[],
            waveforms: &[],
        },
        &changed_slice,
        &rejected_metric,
        &[],
    );
    assert!(
        database
            .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 99), &changed)
            .is_err()
    );

    let session = database
        .sessions()
        .get(created.session.id)?
        .expect("original session");
    assert_eq!(session.updated_at_ms, END_MS + 1);
    assert_eq!(database.events().list_by_session(session.id)?.len(), 2);
    assert_eq!(
        database
            .waveforms()
            .list_metadata_by_session(session.id)?
            .len(),
        1
    );
    let stored = database
        .session_snapshots()
        .get(session.id)?
        .expect("original snapshot");
    assert_eq!(stored.provenance.content_digest, CONTENT_DIGEST);
    assert_eq!(
        stored.slices,
        database.session_snapshots().list_slices(session.id)?
    );
    assert_eq!(stored.slices.len(), 2);
    assert_eq!(stored.summary.metrics.len(), 2);
    assert_eq!(stored.settings.len(), 4);
    Ok(())
}

#[test]
fn typed_setting_constraint_rejects_zero_or_multiple_values_in_direct_sql() -> TestResult {
    let (mut database, machine_id) = database_with_machine()?;
    let replacement = snapshot(
        CONTENT_DIGEST,
        SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        &SLICES,
        &METRICS,
        &SETTINGS,
    );
    let stored = database
        .replace_session_snapshot(&session(machine_id, END_MS, END_MS + 1), &replacement)?;
    for sql in [
        "INSERT INTO session_settings (
             session_id, setting_key, value_kind, origin
         ) VALUES (?1, 'missing', 'integer', 'test')",
        "INSERT INTO session_settings (
             session_id, setting_key, value_kind, integer_value, real_value, origin
         ) VALUES (?1, 'multiple', 'integer', 1, 1.0, 'test')",
    ] {
        assert!(
            database
                .connection()
                .execute(sql, [stored.session.id])
                .is_err(),
            "typed-value table constraint must reject malformed rows"
        );
    }
    Ok(())
}
