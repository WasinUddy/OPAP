use opap_storage::repository::Profiles;
use opap_storage::{
    Database, Error as StorageError, ImportBlockCode, ImportCommitInput, ImportCounts,
    ImportExecutionClaim, ImportExecutionLease, ImportFailureCode, ImportMachineInput,
    ImportSessionOutcome, ImportSessionSnapshotInput, ImportStatus, InitialImportStatus, NewImport,
    NewMachine, NewProfile, NewSession, SessionDataKind, SessionDataReplacement, SessionEventInput,
    SessionProvenanceInput, SessionSnapshotReplacement, SessionSummaryInput,
    SessionWaveformChunkInput, SessionWaveformInput,
};
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

const START_MS: i64 = 1_700_000_000_000;
const END_MS: i64 = START_MS + 3_600_000;
const SOURCE_ID: &str = "opap-source:00000000000000000000000000000001";
const REQUEST_ONE: &str = "opap-request:00000000000000000000000000000001";
const REQUEST_TWO: &str = "opap-request:00000000000000000000000000000002";
const REQUEST_THREE: &str = "opap-request:00000000000000000000000000000003";
const TOKEN_ONE: &str = "opap-execution:00000000000000000000000000000001";
const TOKEN_TWO: &str = "opap-execution:00000000000000000000000000000002";
const TOKEN_THREE: &str = "opap-execution:00000000000000000000000000000003";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PAYLOAD: [u8; 4] = [1, 0, 2, 0];
const FIRST_CHUNK_PAYLOAD: [u8; 2] = [1, 0];
const SECOND_CHUNK_PAYLOAD: [u8; 2] = [2, 0];
const EVENTS: [SessionEventInput<'static>; 1] = [SessionEventInput {
    source_key: "event:one",
    channel_key: "pap.event.obstructive_apnea",
    event_type: "obstructive_apnea",
    starts_at_ms: START_MS + 60_000,
    duration_ms: Some(10_000),
    value: None,
    unit: None,
    created_at_ms: END_MS,
}];
const CHUNKS: [SessionWaveformChunkInput<'static>; 1] = [SessionWaveformChunkInput {
    chunk_index: 0,
    start_sample: 0,
    sample_count: 2,
    payload: &PAYLOAD,
    min_value: Some(1.0),
    max_value: Some(2.0),
}];
const WAVEFORMS: [SessionWaveformInput<'static>; 1] = [SessionWaveformInput {
    source_key: "waveform:one",
    channel_key: "pap.series.flow_rate",
    unit: Some("L/min"),
    started_at_ms: START_MS,
    sample_interval_us: 1_000_000,
    sample_count: 2,
    encoding: "i16-le",
    min_value: Some(1.0),
    max_value: Some(2.0),
    created_at_ms: END_MS,
    chunks: &CHUNKS,
}];
const MULTI_CHUNKS: [SessionWaveformChunkInput<'static>; 2] = [
    SessionWaveformChunkInput {
        chunk_index: 0,
        start_sample: 0,
        sample_count: 1,
        payload: &FIRST_CHUNK_PAYLOAD,
        min_value: Some(1.0),
        max_value: Some(1.0),
    },
    SessionWaveformChunkInput {
        chunk_index: 1,
        start_sample: 1,
        sample_count: 1,
        payload: &SECOND_CHUNK_PAYLOAD,
        min_value: Some(2.0),
        max_value: Some(2.0),
    },
];
const MULTI_WAVEFORMS: [SessionWaveformInput<'static>; 1] = [SessionWaveformInput {
    source_key: "waveform:multi",
    channel_key: "pap.series.flow_rate",
    unit: Some("L/min"),
    started_at_ms: START_MS,
    sample_interval_us: 1_000_000,
    sample_count: 2,
    encoding: "i16-le",
    min_value: Some(1.0),
    max_value: Some(2.0),
    created_at_ms: END_MS,
    chunks: &MULTI_CHUNKS,
}];

fn open_with_profile() -> Result<(Database, i64), Box<dyn Error>> {
    let mut database = Database::open_in_memory()?;
    let transaction = database.transaction()?;
    let profile = Profiles::new(&transaction).insert(&NewProfile {
        display_name: "Atomic import fixture",
        now_ms: START_MS - 10,
    })?;
    transaction.commit()?;
    Ok((database, profile.id))
}

fn claim_job(
    database: &Database,
    profile_id: i64,
    request_id: &str,
    token: &str,
    digest: &str,
    at_ms: i64,
) -> Result<opap_storage::ImportHistory, Box<dyn Error>> {
    let begun = database.imports().begin_or_get(&NewImport {
        profile_id,
        machine_id: None,
        import_key: request_id,
        source_uri: SOURCE_ID,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: Some("prepared"),
        created_at_ms: at_ms,
    })?;
    Ok(database
        .imports()
        .claim_execution(
            begun.history.id,
            &ImportExecutionClaim {
                profile_id,
                importer_name: "resmed",
                source_fingerprint: digest,
                input_digest: digest,
                options_digest: DIGEST_A,
                execution_token: token,
                claimed_at_ms: at_ms + 1,
            },
        )?
        .expect("job exists"))
}

fn snapshot(content_digest: &str) -> SessionSnapshotReplacement<'_> {
    SessionSnapshotReplacement {
        data: SessionDataReplacement {
            events: &EVENTS,
            waveforms: &WAVEFORMS,
        },
        provenance: SessionProvenanceInput {
            therapy_day: "2023-11-14",
            start_local_wall: "2023-11-14T22:13:20.000",
            end_local_wall: "2023-11-14T23:13:20.000",
            start_utc_offset_seconds: Some(0),
            end_utc_offset_seconds: Some(0),
            start_clock_correction_ms: 0,
            end_clock_correction_ms: 0,
            data_kind: SessionDataKind::Detailed,
            importer_name: "resmed",
            importer_schema: "opap-import/v1",
            id_algorithm: "resmed-session-id/v1",
            source_digest: DIGEST_A,
            content_digest,
        },
        slices: &[],
        summary: SessionSummaryInput {
            usage_ms: 3_600_000,
            metrics: &[],
        },
        settings: &[],
    }
}

fn sessions(content_digest: &str) -> [ImportSessionSnapshotInput<'_>; 2] {
    [
        ImportSessionSnapshotInput {
            source_key: "session:one",
            started_at_ms: START_MS,
            ended_at_ms: Some(END_MS),
            timezone_offset_minutes: Some(0),
            now_ms: END_MS + 1,
            snapshot: snapshot(content_digest),
        },
        ImportSessionSnapshotInput {
            source_key: "session:two",
            started_at_ms: START_MS,
            ended_at_ms: Some(END_MS),
            timezone_offset_minutes: Some(0),
            now_ms: END_MS + 1,
            snapshot: snapshot(content_digest),
        },
    ]
}

fn multi_chunk_session() -> [ImportSessionSnapshotInput<'static>; 1] {
    [ImportSessionSnapshotInput {
        source_key: "session:multi-chunk",
        started_at_ms: START_MS,
        ended_at_ms: Some(END_MS),
        timezone_offset_minutes: Some(0),
        now_ms: END_MS + 1,
        snapshot: SessionSnapshotReplacement {
            data: SessionDataReplacement {
                events: &EVENTS,
                waveforms: &MULTI_WAVEFORMS,
            },
            provenance: SessionProvenanceInput {
                therapy_day: "2023-11-14",
                start_local_wall: "2023-11-14T22:13:20.000",
                end_local_wall: "2023-11-14T23:13:20.000",
                start_utc_offset_seconds: Some(0),
                end_utc_offset_seconds: Some(0),
                start_clock_correction_ms: 0,
                end_clock_correction_ms: 0,
                data_kind: SessionDataKind::Detailed,
                importer_name: "resmed",
                importer_schema: "opap-import/v1",
                id_algorithm: "resmed-session-id/v1",
                source_digest: DIGEST_A,
                content_digest: DIGEST_A,
            },
            slices: &[],
            summary: SessionSummaryInput {
                usage_ms: 3_600_000,
                metrics: &[],
            },
            settings: &[],
        },
    }]
}

fn commit_input<'a>(
    profile_id: i64,
    job: &opap_storage::ImportHistory,
    digest: &'a str,
    token: &'a str,
    imported_sessions: &'a [ImportSessionSnapshotInput<'a>],
) -> ImportCommitInput<'a> {
    ImportCommitInput {
        profile_id,
        import_id: job.id,
        importer_name: "resmed",
        source_fingerprint: digest,
        input_digest: digest,
        options_digest: DIGEST_A,
        execution_token: token,
        execution_generation: job.execution_generation,
        machine: ImportMachineInput {
            source_key: "machine:stable",
            device_type: "positive_airway_pressure",
            manufacturer: "ResMed",
            model: "AirSense",
            model_number: "fixture",
            serial_number: "fixture",
            seen_at_ms: END_MS,
        },
        sessions: imported_sessions,
        finished_at_ms: job.updated_at_ms + 10,
    }
}

fn assert_no_import_output(database: &Database, profile_id: i64, import_id: i64) -> TestResult {
    assert_eq!(database.machines().list_by_profile(profile_id)?.len(), 0);
    for table in [
        "sessions",
        "events",
        "waveforms",
        "waveform_chunks",
        "session_provenance",
        "session_slices",
        "session_summary",
        "summary_metrics",
        "session_settings",
        "import_session_results",
    ] {
        let count: i64 = database.connection().query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0, "{table} must roll back");
    }
    let job = database.imports().get(import_id)?.expect("job");
    assert_eq!(job.status, ImportStatus::Running);
    assert_eq!(job.machine_id, None);
    assert_eq!(job.state_message, None);
    assert_eq!(job.sessions_created, 0);
    assert_eq!(job.sessions_updated, 0);
    assert_eq!(job.events_written, 0);
    assert_eq!(job.waveform_chunks_written, 0);
    Ok(())
}

#[test]
fn successful_two_session_commit_is_atomic_and_derives_counts_and_links() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let imported_sessions = sessions(DIGEST_A);
    let result = database.commit_import_result(
        &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
        || Ok(()),
    )?;

    assert_eq!(result.history.status, ImportStatus::Completed);
    assert_eq!(result.history.machine_id, Some(result.machine.id));
    assert_eq!(result.history.sessions_created, 2);
    assert_eq!(result.history.sessions_updated, 0);
    assert_eq!(result.history.events_written, 2);
    assert_eq!(result.history.waveform_chunks_written, 2);
    assert_eq!(result.history.execution_token, None);
    assert_eq!(
        result
            .sessions
            .iter()
            .map(|item| item.outcome)
            .collect::<Vec<_>>(),
        vec![ImportSessionOutcome::Created, ImportSessionOutcome::Created]
    );
    assert_eq!(
        database.imports().list_session_results(job.id)?,
        result.sessions
    );
    assert_eq!(
        database
            .sessions()
            .list_by_machine(result.machine.id)?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn completed_session_results_are_immutable_but_profile_cleanup_cascades() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let imported_sessions = sessions(DIGEST_A);
    let result = database.commit_import_result(
        &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
        || Ok(()),
    )?;
    let linked = result.sessions[0].clone();
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_session_results SET outcome = 'updated'
                 WHERE import_id = ?1 AND session_id = ?2",
                rusqlite::params![linked.import_id, linked.session_id],
            )
            .is_err()
    );
    assert!(
        database
            .connection()
            .execute(
                "DELETE FROM import_session_results
                 WHERE import_id = ?1 AND session_id = ?2",
                rusqlite::params![linked.import_id, linked.session_id],
            )
            .is_err()
    );

    let extra_session = database.sessions().upsert(&NewSession {
        machine_id: result.machine.id,
        source_key: "session:post-completion-forgery",
        started_at_ms: START_MS,
        ended_at_ms: Some(END_MS),
        timezone_offset_minutes: Some(0),
        now_ms: END_MS + 50,
    })?;
    assert!(
        database
            .connection()
            .execute(
                "INSERT INTO import_session_results (import_id, session_id, outcome)
                 VALUES (?1, ?2, 'created')",
                rusqlite::params![job.id, extra_session.id],
            )
            .is_err(),
        "completed jobs cannot acquire new result links"
    );
    assert_eq!(database.imports().list_session_results(job.id)?.len(), 2);

    database
        .connection()
        .execute("DELETE FROM profiles WHERE id = ?1", [profile_id])?;
    assert!(database.imports().get(job.id)?.is_none());
    let remaining: i64 = database.connection().query_row(
        "SELECT COUNT(*) FROM import_session_results",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(remaining, 0);
    Ok(())
}

#[test]
fn running_result_links_require_the_jobs_exact_machine_and_profile() -> TestResult {
    let (database, profile_id) = open_with_profile()?;
    let other_profile = database.profiles().insert(&NewProfile {
        display_name: "Other profile",
        now_ms: START_MS,
    })?;
    let own_machine = database.machines().upsert(&NewMachine {
        profile_id,
        source_key: "machine:owned",
        device_type: "positive_airway_pressure",
        manufacturer: "Test",
        model: "Owned",
        model_number: "",
        serial_number: "",
        seen_at_ms: START_MS,
    })?;
    let other_machine_same_profile = database.machines().upsert(&NewMachine {
        profile_id,
        source_key: "machine:other-same-profile",
        device_type: "positive_airway_pressure",
        manufacturer: "Test",
        model: "Other",
        model_number: "",
        serial_number: "",
        seen_at_ms: START_MS,
    })?;
    let cross_profile_machine = database.machines().upsert(&NewMachine {
        profile_id: other_profile.id,
        source_key: "machine:cross-profile",
        device_type: "positive_airway_pressure",
        manufacturer: "Test",
        model: "Cross",
        model_number: "",
        serial_number: "",
        seen_at_ms: START_MS,
    })?;
    let own_session = database.sessions().upsert(&NewSession {
        machine_id: own_machine.id,
        source_key: "session:owned",
        started_at_ms: START_MS,
        ended_at_ms: Some(END_MS),
        timezone_offset_minutes: Some(0),
        now_ms: END_MS,
    })?;
    let wrong_machine_session = database.sessions().upsert(&NewSession {
        machine_id: other_machine_same_profile.id,
        source_key: "session:wrong-machine",
        started_at_ms: START_MS,
        ended_at_ms: Some(END_MS),
        timezone_offset_minutes: Some(0),
        now_ms: END_MS,
    })?;
    let cross_profile_session = database.sessions().upsert(&NewSession {
        machine_id: cross_profile_machine.id,
        source_key: "session:cross-profile",
        started_at_ms: START_MS,
        ended_at_ms: Some(END_MS),
        timezone_offset_minutes: Some(0),
        now_ms: END_MS,
    })?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    database.connection().execute(
        "UPDATE import_history SET
             machine_id = ?2,
             state_message = 'committing_result'
         WHERE id = ?1",
        rusqlite::params![job.id, own_machine.id],
    )?;
    database.connection().execute(
        "INSERT INTO import_session_results (import_id, session_id, outcome)
         VALUES (?1, ?2, 'created')",
        rusqlite::params![job.id, own_session.id],
    )?;
    for invalid_session_id in [wrong_machine_session.id, cross_profile_session.id] {
        assert!(
            database
                .connection()
                .execute(
                    "INSERT INTO import_session_results (import_id, session_id, outcome)
                     VALUES (?1, ?2, 'created')",
                    rusqlite::params![job.id, invalid_session_id],
                )
                .is_err()
        );
    }
    assert_eq!(database.imports().list_session_results(job.id)?.len(), 1);
    Ok(())
}

#[test]
fn exact_replay_preserves_session_ids_and_records_unchanged_outcomes() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let first = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let imported_sessions = sessions(DIGEST_A);
    let first_result = database.commit_import_result(
        &commit_input(profile_id, &first, DIGEST_A, TOKEN_ONE, &imported_sessions),
        || Ok(()),
    )?;
    let preserved_session = database
        .sessions()
        .get(first_result.sessions[0].session_id)?
        .expect("first session");
    let preserved_events = database.events().list_by_session(preserved_session.id)?;
    let preserved_waveforms = database
        .waveforms()
        .list_metadata_by_session(preserved_session.id)?;
    let preserved_snapshot = database
        .session_snapshots()
        .get(preserved_session.id)?
        .expect("first snapshot");

    let replay = claim_job(
        &database,
        profile_id,
        REQUEST_TWO,
        TOKEN_TWO,
        DIGEST_A,
        END_MS + 20,
    )?;
    let replay_result = database.commit_import_result(
        &commit_input(profile_id, &replay, DIGEST_A, TOKEN_TWO, &imported_sessions),
        || Ok(()),
    )?;
    assert_eq!(
        first_result
            .sessions
            .iter()
            .map(|item| item.session_id)
            .collect::<Vec<_>>(),
        replay_result
            .sessions
            .iter()
            .map(|item| item.session_id)
            .collect::<Vec<_>>()
    );
    assert!(
        replay_result
            .sessions
            .iter()
            .all(|item| item.outcome == ImportSessionOutcome::Unchanged)
    );
    assert_eq!(replay_result.history.sessions_created, 0);
    assert_eq!(replay_result.history.sessions_updated, 0);
    assert_eq!(replay_result.history.events_written, 0);
    assert_eq!(replay_result.history.waveform_chunks_written, 0);
    assert_eq!(
        database
            .sessions()
            .get(preserved_session.id)?
            .expect("replayed session"),
        preserved_session
    );
    assert_eq!(
        database.events().list_by_session(preserved_session.id)?,
        preserved_events
    );
    assert_eq!(
        database
            .waveforms()
            .list_metadata_by_session(preserved_session.id)?,
        preserved_waveforms
    );
    assert_eq!(
        database
            .session_snapshots()
            .get(preserved_session.id)?
            .expect("replayed snapshot"),
        preserved_snapshot
    );
    Ok(())
}

#[test]
fn changed_content_updates_the_same_stable_sessions() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let first = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let original_sessions = sessions(DIGEST_A);
    let original = database.commit_import_result(
        &commit_input(profile_id, &first, DIGEST_A, TOKEN_ONE, &original_sessions),
        || Ok(()),
    )?;

    let changed = claim_job(
        &database,
        profile_id,
        REQUEST_THREE,
        TOKEN_THREE,
        DIGEST_C,
        END_MS + 20,
    )?;
    let changed_sessions = sessions(DIGEST_C);
    let updated = database.commit_import_result(
        &commit_input(
            profile_id,
            &changed,
            DIGEST_C,
            TOKEN_THREE,
            &changed_sessions,
        ),
        || Ok(()),
    )?;
    assert_eq!(
        original
            .sessions
            .iter()
            .map(|item| item.session_id)
            .collect::<Vec<_>>(),
        updated
            .sessions
            .iter()
            .map(|item| item.session_id)
            .collect::<Vec<_>>()
    );
    assert!(
        updated
            .sessions
            .iter()
            .all(|item| item.outcome == ImportSessionOutcome::Updated)
    );
    assert_eq!(updated.history.sessions_created, 0);
    assert_eq!(updated.history.sessions_updated, 2);
    Ok(())
}

#[test]
fn failure_on_second_snapshot_rolls_back_every_import_write() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    database.connection().execute_batch(
        "CREATE TRIGGER fail_second_snapshot
         BEFORE INSERT ON session_provenance
         WHEN (SELECT COUNT(*) FROM session_provenance) = 1
         BEGIN
             SELECT RAISE(ABORT, 'injected second snapshot failure');
         END;",
    )?;
    let imported_sessions = sessions(DIGEST_A);
    assert!(
        database
            .commit_import_result(
                &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
                || Ok(()),
            )
            .is_err()
    );
    assert_no_import_output(&database, profile_id, job.id)?;
    let persisted = database.imports().get(job.id)?.expect("job");
    assert_eq!(persisted.execution_token.as_deref(), Some(TOKEN_ONE));
    Ok(())
}

#[test]
fn final_job_update_failure_rolls_back_machine_sessions_and_links() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    database.connection().execute_batch(
        "CREATE TRIGGER fail_atomic_completion
         BEFORE UPDATE OF status ON import_history
         WHEN NEW.status = 'completed'
         BEGIN
             SELECT RAISE(ABORT, 'injected final update failure');
         END;",
    )?;
    let imported_sessions = sessions(DIGEST_A);
    assert!(
        database
            .commit_import_result(
                &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
                || Ok(()),
            )
            .is_err()
    );
    assert_no_import_output(&database, profile_id, job.id)?;
    Ok(())
}

#[test]
fn initial_commit_cas_misses_return_typed_lease_and_timestamp_errors() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let imported_sessions = sessions(DIGEST_A);

    let mut stale_token = commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions);
    stale_token.execution_token = TOKEN_TWO;
    assert!(matches!(
        database.commit_import_result(&stale_token, || Ok(())),
        Err(StorageError::StaleImportExecution { id }) if id == job.id
    ));

    let mut missing_job = commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions);
    missing_job.import_id = i64::MAX;
    assert!(matches!(
        database.commit_import_result(&missing_job, || Ok(())),
        Err(StorageError::StaleImportExecution { id }) if id == i64::MAX
    ));

    let mut stale_time = commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions);
    stale_time.finished_at_ms = job.updated_at_ms - 1;
    assert!(matches!(
        database.commit_import_result(&stale_time, || Ok(())),
        Err(StorageError::ImportTimestampRegression {
            id,
            previous_at_ms,
            attempted_at_ms,
        }) if id == job.id
            && previous_at_ms == job.updated_at_ms
            && attempted_at_ms == job.updated_at_ms - 1
    ));
    assert_no_import_output(&database, profile_id, job.id)?;
    Ok(())
}

#[test]
fn ignored_final_cas_update_returns_stale_lease_and_rolls_back() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    database.connection().execute_batch(
        "CREATE TRIGGER ignore_atomic_completion
         BEFORE UPDATE OF status ON import_history
         WHEN NEW.status = 'completed'
         BEGIN
             SELECT RAISE(IGNORE);
         END;",
    )?;
    let imported_sessions = sessions(DIGEST_A);
    let error = database
        .commit_import_result(
            &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
            || Ok(()),
        )
        .expect_err("ignored final update must be a stale CAS");
    assert!(matches!(
        error,
        StorageError::StaleImportExecution { id } if id == job.id
    ));
    assert_no_import_output(&database, profile_id, job.id)?;
    Ok(())
}

#[test]
fn cancellation_checkpoint_wins_before_commit_and_rolls_back() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let imported_sessions = sessions(DIGEST_A);
    let mut checkpoints = 0;
    let error = database
        .commit_import_result(
            &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
            || {
                checkpoints += 1;
                if checkpoints == 5 {
                    Err(StorageError::ImportInterrupted)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("checkpoint cancellation must abort the commit");
    assert!(matches!(error, StorageError::ImportInterrupted));
    assert_no_import_output(&database, profile_id, job.id)?;
    Ok(())
}

#[test]
fn cancellation_between_waveform_chunks_rolls_back_the_partial_chunk_set() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let job = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    let imported_sessions = multi_chunk_session();
    let mut checkpoints = 0;
    let error = database
        .commit_import_result(
            &commit_input(profile_id, &job, DIGEST_A, TOKEN_ONE, &imported_sessions),
            || {
                checkpoints += 1;
                if checkpoints == 9 {
                    Err(StorageError::ImportInterrupted)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("the second chunk checkpoint must interrupt the commit");
    assert!(matches!(error, StorageError::ImportInterrupted));
    assert_eq!(checkpoints, 9);
    assert_no_import_output(&database, profile_id, job.id)?;
    Ok(())
}

#[test]
fn recovery_invalidates_a_stale_token_and_increments_generation() -> TestResult {
    let (mut database, profile_id) = open_with_profile()?;
    let first_lease = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    assert_eq!(first_lease.execution_generation, 1);
    let recovered = database
        .imports()
        .recover_running(START_MS + 2, "/Users/alice/private-card")?;
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].state_message.as_deref(),
        Some("recovered_after_restart")
    );
    let second_lease = database
        .imports()
        .claim_execution(
            first_lease.id,
            &ImportExecutionClaim {
                profile_id,
                importer_name: "resmed",
                source_fingerprint: DIGEST_A,
                input_digest: DIGEST_A,
                options_digest: DIGEST_A,
                execution_token: TOKEN_TWO,
                claimed_at_ms: START_MS + 3,
            },
        )?
        .expect("recovered job");
    assert_eq!(second_lease.execution_generation, 2);

    let imported_sessions = sessions(DIGEST_A);
    let stale_error = database
        .commit_import_result(
            &commit_input(
                profile_id,
                &first_lease,
                DIGEST_A,
                TOKEN_ONE,
                &imported_sessions,
            ),
            || Ok(()),
        )
        .expect_err("old lease must not commit");
    assert!(matches!(
        stale_error,
        StorageError::StaleImportExecution { id } if id == first_lease.id
    ));
    assert_no_import_output(&database, profile_id, first_lease.id)?;

    let result = database.commit_import_result(
        &commit_input(
            profile_id,
            &second_lease,
            DIGEST_A,
            TOKEN_TWO,
            &imported_sessions,
        ),
        || Ok(()),
    )?;
    assert_eq!(result.history.status, ImportStatus::Completed);
    Ok(())
}

#[test]
fn worker_block_and_fail_cannot_mutate_a_newer_execution_generation() -> TestResult {
    let (database, profile_id) = open_with_profile()?;
    let first = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    database
        .imports()
        .recover_running(START_MS + 2, "application restarted")?;
    let second = database
        .imports()
        .claim_execution(
            first.id,
            &ImportExecutionClaim {
                profile_id,
                importer_name: "resmed",
                source_fingerprint: DIGEST_A,
                input_digest: DIGEST_A,
                options_digest: DIGEST_A,
                execution_token: TOKEN_TWO,
                claimed_at_ms: START_MS + 3,
            },
        )?
        .expect("new lease");
    let stale = ImportExecutionLease {
        profile_id,
        importer_name: "resmed",
        execution_token: TOKEN_ONE,
        execution_generation: first.execution_generation,
    };
    assert!(matches!(
        database
            .imports()
            .block_execution(
                first.id,
                &stale,
                START_MS + 4,
                ImportBlockCode::WorkerInterrupted,
            ),
        Err(StorageError::StaleImportExecution { id }) if id == first.id
    ));
    assert!(matches!(
        database
            .imports()
            .fail_execution(
                first.id,
                &stale,
                START_MS + 4,
                ImportFailureCode::InternalFailure,
            ),
        Err(StorageError::StaleImportExecution { id }) if id == first.id
    ));
    let still_current = database.imports().get(first.id)?.expect("current lease");
    assert_eq!(
        still_current.execution_generation,
        second.execution_generation
    );
    assert_eq!(still_current.execution_token.as_deref(), Some(TOKEN_TWO));

    let current = ImportExecutionLease {
        profile_id,
        importer_name: "resmed",
        execution_token: TOKEN_TWO,
        execution_generation: second.execution_generation,
    };
    let blocked = database
        .imports()
        .block_execution(
            first.id,
            &current,
            START_MS + 4,
            ImportBlockCode::RetryPending,
        )?
        .expect("current worker blocks");
    assert_eq!(blocked.status, ImportStatus::Blocked);
    assert_eq!(blocked.execution_token, None);
    assert_eq!(blocked.state_message.as_deref(), Some("retry_pending"));

    let third = database
        .imports()
        .claim_execution(
            first.id,
            &ImportExecutionClaim {
                profile_id,
                importer_name: "resmed",
                source_fingerprint: DIGEST_A,
                input_digest: DIGEST_A,
                options_digest: DIGEST_A,
                execution_token: TOKEN_THREE,
                claimed_at_ms: START_MS + 5,
            },
        )?
        .expect("third lease");
    let failed = database
        .imports()
        .fail_execution(
            first.id,
            &ImportExecutionLease {
                profile_id,
                importer_name: "resmed",
                execution_token: TOKEN_THREE,
                execution_generation: third.execution_generation,
            },
            START_MS + 6,
            ImportFailureCode::DecodeFailed,
        )?
        .expect("current worker fails");
    assert_eq!(failed.status, ImportStatus::Failed);
    assert_eq!(failed.execution_token, None);
    assert_eq!(failed.error_message.as_deref(), Some("decode_failed"));
    Ok(())
}

#[test]
fn leased_outcomes_reject_sensitive_free_text_even_through_direct_sql() -> TestResult {
    let (database, profile_id) = open_with_profile()?;
    let leased = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    for sql in [
        "UPDATE import_history SET
             state_message = '/Users/alice/private-card'
         WHERE id = ?1",
        "UPDATE import_history SET
             status = 'blocked',
             state_message = NULL,
             error_message = NULL,
             updated_at_ms = updated_at_ms + 1,
             execution_token = NULL
         WHERE id = ?1",
        "UPDATE import_history SET
             status = 'failed',
             state_message = NULL,
             error_message = NULL,
             updated_at_ms = updated_at_ms + 1,
             completed_at_ms = updated_at_ms + 1,
             execution_token = NULL
         WHERE id = ?1",
        "UPDATE import_history SET
             status = 'cancelled',
             state_message = NULL,
             error_message = NULL,
             updated_at_ms = updated_at_ms + 1,
             completed_at_ms = updated_at_ms + 1,
             execution_token = NULL
         WHERE id = ?1",
        "UPDATE import_history SET
             status = 'blocked',
             state_message = '/Users/alice/private-card',
             updated_at_ms = updated_at_ms + 1,
             execution_token = NULL
         WHERE id = ?1",
        "UPDATE import_history SET
             status = 'failed',
             state_message = NULL,
             error_message = 'opap-execution:00000000000000000000000000000001',
             updated_at_ms = updated_at_ms + 1,
             completed_at_ms = updated_at_ms + 1,
             execution_token = NULL
         WHERE id = ?1",
        "UPDATE import_history SET
             status = 'completed',
             state_message = NULL,
             error_message = NULL,
             sessions_created = 999,
             sessions_updated = 999,
             events_written = 999,
             waveform_chunks_written = 999,
             updated_at_ms = updated_at_ms + 1,
             completed_at_ms = updated_at_ms + 1,
             execution_token = NULL
         WHERE id = ?1",
    ] {
        assert!(
            database.connection().execute(sql, [leased.id]).is_err(),
            "leased transitions accept only stable non-sensitive codes"
        );
    }
    let unchanged = database.imports().get(leased.id)?.expect("leased job");
    assert_eq!(unchanged.status, ImportStatus::Running);
    assert_eq!(unchanged.execution_token.as_deref(), Some(TOKEN_ONE));
    assert_eq!(unchanged.state_message, None);
    assert_eq!(unchanged.error_message, None);
    Ok(())
}

#[test]
fn execution_identity_constraints_reject_paths_uppercase_and_wrong_lengths() -> TestResult {
    let (database, profile_id) = open_with_profile()?;
    let begun = database.imports().begin_or_get(&NewImport {
        profile_id,
        machine_id: None,
        import_key: REQUEST_ONE,
        source_uri: SOURCE_ID,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: None,
        created_at_ms: START_MS,
    })?;
    for claim in [
        ImportExecutionClaim {
            profile_id,
            importer_name: "resmed",
            source_fingerprint: "/Users/alice/private-card",
            input_digest: DIGEST_A,
            options_digest: DIGEST_A,
            execution_token: TOKEN_ONE,
            claimed_at_ms: START_MS + 1,
        },
        ImportExecutionClaim {
            profile_id,
            importer_name: "resmed",
            source_fingerprint: DIGEST_A,
            input_digest: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            options_digest: DIGEST_A,
            execution_token: TOKEN_ONE,
            claimed_at_ms: START_MS + 1,
        },
        ImportExecutionClaim {
            profile_id,
            importer_name: "resmed",
            source_fingerprint: DIGEST_A,
            input_digest: DIGEST_A,
            options_digest: DIGEST_A,
            execution_token: "opap-execution:/Volumes/private-card",
            claimed_at_ms: START_MS + 1,
        },
    ] {
        assert!(
            database
                .imports()
                .claim_execution(begun.history.id, &claim)
                .is_err()
        );
    }
    let (fingerprint, input_digest, options_digest, token): (
        String,
        String,
        String,
        Option<String>,
    ) = database.connection().query_row(
        "SELECT source_fingerprint, input_digest, options_digest, execution_token
             FROM import_history WHERE id = ?1",
        [begun.history.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(
        (fingerprint, input_digest, options_digest),
        Default::default()
    );
    assert_eq!(token, None);
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET source_fingerprint = ?2 WHERE id = ?1",
                rusqlite::params![begun.history.id, format!("{DIGEST_A}0")],
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn execution_claim_atomically_checks_profile_and_importer() -> TestResult {
    let (database, profile_id) = open_with_profile()?;
    let begun = database.imports().begin_or_get(&NewImport {
        profile_id,
        machine_id: None,
        import_key: REQUEST_ONE,
        source_uri: SOURCE_ID,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: None,
        created_at_ms: START_MS,
    })?;
    for (expected_profile, expected_importer) in
        [(profile_id + 1, "resmed"), (profile_id, "not-resmed")]
    {
        assert!(
            database
                .imports()
                .claim_execution(
                    begun.history.id,
                    &ImportExecutionClaim {
                        profile_id: expected_profile,
                        importer_name: expected_importer,
                        source_fingerprint: DIGEST_A,
                        input_digest: DIGEST_A,
                        options_digest: DIGEST_A,
                        execution_token: TOKEN_ONE,
                        claimed_at_ms: START_MS + 1,
                    },
                )
                .is_err()
        );
    }
    let unchanged = database
        .imports()
        .get(begun.history.id)?
        .expect("unclaimed job");
    assert_eq!(unchanged.status, ImportStatus::Blocked);
    assert_eq!(unchanged.execution_generation, 0);
    assert_eq!(unchanged.execution_token, None);
    Ok(())
}

#[test]
fn legacy_complete_and_fail_cannot_bypass_an_execution_lease() -> TestResult {
    let (database, profile_id) = open_with_profile()?;
    let leased = claim_job(
        &database,
        profile_id,
        REQUEST_ONE,
        TOKEN_ONE,
        DIGEST_A,
        START_MS,
    )?;
    assert!(
        database
            .imports()
            .complete(leased.id, START_MS + 2, ImportCounts::default())
            .is_err()
    );
    assert!(
        database
            .imports()
            .fail(leased.id, START_MS + 2, "legacy failure")
            .is_err()
    );
    let still_leased = database.imports().get(leased.id)?.expect("leased job");
    assert_eq!(still_leased.status, ImportStatus::Running);
    assert_eq!(still_leased.execution_token.as_deref(), Some(TOKEN_ONE));
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET execution_token = NULL WHERE id = ?1",
                [leased.id],
            )
            .is_err(),
        "a token cannot be cleared while its job remains running"
    );

    let blocked = database
        .imports()
        .block(leased.id, START_MS + 2, "/Users/alice/private-card")?
        .expect("blocked job");
    assert_eq!(blocked.status, ImportStatus::Blocked);
    assert_eq!(blocked.execution_token, None);
    assert_eq!(blocked.state_message.as_deref(), Some("admin_revoked"));
    assert!(database.imports().start(leased.id, START_MS + 3).is_err());
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET status = 'running', state_message = NULL
                 WHERE id = ?1",
                [leased.id],
            )
            .is_err(),
        "a generation-scoped blocked job requires claim_execution"
    );
    let still_blocked = database.imports().get(leased.id)?.expect("blocked job");
    assert_eq!(still_blocked.status, ImportStatus::Blocked);
    assert_eq!(still_blocked.sessions_created, 0);
    let cancelled = database
        .imports()
        .cancel(
            leased.id,
            START_MS + 4,
            Some("opap-execution:private-token"),
        )?
        .expect("cancelled job");
    assert_eq!(cancelled.status, ImportStatus::Cancelled);
    assert_eq!(cancelled.state_message.as_deref(), Some("user_cancelled"));
    Ok(())
}
