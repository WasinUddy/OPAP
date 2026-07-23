// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

use cucumber::{World as _, given, then, when};
use opap_service::{
    API_SCHEMA_VERSION, ApiError, ApiErrorCode, AppBootstrap, AppService, CreateProfileRequest,
    ImportJobDto, ImportJobPhase, ImportJobStatus, PrepareImportJobRequest,
    PrepareImportJobResponse, SESSION_IMPORT_UNAVAILABLE_REASON, SourceInspection,
};
use opap_storage::{
    Database, ImportStatus as StorageImportStatus, InitialImportStatus, NewImport, NewProfile,
};
use serde_json::Value;
use std::{fs, path::PathBuf};
use tempfile::{TempDir, tempdir};

const FULL_SYNTHETIC_SERIAL: &str = "0123456789abcdef0123456789abcdef";
const SERIAL_SUFFIX: &str = "cdef";
const TEST_NOW_MS: i64 = 1_700_000_000_000;
const SUPPORTED_SOURCE_CANARY: &str = "private-supported-source-canary";
const UNSUPPORTED_SOURCE_CANARY: &str = "private-unsupported-source-canary";
const MISSING_SOURCE_CANARY: &str = "private-missing-source-canary";
const LEGACY_PRIVATE_REQUEST_KEY: &str = "opap-request:0123456789abcdef0123456789abcdef";
const INTERRUPTED_SOURCE_ID: &str = "opap-source:44444444444444444444444444444444";

#[derive(Debug, Default, cucumber::World)]
struct ServiceWorld {
    fixture: Option<TempDir>,
    database_path: Option<PathBuf>,
    source_path: Option<PathBuf>,
    full_serial: Option<String>,
    bootstrap: Option<AppBootstrap>,
    inspection: Option<SourceInspection>,
    first_preparation: Option<PrepareImportJobResponse>,
    repeated_preparation: Option<PrepareImportJobResponse>,
    cancelled_job: Option<ImportJobDto>,
    reopened_job: Option<ImportJobDto>,
    service_error: Option<ApiError>,
    profile_id: Option<i64>,
    job_id: Option<i64>,
    persisted_job_count: Option<usize>,
    persisted_source_id: Option<String>,
    persisted_request_key: Option<String>,
    renderer_request_json: Option<Value>,
    renderer_json: Vec<Value>,
}

impl ServiceWorld {
    fn database_path(&self) -> PathBuf {
        self.database_path
            .clone()
            .expect("a temporary service database path must exist")
    }

    fn source_path(&self) -> PathBuf {
        self.source_path
            .clone()
            .expect("a temporary synthetic source must exist")
    }

    fn open_service(&self) -> AppService {
        AppService::open(self.database_path()).expect("open temporary OPAP service")
    }
}

#[given("a fresh local OPAP service database")]
fn fresh_service_database(world: &mut ServiceWorld) {
    install_service_workspace(world);
}

#[given("a synthetic supported ResMed source with a full serial")]
fn supported_resmed_source(world: &mut ServiceWorld) {
    let root = world
        .fixture
        .as_ref()
        .expect("service workspace")
        .path()
        .join(SUPPORTED_SOURCE_CANARY);
    fs::create_dir(&root).expect("create synthetic ResMed source");
    fs::create_dir(root.join("DATALOG")).expect("create synthetic DATALOG directory");
    fs::write(root.join("STR.edf"), []).expect("write synthetic STR.edf marker");
    fs::write(
        root.join("Identification.tgt"),
        format!("#SRN {FULL_SYNTHETIC_SERIAL}\n#PNA AirSense_10_AutoSet\n#PCD SYN-SERVICE-10\n"),
    )
    .expect("write synthetic identification");
    world.source_path = Some(root);
    world.full_serial = Some(FULL_SYNTHETIC_SERIAL.to_owned());
}

#[given("its device identity fields contain privacy canaries")]
fn device_fields_contain_privacy_canaries(world: &mut ServiceWorld) {
    let source = world.source_path();
    let path_canary = source.join("private-device-model");
    let identification = serde_json::json!({
        "FlowGenerator": {
            "IdentificationProfiles": {
                "Product": {
                    "SerialNumber": FULL_SYNTHETIC_SERIAL,
                    "ProductCode": FULL_SYNTHETIC_SERIAL,
                    "ProductName": path_canary.to_string_lossy()
                }
            }
        }
    });
    fs::write(
        source.join("Identification.json"),
        serde_json::to_vec_pretty(&identification).expect("serialize adversarial identification"),
    )
    .expect("write adversarial synthetic identification");
}

#[given("a synthetic unsupported source directory")]
fn unsupported_source(world: &mut ServiceWorld) {
    let root = world
        .fixture
        .as_ref()
        .expect("service workspace")
        .path()
        .join(UNSUPPORTED_SOURCE_CANARY);
    fs::create_dir(&root).expect("create synthetic unsupported source");
    fs::write(root.join("notes.txt"), b"synthetic non-clinical fixture")
        .expect("write synthetic unsupported marker");
    world.source_path = Some(root);
}

#[given("a durable running job from an interrupted process")]
fn durable_running_job(world: &mut ServiceWorld) {
    install_service_workspace(world);
    let database = Database::open(world.database_path()).expect("open temporary storage");
    let profile = database
        .profiles()
        .insert(&NewProfile {
            display_name: "Synthetic acceptance profile",
            now_ms: TEST_NOW_MS,
        })
        .expect("create synthetic profile");
    let running = database
        .imports()
        .begin_or_get(&NewImport {
            profile_id: profile.id,
            machine_id: None,
            import_key: LEGACY_PRIVATE_REQUEST_KEY,
            source_uri: INTERRUPTED_SOURCE_ID,
            loader_name: FULL_SYNTHETIC_SERIAL,
            initial_status: InitialImportStatus::Running,
            state_message: None,
            created_at_ms: TEST_NOW_MS,
        })
        .expect("persist running synthetic job");
    assert_eq!(running.history.status, StorageImportStatus::Running);
    world.profile_id = Some(profile.id);
    world.job_id = Some(running.history.id);
    world.full_serial = Some(FULL_SYNTHETIC_SERIAL.to_owned());
}

#[when("the renderer requests application bootstrap")]
fn request_bootstrap(world: &mut ServiceWorld) {
    let service = world.open_service();
    let bootstrap = service.bootstrap().expect("bootstrap OPAP service");
    world
        .renderer_json
        .push(serde_json::to_value(&bootstrap).expect("serialize bootstrap for the renderer"));
    world.bootstrap = Some(bootstrap);
}

#[when("the native service inspects the source")]
fn inspect_native_source(world: &mut ServiceWorld) {
    let service = world.open_service();
    let inspection = service
        .inspect_source(world.source_path())
        .expect("inspect synthetic ResMed source");
    world
        .renderer_json
        .push(serde_json::to_value(&inspection).expect("serialize inspection for the renderer"));
    world.inspection = Some(inspection);
}

#[when("the same supported source import is prepared twice")]
fn prepare_supported_source_twice(world: &mut ServiceWorld) {
    let service = world.open_service();
    let profile = service
        .create_profile(CreateProfileRequest {
            display_name: "Synthetic acceptance profile".to_owned(),
        })
        .expect("create synthetic profile");
    let inspection = service
        .inspect_source(world.source_path())
        .expect("inspect synthetic ResMed source");
    let request = PrepareImportJobRequest {
        profile_id: profile.id,
        source_id: inspection.source_id.clone(),
    };
    let request_json = serde_json::to_value(&request).expect("serialize preparation request");
    let first = service
        .prepare_import_job(request.clone())
        .expect("prepare supported source");
    let run_error = service
        .run_import_job(profile.id, first.job.id)
        .expect_err("session import must remain unavailable");
    let repeated = service
        .prepare_import_job(request)
        .expect("repeat idempotent preparation");

    world.renderer_json.extend([
        serde_json::to_value(&inspection).expect("serialize inspection"),
        request_json.clone(),
        serde_json::to_value(&first).expect("serialize first preparation"),
        serde_json::to_value(&repeated).expect("serialize repeated preparation"),
        serde_json::to_value(&run_error).expect("serialize unavailable error"),
    ]);
    world.inspection = Some(inspection);
    world.renderer_request_json = Some(request_json);
    world.service_error = Some(run_error);
    world.profile_id = Some(profile.id);
    world.job_id = Some(first.job.id);
    world.first_preparation = Some(first);
    world.repeated_preparation = Some(repeated);

    drop(service);
    let reopened = world.open_service();
    let reopened_job = reopened
        .get_import_job(profile.id, world.job_id.expect("prepared job ID"))
        .expect("read durable job after reopening storage");
    world
        .renderer_json
        .push(serde_json::to_value(&reopened_job).expect("serialize reopened durable job"));
    world.reopened_job = Some(reopened_job);
    world.persisted_job_count = Some(
        reopened
            .list_import_jobs(profile.id)
            .expect("list durable jobs")
            .len(),
    );
    drop(reopened);
    let persisted = Database::open(world.database_path())
        .expect("reopen storage directly")
        .imports()
        .get(world.job_id.expect("prepared job ID"))
        .expect("read persisted job")
        .expect("persisted job exists");
    world.persisted_source_id = Some(persisted.source_uri);
    world.persisted_request_key = Some(persisted.import_key);
}

#[when("a prepared job is cancelled and the service is reopened")]
fn cancel_and_reopen(world: &mut ServiceWorld) {
    let service = world.open_service();
    let profile = service
        .create_profile(CreateProfileRequest {
            display_name: "Synthetic acceptance profile".to_owned(),
        })
        .expect("create synthetic profile");
    let inspection = service
        .inspect_source(world.source_path())
        .expect("inspect synthetic ResMed source");
    let prepared = service
        .prepare_import_job(PrepareImportJobRequest {
            profile_id: profile.id,
            source_id: inspection.source_id.clone(),
        })
        .expect("prepare cancellable job");
    let cancelled = service
        .cancel_import_job(profile.id, prepared.job.id)
        .expect("cancel prepared job");

    world.renderer_json.extend([
        serde_json::to_value(&inspection).expect("serialize inspection"),
        serde_json::to_value(&prepared).expect("serialize preparation"),
        serde_json::to_value(&cancelled).expect("serialize cancellation"),
    ]);
    world.profile_id = Some(profile.id);
    world.job_id = Some(prepared.job.id);
    world.inspection = Some(inspection);
    world.first_preparation = Some(prepared);
    world.cancelled_job = Some(cancelled);

    drop(service);
    let reopened = world.open_service();
    let reopened_job = reopened
        .get_import_job(profile.id, world.job_id.expect("cancelled job ID"))
        .expect("read cancelled job after reopening storage");
    world
        .renderer_json
        .push(serde_json::to_value(&reopened_job).expect("serialize reopened cancelled job"));
    world.reopened_job = Some(reopened_job);
}

#[when("the OPAP service starts after the interruption")]
fn start_after_interruption(world: &mut ServiceWorld) {
    let service = world.open_service();
    let profile_id = world.profile_id.expect("interrupted job profile");
    let job_id = world.job_id.expect("interrupted job ID");
    let recovered = service
        .get_import_job(profile_id, job_id)
        .expect("read recovered import job");
    world
        .renderer_json
        .push(serde_json::to_value(&recovered).expect("serialize recovered import job"));
    world.reopened_job = Some(recovered);
}

#[when("the unsupported source import is prepared")]
fn prepare_unsupported_source(world: &mut ServiceWorld) {
    let service = world.open_service();
    let profile = service
        .create_profile(CreateProfileRequest {
            display_name: "Synthetic acceptance profile".to_owned(),
        })
        .expect("create synthetic profile");
    let inspection = service
        .inspect_source(world.source_path())
        .expect("inspect synthetic unsupported source");
    let error = service
        .prepare_import_job(PrepareImportJobRequest {
            profile_id: profile.id,
            source_id: inspection.source_id.clone(),
        })
        .expect_err("unsupported source must not prepare a job");
    let job_count = service
        .list_import_jobs(profile.id)
        .expect("list jobs after rejected preparation")
        .len();

    world.renderer_json.extend([
        serde_json::to_value(&inspection).expect("serialize unsupported inspection"),
        serde_json::to_value(&error).expect("serialize unsupported error"),
    ]);
    world.inspection = Some(inspection);
    world.service_error = Some(error);
    world.persisted_job_count = Some(job_count);
}

#[when("a missing private native source is inspected")]
fn inspect_missing_private_source(world: &mut ServiceWorld) {
    let missing = world
        .fixture
        .as_ref()
        .expect("service workspace")
        .path()
        .join(MISSING_SOURCE_CANARY);
    world.source_path = Some(missing.clone());
    let service = world.open_service();
    let error = service
        .inspect_source(&missing)
        .expect_err("missing source must fail inspection");
    world.renderer_json.push(
        serde_json::to_value(&error).expect("serialize source-unavailable error for renderer"),
    );
    world.service_error = Some(error);
}

#[then("bootstrap reports session import is unavailable")]
fn bootstrap_is_honest(world: &mut ServiceWorld) {
    let bootstrap = world.bootstrap.as_ref().expect("bootstrap response");
    assert_eq!(API_SCHEMA_VERSION, 2);
    assert_eq!(bootstrap.api_schema_version, 2);
    assert!(bootstrap.capabilities.profile_management);
    assert!(bootstrap.capabilities.source_inspection);
    assert!(bootstrap.capabilities.import_job_preparation);
    assert!(!bootstrap.capabilities.session_import);
    let importer = bootstrap
        .importers
        .iter()
        .find(|importer| importer.id == "resmed")
        .expect("ResMed importer capability");
    assert_eq!(importer.id, "resmed");
    assert!(importer.source_inspection);
    assert!(!importer.session_import);
    assert_eq!(
        importer.unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );
}

#[then("the inspection returns an opaque source handle and only a serial suffix")]
fn inspection_is_opaque_and_redacted(world: &mut ServiceWorld) {
    let inspection = world.inspection.as_ref().expect("source inspection");
    assert!(inspection.recognized);
    assert_eq!(inspection.importer_id.as_deref(), Some("resmed"));
    assert_eq!(inspection.source_label, "ResMed SD card");
    assert_opaque_source_id(&inspection.source_id);
    let device = inspection
        .device
        .as_ref()
        .expect("redacted device identity");
    assert_eq!(device.serial_suffix, SERIAL_SUFFIX);
    assert_ne!(device.serial_suffix, FULL_SYNTHETIC_SERIAL);
    assert!(!inspection.session_import.available);
    assert_eq!(
        inspection.session_import.unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );
}

#[then("the renderer inspection JSON contains no absolute path or full serial")]
fn inspection_json_is_private(world: &mut ServiceWorld) {
    assert_renderer_json_is_private(world);
    let json = world.renderer_json.first().expect("inspection JSON");
    assert_eq!(json["device"]["serial_suffix"], SERIAL_SUFFIX);
    assert!(json.get("source_id").is_some());
    assert!(json.get("source_path").is_none());
    assert!(json["device"].get("serial").is_none());
}

#[then("exactly one durable blocked job exists and no import ran")]
fn one_durable_blocked_job(world: &mut ServiceWorld) {
    let first = world.first_preparation.as_ref().expect("first preparation");
    let reopened = world.reopened_job.as_ref().expect("durable reopened job");
    assert!(first.created);
    assert_eq!(first.job.status, ImportJobStatus::Blocked);
    assert_eq!(first.job.phase, ImportJobPhase::AwaitingSessionImporter);
    assert!(first.job.can_cancel);
    assert!(first.job.started_at_ms.is_none());
    assert!(first.job.finished_at_ms.is_none());
    assert_eq!(first.job.counts.sessions_created, 0);
    assert_eq!(first.job.counts.sessions_updated, 0);
    assert_eq!(first.job.counts.events_written, 0);
    assert_eq!(first.job.counts.waveform_chunks_written, 0);
    assert_eq!(reopened.status, ImportJobStatus::Blocked);
    assert_eq!(reopened.id, first.job.id);
    assert_eq!(world.persisted_job_count, Some(1));
    let persisted_source_id = world
        .persisted_source_id
        .as_deref()
        .expect("persisted opaque source ID");
    assert_opaque_source_id(persisted_source_id);
    assert_eq!(persisted_source_id, first.job.source_id);
    let internal_request_key = world
        .persisted_request_key
        .as_deref()
        .expect("service-generated persisted request key");
    assert_canonical_internal_request_key(internal_request_key);
    assert!(!internal_request_key.contains(FULL_SYNTHETIC_SERIAL));
    assert_eq!(
        world.service_error.as_ref().expect("run guard").code,
        ApiErrorCode::CapabilityUnavailable
    );
}

#[then("the repeated preparation returns the same job without creating one")]
fn repeated_preparation_is_idempotent(world: &mut ServiceWorld) {
    let first = world.first_preparation.as_ref().expect("first preparation");
    let repeated = world
        .repeated_preparation
        .as_ref()
        .expect("repeated preparation");
    assert!(!repeated.created);
    assert_eq!(repeated.job.id, first.job.id);
    assert_eq!(repeated.job.source_id, first.job.source_id);
    assert_eq!(repeated.job.status, ImportJobStatus::Blocked);
}

#[then("the renderer cannot supply an import request key")]
fn renderer_cannot_supply_request_key(world: &mut ServiceWorld) {
    let request_json = world
        .renderer_request_json
        .as_ref()
        .expect("serialized preparation request");
    assert!(request_json.get("profile_id").is_some());
    assert!(request_json.get("source_id").is_some());
    assert!(request_json.get("request_key").is_none());
    let first_response = serde_json::to_value(
        world
            .first_preparation
            .as_ref()
            .expect("first preparation response"),
    )
    .expect("serialize first preparation response");
    assert!(first_response["job"].get("request_key").is_none());

    let mut injected = request_json.clone();
    injected
        .as_object_mut()
        .expect("request JSON object")
        .insert(
            "request_key".to_owned(),
            Value::String(format!("opap-request:{FULL_SYNTHETIC_SERIAL}")),
        );
    assert!(
        serde_json::from_value::<PrepareImportJobRequest>(injected).is_err(),
        "the service request DTO accepted a caller-controlled request key"
    );
}

#[then("the renderer job JSON contains no absolute path or full serial")]
fn job_json_is_private(world: &mut ServiceWorld) {
    assert_renderer_json_is_private(world);
    let first = world.first_preparation.as_ref().expect("first preparation");
    assert_opaque_source_id(&first.job.source_id);
    assert_eq!(first.job.source_label, "ResMed SD card");
}

#[then("the reopened job has the typed cancelled state")]
fn cancellation_is_durable_and_typed(world: &mut ServiceWorld) {
    let cancelled = world.cancelled_job.as_ref().expect("cancelled response");
    let reopened = world.reopened_job.as_ref().expect("reopened cancelled job");
    assert_eq!(cancelled.status, ImportJobStatus::Cancelled);
    assert_eq!(cancelled.phase, ImportJobPhase::Finished);
    assert!(!cancelled.can_cancel);
    assert_eq!(reopened.status, ImportJobStatus::Cancelled);
    assert_eq!(reopened.phase, ImportJobPhase::Finished);
    assert_eq!(reopened.id, cancelled.id);
    assert!(reopened.finished_at_ms.is_some());
    assert!(reopened.failure_message.is_none());
    assert_renderer_json_is_private(world);
    let cancelled_json = serde_json::to_value(reopened).expect("serialize reopened cancellation");
    assert_eq!(cancelled_json["status"], "cancelled");
}

#[then("the running job is recovered to blocked")]
fn running_job_recovers_to_blocked(world: &mut ServiceWorld) {
    let recovered = world.reopened_job.as_ref().expect("recovered job");
    assert_eq!(recovered.status, ImportJobStatus::Blocked);
    assert_eq!(recovered.phase, ImportJobPhase::AwaitingSessionImporter);
    assert!(recovered.can_cancel);
    assert!(recovered.started_at_ms.is_some());
    assert!(recovered.finished_at_ms.is_none());
    assert_eq!(
        recovered.unavailable_reason.as_deref(),
        Some(SESSION_IMPORT_UNAVAILABLE_REASON)
    );
}

#[then("untrusted legacy importer metadata is redacted")]
fn legacy_importer_metadata_is_redacted(world: &mut ServiceWorld) {
    let recovered = world.reopened_job.as_ref().expect("recovered job");
    assert_eq!(recovered.importer_id, "unknown");
    assert_eq!(recovered.source_label, "Selected folder");
    assert_renderer_json_is_private(world);
}

#[then("preparation fails with source not supported and creates no job")]
fn unsupported_preparation_is_rejected(world: &mut ServiceWorld) {
    let inspection = world.inspection.as_ref().expect("unsupported inspection");
    assert!(!inspection.recognized);
    assert!(inspection.importer_id.is_none());
    assert_eq!(
        world
            .service_error
            .as_ref()
            .expect("unsupported error")
            .code,
        ApiErrorCode::SourceNotSupported
    );
    assert_eq!(world.persisted_job_count, Some(0));
}

#[then("the renderer error JSON contains no absolute path")]
fn unsupported_error_json_is_private(world: &mut ServiceWorld) {
    assert_renderer_json_is_private(world);
}

#[then("inspection fails with a sanitized source unavailable error")]
fn missing_source_error_is_sanitized(world: &mut ServiceWorld) {
    let error = world
        .service_error
        .as_ref()
        .expect("source-unavailable error");
    assert_eq!(error.code, ApiErrorCode::SourceUnavailable);
    assert_eq!(error.message, "could not read the selected source");
    assert!(error.retryable);
    assert!(error.field.is_none());
}

fn install_service_workspace(world: &mut ServiceWorld) {
    let fixture = tempdir().expect("create temporary service workspace");
    let fixture_root = fixture
        .path()
        .canonicalize()
        .expect("canonical temporary workspace");
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repository path");
    assert!(
        !fixture_root.starts_with(&repository),
        "synthetic service workspace unexpectedly exists inside {}",
        repository.display()
    );
    world.database_path = Some(fixture.path().join("opap-acceptance.sqlite3"));
    world.fixture = Some(fixture);
}

fn assert_opaque_source_id(source_id: &str) {
    let suffix = source_id
        .strip_prefix("opap-source:")
        .expect("source handle has OPAP prefix");
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert!(!source_id.contains('/'));
    assert!(!source_id.contains('\\'));
}

fn assert_canonical_internal_request_key(request_key: &str) {
    let suffix = request_key
        .strip_prefix("opap-request:")
        .expect("request key has OPAP prefix");
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
}

fn assert_renderer_json_is_private(world: &ServiceWorld) {
    assert!(
        !world.renderer_json.is_empty(),
        "scenario should exercise a serialized renderer boundary"
    );
    let fixture = world.fixture.as_ref().expect("temporary service workspace");
    let mut forbidden = vec![fixture.path().to_string_lossy().into_owned()];
    let canonical_fixture = fixture
        .path()
        .canonicalize()
        .expect("canonical temporary workspace")
        .to_string_lossy()
        .into_owned();
    forbidden.push(canonical_fixture);
    if let Some(source_path) = &world.source_path {
        forbidden.push(source_path.to_string_lossy().into_owned());
        if let Ok(canonical_source) = source_path.canonicalize() {
            forbidden.push(canonical_source.to_string_lossy().into_owned());
        }
        if let Some(source_name) = source_path.file_name() {
            forbidden.push(source_name.to_string_lossy().into_owned());
        }
    }
    if let Some(full_serial) = &world.full_serial {
        forbidden.push(full_serial.clone());
    }
    forbidden.sort();
    forbidden.dedup();

    for value in &world.renderer_json {
        for forbidden in &forbidden {
            assert_json_does_not_contain(value, forbidden);
        }
    }
}

fn assert_json_does_not_contain(value: &Value, forbidden: &str) {
    match value {
        Value::String(text) => assert!(
            !text.contains(forbidden),
            "renderer JSON exposed forbidden synthetic fixture detail"
        ),
        Value::Array(values) => {
            for value in values {
                assert_json_does_not_contain(value, forbidden);
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    !key.contains(forbidden),
                    "renderer JSON key exposed forbidden synthetic fixture detail"
                );
                assert_json_does_not_contain(value, forbidden);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[tokio::main]
async fn main() {
    ServiceWorld::run(format!(
        "{}/features/service_workflow.feature",
        env!("CARGO_MANIFEST_DIR")
    ))
    .await;
}
