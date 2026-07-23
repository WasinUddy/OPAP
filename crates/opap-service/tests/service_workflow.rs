// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

use opap_service::{
    API_SCHEMA_VERSION, ApiErrorCode, AppService, Clock, CreateProfileRequest, ImportJobPhase,
    ImportJobStatus, PrepareImportJobRequest, SESSION_IMPORT_UNAVAILABLE_REASON,
};
use opap_storage::{
    Database, ImportStatus as StorageImportStatus, InitialImportStatus, NewImport, NewProfile,
};
use std::{error::Error, fs, path::Path};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Clone, Copy)]
struct FixedClock(i64);

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

fn test_service(
    directory: &TempDir,
    now_ms: i64,
) -> Result<AppService<FixedClock>, Box<dyn Error>> {
    let database = Database::open(directory.path().join("opap.sqlite3"))?;
    Ok(AppService::from_database(database, FixedClock(now_ms))?)
}

fn resmed_card(root: &Path) -> Result<(), Box<dyn Error>> {
    resmed_card_with_identification(
        root,
        "#SRN 23123456789\n#PNA AirSense_10_AutoSet\n#PCD 37028\n",
    )
}

fn resmed_card_with_identification(
    root: &Path,
    identification: &str,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir(root.join("DATALOG"))?;
    fs::write(root.join("STR.edf"), [])?;
    fs::write(root.join("Identification.tgt"), identification)?;
    Ok(())
}

fn request_key(discriminator: u128) -> String {
    format!("opap-request:{discriminator:032x}")
}

fn assert_json_excludes(value: &serde_json::Value, canaries: &[&str]) {
    match value {
        serde_json::Value::String(text) => {
            let text = text.to_lowercase();
            for canary in canaries {
                assert!(
                    !text.contains(&canary.to_lowercase()),
                    "serialized string leaked canary"
                );
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                assert_json_excludes(value, canaries);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                assert_json_excludes(value, canaries);
            }
        }
        _ => {}
    }
}

#[test]
fn bootstrap_and_profile_lifecycle_are_deterministic() -> TestResult {
    let directory = tempfile::tempdir()?;
    let service = test_service(&directory, 1_700_000_000_000)?;

    let empty = service.bootstrap()?;
    assert_eq!(API_SCHEMA_VERSION, 2);
    assert_eq!(empty.api_schema_version, API_SCHEMA_VERSION);
    assert!(!empty.capabilities.session_import);
    assert_eq!(empty.profiles, []);
    assert_eq!(
        empty.importers[0].unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );

    let profile = service.create_profile(CreateProfileRequest {
        display_name: "  Alex  ".to_owned(),
    })?;
    assert_eq!(profile.display_name, "Alex");
    assert_eq!(profile.created_at_ms, 1_700_000_000_000);
    assert_eq!(
        service.list_profiles()?.as_slice(),
        std::slice::from_ref(&profile)
    );
    assert_eq!(service.bootstrap()?.profiles, [profile]);
    Ok(())
}

#[test]
fn source_inspection_recognizes_resmed_without_claiming_session_support() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("RESMED");
    fs::create_dir(&card)?;
    resmed_card(&card)?;
    let service = test_service(&directory, 1_700_000_000_000)?;

    let inspection = service.inspect_source(&card)?;

    assert!(inspection.recognized);
    assert_eq!(inspection.importer_id.as_deref(), Some("resmed"));
    assert_eq!(inspection.source_label, "ResMed SD card");
    assert!(inspection.source_id.starts_with("opap-source:"));
    let device = inspection.device.expect("device");
    assert_eq!(device.model, "AirSense 10");
    assert!(device.model_number.is_empty());
    assert_eq!(device.series, "AirSense 10");
    assert_eq!(device.serial_suffix, "6789");
    assert_eq!(inspection.files, 2);
    assert_eq!(inspection.directories, 1);
    assert!(!inspection.session_import.available);
    assert_eq!(
        inspection.session_import.unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );
    Ok(())
}

#[test]
fn source_inspection_redacts_untrusted_device_display_fields() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("RESMED");
    fs::create_dir(&card)?;
    let full_serial = "PrivateSerialABC123456789";
    let unix_path = "/Users/alice/private-card";
    let windows_path = r"C:\Users\Alice\private-card";
    let identification = format!(
        "#SRN {full_serial}\n#PNA AirSense_10_{}\n#PCD {unix_path} {windows_path}\n",
        full_serial.to_lowercase()
    );
    resmed_card_with_identification(&card, &identification)?;
    let service = test_service(&directory, 1_700_000_000_000)?;

    let inspection = service.inspect_source(&card)?;
    let device = inspection.device.as_ref().expect("device");
    assert_eq!(device.brand, "ResMed");
    assert_eq!(device.model, "Unknown ResMed device");
    assert!(device.model_number.is_empty());
    assert!(device.series.is_empty());
    assert_eq!(device.serial_suffix, "6789");

    let serialized = serde_json::to_value(&inspection)?;
    assert_json_excludes(&serialized, &[full_serial, unix_path, windows_path]);
    Ok(())
}

#[test]
fn source_inspection_blocks_split_serial_and_encoded_path_reconstruction() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("RESMED");
    fs::create_dir(&card)?;
    let full_serial = "23123456789";
    let serial_prefix = "2312345";
    let unicode_path = "∕Users∕alice∕private-card";
    let encoded_path = "%2fVolumes%2fprivate-card";
    let identification = format!(
        "#SRN {full_serial}\n#PNA AirSense_10_{serial_prefix}_{unicode_path}_{encoded_path}\n#PCD 6789\n"
    );
    resmed_card_with_identification(&card, &identification)?;
    let service = test_service(&directory, 1_700_000_000_000)?;

    let inspection = service.inspect_source(&card)?;
    let device = inspection.device.as_ref().expect("device");
    assert_eq!(device.brand, "ResMed");
    assert_eq!(device.model, "Unknown ResMed device");
    assert!(device.model_number.is_empty());
    assert!(device.series.is_empty());
    assert_eq!(device.serial_suffix, "6789");

    let serialized = serde_json::to_value(&inspection)?;
    assert_json_excludes(
        &serialized,
        &[full_serial, serial_prefix, unicode_path, encoded_path],
    );
    Ok(())
}

#[test]
fn unknown_source_is_inspectable_but_cannot_be_prepared() -> TestResult {
    let directory = tempfile::tempdir()?;
    let unknown = directory.path().join("unknown");
    fs::create_dir(&unknown)?;
    fs::write(unknown.join("notes.txt"), b"nothing clinical")?;
    let service = test_service(&directory, 1_700_000_000_000)?;
    let profile = service.create_profile(CreateProfileRequest {
        display_name: "Alex".to_owned(),
    })?;

    let inspection = service.inspect_source(&unknown)?;
    assert!(!inspection.recognized);
    assert_eq!(inspection.files, 1);

    let error = service
        .prepare_import_job(PrepareImportJobRequest {
            profile_id: profile.id,
            source_id: inspection.source_id,
        })
        .expect_err("unsupported source");
    assert_eq!(error.code, ApiErrorCode::SourceNotSupported);
    assert!(service.list_import_jobs(profile.id)?.is_empty());
    Ok(())
}

#[test]
fn prepared_job_is_persisted_idempotent_honest_and_cancellable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("RESMED");
    fs::create_dir(&card)?;
    resmed_card(&card)?;
    let service = test_service(&directory, 1_700_000_000_000)?;
    let profile = service.create_profile(CreateProfileRequest {
        display_name: "Alex".to_owned(),
    })?;
    let source = service.inspect_source(&card)?;
    let request = PrepareImportJobRequest {
        profile_id: profile.id,
        source_id: source.source_id,
    };

    let first = service.prepare_import_job(request.clone())?;
    assert!(first.created);
    assert_eq!(first.job.status, ImportJobStatus::Blocked);
    assert_eq!(first.job.phase, ImportJobPhase::AwaitingSessionImporter);
    assert!(first.job.can_cancel);
    assert_eq!(
        first.job.unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );
    assert_eq!(first.job.counts.sessions_created, 0);

    let repeated = service.prepare_import_job(request)?;
    assert!(!repeated.created);
    assert_eq!(repeated.job.id, first.job.id);

    let run_error = service
        .run_import_job(profile.id, first.job.id)
        .expect_err("session parser is unavailable");
    assert_eq!(run_error.code, ApiErrorCode::CapabilityUnavailable);
    assert_eq!(
        service.get_import_job(profile.id, first.job.id)?.status,
        ImportJobStatus::Blocked
    );

    let cancelled = service.cancel_import_job(profile.id, first.job.id)?;
    assert_eq!(cancelled.status, ImportJobStatus::Cancelled);
    assert_eq!(cancelled.finished_at_ms, Some(1_700_000_000_000));
    assert!(!cancelled.can_cancel);
    assert!(cancelled.failure_message.is_none());
    assert_eq!(
        service
            .cancel_import_job(profile.id, first.job.id)
            .expect_err("terminal job")
            .code,
        ApiErrorCode::JobNotCancellable
    );
    drop(service);
    let persisted = Database::open(directory.path().join("opap.sqlite3"))?
        .imports()
        .get(first.job.id)?
        .expect("cancelled job");
    assert_eq!(persisted.status, StorageImportStatus::Cancelled);
    assert!(persisted.error_message.is_none());
    Ok(())
}

#[test]
fn idempotent_job_replay_works_after_the_source_is_unplugged() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("RESMED");
    let unplugged = directory.path().join("unplugged");
    fs::create_dir(&card)?;
    resmed_card(&card)?;
    let service = test_service(&directory, 100)?;
    let profile = service.create_profile(CreateProfileRequest {
        display_name: "Alex".to_owned(),
    })?;
    let source = service.inspect_source(&card)?;
    let request = PrepareImportJobRequest {
        profile_id: profile.id,
        source_id: source.source_id,
    };
    let created = service.prepare_import_job(request.clone())?;

    fs::rename(&card, &unplugged)?;
    let replayed = service.prepare_import_job(request)?;

    assert!(!replayed.created);
    assert_eq!(replayed.job.id, created.job.id);
    assert_eq!(replayed.job.status, ImportJobStatus::Blocked);
    Ok(())
}

#[test]
fn jobs_survive_database_reopen_and_source_handles_expire() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("RESMED");
    let second_card = directory.path().join("RESMED-BACKUP");
    fs::create_dir(&card)?;
    fs::create_dir(&second_card)?;
    resmed_card(&card)?;
    resmed_card(&second_card)?;

    let service = test_service(&directory, 100)?;
    let profile = service.create_profile(CreateProfileRequest {
        display_name: "Alex".to_owned(),
    })?;
    let source = service.inspect_source(&card)?;
    let prepared = service.prepare_import_job(PrepareImportJobRequest {
        profile_id: profile.id,
        source_id: source.source_id,
    })?;
    let expired_source_id = prepared.job.source_id.clone();
    drop(service);

    let reopened = test_service(&directory, 200)?;
    assert_eq!(
        reopened.get_import_job(profile.id, prepared.job.id)?.status,
        ImportJobStatus::Blocked
    );
    let second_source = reopened.inspect_source(&second_card)?;
    let second = reopened.prepare_import_job(PrepareImportJobRequest {
        profile_id: profile.id,
        source_id: second_source.source_id,
    })?;
    assert!(second.created);
    assert_ne!(second.job.id, prepared.job.id);
    let expired = reopened
        .prepare_import_job(PrepareImportJobRequest {
            profile_id: profile.id,
            source_id: expired_source_id,
        })
        .expect_err("native capability expires at restart");
    assert_eq!(expired.code, ApiErrorCode::SourceUnavailable);
    Ok(())
}

#[test]
fn validation_and_profile_ownership_have_stable_codes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let service = test_service(&directory, 1)?;

    assert_eq!(
        service
            .create_profile(CreateProfileRequest {
                display_name: "  ".to_owned(),
            })
            .expect_err("blank name")
            .code,
        ApiErrorCode::InvalidRequest
    );
    assert_eq!(
        service
            .inspect_source("relative/card")
            .expect_err("relative path")
            .code,
        ApiErrorCode::SourcePathInvalid
    );
    assert_eq!(
        service
            .get_import_job(999, 999)
            .expect_err("unknown job")
            .code,
        ApiErrorCode::JobNotFound
    );
    let profile = service.create_profile(CreateProfileRequest {
        display_name: "Alex".to_owned(),
    })?;
    let injected_request = serde_json::json!({
        "profile_id": profile.id,
        "source_id": "opap-source:0123456789abcdef0123456789abcdef",
        "request_key": "0123456789abcdef0123456789abcdef"
    });
    assert!(
        serde_json::from_value::<PrepareImportJobRequest>(injected_request).is_err(),
        "renderers cannot supply request identifiers"
    );
    assert_eq!(
        service
            .prepare_import_job(PrepareImportJobRequest {
                profile_id: profile.id,
                source_id: "opap-source:/Volumes/private-card".to_owned(),
            })
            .expect_err("path-like source ID")
            .code,
        ApiErrorCode::InvalidRequest
    );
    Ok(())
}

#[test]
fn source_errors_do_not_expose_absolute_local_paths() -> TestResult {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("personal-name/RESMED");
    let service = test_service(&directory, 1)?;

    let error = service
        .inspect_source(&missing)
        .expect_err("missing source");

    assert_eq!(error.code, ApiErrorCode::SourceUnavailable);
    assert!(
        !error
            .message
            .contains(&directory.path().to_string_lossy()[..])
    );
    Ok(())
}

#[test]
fn web_dtos_and_persisted_jobs_contain_only_opaque_source_ids() -> TestResult {
    let directory = tempfile::tempdir()?;
    let card = directory.path().join("patient-name-private-card");
    fs::create_dir(&card)?;
    resmed_card(&card)?;
    let service = test_service(&directory, 10)?;
    let profile = service.create_profile(CreateProfileRequest {
        display_name: "Alex".to_owned(),
    })?;

    let inspection = service.inspect_source(&card)?;
    let source_id = inspection.source_id.clone();
    let inspection_json = serde_json::to_string(&inspection)?;
    assert!(!inspection_json.contains(&card.to_string_lossy()[..]));
    assert!(!inspection_json.contains("23123456789"));
    assert!(inspection_json.contains("6789"));

    let request = PrepareImportJobRequest {
        profile_id: profile.id,
        source_id: source_id.clone(),
    };
    let request_json = serde_json::to_string(&request)?;
    assert!(!request_json.contains("directory"));
    assert!(!request_json.contains("request_key"));
    assert!(!request_json.contains(&card.to_string_lossy()[..]));

    let prepared = service.prepare_import_job(request)?;
    let job_json = serde_json::to_string(&prepared.job)?;
    assert!(!job_json.contains("source_directory"));
    assert!(!job_json.contains(&card.to_string_lossy()[..]));
    assert_eq!(prepared.job.source_id, source_id);
    drop(service);

    let persisted = Database::open(directory.path().join("opap.sqlite3"))?
        .imports()
        .get(prepared.job.id)?
        .expect("persisted import job");
    assert_eq!(persisted.source_uri, source_id);
    let request_suffix = persisted
        .import_key
        .strip_prefix("opap-request:")
        .expect("service-generated request key");
    assert_eq!(request_suffix.len(), 32);
    assert!(
        request_suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert!(!job_json.contains(&persisted.import_key));
    assert_eq!(persisted.status, StorageImportStatus::Blocked);
    assert!(persisted.started_at_ms.is_none());
    assert!(!persisted.source_uri.contains(&card.to_string_lossy()[..]));
    Ok(())
}

#[test]
fn storage_boundary_rejects_raw_source_paths() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("opap.sqlite3");
    let database = Database::open(&database_path)?;
    let profile = database.profiles().insert(&NewProfile {
        display_name: "Alex",
        now_ms: 1,
    })?;
    let raw_path = "/Volumes/private-patient-card";
    let error = database
        .imports()
        .begin_or_get(&NewImport {
            profile_id: profile.id,
            machine_id: None,
            import_key: "opap-request:0123456789abcdef0123456789abcdef",
            source_uri: raw_path,
            loader_name: "resmed",
            initial_status: InitialImportStatus::Blocked,
            state_message: Some("must not persist"),
            created_at_ms: 1,
        })
        .expect_err("raw source paths must be rejected by storage");
    assert!(error.to_string().contains("opaque OPAP source identifier"));
    assert!(database.imports().list_by_profile(profile.id)?.is_empty());
    Ok(())
}

#[test]
fn historical_request_keys_are_never_echoed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = Database::open(directory.path().join("opap.sqlite3"))?;
    let profile = database.profiles().insert(&NewProfile {
        display_name: "Alex",
        now_ms: 1,
    })?;
    let private_key = "opap-request:0123456789abcdef0123456789abcdef";
    database.imports().begin_or_get(&NewImport {
        profile_id: profile.id,
        machine_id: None,
        import_key: private_key,
        source_uri: "opap-source:0123456789abcdef0123456789abcdef",
        loader_name: "resmed",
        initial_status: InitialImportStatus::Blocked,
        state_message: Some(SESSION_IMPORT_UNAVAILABLE_REASON),
        created_at_ms: 1,
    })?;

    let service = AppService::from_database(database, FixedClock(2))?;
    let jobs = service.list_import_jobs(profile.id)?;
    assert_eq!(jobs.len(), 1);
    assert!(!serde_json::to_string(&jobs)?.contains(private_key));
    Ok(())
}

#[test]
fn untrusted_stored_importer_names_are_allowlisted_before_serialization() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = Database::open(directory.path().join("opap.sqlite3"))?;
    let profile = database.profiles().insert(&NewProfile {
        display_name: "Alex",
        now_ms: 1,
    })?;
    let private_loader = "/Users/private/custom-loader";
    database.imports().begin_or_get(&NewImport {
        profile_id: profile.id,
        machine_id: None,
        import_key: &request_key(9),
        source_uri: "opap-source:0123456789abcdef0123456789abcdef",
        loader_name: private_loader,
        initial_status: InitialImportStatus::Blocked,
        state_message: Some(SESSION_IMPORT_UNAVAILABLE_REASON),
        created_at_ms: 1,
    })?;

    let service = AppService::from_database(database, FixedClock(2))?;
    let jobs = service.list_import_jobs(profile.id)?;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].importer_id, "unknown");
    assert_eq!(jobs[0].source_label, "Selected folder");
    assert!(!serde_json::to_string(&jobs)?.contains(private_loader));
    Ok(())
}

#[test]
fn running_jobs_are_recovered_to_blocked_when_service_opens() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("opap.sqlite3");
    let database = Database::open(&database_path)?;
    let profile = database.profiles().insert(&NewProfile {
        display_name: "Alex",
        now_ms: 10,
    })?;
    let running = database.imports().begin_or_get(&NewImport {
        profile_id: profile.id,
        machine_id: None,
        import_key: &request_key(8),
        source_uri: "opap-source:0123456789abcdef0123456789abcdef",
        loader_name: "resmed",
        initial_status: InitialImportStatus::Running,
        state_message: None,
        created_at_ms: 10,
    })?;
    assert_eq!(running.history.status, StorageImportStatus::Running);

    let service = AppService::from_database(database, FixedClock(20))?;
    let recovered = service.get_import_job(profile.id, running.history.id)?;
    assert_eq!(recovered.status, ImportJobStatus::Blocked);
    assert_eq!(recovered.created_at_ms, 10);
    assert_eq!(recovered.updated_at_ms, 20);
    assert_eq!(recovered.started_at_ms, Some(10));
    assert_eq!(
        recovered.unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );
    Ok(())
}
