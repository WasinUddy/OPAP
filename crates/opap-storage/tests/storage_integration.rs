use opap_storage::repository::{Events, Imports, Machines, Profiles, Sessions, Waveforms};
use opap_storage::{
    APPLICATION_ID, Database, Error as StorageError, ImportCounts, ImportStatus,
    InitialImportStatus, LATEST_SCHEMA_VERSION, NewEvent, NewImport, NewMachine, NewProfile,
    NewSession, NewWaveformChunk, NewWaveformMetadata, RetryImport, SessionDataReplacement,
    SessionEventInput, SessionWaveformChunkInput, SessionWaveformInput, is_canonical_request_id,
    is_legacy_request_id, is_persistable_import_key,
};
use std::error::Error;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

const SOURCE_ONE: &str = "opap-source:00000000000000000000000000000001";
const SOURCE_TWO: &str = "opap-source:00000000000000000000000000000002";
const SOURCE_THREE: &str = "opap-source:00000000000000000000000000000003";
const SOURCE_FOUR: &str = "opap-source:00000000000000000000000000000004";
const REQUEST_ONE: &str = "opap-request:00000000000000000000000000000001";
const REQUEST_TWO: &str = "opap-request:00000000000000000000000000000002";
const REQUEST_THREE: &str = "opap-request:00000000000000000000000000000003";
const REQUEST_FOUR: &str = "opap-request:00000000000000000000000000000004";
const REQUEST_FIVE: &str = "opap-request:00000000000000000000000000000005";
const REQUEST_SIX: &str = "opap-request:00000000000000000000000000000006";

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

fn sqlite_wal_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push("-wal");
    value.into()
}

fn database_at_schema_version(
    path: &Path,
    version: usize,
) -> Result<rusqlite::Connection, Box<dyn Error>> {
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
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", version as i64)?;
    Ok(connection)
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
        import_key: REQUEST_ONE,
        source_uri: SOURCE_ONE,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Running,
        state_message: None,
        created_at_ms: 1_700_028_800_000,
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
            (4, "import_job_states"),
            (5, "opaque_import_sources"),
            (6, "import_history_link_integrity"),
            (7, "opaque_import_keys"),
            (8, "session_snapshots"),
            (9, "atomic_import_commits"),
        ]
    );
    assert!(migrations.iter().all(|item| item.applied_at_ms > 0));

    let foreign_keys: i64 = database
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    assert_eq!(foreign_keys, 1);
    let secure_delete: i64 =
        database
            .connection()
            .query_row("PRAGMA secure_delete", [], |row| row.get(0))?;
    assert_eq!(secure_delete, 1);
    let application_id: i64 =
        database
            .connection()
            .query_row("PRAGMA application_id", [], |row| row.get(0))?;
    assert_eq!(application_id, i64::from(APPLICATION_ID));
    let user_version: i64 = database
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);
    drop(database);

    let reopened = Database::open(directory.path().join("opap.sqlite3"))?;
    assert_eq!(reopened.schema_version()?, LATEST_SCHEMA_VERSION);
    assert_eq!(
        reopened.applied_migrations()?.len(),
        LATEST_SCHEMA_VERSION as usize
    );
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
        import_key: REQUEST_ONE,
        source_uri: SOURCE_ONE,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Running,
        state_message: None,
        created_at_ms: 1_700_100_000_000,
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
    let updated_session = NewSession {
        machine_id: ids.machine,
        source_key: "2023-11-14T22:13:20Z",
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: Some(1_700_029_900_000),
        timezone_offset_minutes: Some(420),
        now_ms: 1_700_100_000_000,
    };
    let failed = database.replace_session(
        &updated_session,
        &SessionDataReplacement {
            events: &events,
            waveforms: &invalid_waveforms,
        },
    );
    assert!(failed.is_err());
    let unchanged_session = database.sessions().get(ids.session)?.expect("session");
    assert_eq!(unchanged_session.ended_at_ms, Some(1_700_028_800_000));
    assert_eq!(unchanged_session.updated_at_ms, 1_700_028_801_000);
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
    let replacement = database.replace_session(
        &updated_session,
        &SessionDataReplacement {
            events: &events,
            waveforms: &valid_waveforms,
        },
    )?;

    assert_eq!(replacement.session.id, ids.session);
    assert_eq!(replacement.session.ended_at_ms, Some(1_700_029_900_000));
    assert_eq!(replacement.stats.events_pruned, 1);
    assert_eq!(replacement.stats.waveforms_pruned, 1);
    assert_eq!(replacement.stats.waveform_chunks_written, 1);
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
        import_key: REQUEST_TWO,
        source_uri: SOURCE_TWO,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: Some("waiting for approval"),
        created_at_ms: 1_700_200_000_000,
    });
    assert!(result.is_err());
    assert!(
        database
            .imports()
            .find_by_key(other.id, REQUEST_TWO)?
            .is_none()
    );
    Ok(())
}

#[test]
fn import_completion_and_failure_cannot_rewrite_terminal_state() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;

    let completion_error = database
        .imports()
        .complete(ids.import, 1_700_300_000_000, ImportCounts::default())
        .expect_err("completed job cannot complete twice");
    assert!(matches!(
        completion_error,
        StorageError::InvalidImportTransition {
            operation: "complete",
            ..
        }
    ));
    let failure_error = database
        .imports()
        .fail(ids.import, 1_700_300_000_000, "too late")
        .expect_err("completed job cannot fail");
    assert!(matches!(
        failure_error,
        StorageError::InvalidImportTransition {
            operation: "fail",
            ..
        }
    ));
    let history = database.imports().get(ids.import)?.expect("history");
    assert_eq!(history.status, ImportStatus::Completed);
    assert_eq!(history.sessions_created, 1);
    assert!(history.error_message.is_none());
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET sessions_created = 99 WHERE id = ?1",
                [ids.import],
            )
            .is_err(),
        "database must reject direct mutation of terminal job results"
    );
    Ok(())
}

#[test]
fn terminal_import_machine_links_only_clear_during_foreign_key_cleanup() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    let replacement_machine = database.machines().upsert(&NewMachine {
        profile_id: ids.profile,
        source_key: "resmed:replacement",
        device_type: "positive_airway_pressure",
        manufacturer: "ResMed",
        model: "AirSense 11 AutoSet",
        model_number: "39421",
        serial_number: "replacement",
        seen_at_ms: 1_700_300_001_000,
    })?;

    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET machine_id = ?2 WHERE id = ?1",
                rusqlite::params![ids.import, replacement_machine.id],
            )
            .is_err(),
        "terminal history must not be relinked to another machine"
    );
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET machine_id = NULL WHERE id = ?1",
                [ids.import],
            )
            .is_err(),
        "terminal history must not be unlinked while its machine exists"
    );
    assert_eq!(
        database
            .imports()
            .get(ids.import)?
            .expect("terminal import")
            .machine_id,
        Some(ids.machine)
    );

    database
        .connection()
        .execute("DELETE FROM machines WHERE id = ?1", [ids.machine])?;
    assert_eq!(
        database
            .imports()
            .get(ids.import)?
            .expect("terminal import after machine removal")
            .machine_id,
        None,
        "ON DELETE SET NULL must remain available for genuine FK cleanup"
    );
    Ok(())
}

#[test]
fn blocked_jobs_start_and_cancel_through_guarded_transitions() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    let request = NewImport {
        profile_id: ids.profile,
        machine_id: Some(ids.machine),
        import_key: REQUEST_THREE,
        source_uri: SOURCE_THREE,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: Some("waiting for session importer"),
        created_at_ms: 1_700_400_000_000,
    };

    let begun = database.imports().begin_or_get(&request)?;
    assert!(begun.inserted);
    assert_eq!(begun.history.status, ImportStatus::Blocked);
    assert_eq!(begun.history.attempt, 1);
    assert_eq!(begun.history.started_at_ms, None);
    assert_eq!(
        begun.history.state_message.as_deref(),
        Some("waiting for session importer")
    );
    let duplicate = database.imports().begin_or_get(&request)?;
    assert!(!duplicate.inserted);
    assert_eq!(duplicate.history.id, begun.history.id);

    let blocked_completion =
        database
            .imports()
            .complete(begun.history.id, 1_700_400_001_000, ImportCounts::default());
    assert!(matches!(
        blocked_completion,
        Err(StorageError::InvalidImportTransition {
            operation: "complete",
            ..
        })
    ));
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET
                     status = 'completed', started_at_ms = created_at_ms,
                     completed_at_ms = ?2, updated_at_ms = ?2
                 WHERE id = ?1",
                rusqlite::params![begun.history.id, 1_700_400_001_000_i64],
            )
            .is_err(),
        "raw SQL must not bypass the import state machine"
    );

    let running = database
        .imports()
        .start(begun.history.id, 1_700_400_002_000)?
        .expect("job");
    assert_eq!(running.status, ImportStatus::Running);
    assert_eq!(running.started_at_ms, Some(1_700_400_002_000));
    assert!(running.state_message.is_none());
    assert!(matches!(
        database
            .imports()
            .block(begun.history.id, 1_700_400_001_000, "stale update"),
        Err(StorageError::ImportTimestampRegression { .. })
    ));
    assert_eq!(
        database
            .imports()
            .get(begun.history.id)?
            .expect("job")
            .status,
        ImportStatus::Running
    );

    let cancelled = database
        .imports()
        .cancel(
            begun.history.id,
            1_700_400_003_000,
            Some("cancelled by user"),
        )?
        .expect("job");
    assert_eq!(cancelled.status, ImportStatus::Cancelled);
    assert!(cancelled.status.is_terminal());
    assert_eq!(cancelled.completed_at_ms, Some(1_700_400_003_000));
    assert_eq!(
        cancelled.state_message.as_deref(),
        Some("cancelled by user")
    );
    assert!(cancelled.error_message.is_none());
    assert!(matches!(
        database.imports().start(cancelled.id, 1_700_400_004_000),
        Err(StorageError::InvalidImportTransition {
            operation: "start",
            ..
        })
    ));
    Ok(())
}

#[test]
fn fresh_databases_accept_only_opaque_source_and_request_ids() -> TestResult {
    let (directory, database) = temporary_database()?;
    let profile = database.profiles().insert(&NewProfile {
        display_name: "Import privacy",
        now_ms: 1,
    })?;
    for invalid in [
        "/Volumes/private-card",
        "file:///Users/alice/card",
        "opap-source:fixture",
        "opap-source:ABCDEF00000000000000000000000000",
        "opap-source:legacy-0",
        "opap-source:legacy-12345678901234567890",
    ] {
        let result = database.imports().begin_or_get(&NewImport {
            profile_id: profile.id,
            machine_id: None,
            import_key: REQUEST_SIX,
            source_uri: invalid,
            loader_name: "resmed",
            initial_status: InitialImportStatus::Blocked,
            state_message: None,
            created_at_ms: 1,
        });
        assert!(
            matches!(result, Err(StorageError::Integrity(_))),
            "accepted invalid source identifier {invalid:?}"
        );
    }

    for invalid in [
        "/Users/alice/23123456789/card",
        "opap-request:fixture",
        "opap-request:ABCDEF00000000000000000000000000",
        "opap-request:legacy-0",
        "opap-request:legacy-01",
        "opap-request:legacy-9223372036854775808",
        "opap-request:000000000000000000000000000000000",
    ] {
        let result = database.imports().begin_or_get(&NewImport {
            profile_id: profile.id,
            machine_id: None,
            import_key: invalid,
            source_uri: SOURCE_FOUR,
            loader_name: "resmed",
            initial_status: InitialImportStatus::Blocked,
            state_message: None,
            created_at_ms: 1,
        });
        assert!(
            matches!(result, Err(StorageError::Integrity(_))),
            "accepted invalid request identifier {invalid:?}"
        );
    }

    assert!(is_canonical_request_id(REQUEST_FIVE));
    assert!(is_persistable_import_key(REQUEST_FIVE));
    assert!(is_legacy_request_id(
        "opap-request:legacy-9223372036854775807"
    ));
    assert!(!is_legacy_request_id(
        "opap-request:legacy-9223372036854775808"
    ));

    let canonical = database.imports().begin_or_get(&NewImport {
        profile_id: profile.id,
        machine_id: None,
        import_key: REQUEST_FIVE,
        source_uri: SOURCE_ONE,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: None,
        created_at_ms: 2,
    })?;
    assert_eq!(canonical.history.import_key, REQUEST_FIVE);
    assert_eq!(canonical.history.source_uri, SOURCE_ONE);
    let legacy_source = database.imports().begin_or_get(&NewImport {
        profile_id: profile.id,
        machine_id: None,
        import_key: REQUEST_SIX,
        source_uri: "opap-source:legacy-9223372036854775807",
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: None,
        created_at_ms: 3,
    })?;
    assert_eq!(
        legacy_source.history.source_uri,
        "opap-source:legacy-9223372036854775807"
    );
    assert!(matches!(
        database.imports().begin_or_get(&NewImport {
            profile_id: profile.id,
            machine_id: None,
            import_key: "opap-request:legacy-9223372036854775807",
            source_uri: SOURCE_TWO,
            loader_name: "resmed",
            initial_status: InitialImportStatus::Blocked,
            state_message: None,
            created_at_ms: 4,
        }),
        Err(StorageError::Integrity(_))
    ));
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET source_uri = '/tmp/leak' WHERE id = ?1",
                [canonical.history.id],
            )
            .is_err(),
        "raw SQL must not bypass source identifier validation"
    );
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET import_key = '/tmp/serial-23123456789'
                 WHERE id = ?1",
                [canonical.history.id],
            )
            .is_err(),
        "raw SQL must not bypass request identifier validation on update"
    );
    assert!(
        database
            .connection()
            .execute(
                "INSERT INTO import_history (
                     profile_id, import_key, source_uri, loader_name, attempt,
                     status, created_at_ms, updated_at_ms
                 ) VALUES (?1, '/tmp/serial-23123456789', ?2, 'resmed', 1,
                           'blocked', 4, 4)",
                rusqlite::params![profile.id, SOURCE_TWO],
            )
            .is_err(),
        "raw SQL must not bypass request identifier validation on insert"
    );
    assert!(
        database
            .connection()
            .execute(
                "INSERT INTO import_history (
                     profile_id, import_key, source_uri, loader_name, attempt,
                     status, created_at_ms, updated_at_ms
                 ) VALUES (?1, 'opap-request:legacy-1', ?2, 'resmed', 1,
                           'blocked', 4, 4)",
                rusqlite::params![profile.id, SOURCE_TWO],
            )
            .is_err(),
        "callers must not mint migration-only legacy request keys"
    );
    let canonical_id = canonical.history.id;
    drop(database);
    let reopened = Database::open(directory.path().join("opap.sqlite3"))?;
    assert_eq!(
        reopened
            .imports()
            .get(canonical_id)?
            .expect("canonical import after reopen")
            .import_key,
        REQUEST_FIVE
    );
    Ok(())
}

#[test]
fn interrupted_jobs_recover_and_failed_attempts_retry_without_rewriting_history() -> TestResult {
    let (_directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    let begun = database.imports().begin_or_get(&NewImport {
        profile_id: ids.profile,
        machine_id: Some(ids.machine),
        import_key: REQUEST_FOUR,
        source_uri: SOURCE_FOUR,
        loader_name: "resmed",
        initial_status: InitialImportStatus::Running,
        state_message: Some("scanning"),
        created_at_ms: 1_700_500_000_000,
    })?;
    assert_eq!(begun.history.started_at_ms, Some(1_700_500_000_000));

    let recovered = database
        .imports()
        .recover_running(1_700_499_999_000, "interrupted during application shutdown")?;
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, begun.history.id);
    assert_eq!(recovered[0].status, ImportStatus::Blocked);
    assert_eq!(recovered[0].started_at_ms, Some(1_700_500_000_000));
    assert_eq!(recovered[0].updated_at_ms, 1_700_500_000_000);

    let resumed = database
        .imports()
        .start(begun.history.id, 1_700_500_002_000)?
        .expect("job");
    assert_eq!(resumed.started_at_ms, Some(1_700_500_000_000));
    let failed = database
        .imports()
        .fail(begun.history.id, 1_700_500_003_000, "card read failed")?
        .expect("job");
    assert_eq!(failed.status, ImportStatus::Failed);
    assert_eq!(failed.error_message.as_deref(), Some("card read failed"));

    let stale_retry_at_ms = failed.updated_at_ms - 1;
    assert!(matches!(
        database.imports().retry_or_get(
            failed.id,
            &RetryImport {
                initial_status: InitialImportStatus::Blocked,
                state_message: Some("stale retry"),
                created_at_ms: stale_retry_at_ms,
            }
        ),
        Err(StorageError::ImportTimestampRegression {
            id,
            previous_at_ms,
            attempted_at_ms,
        }) if id == failed.id
            && previous_at_ms == failed.updated_at_ms
            && attempted_at_ms == stale_retry_at_ms
    ));
    assert!(
        database
            .connection()
            .execute(
                "INSERT INTO import_history (
                     profile_id, machine_id, import_key, source_uri, loader_name,
                     attempt, retry_of_id, status, created_at_ms, updated_at_ms
                 )
                 SELECT
                     profile_id, machine_id, import_key, source_uri, loader_name,
                     attempt + 1, id, 'blocked', ?2, ?2
                 FROM import_history WHERE id = ?1",
                rusqlite::params![failed.id, stale_retry_at_ms],
            )
            .is_err(),
        "raw SQL must not create a retry before its parent attempt finished"
    );
    assert_eq!(
        database
            .imports()
            .list_by_profile(ids.profile)?
            .into_iter()
            .filter(|job| job.import_key == REQUEST_FOUR)
            .count(),
        1
    );

    let retry_request = RetryImport {
        initial_status: InitialImportStatus::Blocked,
        state_message: Some("ready to retry"),
        created_at_ms: 1_700_500_004_000,
    };
    let retry = database
        .imports()
        .retry_or_get(failed.id, &retry_request)?
        .expect("source attempt");
    assert!(retry.inserted);
    assert_eq!(retry.history.attempt, 2);
    assert_eq!(retry.history.retry_of_id, Some(failed.id));
    assert_eq!(retry.history.status, ImportStatus::Blocked);
    assert_eq!(retry.history.started_at_ms, None);
    let duplicate_retry = database
        .imports()
        .retry_or_get(failed.id, &retry_request)?
        .expect("source attempt");
    assert!(!duplicate_retry.inserted);
    assert_eq!(duplicate_retry.history.id, retry.history.id);
    assert_eq!(
        database
            .imports()
            .find_by_key(ids.profile, REQUEST_FOUR)?
            .expect("latest attempt")
            .id,
        retry.history.id
    );

    database
        .imports()
        .start(retry.history.id, 1_700_500_005_000)?
        .expect("retry");
    let completed_retry = database
        .imports()
        .complete(
            retry.history.id,
            1_700_500_006_000,
            ImportCounts {
                sessions_created: 1,
                ..ImportCounts::default()
            },
        )?
        .expect("retry");
    assert_eq!(completed_retry.status, ImportStatus::Completed);
    assert!(matches!(
        database.imports().retry_or_get(
            completed_retry.id,
            &RetryImport {
                created_at_ms: 1_700_500_007_000,
                ..retry_request
            }
        ),
        Err(StorageError::InvalidImportTransition {
            operation: "retry",
            ..
        })
    ));
    assert_eq!(
        database
            .imports()
            .list_by_profile(ids.profile)?
            .into_iter()
            .filter(|job| job.import_key == REQUEST_FOUR)
            .count(),
        2
    );
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET retry_of_id = ?2 WHERE id = ?1",
                rusqlite::params![completed_retry.id, ids.import],
            )
            .is_err(),
        "terminal retry history must not be relinked"
    );
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET retry_of_id = NULL WHERE id = ?1",
                [completed_retry.id],
            )
            .is_err(),
        "terminal retry history must not be directly unlinked"
    );
    database
        .connection()
        .execute("DELETE FROM import_history WHERE id = ?1", [failed.id])?;
    let detached_retry = database
        .imports()
        .get(completed_retry.id)?
        .expect("retry remains after its parent is removed");
    assert_eq!(detached_retry.status, ImportStatus::Completed);
    assert_eq!(detached_retry.retry_of_id, None);
    Ok(())
}

#[test]
fn migrates_legacy_in_progress_and_magic_cancelled_jobs_to_typed_states() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("legacy-v3.sqlite3");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE schema_migrations (
             version INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             applied_at_ms INTEGER NOT NULL
         ) STRICT;",
    )?;
    for (version, name, sql) in [
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
    ] {
        connection.execute_batch(sql)?;
        connection.execute(
            "INSERT INTO schema_migrations (version, name, applied_at_ms) VALUES (?1, ?2, ?1)",
            rusqlite::params![version, name],
        )?;
    }
    connection.execute(
        "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
         VALUES (1, 'Legacy', 1000, 1000)",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, status,
             started_at_ms, completed_at_ms
         ) VALUES (
             6, 1, 'legacy-clock-rollback', '/private/clock-rollback', 'resmed', 'completed',
             2100, 2000
         )",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, status,
             started_at_ms, completed_at_ms
         ) VALUES (
             3, 1, 'SN23123456789:/Users/alice/private-card',
             '/Users/alice/private-card', 'resmed', 'completed',
             1400, 1500
         )",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, status,
             started_at_ms, completed_at_ms, error_message
         ) VALUES (
             4, 1, 'legacy-failed', 'file:///Volumes/patient-card', 'resmed', 'failed',
             1600, 1700, 'card read failed'
         )",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, status,
             started_at_ms, completed_at_ms, error_message
         ) VALUES (
             5, 1, 'legacy-empty-error', 'C:\\Patients\\card', 'resmed', 'failed',
             1800, 1900, ''
         )",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, status, started_at_ms
         ) VALUES (1, 1, 'legacy-running', 'source:one', 'resmed', 'in_progress', 1100)",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, status,
             started_at_ms, completed_at_ms, error_message
         ) VALUES (
             2, 1, 'legacy-cancelled', 'source:two', 'resmed', 'failed',
             1200, 1300, 'opap.service.cancelled.v1'
         )",
        [],
    )?;
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", 3_i64)?;
    drop(connection);

    let database = Database::open(&path)?;
    assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
    let running = database.imports().get(1)?.expect("legacy running job");
    assert_eq!(running.status, ImportStatus::Running);
    assert_eq!(running.attempt, 1);
    assert_eq!(running.retry_of_id, None);
    assert_eq!(running.created_at_ms, 1100);
    assert_eq!(running.started_at_ms, Some(1100));
    let cancelled = database.imports().get(2)?.expect("legacy cancelled job");
    assert_eq!(cancelled.status, ImportStatus::Cancelled);
    assert_eq!(cancelled.completed_at_ms, Some(1300));
    assert!(cancelled.error_message.is_none());
    assert!(cancelled.state_message.is_some());
    for id in 1..=6 {
        let history = database.imports().get(id)?.expect("migrated job");
        assert_eq!(history.import_key, format!("opap-request:legacy-{id}"));
        assert!(!history.import_key.contains("SN23123456789"));
        assert!(!history.import_key.contains('/'));
        assert_eq!(history.source_uri, format!("opap-source:legacy-{id}"));
        assert!(!history.source_uri.contains('/'));
        assert!(!history.source_uri.contains('\\'));
    }
    assert_eq!(
        database.imports().get(3)?.expect("completed").status,
        ImportStatus::Completed
    );
    assert_eq!(
        database.imports().get(4)?.expect("failed").status,
        ImportStatus::Failed
    );
    assert_eq!(
        database
            .imports()
            .get(5)?
            .expect("normalized failure")
            .error_message
            .as_deref(),
        Some("legacy import failed without an error message")
    );
    let clock_rollback = database.imports().get(6)?.expect("clock rollback job");
    assert_eq!(clock_rollback.completed_at_ms, Some(2100));
    assert_eq!(clock_rollback.updated_at_ms, 2100);
    Ok(())
}

#[test]
fn upgrades_v1_and_v2_databases_directly_without_retaining_source_paths() -> TestResult {
    for legacy_version in [1_usize, 2_usize] {
        let directory = tempfile::tempdir()?;
        let path = directory
            .path()
            .join(format!("legacy-v{legacy_version}.sqlite3"));
        let connection = rusqlite::Connection::open(&path)?;
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
        ];
        for (version, name, sql) in migrations.into_iter().take(legacy_version) {
            connection.execute_batch(sql)?;
            connection.execute(
                "INSERT INTO schema_migrations (version, name, applied_at_ms)
                 VALUES (?1, ?2, ?1)",
                rusqlite::params![version, name],
            )?;
        }
        connection.execute(
            "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
             VALUES (1, 'Legacy', 1000, 1000)",
            [],
        )?;
        connection.execute(
            "INSERT INTO import_history (
                 id, profile_id, import_key, source_uri, loader_name, status, started_at_ms
             ) VALUES (
                 1, 1, 'SN23123456789:/Volumes/private-card', '/Volumes/private-card',
                 'resmed', 'in_progress', 1100
             )",
            [],
        )?;
        connection.pragma_update(None, "application_id", APPLICATION_ID)?;
        connection.pragma_update(None, "user_version", legacy_version as i64)?;
        drop(connection);

        let database = Database::open(&path)?;
        assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
        let history = database.imports().get(1)?.expect("upgraded job");
        assert_eq!(history.status, ImportStatus::Running);
        assert_eq!(history.import_key, "opap-request:legacy-1");
        assert!(!history.import_key.contains("SN23123456789"));
        assert_eq!(history.source_uri, "opap-source:legacy-1");
        assert!(!history.source_uri.contains("private-card"));
    }
    Ok(())
}

#[test]
fn every_pre_v7_schema_scrubs_serial_and_path_import_keys() -> TestResult {
    for legacy_version in 1_usize..=6 {
        let directory = tempfile::tempdir()?;
        let path = directory
            .path()
            .join(format!("import-key-privacy-v{legacy_version}.sqlite3"));
        let connection = database_at_schema_version(&path, legacy_version)?;
        connection.execute(
            "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
             VALUES (1, 'Legacy privacy', 1000, 1000)",
            [],
        )?;
        let canary = format!("SN23123456789:/Users/alice/v{legacy_version}/SD_CARD");
        if legacy_version <= 3 {
            connection.execute(
                "INSERT INTO import_history (
                     id, profile_id, import_key, source_uri, loader_name, status, started_at_ms
                 ) VALUES (71, 1, ?1, '/Volumes/patient-card',
                           'resmed', 'in_progress', 1100)",
                [&canary],
            )?;
        } else {
            connection.execute(
                "INSERT INTO import_history (
                     id, profile_id, import_key, source_uri, loader_name, attempt,
                     status, created_at_ms, updated_at_ms, started_at_ms
                 ) VALUES (71, 1, ?1, ?2, 'resmed', 1,
                           'running', 1100, 1100, 1100)",
                rusqlite::params![canary, SOURCE_ONE],
            )?;
        }
        drop(connection);

        let database = Database::open(&path)?;
        let history = database.imports().get(71)?.expect("upgraded import");
        assert_eq!(history.import_key, "opap-request:legacy-71");
        assert!(!history.import_key.contains("23123456789"));
        assert!(!history.import_key.contains("alice"));
        assert!(!history.import_key.contains("SD_CARD"));
        assert_eq!(history.attempt, 1);
        if legacy_version <= 3 {
            assert_eq!(history.source_uri, "opap-source:legacy-71");
        } else {
            assert_eq!(history.source_uri, SOURCE_ONE);
        }
        assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
    }
    Ok(())
}

#[test]
fn v7_rebuild_scrubs_collisions_and_preserves_retry_attempt_groups() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v6-import-key-privacy.sqlite3");
    let connection = database_at_schema_version(&path, 6)?;
    connection.execute(
        "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
         VALUES (1, 'V6 privacy', 1, 1)",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms
         ) VALUES (1, 1, 'SN23123456789:/Users/alice/card', ?1, 'resmed', 1,
                   'blocked', 1, 1)",
        [SOURCE_ONE],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms
         ) VALUES (2, 1, 'opap-request:legacy-1',
                   ?1 || char(0) || '/Users/alice/source-leak', 'resmed', 1,
                   'blocked', 2, 2)",
        [SOURCE_TWO],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms, started_at_ms, completed_at_ms,
             sessions_created, error_message
         ) VALUES (3, 1, ?1, ?2, 'resmed', 1,
                   'failed', 10, 20, 10, 20, 7, 'legacy failure')",
        rusqlite::params![REQUEST_ONE, SOURCE_THREE],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             retry_of_id, status, created_at_ms, updated_at_ms
         ) VALUES (4, 1, ?1, ?2, 'resmed', 2, 3, 'blocked', 30, 30)",
        rusqlite::params![REQUEST_ONE, SOURCE_THREE],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms, completed_at_ms
         ) VALUES (6, 1, 'SN99887766:/private/retry', ?1, 'resmed', 1,
                   'cancelled', 50, 60, 60)",
        [SOURCE_FOUR],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms
         ) VALUES (8, 1, ?1 || char(0) || '/private/key-leak', ?2, 'resmed', 1,
                   'blocked', 80, 80)",
        rusqlite::params![REQUEST_TWO, SOURCE_ONE],
    )?;
    drop(connection);

    let before_migration = std::fs::read(&path)?;
    for canary in [
        b"SN23123456789".as_slice(),
        b"/Users/alice".as_slice(),
        b"/private/key-leak".as_slice(),
    ] {
        assert!(
            before_migration
                .windows(canary.len())
                .any(|window| window == canary),
            "v6 fixture must contain the privacy canary before migration"
        );
    }

    let database = Database::open(&path)?;
    let wal_path = sqlite_wal_path(&path);
    for storage_path in [&path, &wal_path] {
        if storage_path.exists() {
            let bytes = std::fs::read(storage_path)?;
            for canary in [
                b"SN23123456789".as_slice(),
                b"/Users/alice".as_slice(),
                b"/private/key-leak".as_slice(),
            ] {
                assert!(
                    !bytes.windows(canary.len()).any(|window| window == canary),
                    "privacy canary remained in {}",
                    storage_path.display()
                );
            }
            if storage_path == &wal_path {
                assert!(bytes.is_empty(), "privacy checkpoint must truncate the WAL");
            }
        }
    }
    let first = database.imports().get(1)?.expect("first legacy import");
    let collision = database.imports().get(2)?.expect("collision import");
    let failed = database.imports().get(3)?.expect("failed attempt");
    let retry = database.imports().get(4)?.expect("retry attempt");
    let nul_key = database.imports().get(8)?.expect("NUL-bearing key import");
    assert_eq!(first.import_key, "opap-request:legacy-1");
    assert_eq!(collision.import_key, "opap-request:legacy-2");
    assert_eq!(collision.source_uri, "opap-source:legacy-2");
    assert_eq!(failed.import_key, "opap-request:legacy-3");
    assert_eq!(retry.import_key, failed.import_key);
    assert_eq!(failed.attempt, 1);
    assert_eq!(failed.sessions_created, 7);
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.retry_of_id, Some(failed.id));
    assert_eq!(nul_key.import_key, "opap-request:legacy-8");
    assert!(!nul_key.import_key.contains('\0'));
    assert_eq!(
        database
            .imports()
            .find_by_key(1, "opap-request:legacy-3")?
            .expect("latest logical attempt")
            .id,
        retry.id
    );
    let inherited_retry = database
        .imports()
        .retry_or_get(
            6,
            &RetryImport {
                initial_status: InitialImportStatus::Blocked,
                state_message: Some("retry migrated job"),
                created_at_ms: 70,
            },
        )?
        .expect("migrated cancelled attempt");
    assert!(inherited_retry.inserted);
    assert_eq!(inherited_retry.history.import_key, "opap-request:legacy-6");
    assert_eq!(inherited_retry.history.attempt, 2);
    assert_eq!(inherited_retry.history.retry_of_id, Some(6));
    let other_profile = database.profiles().insert(&NewProfile {
        display_name: "Wrong retry owner",
        now_ms: 90,
    })?;
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history
                 SET profile_id = ?2, attempt = 8, retry_of_id = NULL
                 WHERE id = ?1",
                rusqlite::params![retry.id, other_profile.id],
            )
            .is_err(),
        "nonterminal retry identity must be immutable"
    );
    let unchanged_retry = database.imports().get(retry.id)?.expect("unchanged retry");
    assert_eq!(unchanged_retry.profile_id, 1);
    assert_eq!(unchanged_retry.attempt, 2);
    assert_eq!(unchanged_retry.retry_of_id, Some(failed.id));
    for id in [first.id, failed.id] {
        assert!(
            database
                .connection()
                .execute("UPDATE import_history SET id = 99 WHERE id = ?1", [id],)
                .is_err(),
            "import row ids must be immutable in every state"
        );
    }
    let foreign_key_violations: i64 = database.connection().query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_check",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(foreign_key_violations, 0);
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET import_key = ?2 WHERE id = ?1",
                rusqlite::params![failed.id, REQUEST_TWO],
            )
            .is_err(),
        "terminal history protection must survive the rebuild"
    );

    database.connection().execute_batch(
        "DROP TRIGGER import_history_validate_request_key_insert;
         DROP TRIGGER import_history_validate_request_key_update;",
    )?;
    assert!(
        database
            .connection()
            .execute(
                "INSERT INTO import_history (
                     id, profile_id, import_key, source_uri, loader_name, attempt,
                     status, created_at_ms, updated_at_ms
                 ) VALUES (5, 1, 'opap-request:legacy-3', ?1, 'resmed', 2,
                           'blocked', 40, 40)",
                [SOURCE_FOUR],
            )
            .is_err(),
        "logical attempt uniqueness must survive the rebuild"
    );
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET import_key = '/Users/alice/key-leak' WHERE id = 1",
                [],
            )
            .is_err(),
        "the table CHECK must enforce request privacy without triggers"
    );
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET import_key = ?1 || char(0) || '/secret'
                 WHERE id = 1",
                [REQUEST_TWO],
            )
            .is_err(),
        "embedded NUL must not bypass the table CHECK"
    );
    database
        .connection()
        .execute("DELETE FROM import_history WHERE id = ?1", [failed.id])?;
    assert_eq!(
        database
            .imports()
            .get(retry.id)?
            .expect("retry after parent cleanup")
            .retry_of_id,
        None,
        "FK cleanup must still clear a nonterminal retry parent"
    );
    Ok(())
}

#[test]
fn failed_v7_rebuild_rolls_back_schema_triggers_indexes_and_data() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("v7-rollback.sqlite3");
    let connection = database_at_schema_version(&path, 6)?;
    connection.execute(
        "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
         VALUES (1, 'Rollback', 1, 1)",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms
         ) VALUES (0, 1, 'SN23123456789:/private/card', ?1, 'resmed', 1,
                   'blocked', 1, 1)",
        [SOURCE_ONE],
    )?;
    drop(connection);

    for _ in 0..2 {
        assert!(
            Database::open(&path).is_err(),
            "non-positive legacy row ids must fail v7 deterministically"
        );
        let unchanged = rusqlite::Connection::open(&path)?;
        let user_version: i64 = unchanged.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let migration_version: i64 =
            unchanged.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
        let import_key: String = unchanged.query_row(
            "SELECT import_key FROM import_history WHERE id = 0",
            [],
            |row| row.get(0),
        )?;
        let old_table_count: i64 = unchanged.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'import_history_v6'",
            [],
            |row| row.get(0),
        )?;
        let trigger_count: i64 = unchanged.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'trigger' AND tbl_name = 'import_history'",
            [],
            |row| row.get(0),
        )?;
        let index_count: i64 = unchanged.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name IN ('imports_by_start', 'imports_by_logical_key')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!((user_version, migration_version), (6, 6));
        assert_eq!(import_key, "SN23123456789:/private/card");
        assert_eq!(old_table_count, 0);
        assert_eq!(trigger_count, 9);
        assert_eq!(index_count, 2);
    }
    Ok(())
}

#[test]
fn v7_rejects_malformed_legacy_retry_lineage_without_mutation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("invalid-v6-retry-lineage.sqlite3");
    let connection = database_at_schema_version(&path, 6)?;
    connection.execute(
        "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
         VALUES (1, 'Invalid retry lineage', 1, 1)",
        [],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             status, created_at_ms, updated_at_ms, started_at_ms,
             completed_at_ms, error_message
         ) VALUES (1, 1, 'parent-private-key', ?1, 'resmed', 1,
                   'failed', 10, 20, 10, 20, 'failed')",
        [SOURCE_ONE],
    )?;
    connection.execute(
        "INSERT INTO import_history (
             id, profile_id, import_key, source_uri, loader_name, attempt,
             retry_of_id, status, created_at_ms, updated_at_ms
         ) VALUES (2, 1, 'different-private-key', ?1, 'resmed', 8,
                   1, 'blocked', 30, 30)",
        [SOURCE_ONE],
    )?;
    drop(connection);

    assert!(Database::open(&path).is_err());
    let unchanged = rusqlite::Connection::open(&path)?;
    let user_version: i64 = unchanged.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let migration_version: i64 =
        unchanged.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    let child: (String, i64, Option<i64>) = unchanged.query_row(
        "SELECT import_key, attempt, retry_of_id FROM import_history WHERE id = 2",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((user_version, migration_version), (6, 6));
    assert_eq!(child, ("different-private-key".to_owned(), 8, Some(1)));
    assert_eq!(
        unchanged.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'import_history_v6'",
            [],
            |row| row.get::<_, i64>(0),
        )?,
        0
    );
    Ok(())
}

#[test]
fn v5_redacts_terminal_paths_from_early_v4_databases_before_reprotecting_them() -> TestResult {
    let (directory, mut database) = temporary_database()?;
    let ids = seed_database(&mut database)?;
    drop(database);
    let path = directory.path().join("opap.sqlite3");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "DROP TRIGGER import_history_validate_source_insert;
         DROP TRIGGER import_history_validate_source_update;
         DROP TRIGGER import_history_protect_terminal_state;
         DROP TRIGGER import_history_validate_retry_time_insert;
         DROP TRIGGER import_history_protect_terminal_links;
         DROP TRIGGER import_history_validate_request_key_insert;
         DROP TRIGGER import_history_validate_request_key_update;
         DROP TRIGGER import_history_protect_import_identity;
         DROP TRIGGER import_history_validate_execution_identity;
         DROP TRIGGER import_history_validate_execution_outcome_code;
         DROP TRIGGER import_history_validate_execution_state;
         DROP TRIGGER import_history_validate_generation_state;
         DROP TABLE import_session_results;
         ALTER TABLE import_history DROP COLUMN execution_token;
         ALTER TABLE import_history DROP COLUMN execution_generation;
         ALTER TABLE import_history DROP COLUMN options_digest;
         ALTER TABLE import_history DROP COLUMN input_digest;
         ALTER TABLE import_history DROP COLUMN source_fingerprint;
         PRAGMA ignore_check_constraints = ON;",
    )?;
    connection.execute(
        "UPDATE import_history
         SET source_uri = '/Users/alice/terminal-card',
             import_key = 'SN23123456789:/Users/alice/terminal-card'
         WHERE id = ?1",
        [ids.import],
    )?;
    connection.execute_batch(
        "PRAGMA ignore_check_constraints = OFF;
         CREATE TRIGGER import_history_protect_terminal_state
         BEFORE UPDATE OF source_uri ON import_history
         WHEN OLD.status IN ('completed', 'failed', 'cancelled')
         BEGIN
             SELECT RAISE(ABORT, 'terminal import job cannot be changed');
         END;
         DROP TABLE session_settings;
         DROP TABLE summary_metrics;
         DROP TABLE session_summary;
         DROP TABLE session_slices;
         DROP TABLE session_provenance;
         DELETE FROM schema_migrations WHERE version >= 5;
         PRAGMA user_version = 4;",
    )?;
    drop(connection);

    let database = Database::open(&path)?;
    assert_eq!(database.schema_version()?, LATEST_SCHEMA_VERSION);
    let history = database.imports().get(ids.import)?.expect("terminal job");
    assert_eq!(history.status, ImportStatus::Completed);
    assert_eq!(
        history.import_key,
        format!("opap-request:legacy-{}", ids.import)
    );
    assert!(!history.import_key.contains("23123456789"));
    assert_eq!(
        history.source_uri,
        format!("opap-source:legacy-{}", ids.import)
    );
    assert!(!history.source_uri.contains("alice"));
    assert!(
        database
            .connection()
            .execute(
                "UPDATE import_history SET source_uri = ?2 WHERE id = ?1",
                rusqlite::params![ids.import, SOURCE_TWO],
            )
            .is_err(),
        "terminal protection must be restored after redaction"
    );
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

#[test]
fn rejects_weakened_v9_execution_and_result_triggers_on_open() -> TestResult {
    for (trigger, replacement) in [
        (
            "import_history_validate_execution_identity",
            "CREATE TRIGGER import_history_validate_execution_identity
             BEFORE UPDATE OF execution_token ON import_history
             BEGIN
                 SELECT RAISE(ABORT, 'weak identity guard');
             END;",
        ),
        (
            "import_history_validate_execution_outcome_code",
            "CREATE TRIGGER import_history_validate_execution_outcome_code
             BEFORE UPDATE OF status ON import_history
             BEGIN
                 SELECT RAISE(ABORT, 'weak outcome guard');
             END;",
        ),
        (
            "import_history_validate_execution_state",
            "CREATE TRIGGER import_history_validate_execution_state
             BEFORE UPDATE OF status ON import_history
             BEGIN
                 SELECT RAISE(ABORT, 'weak execution state guard');
             END;",
        ),
        (
            "import_history_validate_generation_state",
            "CREATE TRIGGER import_history_validate_generation_state
             BEFORE UPDATE ON import_history
             BEGIN
                 SELECT RAISE(ABORT, 'weak generation state guard');
             END;",
        ),
        (
            "import_session_results_validate_job_insert",
            "CREATE TRIGGER import_session_results_validate_job_insert
             BEFORE INSERT ON import_session_results
             BEGIN
                 SELECT RAISE(ABORT, 'weak result insert guard');
             END;",
        ),
        (
            "import_session_results_protect_update",
            "CREATE TRIGGER import_session_results_protect_update
             BEFORE UPDATE ON import_session_results
             BEGIN
                 SELECT RAISE(ABORT, 'weak result update guard');
             END;",
        ),
        (
            "import_session_results_protect_delete",
            "CREATE TRIGGER import_session_results_protect_delete
             BEFORE DELETE ON import_session_results
             BEGIN
                 SELECT RAISE(ABORT, 'weak result delete guard');
             END;",
        ),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join(format!("{trigger}.sqlite3"));
        drop(Database::open(&path)?);
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(&format!("DROP TRIGGER {trigger}; {replacement}"))?;
        drop(connection);

        let error = Database::open(&path)
            .err()
            .expect("weakened critical trigger must be rejected");
        assert!(
            matches!(error, StorageError::InvalidSchemaFingerprint(_)),
            "{trigger} produced {error:?}"
        );
    }
    Ok(())
}

#[test]
fn rejects_v9_trigger_fragments_hidden_in_comments_on_open() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("comment-bypass.sqlite3");
    drop(Database::open(&path)?);
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "DROP TRIGGER import_history_validate_execution_outcome_code;
         CREATE TRIGGER import_history_validate_execution_outcome_code
         BEFORE UPDATE OF status ON import_history
         WHEN 0
         BEGIN
             /*
             old.execution_generation>0andold.status<>new.status
             new.status='blocked'andnew.state_messagein('worker_interrupted','retry_pending','source_unavailable','admin_revoked','recovered_after_restart')
             new.status='failed'andnew.state_messageisnullandnew.error_messagein('invalid_source','unsupported_source','decode_failed','resource_limit','source_unavailable','internal_failure')
             new.status='cancelled'andnew.state_message='user_cancelled'
             new.status='completed'andnew.state_messageisnullandnew.error_messageisnullandnew.machine_idisnotnull
             new.sessions_created=(selectcount(*)fromimport_session_resultswhereimport_id=new.idandoutcome='created')
             new.events_written=(selectcount(*)fromimport_session_resultsasresultjoineventsaseventonevent.session_id=result.session_id
             new.waveform_chunks_written=(selectcount(*)fromimport_session_resultsasresultjoinwaveformsaswaveformonwaveform.session_id=result.session_idjoinwaveform_chunksaschunkonchunk.waveform_id=waveform.id
             */
             SELECT RAISE(ABORT, 'invalid import execution outcome code');
         END;",
    )?;
    drop(connection);

    let error = Database::open(&path)
        .err()
        .expect("comment-only canonical fragments must not validate a disabled trigger");
    assert!(matches!(error, StorageError::InvalidSchemaFingerprint(_)));
    Ok(())
}

#[test]
fn rejects_disagreement_between_user_version_and_migration_history() -> TestResult {
    for (case_name, tamper_sql, expected_user, expected_history) in [
        (
            "stale-user-version",
            "PRAGMA user_version = 8;",
            8_i64,
            9_i64,
        ),
        (
            "missing-history-row",
            "DELETE FROM schema_migrations WHERE version = 9;",
            9_i64,
            8_i64,
        ),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join(format!("{case_name}.sqlite3"));
        drop(Database::open(&path)?);
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(tamper_sql)?;
        drop(connection);

        let error = Database::open(&path)
            .err()
            .expect("mismatched schema metadata must be rejected");
        assert!(matches!(
            error,
            StorageError::MigrationVersionMismatch {
                user_version,
                history_version,
            } if user_version == expected_user && history_version == expected_history
        ));

        let unchanged = rusqlite::Connection::open(&path)?;
        let user_version: i64 = unchanged.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let history_version: i64 = unchanged.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            (user_version, history_version),
            (expected_user, expected_history)
        );
    }
    Ok(())
}

#[test]
fn rejects_committed_foreign_key_corruption_on_open() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("foreign-key-corruption.sqlite3");
    drop(Database::open(&path)?);

    let connection = rusqlite::Connection::open(&path)?;
    connection.pragma_update(None, "foreign_keys", false)?;
    connection.execute(
        "INSERT INTO machines (
             id, profile_id, source_key, device_type, manufacturer, model,
             model_number, serial_number, first_seen_at_ms, last_seen_at_ms
         ) VALUES (42, 999, 'orphan', 'pap', '', '', '', '', 1, 1)",
        [],
    )?;
    drop(connection);

    let error = Database::open(&path)
        .err()
        .expect("foreign-key corruption must be rejected");
    assert!(matches!(
        error,
        StorageError::ForeignKeyViolation {
            table,
            row_id: Some(42),
            parent,
            foreign_key_index: 0,
        } if table == "machines" && parent == "profiles"
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn refuses_to_open_a_database_through_a_symbolic_link() -> TestResult {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let target = directory.path().join("target.sqlite3");
    let link = directory.path().join("database-link.sqlite3");
    drop(Database::open(&target)?);
    symlink(&target, &link)?;

    assert!(
        Database::open(&link).is_err(),
        "SQLITE_OPEN_NOFOLLOW must reject a symlinked database path"
    );
    assert_eq!(
        Database::open(&target)?.schema_version()?,
        LATEST_SCHEMA_VERSION
    );
    Ok(())
}
