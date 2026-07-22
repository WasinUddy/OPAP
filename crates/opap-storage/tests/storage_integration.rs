use opap_storage::repository::{Events, Imports, Machines, Profiles, Sessions, Waveforms};
use opap_storage::{
    APPLICATION_ID, Database, Error as StorageError, ImportCounts, ImportStatus,
    LATEST_SCHEMA_VERSION, NewEvent, NewImport, NewMachine, NewProfile, NewSession,
    NewWaveformChunk, NewWaveformMetadata, SessionDataReplacement, SessionEventInput,
    SessionWaveformChunkInput, SessionWaveformInput,
};
use std::error::Error;
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

struct FixtureIds {
    profile: i64,
    machine: i64,
    session: i64,
    event: i64,
    waveform: i64,
    import: i64,
}

fn temporary_database() -> Result<(TempDir, Database), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(directory.path().join("opap.sqlite3"))?;
    Ok((directory, database))
}

fn seed_database(database: &mut Database) -> Result<FixtureIds, Box<dyn Error>> {
    let transaction = database.transaction()?;
    let profile = Profiles::new(&transaction).insert(&NewProfile {
        display_name: "Alex",
        now_ms: 1_700_000_000_000,
    })?;
    let machine = Machines::new(&transaction).upsert(&NewMachine {
        profile_id: profile.id,
        source_key: "resmed:23212345678",
        device_type: "positive_airway_pressure",
        manufacturer: "ResMed",
        model: "AirSense 11 AutoSet",
        model_number: "39421",
        serial_number: "23212345678",
        seen_at_ms: 1_700_000_001_000,
    })?;
    let session = Sessions::new(&transaction).upsert(&NewSession {
        machine_id: machine.id,
        source_key: "2023-11-14T22:13:20Z",
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: Some(1_700_028_800_000),
        timezone_offset_minutes: Some(420),
        now_ms: 1_700_028_801_000,
    })?;
    let event = Events::new(&transaction).upsert(&NewEvent {
        session_id: session.id,
        source_key: "oa:42",
        channel_key: "respiratory_events",
        event_type: "obstructive_apnea",
        starts_at_ms: 1_700_003_000_000,
        duration_ms: Some(12_000),
        value: Some(1.0),
        unit: None,
        created_at_ms: 1_700_028_801_000,
    })?;
    let waveform = Waveforms::new(&transaction).upsert_metadata(&NewWaveformMetadata {
        session_id: session.id,
        source_key: "flow:2023-11-14T22:13:20Z",
        channel_key: "flow_rate",
        unit: Some("L/min"),
        started_at_ms: 1_700_000_000_000,
        sample_interval_us: 40_000,
        sample_count: 4,
        encoding: "f32-le",
        min_value: Some(-12.5),
        max_value: Some(18.75),
        created_at_ms: 1_700_028_801_000,
    })?;
    Waveforms::new(&transaction).upsert_chunk(&NewWaveformChunk {
        waveform_id: waveform.id,
        chunk_index: 0,
        start_sample: 0,
        sample_count: 4,
        payload: &[0, 0, 72, 65, 0, 0, 0, 0, 0, 0, 150, 193, 0, 0, 128, 63],
        min_value: Some(-12.5),
        max_value: Some(18.75),
    })?;
    let started = Imports::new(&transaction).begin_or_get(&NewImport {
        profile_id: profile.id,
        machine_id: Some(machine.id),
        import_key: "sha256:fixture-card-v1",
        source_uri: "file:///Volumes/RESMED",
        loader_name: "resmed",
        started_at_ms: 1_700_028_800_000,
    })?;
    let completed = Imports::new(&transaction)
        .complete(
            started.history.id,
            1_700_028_802_000,
            ImportCounts {
                sessions_created: 1,
                sessions_updated: 0,
                events_written: 1,
                waveform_chunks_written: 1,
            },
        )?
        .expect("new import history row exists");
    transaction.commit()?;

    Ok(FixtureIds {
        profile: profile.id,
        machine: machine.id,
        session: session.id,
        event: event.id,
        waveform: waveform.id,
        import: completed.id,
    })
}

#[test]
fn migrates_new_database_and_reopening_is_a_noop() -> TestResult {
    let (directory, database) = temporary_database()?;

    assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
    let migrations = database.applied_migrations()?;
    assert_eq!(
        migrations
            .iter()
            .map(|migration| (migration.version, migration.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (1, "initial_storage"),
            (2, "query_indexes"),
            (3, "storage_integrity"),
        ]
    );
    assert!(migrations.iter().all(|item| item.applied_at_ms > 0));

    let foreign_keys: i64 = database
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    assert_eq!(foreign_keys, 1);
    let application_id: i64 =
        database
            .connection()
            .query_row("PRAGMA application_id", [], |row| row.get(0))?;
    assert_eq!(application_id, i64::from(APPLICATION_ID));
    drop(database);

    let reopened = Database::open(directory.path().join("opap.sqlite3"))?;
    assert_eq!(reopened.schema_version()?, LATEST_SCHEMA_VERSION);
    assert_eq!(reopened.applied_migrations()?.len(), 3);
    Ok(())
}

#[test]
fn repositories_round_trip_a_complete_import() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;

    let profile = database.profiles().get(ids.profile)?.expect("profile");
    assert_eq!(profile.display_name, "Alex");

    let machine = database.machines().get(ids.machine)?.expect("machine");
    assert_eq!(machine.manufacturer, "ResMed");
    assert_eq!(machine.serial_number, "23212345678");
    assert_eq!(database.machines().list_by_profile(profile.id)?.len(), 1);

    let session = database.sessions().get(ids.session)?.expect("session");
    assert_eq!(session.timezone_offset_minutes, Some(420));
    assert_eq!(database.sessions().list_by_machine(machine.id)?.len(), 1);

    let events = database.events().list_by_session(session.id)?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, ids.event);
    assert_eq!(events[0].event_type, "obstructive_apnea");
    assert_eq!(events[0].duration_ms, Some(12_000));

    let metadata = database
        .waveforms()
        .get_metadata(ids.waveform)?
        .expect("waveform metadata");
    assert_eq!(metadata.encoding, "f32-le");
    assert_eq!(metadata.sample_interval_us, 40_000);
    let chunks = database.waveforms().list_chunks(metadata.id)?;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].payload.len(), 16);
    database.waveforms().validate_complete(metadata.id)?;

    let import = database.imports().get(ids.import)?.expect("import history");
    assert_eq!(import.status, ImportStatus::Completed);
    assert_eq!(import.sessions_created, 1);
    assert_eq!(import.events_written, 1);
    assert_eq!(import.waveform_chunks_written, 1);
    Ok(())
}

#[test]
fn reimport_upserts_are_idempotent() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;

    let machine = database.machines().upsert(&NewMachine {
        profile_id: ids.profile,
        source_key: "resmed:23212345678",
        device_type: "positive_airway_pressure",
        manufacturer: "ResMed",
        model: "AirSense 11 AutoSet",
        model_number: "39421",
        serial_number: "23212345678",
        seen_at_ms: 1_700_100_000_000,
    })?;
    let session = database.sessions().upsert(&NewSession {
        machine_id: machine.id,
        source_key: "2023-11-14T22:13:20Z",
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: Some(1_700_028_900_000),
        timezone_offset_minutes: Some(420),
        now_ms: 1_700_100_000_000,
    })?;
    let event = database.events().upsert(&NewEvent {
        session_id: session.id,
        source_key: "oa:42",
        channel_key: "respiratory_events",
        event_type: "obstructive_apnea",
        starts_at_ms: 1_700_003_000_000,
        duration_ms: Some(13_000),
        value: Some(1.0),
        unit: None,
        created_at_ms: 1_700_100_000_000,
    })?;
    let waveform = database.waveforms().upsert_metadata(&NewWaveformMetadata {
        session_id: session.id,
        source_key: "flow:2023-11-14T22:13:20Z",
        channel_key: "flow_rate",
        unit: Some("L/min"),
        started_at_ms: 1_700_000_000_000,
        sample_interval_us: 40_000,
        sample_count: 4,
        encoding: "f32-le",
        min_value: Some(-12.5),
        max_value: Some(20.0),
        created_at_ms: 1_700_100_000_000,
    })?;
    let chunk = database.waveforms().upsert_chunk(&NewWaveformChunk {
        waveform_id: waveform.id,
        chunk_index: 0,
        start_sample: 0,
        sample_count: 4,
        payload: &[9; 16],
        min_value: Some(-12.5),
        max_value: Some(20.0),
    })?;
    let repeated_import = database.imports().begin_or_get(&NewImport {
        profile_id: ids.profile,
        machine_id: Some(ids.machine),
        import_key: "sha256:fixture-card-v1",
        source_uri: "file:///Volumes/RESMED",
        loader_name: "resmed",
        started_at_ms: 1_700_100_000_000,
    })?;

    assert_eq!(machine.id, ids.machine);
    assert_eq!(session.id, ids.session);
    assert_eq!(event.id, ids.event);
    assert_eq!(event.duration_ms, Some(13_000));
    assert_eq!(waveform.id, ids.waveform);
    assert_eq!(chunk.payload, vec![9; 16]);
    assert!(!repeated_import.inserted);
    assert_eq!(repeated_import.history.id, ids.import);
    assert_eq!(repeated_import.history.status, ImportStatus::Completed);

    for table in [
        "machines",
        "sessions",
        "events",
        "waveforms",
        "waveform_chunks",
        "import_history",
    ] {
        let count: i64 = database.connection().query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1, "{table} should not contain duplicates");
    }
    Ok(())
}

#[test]
fn deleting_profile_cascades_all_owned_data() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    assert!(database.profiles().delete(ids.profile)?);

    for table in [
        "profiles",
        "machines",
        "sessions",
        "events",
        "waveforms",
        "waveform_chunks",
        "import_history",
    ] {
        let count: i64 = database.connection().query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0, "{table} should be removed by cascade");
    }
    Ok(())
}

#[test]
fn dropping_import_transaction_rolls_back_every_write() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    {
        let transaction = database.transaction()?;
        let profile = Profiles::new(&transaction).insert(&NewProfile {
            display_name: "Rolled back",
            now_ms: 1_700_000_000_000,
        })?;
        Machines::new(&transaction).upsert(&NewMachine {
            profile_id: profile.id,
            source_key: "device:rollback",
            device_type: "test_device",
            manufacturer: "Test",
            model: "Rollback",
            model_number: "0",
            serial_number: "0",
            seen_at_ms: 1_700_000_000_000,
        })?;
        // No commit: rusqlite rolls the entire transaction back on drop.
    }

    assert!(database.profiles().list()?.is_empty());
    let machine_count: i64 =
        database
            .connection()
            .query_row("SELECT COUNT(*) FROM machines", [], |row| row.get(0))?;
    assert_eq!(machine_count, 0);
    Ok(())
}

#[test]
fn session_replacement_is_atomic_and_prunes_stale_derived_data() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    database.events().upsert(&NewEvent {
        session_id: ids.session,
        source_key: "stale-event",
        channel_key: "respiratory_events",
        event_type: "hypopnea",
        starts_at_ms: 1_700_004_000_000,
        duration_ms: Some(10_000),
        value: None,
        unit: None,
        created_at_ms: 1_700_028_801_000,
    })?;
    let stale_waveform = database.waveforms().upsert_metadata(&NewWaveformMetadata {
        session_id: ids.session,
        source_key: "stale-pressure",
        channel_key: "mask_pressure",
        unit: Some("cmH2O"),
        started_at_ms: 1_700_000_000_000,
        sample_interval_us: 500_000,
        sample_count: 1,
        encoding: "f32-le",
        min_value: Some(8.0),
        max_value: Some(8.0),
        created_at_ms: 1_700_028_801_000,
    })?;
    database.waveforms().upsert_chunk(&NewWaveformChunk {
        waveform_id: stale_waveform.id,
        chunk_index: 0,
        start_sample: 0,
        sample_count: 1,
        payload: &[0, 0, 0, 65],
        min_value: Some(8.0),
        max_value: Some(8.0),
    })?;

    let events = [SessionEventInput {
        source_key: "oa:42",
        channel_key: "respiratory_events",
        event_type: "obstructive_apnea",
        starts_at_ms: 1_700_003_000_000,
        duration_ms: Some(15_000),
        value: Some(1.0),
        unit: None,
        created_at_ms: 1_700_100_000_000,
    }];
    let invalid_chunks = [SessionWaveformChunkInput {
        chunk_index: 0,
        start_sample: 0,
        sample_count: 4,
        payload: &[1, 2, 3, 4],
        min_value: Some(-10.0),
        max_value: Some(20.0),
    }];
    let invalid_waveforms = [SessionWaveformInput {
        source_key: "flow:2023-11-14T22:13:20Z",
        channel_key: "flow_rate",
        unit: Some("L/min"),
        started_at_ms: 1_700_000_000_000,
        sample_interval_us: 40_000,
        sample_count: 4,
        encoding: "f32-le",
        min_value: Some(-10.0),
        max_value: Some(20.0),
        created_at_ms: 1_700_100_000_000,
        chunks: &invalid_chunks,
    }];
    let failed = database.replace_session_data(
        ids.session,
        &SessionDataReplacement {
            events: &events,
            waveforms: &invalid_waveforms,
        },
    );
    assert!(failed.is_err());
    assert_eq!(database.events().list_by_session(ids.session)?.len(), 2);
    assert_eq!(
        database
            .waveforms()
            .list_metadata_by_session(ids.session)?
            .len(),
        2
    );
    assert_eq!(
        database.waveforms().list_chunks(ids.waveform)?[0]
            .payload
            .len(),
        16
    );

    let valid_payload = [7_u8; 16];
    let valid_chunks = [SessionWaveformChunkInput {
        chunk_index: 0,
        start_sample: 0,
        sample_count: 4,
        payload: &valid_payload,
        min_value: Some(-10.0),
        max_value: Some(20.0),
    }];
    let valid_waveforms = [SessionWaveformInput {
        chunks: &valid_chunks,
        ..invalid_waveforms[0]
    }];
    let stats = database.replace_session_data(
        ids.session,
        &SessionDataReplacement {
            events: &events,
            waveforms: &valid_waveforms,
        },
    )?;

    assert_eq!(stats.events_pruned, 1);
    assert_eq!(stats.waveforms_pruned, 1);
    assert_eq!(stats.waveform_chunks_written, 1);
    let remaining_events = database.events().list_by_session(ids.session)?;
    assert_eq!(remaining_events.len(), 1);
    assert_eq!(remaining_events[0].source_key, "oa:42");
    assert_eq!(remaining_events[0].duration_ms, Some(15_000));
    let remaining_waveforms = database.waveforms().list_metadata_by_session(ids.session)?;
    assert_eq!(remaining_waveforms.len(), 1);
    assert_eq!(remaining_waveforms[0].id, ids.waveform);
    assert_eq!(
        database.waveforms().list_chunks(ids.waveform)?[0].payload,
        valid_payload
    );
    let chunk_count: i64 =
        database
            .connection()
            .query_row("SELECT COUNT(*) FROM waveform_chunks", [], |row| row.get(0))?;
    assert_eq!(chunk_count, 1, "stale waveform chunks must cascade away");
    assert!(
        database
            .waveforms()
            .get_metadata(stale_waveform.id)?
            .is_none()
    );
    Ok(())
}

#[test]
fn waveform_chunks_enforce_payload_bounds_overlap_and_coverage() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    let waveform = database.waveforms().upsert_metadata(&NewWaveformMetadata {
        session_id: ids.session,
        source_key: "integrity-test",
        channel_key: "flow_rate",
        unit: Some("L/min"),
        started_at_ms: 1_700_000_000_000,
        sample_interval_us: 40_000,
        sample_count: 4,
        encoding: "f32-le",
        min_value: None,
        max_value: None,
        created_at_ms: 1_700_100_000_000,
    })?;

    assert!(
        database
            .waveforms()
            .upsert_chunk(&NewWaveformChunk {
                waveform_id: waveform.id,
                chunk_index: 0,
                start_sample: 0,
                sample_count: 2,
                payload: &[0; 4],
                min_value: None,
                max_value: None,
            })
            .is_err()
    );
    assert!(
        database
            .waveforms()
            .upsert_chunk(&NewWaveformChunk {
                waveform_id: waveform.id,
                chunk_index: 0,
                start_sample: 3,
                sample_count: 2,
                payload: &[0; 8],
                min_value: None,
                max_value: None,
            })
            .is_err()
    );
    database.waveforms().upsert_chunk(&NewWaveformChunk {
        waveform_id: waveform.id,
        chunk_index: 0,
        start_sample: 0,
        sample_count: 2,
        payload: &[0; 8],
        min_value: None,
        max_value: None,
    })?;
    assert!(
        database
            .waveforms()
            .upsert_chunk(&NewWaveformChunk {
                waveform_id: waveform.id,
                chunk_index: 1,
                start_sample: 1,
                sample_count: 2,
                payload: &[0; 8],
                min_value: None,
                max_value: None,
            })
            .is_err()
    );
    database.waveforms().upsert_chunk(&NewWaveformChunk {
        waveform_id: waveform.id,
        chunk_index: 1,
        start_sample: 3,
        sample_count: 1,
        payload: &[0; 4],
        min_value: None,
        max_value: None,
    })?;
    assert!(database.waveforms().validate_complete(waveform.id).is_err());

    database.waveforms().delete_chunks(waveform.id)?;
    for (index, start) in [(0, 0), (1, 2)] {
        database.waveforms().upsert_chunk(&NewWaveformChunk {
            waveform_id: waveform.id,
            chunk_index: index,
            start_sample: start,
            sample_count: 2,
            payload: &[0; 8],
            min_value: None,
            max_value: None,
        })?;
    }
    database.waveforms().validate_complete(waveform.id)?;

    let direct_insert = database.connection().execute(
        "INSERT INTO waveform_chunks
         (waveform_id, chunk_index, start_sample, sample_count, payload)
         VALUES (?1, 2, 0, 1, X'00')",
        [waveform.id],
    );
    assert!(
        direct_insert.is_err(),
        "database triggers must prevent bypass"
    );
    Ok(())
}

#[test]
fn import_machine_must_belong_to_the_same_profile() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    let other = database.profiles().insert(&NewProfile {
        display_name: "Other person",
        now_ms: 1_700_200_000_000,
    })?;

    let result = database.imports().begin_or_get(&NewImport {
        profile_id: other.id,
        machine_id: Some(ids.machine),
        import_key: "cross-profile",
        source_uri: "file:///tmp/card",
        loader_name: "resmed",
        started_at_ms: 1_700_200_000_000,
    });
    assert!(result.is_err());
    assert!(
        database
            .imports()
            .find_by_key(other.id, "cross-profile")?
            .is_none()
    );
    Ok(())
}

#[test]
fn import_completion_and_failure_cannot_rewrite_terminal_state() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;

    assert!(
        database
            .imports()
            .complete(ids.import, 1_700_300_000_000, ImportCounts::default())?
            .is_none()
    );
    assert!(
        database
            .imports()
            .fail(ids.import, 1_700_300_000_000, "too late")?
            .is_none()
    );
    let history = database.imports().get(ids.import)?.expect("history");
    assert_eq!(history.status, ImportStatus::Completed);
    assert_eq!(history.sessions_created, 1);
    assert!(history.error_message.is_none());
    Ok(())
}

#[test]
fn rejects_foreign_application_ids_and_tampered_migration_names() -> TestResult {
    let foreign_directory = tempfile::tempdir()?;
    let foreign_path = foreign_directory.path().join("foreign.sqlite3");
    let foreign = rusqlite::Connection::open(&foreign_path)?;
    foreign.pragma_update(None, "application_id", 123_456_i64)?;
    drop(foreign);
    let error = Database::open(&foreign_path)
        .err()
        .expect("foreign application id should fail");
    assert!(matches!(
        error,
        StorageError::UnexpectedApplicationId { found: 123_456, .. }
    ));

    let tampered_directory = tempfile::tempdir()?;
    let tampered_path = tampered_directory.path().join("tampered.sqlite3");
    drop(Database::open(&tampered_path)?);
    let tampered = rusqlite::Connection::open(&tampered_path)?;
    tampered.execute(
        "UPDATE schema_migrations SET name = 'not_the_real_migration' WHERE version = 2",
        [],
    )?;
    drop(tampered);
    let error = Database::open(&tampered_path)
        .err()
        .expect("tampered migration name should fail");
    assert!(matches!(
        error,
        StorageError::InvalidMigrationName { version: 2, .. }
    ));
    Ok(())
}
