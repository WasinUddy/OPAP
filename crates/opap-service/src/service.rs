// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    API_SCHEMA_VERSION, ApiError, ApiErrorCode, ApiResult, AppBootstrap, AppCapabilities,
    CreateProfileRequest, DeviceDto, ImportJobCounts, ImportJobDto, ImportJobPhase,
    ImportJobStatus, ImportWarningDto, ImporterCapability, PrepareImportJobRequest,
    PrepareImportJobResponse, ProfileDto, SESSION_IMPORT_UNAVAILABLE_REASON,
    SessionImportCapability, SourceInspection, WarningSeverityDto,
};
use opap_core::{
    DirectorySource, IMPORT_SCHEMA_VERSION, ImportSource, Importer, MachineInfo, SourceEntryKind,
    WarningSeverity, resmed::ResmedImporter,
};
use opap_storage::{
    Database, Error as StorageError, ImportHistory, ImportStatus, InitialImportStatus, NewImport,
    NewProfile, Profile,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const PROFILE_NAME_MAX_CHARS: usize = 100;
const REQUEST_KEY_PREFIX: &str = "opap-request:";
const DEVICE_DISPLAY_MAX_CHARS: usize = 80;
const DEVICE_DISPLAY_MAX_BYTES: usize = 256;
const UNKNOWN_DEVICE_MODEL: &str = "Unknown ResMed device";
const UNKNOWN_IMPORTER_ID: &str = "unknown";
const REQUEST_KEY_GENERATION_ATTEMPTS: usize = 8;

/// Injectable time source; hosts use [`SystemClock`] and tests can be exact.
pub trait Clock {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        signed_milliseconds(SystemTime::now())
    }
}

/// Stateful application boundary backed by one local OPAP database.
pub struct AppService<C = SystemClock> {
    database: Database,
    clock: C,
    sources: Mutex<BTreeMap<String, RegisteredSource>>,
}

impl AppService<SystemClock> {
    pub fn open(database_path: impl AsRef<Path>) -> ApiResult<Self> {
        let database = Database::open(database_path).map_err(ApiError::storage)?;
        Self::from_database(database, SystemClock)
    }

    pub fn open_in_memory() -> ApiResult<Self> {
        let database = Database::open_in_memory().map_err(ApiError::storage)?;
        Self::from_database(database, SystemClock)
    }
}

impl<C: Clock> AppService<C> {
    pub fn from_database(database: Database, clock: C) -> ApiResult<Self> {
        database
            .imports()
            .recover_running(
                clock.now_ms(),
                "execution interrupted; session importer is unavailable",
            )
            .map_err(ApiError::storage)?;
        Ok(Self {
            database,
            clock,
            sources: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn bootstrap(&self) -> ApiResult<AppBootstrap> {
        Ok(AppBootstrap {
            api_schema_version: API_SCHEMA_VERSION,
            import_report_schema_version: IMPORT_SCHEMA_VERSION,
            storage_schema_version: self.database.schema_version().map_err(ApiError::storage)?,
            capabilities: AppCapabilities {
                profile_management: true,
                source_inspection: true,
                import_job_preparation: true,
                session_import: false,
            },
            importers: vec![ImporterCapability {
                id: opap_core::resmed::IMPORTER_ID.to_owned(),
                display_name: "ResMed SD card".to_owned(),
                source_inspection: true,
                session_import: false,
                unavailable_reason: Some(SESSION_IMPORT_UNAVAILABLE_REASON.to_owned()),
            }],
            profiles: self.list_profiles()?,
        })
    }

    pub fn list_profiles(&self) -> ApiResult<Vec<ProfileDto>> {
        self.database
            .profiles()
            .list()
            .map(|profiles| profiles.into_iter().map(profile_dto).collect())
            .map_err(ApiError::storage)
    }

    pub fn create_profile(&self, request: CreateProfileRequest) -> ApiResult<ProfileDto> {
        let display_name = validate_profile_name(&request.display_name)?;
        let profile = self
            .database
            .profiles()
            .insert(&NewProfile {
                display_name,
                now_ms: self.clock.now_ms(),
            })
            .map_err(ApiError::storage)?;
        Ok(profile_dto(profile))
    }

    /// Inspects a native directory and retains its capability behind an opaque
    /// identifier. The filesystem path is deliberately not a serializable DTO.
    pub fn inspect_source(&self, directory: impl AsRef<Path>) -> ApiResult<SourceInspection> {
        let source_id = new_source_id();
        let (inspection, source, importer_id) =
            inspect_directory(directory.as_ref(), source_id.clone())?;
        self.sources
            .lock()
            .map_err(|_| source_registry_error())?
            .insert(
                source_id,
                RegisteredSource {
                    _source: source,
                    importer_id,
                    request_keys: BTreeMap::new(),
                },
            );
        Ok(inspection)
    }

    /// Persists an idempotent import job without claiming that it can run.
    ///
    /// Until `opap-core` implements ResMed session parsing, returned jobs are
    /// explicitly `blocked`/`awaiting_session_importer`. They remain
    /// cancellable and survive application restarts.
    pub fn prepare_import_job(
        &self,
        request: PrepareImportJobRequest,
    ) -> ApiResult<PrepareImportJobResponse> {
        self.require_profile(request.profile_id)?;
        validate_source_id(&request.source_id)?;

        let mut sources = self.sources.lock().map_err(|_| source_registry_error())?;
        let registered = sources
            .get_mut(&request.source_id)
            .ok_or_else(source_handle_unavailable)?;
        let importer_id = registered.importer_id.clone().ok_or_else(|| {
            ApiError::for_field(
                ApiErrorCode::SourceNotSupported,
                "the selected source is not a supported CPAP source",
                "source_id",
            )
        })?;

        if let Some(request_key) = registered.request_keys.get(&request.profile_id) {
            if let Some(existing) = self
                .database
                .imports()
                .find_by_key(request.profile_id, request_key)
                .map_err(ApiError::storage)?
            {
                if existing.source_uri != request.source_id || existing.loader_name != importer_id {
                    return Err(source_registry_error());
                }
                return Ok(PrepareImportJobResponse {
                    job: job_dto(existing),
                    created: false,
                });
            }
            registered.request_keys.remove(&request.profile_id);
        }

        for _ in 0..REQUEST_KEY_GENERATION_ATTEMPTS {
            let request_key = new_request_key();
            let begun = self
                .database
                .imports()
                .begin_or_get(&NewImport {
                    profile_id: request.profile_id,
                    machine_id: None,
                    import_key: &request_key,
                    source_uri: &request.source_id,
                    loader_name: &importer_id,
                    initial_status: InitialImportStatus::Blocked,
                    state_message: Some(SESSION_IMPORT_UNAVAILABLE_REASON),
                    created_at_ms: self.clock.now_ms(),
                })
                .map_err(ApiError::storage)?;

            if begun.inserted {
                registered
                    .request_keys
                    .insert(request.profile_id, request_key);
                return Ok(PrepareImportJobResponse {
                    job: job_dto(begun.history),
                    created: true,
                });
            }
        }

        Err(ApiError::new(
            ApiErrorCode::Internal,
            "could not allocate an opaque import request identifier",
        ))
    }

    pub fn list_import_jobs(&self, profile_id: i64) -> ApiResult<Vec<ImportJobDto>> {
        self.require_profile(profile_id)?;
        let jobs = self
            .database
            .imports()
            .list_by_profile(profile_id)
            .map_err(ApiError::storage)?;
        Ok(jobs.into_iter().map(job_dto).collect())
    }

    pub fn get_import_job(&self, profile_id: i64, job_id: i64) -> ApiResult<ImportJobDto> {
        let history = self.find_profile_job(profile_id, job_id)?;
        Ok(job_dto(history))
    }

    pub fn cancel_import_job(&self, profile_id: i64, job_id: i64) -> ApiResult<ImportJobDto> {
        let history = self.find_profile_job(profile_id, job_id)?;
        if !matches!(
            history.status,
            ImportStatus::Blocked | ImportStatus::Running
        ) {
            return Err(job_not_cancellable());
        }

        match self
            .database
            .imports()
            .cancel(job_id, self.clock.now_ms(), Some("cancelled by user"))
        {
            Ok(Some(cancelled)) => Ok(job_dto(cancelled)),
            Ok(None) => Err(job_not_found()),
            Err(StorageError::InvalidImportTransition { .. }) => Err(job_not_cancellable()),
            Err(error) => Err(ApiError::storage(error)),
        }
    }

    /// Explicit guard used by hosts until a real session-import executor exists.
    pub fn run_import_job(&self, profile_id: i64, job_id: i64) -> ApiResult<ImportJobDto> {
        let _job = self.find_profile_job(profile_id, job_id)?;
        Err(ApiError::new(
            ApiErrorCode::CapabilityUnavailable,
            "ResMed session import is not implemented yet; the prepared job was not run",
        ))
    }

    fn require_profile(&self, profile_id: i64) -> ApiResult<Profile> {
        self.database
            .profiles()
            .get(profile_id)
            .map_err(ApiError::storage)?
            .ok_or_else(|| {
                ApiError::new(
                    ApiErrorCode::ProfileNotFound,
                    "the requested profile does not exist",
                )
            })
    }

    fn find_profile_job(&self, profile_id: i64, job_id: i64) -> ApiResult<ImportHistory> {
        let history = self
            .database
            .imports()
            .get(job_id)
            .map_err(ApiError::storage)?;
        history
            .filter(|job| job.profile_id == profile_id)
            .ok_or_else(job_not_found)
    }
}

fn inspect_directory(
    directory: &Path,
    source_id: String,
) -> ApiResult<(SourceInspection, DirectorySource, Option<String>)> {
    validate_directory(directory)?;
    let source = DirectorySource::open(directory).map_err(ApiError::from)?;
    let inventory = source.inventory().map_err(ApiError::from)?;
    let importer = ResmedImporter;
    let discovery = {
        let inventoried = InventoriedSource {
            source: &source,
            inventory: &inventory,
        };
        importer.discover(&inventoried).map_err(ApiError::from)?
    };

    let (inventory, importer_id, device, warnings) = match discovery {
        Some(discovery) => {
            let device = discovery.device.machine;
            (
                discovery.inventory,
                Some(importer.id().to_owned()),
                Some(device_dto(device)),
                discovery
                    .warnings
                    .into_iter()
                    .map(|warning| {
                        let message = safe_warning_message(&warning.code);
                        ImportWarningDto {
                            code: warning.code,
                            severity: match warning.severity {
                                WarningSeverity::Info => WarningSeverityDto::Info,
                                WarningSeverity::Warning => WarningSeverityDto::Warning,
                            },
                            message,
                        }
                    })
                    .collect(),
            )
        }
        None => (inventory, None, None, Vec::new()),
    };

    let files = count_entries(&inventory, SourceEntryKind::File);
    let directories = count_entries(&inventory, SourceEntryKind::Directory);
    let recognized = importer_id.is_some();
    let source_label = source_label(importer_id.as_deref());
    let inspection = SourceInspection {
        source_id,
        recognized,
        source_label,
        files,
        directories,
        total_bytes: inventory.total_file_bytes,
        importer_id: importer_id.clone(),
        device,
        warnings,
        session_import: SessionImportCapability {
            available: false,
            unavailable_reason: Some(if recognized {
                SESSION_IMPORT_UNAVAILABLE_REASON.to_owned()
            } else {
                "source_not_recognized".to_owned()
            }),
        },
    };
    Ok((inspection, source, importer_id))
}

fn validate_directory(directory: &Path) -> ApiResult<()> {
    if !directory.is_absolute() {
        return Err(ApiError::for_field(
            ApiErrorCode::SourcePathInvalid,
            "native source selection must provide an absolute path",
            "source",
        ));
    }
    Ok(())
}

fn validate_profile_name(display_name: &str) -> ApiResult<&str> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err(ApiError::for_field(
            ApiErrorCode::InvalidRequest,
            "profile display name is required",
            "display_name",
        ));
    }
    if display_name.chars().count() > PROFILE_NAME_MAX_CHARS {
        return Err(ApiError::for_field(
            ApiErrorCode::InvalidRequest,
            format!("profile display name must be at most {PROFILE_NAME_MAX_CHARS} characters"),
            "display_name",
        ));
    }
    if display_name.chars().any(char::is_control) {
        return Err(ApiError::for_field(
            ApiErrorCode::InvalidRequest,
            "profile display name cannot contain control characters",
            "display_name",
        ));
    }
    Ok(display_name)
}

fn validate_source_id(source_id: &str) -> ApiResult<()> {
    if !is_valid_source_id(source_id) {
        return Err(ApiError::for_field(
            ApiErrorCode::InvalidRequest,
            "source ID is invalid",
            "source_id",
        ));
    }
    Ok(())
}

fn is_valid_source_id(source_id: &str) -> bool {
    let Some(suffix) = source_id.strip_prefix("opap-source:") else {
        return false;
    };
    let random_id = suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let legacy_id = suffix.strip_prefix("legacy-").is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 19
            && id.as_bytes()[0].is_ascii_digit()
            && id.as_bytes()[0] != b'0'
            && id.bytes().all(|b| b.is_ascii_digit())
    });
    random_id || legacy_id
}

fn profile_dto(profile: Profile) -> ProfileDto {
    ProfileDto {
        id: profile.id,
        display_name: profile.display_name,
        created_at_ms: profile.created_at_ms,
        updated_at_ms: profile.updated_at_ms,
    }
}

fn job_dto(history: ImportHistory) -> ImportJobDto {
    let (status, phase, unavailable_reason, failure_message, can_cancel) = match history.status {
        ImportStatus::Blocked => (
            ImportJobStatus::Blocked,
            ImportJobPhase::AwaitingSessionImporter,
            Some(SESSION_IMPORT_UNAVAILABLE_REASON.to_owned()),
            None,
            true,
        ),
        ImportStatus::Running => (
            ImportJobStatus::Running,
            ImportJobPhase::Importing,
            None,
            None,
            true,
        ),
        ImportStatus::Completed => (
            ImportJobStatus::Completed,
            ImportJobPhase::Finished,
            None,
            None,
            false,
        ),
        ImportStatus::Cancelled => (
            ImportJobStatus::Cancelled,
            ImportJobPhase::Finished,
            None,
            None,
            false,
        ),
        ImportStatus::Failed => (
            ImportJobStatus::Failed,
            ImportJobPhase::Finished,
            None,
            Some("The import did not complete".to_owned()),
            false,
        ),
    };

    ImportJobDto {
        id: history.id,
        profile_id: history.profile_id,
        attempt: history.attempt,
        retry_of_id: history.retry_of_id,
        source_id: safe_persisted_source_id(&history.source_uri, history.id),
        source_label: source_label(Some(&history.loader_name)),
        importer_id: safe_importer_id(&history.loader_name).to_owned(),
        status,
        phase,
        created_at_ms: history.created_at_ms,
        updated_at_ms: history.updated_at_ms,
        started_at_ms: history.started_at_ms,
        finished_at_ms: history.completed_at_ms,
        counts: ImportJobCounts {
            sessions_created: history.sessions_created,
            sessions_updated: history.sessions_updated,
            events_written: history.events_written,
            waveform_chunks_written: history.waveform_chunks_written,
        },
        can_cancel,
        unavailable_reason,
        failure_message,
    }
}

fn job_not_cancellable() -> ApiError {
    ApiError::new(
        ApiErrorCode::JobNotCancellable,
        "the import job is already in a terminal state",
    )
}

fn job_not_found() -> ApiError {
    ApiError::new(
        ApiErrorCode::JobNotFound,
        "the requested import job does not exist",
    )
}

fn source_handle_unavailable() -> ApiError {
    ApiError::for_field(
        ApiErrorCode::SourceUnavailable,
        "source handle is unavailable; select the folder again",
        "source_id",
    )
}

fn source_registry_error() -> ApiError {
    ApiError::new(
        ApiErrorCode::Internal,
        "the native source registry is unavailable",
    )
}

struct RegisteredSource {
    // Holding the capability keeps the authorized root native-only and ready
    // for a future executor without retaining an absolute path in any DTO.
    _source: DirectorySource,
    importer_id: Option<String>,
    request_keys: BTreeMap<i64, String>,
}

struct InventoriedSource<'source> {
    source: &'source DirectorySource,
    inventory: &'source opap_core::SourceInventory,
}

impl ImportSource for InventoriedSource<'_> {
    fn inventory(&self) -> Result<opap_core::SourceInventory, opap_core::ImportError> {
        Ok((*self.inventory).clone())
    }

    fn read_file(
        &self,
        relative_path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, opap_core::ImportError> {
        self.source.read_file(relative_path, max_bytes)
    }
}

fn count_entries(inventory: &opap_core::SourceInventory, kind: SourceEntryKind) -> u64 {
    u64::try_from(
        inventory
            .entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .count(),
    )
    .unwrap_or(u64::MAX)
}

fn new_source_id() -> String {
    format!("opap-source:{}", Uuid::new_v4().simple())
}

fn new_request_key() -> String {
    format!("{REQUEST_KEY_PREFIX}{}", Uuid::new_v4().simple())
}

fn source_label(importer_id: Option<&str>) -> String {
    match importer_id {
        Some(opap_core::resmed::IMPORTER_ID) => "ResMed SD card",
        _ => "Selected folder",
    }
    .to_owned()
}

fn safe_importer_id(importer_id: &str) -> &'static str {
    match importer_id {
        opap_core::resmed::IMPORTER_ID => opap_core::resmed::IMPORTER_ID,
        _ => UNKNOWN_IMPORTER_ID,
    }
}

fn safe_persisted_source_id(value: &str, job_id: i64) -> String {
    if is_valid_source_id(value) {
        value.to_owned()
    } else {
        format!("opap-source:legacy-{job_id}")
    }
}

fn device_dto(device: MachineInfo) -> DeviceDto {
    let series = sanitize_device_display_field(&device.model, &device.serial)
        .and_then(|_| canonical_device_series(&device.series, &device.serial));
    DeviceDto {
        brand: "ResMed".to_owned(),
        model: series
            .map(str::to_owned)
            .unwrap_or_else(|| UNKNOWN_DEVICE_MODEL.to_owned()),
        // Product codes are untrusted free text and OPAP does not yet carry a
        // service-owned catalog capable of mapping them without leaking data.
        model_number: String::new(),
        serial_suffix: safe_serial_suffix(&device.serial),
        series: series.unwrap_or_default().to_owned(),
    }
}

fn canonical_device_series(value: &str, full_serial: &str) -> Option<&'static str> {
    let value = sanitize_device_display_field(value, full_serial)?;
    match value.as_str() {
        "AirSense 11" => Some("AirSense 11"),
        "AirCurve 11" => Some("AirCurve 11"),
        "AirSense 10" => Some("AirSense 10"),
        "Sleepmate 10" => Some("SleepMate 10"),
        "AirCurve 10" => Some("AirCurve 10"),
        "Lumis" => Some("Lumis"),
        "S9" => Some("S9"),
        _ => None,
    }
}

fn sanitize_device_display_field(value: &str, full_serial: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || !value.is_ascii()
        || value.len() > DEVICE_DISPLAY_MAX_BYTES
        || value.chars().count() > DEVICE_DISPLAY_MAX_CHARS
        || value.chars().any(is_unsafe_display_character)
        || looks_like_path_or_url(value)
        || contains_case_insensitive(value, full_serial.trim())
    {
        return None;
    }
    Some(value.to_owned())
}

fn is_unsafe_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}'
        )
}

fn looks_like_path_or_url(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    value.contains(['/', '\\', ':'])
        || value.starts_with(['.', '~'])
        || value.contains("..")
        || lowercase.starts_with("www.")
        || lowercase.contains("%2f")
        || lowercase.contains("%5c")
}

fn contains_case_insensitive(value: &str, needle: &str) -> bool {
    !needle.is_empty()
        && needle.len() <= value.len()
        && value.to_lowercase().contains(&needle.to_lowercase())
}

fn safe_serial_suffix(serial: &str) -> String {
    let serial = serial.trim();
    if serial.len() <= 4
        || serial.len() > DEVICE_DISPLAY_MAX_BYTES
        || !serial.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return String::new();
    }
    serial[serial.len() - 4..].to_owned()
}

fn safe_warning_message(code: &str) -> String {
    match code {
        "missing_identification" => "The source has no device identification file",
        _ => "The source contains a non-fatal issue",
    }
    .to_owned()
}

fn signed_milliseconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => positive_milliseconds(duration),
        Err(error) => positive_milliseconds(error.duration()).saturating_neg(),
    }
}

fn positive_milliseconds(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_name_is_trimmed_and_bounded() {
        assert_eq!(validate_profile_name("  Alex  ").expect("valid"), "Alex");
        assert_eq!(
            validate_profile_name("\n").expect_err("empty").code,
            ApiErrorCode::InvalidRequest
        );
        assert!(validate_profile_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn service_generated_request_keys_use_the_internal_opaque_format() {
        let request_key = new_request_key();
        let suffix = request_key
            .strip_prefix(REQUEST_KEY_PREFIX)
            .expect("request key prefix");
        assert_eq!(suffix.len(), 32);
        assert!(
            suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }

    #[test]
    fn device_display_fields_are_bounded_and_privacy_sanitized() {
        let serial = "PrivateSerialABC123456789";
        assert_eq!(
            sanitize_device_display_field("  AirSense 10 AutoSet  ", serial).as_deref(),
            Some("AirSense 10 AutoSet")
        );

        for unsafe_value in [
            "/Users/private/model",
            r"C:\Users\private\model",
            "https://example.test/model",
            "www.example.test",
            "../private/model",
            "∕Users∕private∕model",
            "%2fUsers%2fprivate%2fmodel",
            "model\nname",
            "model\u{202e}name",
            "AirSense privateserialabc123456789",
        ] {
            assert_eq!(
                sanitize_device_display_field(unsafe_value, serial),
                None,
                "unsafe value was accepted"
            );
        }
        assert_eq!(
            sanitize_device_display_field(&"x".repeat(DEVICE_DISPLAY_MAX_CHARS + 1), serial),
            None
        );
        assert_eq!(safe_serial_suffix("23123456789"), "6789");
        assert!(safe_serial_suffix("1234").is_empty());
        assert!(safe_serial_suffix("serial/PHI").is_empty());
        assert!(safe_serial_suffix("serial-1234").is_empty());
        assert!(safe_serial_suffix("serial\u{202e}1234").is_empty());
    }

    #[test]
    fn milliseconds_before_epoch_are_negative() {
        assert_eq!(
            signed_milliseconds(UNIX_EPOCH - Duration::from_millis(25)),
            -25
        );
    }
}
