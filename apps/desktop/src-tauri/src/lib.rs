// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Thin Tauri adapter for OPAP's framework-neutral application service.
//!
//! Every clinical and workflow DTO returned over IPC comes directly from
//! `opap-service`. Native paths never enter a command argument or result: the
//! host opens a system folder picker and passes the selected path directly to
//! [`AppService::inspect_source`].

use opap_service::{
    ApiError, ApiErrorCode, ApiResult, AppBootstrap, AppService, CreateProfileRequest,
    ImportJobDto, PrepareImportJobRequest, PrepareImportJobResponse, ProfileDto, SourceInspection,
    SystemClock,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

const DATABASE_FILE_NAME: &str = "opap.sqlite3";
const SOURCE_REPOSITORY: &str = "https://github.com/WasinUddy/OPAP";
const GPL_V3_URL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";
const OSCAR_REFERENCE: &str = "OSCAR-code 64c5e90a26f91fb15868bcfcccde0c1e1522ac86";

/// Non-clinical build and licensing details for the About screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AboutInfo {
    pub name: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision_url: Option<String>,
    pub oscar_compatibility_reference: String,
    pub modified_work_notice: String,
    pub attribution: Vec<String>,
}

type Service = AppService<SystemClock>;

/// The only state shared with Tauri commands.
///
/// `rusqlite::Connection` is `Send` but not `Sync`; the mutex serializes the
/// short service operations and keeps database access off the renderer thread.
#[derive(Clone)]
pub struct ManagedService {
    inner: Arc<Mutex<Service>>,
}

impl ManagedService {
    fn new(service: Service) -> Self {
        Self {
            inner: Arc::new(Mutex::new(service)),
        }
    }

    async fn call<T, F>(&self, operation: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Service) -> ApiResult<T> + Send + 'static,
    {
        let service = Arc::clone(&self.inner);
        tauri::async_runtime::spawn_blocking(move || {
            let service = service.lock().map_err(|_| internal_error())?;
            operation(&service)
        })
        .await
        .map_err(|_| internal_error())?
    }
}

fn internal_error() -> ApiError {
    api_error(
        ApiErrorCode::Internal,
        "the native application service is unavailable",
        false,
        None,
    )
}

fn api_error(
    code: ApiErrorCode,
    message: impl Into<String>,
    retryable: bool,
    field: Option<&str>,
) -> ApiError {
    ApiError {
        code,
        message: message.into(),
        retryable,
        field: field.map(ToOwned::to_owned),
    }
}

fn about_info() -> AboutInfo {
    let build_revision = option_env!("OPAP_BUILD_REVISION")
        .filter(|revision| valid_revision(revision))
        .map(ToOwned::to_owned);
    let source_revision_url = build_revision
        .as_ref()
        .map(|revision| format!("{SOURCE_REPOSITORY}/commit/{revision}"));

    AboutInfo {
        name: "OPAP".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        license: "GPL-3.0-only".to_owned(),
        license_url: GPL_V3_URL.to_owned(),
        source_url: SOURCE_REPOSITORY.to_owned(),
        build_revision,
        source_revision_url,
        oscar_compatibility_reference: OSCAR_REFERENCE.to_owned(),
        modified_work_notice: "OPAP is modified software based in part on OSCAR and on the free and open-source software SleepyHead."
            .to_owned(),
        attribution: vec![
            "Copyright (C) 2011-2018 Mark Watkins".to_owned(),
            "Copyright (C) 2019-2025 The OSCAR Team".to_owned(),
            "Copyright (C) 2026 OPAP contributors".to_owned(),
        ],
    }
}

fn valid_revision(revision: &str) -> bool {
    (7..=40).contains(&revision.len()) && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Returns build provenance without reading user or clinical data.
#[tauri::command]
fn about() -> AboutInfo {
    about_info()
}

/// Returns the canonical service bootstrap contract.
#[tauri::command]
async fn app_bootstrap(state: State<'_, ManagedService>) -> ApiResult<AppBootstrap> {
    state.call(Service::bootstrap).await
}

/// Lists profiles through the canonical service boundary.
#[tauri::command]
async fn profile_list(state: State<'_, ManagedService>) -> ApiResult<Vec<ProfileDto>> {
    state.call(Service::list_profiles).await
}

/// Creates a profile through the canonical service boundary.
#[tauri::command]
async fn profile_create(
    state: State<'_, ManagedService>,
    request: CreateProfileRequest,
) -> ApiResult<ProfileDto> {
    state
        .call(move |service| service.create_profile(request))
        .await
}

/// Opens the native directory picker and inspects the selected source.
///
/// Cancellation is represented by `null`. This command intentionally has no
/// path argument, and [`SourceInspection`] contains only an opaque `source_id`.
#[tauri::command]
async fn source_select<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ManagedService>,
) -> ApiResult<Option<SourceInspection>> {
    let selected = app
        .dialog()
        .file()
        .set_title("Select a CPAP SD card folder")
        .blocking_pick_folder();
    let selected = selected.map(FilePath::into_path).transpose().map_err(|_| {
        api_error(
            ApiErrorCode::SourcePathInvalid,
            "the selected source is not a local directory",
            false,
            None,
        )
    })?;
    if selected.is_none() {
        return Ok(None);
    }

    state
        .call(move |service| inspect_selected_source(service, selected))
        .await
}

fn inspect_selected_source(
    service: &Service,
    selected: Option<PathBuf>,
) -> ApiResult<Option<SourceInspection>> {
    selected
        .map(|path| service.inspect_source(path))
        .transpose()
}

/// Persists an honest blocked job while session parsing remains unavailable.
#[tauri::command]
async fn import_prepare(
    state: State<'_, ManagedService>,
    request: PrepareImportJobRequest,
) -> ApiResult<PrepareImportJobResponse> {
    state
        .call(move |service| service.prepare_import_job(request))
        .await
}

/// Lists durable import jobs for one profile.
#[tauri::command]
async fn import_jobs(
    state: State<'_, ManagedService>,
    profile_id: i64,
) -> ApiResult<Vec<ImportJobDto>> {
    state
        .call(move |service| service.list_import_jobs(profile_id))
        .await
}

/// Cancels a blocked or running import job.
#[tauri::command]
async fn import_cancel(
    state: State<'_, ManagedService>,
    profile_id: i64,
    job_id: i64,
) -> ApiResult<ImportJobDto> {
    state
        .call(move |service| service.cancel_import_job(profile_id, job_id))
        .await
}

fn with_commands<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        about,
        app_bootstrap,
        profile_list,
        profile_create,
        source_select,
        import_prepare,
        import_jobs,
        import_cancel
    ])
}

fn initialize_service(app_data_dir: &Path) -> ApiResult<Service> {
    prepare_private_storage(app_data_dir)?;
    // SQLite's NOFOLLOW flag rejects any symlink component, including macOS's
    // system `/var` -> `/private/var` alias used by temporary and app-data
    // locations. Resolve the already-created private directory once, then let
    // SQLite enforce NOFOLLOW on the final database component.
    let storage_directory = std::fs::canonicalize(app_data_dir).map_err(|error| {
        storage_io_error(error, "could not resolve the application data directory")
    })?;
    let database_path = storage_directory.join(DATABASE_FILE_NAME);
    let service = Service::open(&database_path)?;
    set_private_file_permissions(&database_path)?;
    Ok(service)
}

fn prepare_private_storage(app_data_dir: &Path) -> ApiResult<()> {
    std::fs::create_dir_all(app_data_dir).map_err(|error| {
        storage_io_error(error, "could not create the application data directory")
    })?;
    let metadata = std::fs::symlink_metadata(app_data_dir).map_err(|error| {
        storage_io_error(error, "could not inspect the application data directory")
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(unsafe_storage_error(
            "the application data location must be a real directory",
        ));
    }

    set_private_directory_permissions(app_data_dir)?;
    let database_path = app_data_dir.join(DATABASE_FILE_NAME);
    reject_unsafe_database_file(&database_path)?;
    create_private_file(&database_path)?;
    set_private_file_permissions(&database_path)
}

fn reject_unsafe_database_file(database_path: &Path) -> ApiResult<()> {
    match std::fs::symlink_metadata(database_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(unsafe_storage_error(
                    "the local database location must be a regular file",
                ));
            }
            reject_linked_database(&metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_io_error(
            error,
            "could not inspect the local database file",
        )),
    }
}

#[cfg(unix)]
fn reject_linked_database(metadata: &std::fs::Metadata) -> ApiResult<()> {
    use std::os::unix::fs::MetadataExt;

    if metadata.nlink() != 1 {
        return Err(unsafe_storage_error(
            "the local database file must not have additional hard links",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_linked_database(_metadata: &std::fs::Metadata) -> ApiResult<()> {
    Ok(())
}

fn unsafe_storage_error(message: &'static str) -> ApiError {
    api_error(ApiErrorCode::StorageUnavailable, message, false, None)
}

fn storage_io_error(error: std::io::Error, message: &'static str) -> ApiError {
    let retryable = matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
    );
    api_error(ApiErrorCode::StorageUnavailable, message, retryable, None)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> ApiResult<()> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map(|_| ())
        .map_err(|error| storage_io_error(error, "could not create the private database file"))
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> ApiResult<()> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|error| storage_io_error(error, "could not create the private database file"))
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> ApiResult<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
        storage_io_error(error, "could not restrict the application data directory")
    })
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> ApiResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> ApiResult<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| storage_io_error(error, "could not restrict the local database file"))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> ApiResult<()> {
    Ok(())
}

/// Starts the desktop application.
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let service = initialize_service(&app_data_dir)
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })?;
            app.manage(ManagedService::new(service));
            Ok(())
        });

    with_commands(builder)
        .run(tauri::generate_context!())
        .expect("failed to run the OPAP desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::fs;
    use tauri::{WebviewWindowBuilder, ipc::InvokeBody, test, webview::InvokeRequest};
    use tempfile::TempDir;

    fn test_service() -> Service {
        Service::open_in_memory().expect("in-memory service")
    }

    fn create_test_app() -> tauri::App<test::MockRuntime> {
        with_commands(test::mock_builder())
            .manage(ManagedService::new(test_service()))
            .build(test::mock_context(test::noop_assets()))
            .expect("mock application")
    }

    fn invoke_on(
        webview: &tauri::WebviewWindow<test::MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        test::get_ipc_response(
            webview,
            InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().expect("invoke URL"),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: test::INVOKE_KEY.to_owned(),
            },
        )
        .map(|body| body.deserialize::<Value>().expect("JSON response"))
    }

    fn invoke(command: &str, body: Value) -> Result<Value, Value> {
        let app = create_test_app();
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");
        invoke_on(&webview, command, body)
    }

    fn resmed_card(root: &Path) {
        fs::create_dir(root.join("DATALOG")).expect("DATALOG");
        fs::write(root.join("STR.edf"), []).expect("STR");
        fs::write(
            root.join("Identification.tgt"),
            "#SRN 23123456789\n#PNA AirSense_10_AutoSet\n#PCD 37028\n",
        )
        .expect("identity");
    }

    #[test]
    fn about_contract_is_honest_and_attributed() {
        let about = about_info();
        assert_eq!(about.license, "GPL-3.0-only");
        assert_eq!(about.license_url, GPL_V3_URL);
        assert!(about.modified_work_notice.contains("modified software"));
        assert!(about.modified_work_notice.contains("SleepyHead"));
        assert!(
            about
                .attribution
                .iter()
                .any(|line| line.contains("Mark Watkins"))
        );
        assert!(
            about
                .attribution
                .iter()
                .any(|line| line.contains("OSCAR Team"))
        );
        assert_eq!(
            about.build_revision.is_some(),
            about.source_revision_url.is_some()
        );
    }

    #[test]
    fn bootstrap_and_profile_create_work_through_real_ipc() {
        let bootstrap = invoke("app_bootstrap", json!({})).expect("bootstrap response");
        assert_eq!(bootstrap["api_schema_version"], 2);
        assert_eq!(bootstrap["capabilities"]["session_import"], false);

        let profile = invoke(
            "profile_create",
            json!({ "request": { "display_name": "Alex" } }),
        )
        .expect("profile response");
        assert_eq!(profile["display_name"], "Alex");
        assert!(profile.get("id").is_some());
    }

    #[test]
    fn import_envelopes_and_cancellation_work_through_real_ipc() {
        let app = create_test_app();
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");
        let profile = invoke_on(
            &webview,
            "profile_create",
            json!({ "request": { "display_name": "Import test" } }),
        )
        .expect("profile response");
        let profile_id = profile["id"].as_i64().expect("profile id");

        let root = TempDir::new().expect("temporary root");
        let private_path = root.path().join("private-person-card");
        fs::create_dir(&private_path).expect("card root");
        resmed_card(&private_path);
        let inspection = app
            .state::<ManagedService>()
            .inner
            .lock()
            .expect("service lock")
            .inspect_source(&private_path)
            .expect("source inspection");

        let prepared = invoke_on(
            &webview,
            "import_prepare",
            json!({
                "request": {
                    "profile_id": profile_id,
                    "source_id": inspection.source_id
                }
            }),
        )
        .expect("prepare response");
        assert_eq!(prepared["created"], true);
        assert_eq!(prepared["job"]["status"], "blocked");
        assert_eq!(prepared["job"]["phase"], "awaiting_session_importer");
        assert_eq!(
            prepared["job"]["unavailable_reason"],
            "session_parser_not_implemented"
        );
        let serialized = prepared.to_string();
        assert!(!serialized.contains(&private_path.to_string_lossy()[..]));
        assert!(!serialized.contains("23123456789"));

        let jobs = invoke_on(&webview, "import_jobs", json!({ "profileId": profile_id }))
            .expect("job list");
        assert_eq!(jobs.as_array().expect("job array").len(), 1);
        let job_id = prepared["job"]["id"].as_i64().expect("job id");

        let cancelled = invoke_on(
            &webview,
            "import_cancel",
            json!({ "profileId": profile_id, "jobId": job_id }),
        )
        .expect("cancel response");
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["can_cancel"], false);
        assert!(cancelled["finished_at_ms"].is_number());
    }

    #[test]
    fn acl_lists_only_registered_renderer_commands() {
        let capability: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("capability JSON");
        let permissions = capability["permissions"]
            .as_array()
            .expect("permission list")
            .iter()
            .map(|item| item.as_str().expect("permission"))
            .collect::<Vec<_>>();
        assert_eq!(
            permissions,
            [
                "allow-about",
                "allow-app-bootstrap",
                "allow-profile-list",
                "allow-profile-create",
                "allow-source-select",
                "allow-import-prepare",
                "allow-import-jobs",
                "allow-import-cancel",
            ]
        );
        assert!(
            !permissions
                .iter()
                .any(|permission| permission.contains("dialog"))
        );
    }

    #[test]
    fn picker_cancellation_returns_null_without_inspection() {
        let service = test_service();
        let result = inspect_selected_source(&service, None).expect("cancel selection");
        assert!(result.is_none());
        assert_eq!(
            serde_json::to_value(result).expect("serialize cancellation"),
            Value::Null
        );
    }

    #[test]
    fn source_inspection_never_serializes_native_path_or_full_serial() {
        let root = TempDir::new().expect("temporary root");
        let private_path = root.path().join("person-name-private-card");
        fs::create_dir(&private_path).expect("card root");
        resmed_card(&private_path);
        let service = test_service();

        let inspection = inspect_selected_source(&service, Some(private_path.clone()))
            .expect("inspect selection")
            .expect("selected result");
        let json = serde_json::to_string(&inspection).expect("serialize inspection");

        assert!(inspection.source_id.starts_with("opap-source:"));
        assert!(!json.contains(&private_path.to_string_lossy()[..]));
        assert!(!json.contains("23123456789"));
        assert!(json.contains("6789"));
        assert!(!inspection.session_import.available);
    }

    #[test]
    fn private_storage_is_migrated_with_user_only_permissions() {
        let root = TempDir::new().expect("temporary root");
        let app_data = root.path().join("app-data");
        let service = initialize_service(&app_data).expect("service initialization");
        assert!(
            !service
                .bootstrap()
                .expect("bootstrap")
                .capabilities
                .session_import
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let directory_mode = fs::metadata(&app_data)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777;
            let file_mode = fs::metadata(app_data.join(DATABASE_FILE_NAME))
                .expect("database metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(directory_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }
    }

    #[test]
    #[cfg(unix)]
    fn database_symlink_is_rejected_without_touching_target() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().expect("temporary root");
        let app_data = root.path().join("app-data");
        fs::create_dir(&app_data).expect("app data");
        let target = root.path().join("unrelated.txt");
        fs::write(&target, b"do not change").expect("target");
        symlink(&target, app_data.join(DATABASE_FILE_NAME)).expect("database symlink");

        let error = initialize_service(&app_data)
            .err()
            .expect("symlink rejected");
        assert_eq!(error.code, ApiErrorCode::StorageUnavailable);
        assert!(!error.retryable);
        assert_eq!(
            fs::read(&target).expect("target unchanged"),
            b"do not change"
        );
    }

    #[test]
    fn storage_io_retry_classification_is_stable() {
        let transient = storage_io_error(
            std::io::Error::from(std::io::ErrorKind::Interrupted),
            "transient",
        );
        let permanent = storage_io_error(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            "permanent",
        );
        assert!(transient.retryable);
        assert!(!permanent.retryable);
    }
}
