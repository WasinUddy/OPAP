// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

use opap_core::{ImportError, ImportErrorKind};
use opap_storage::Error as StorageError;
use serde::{Deserialize, Serialize};
use std::fmt;

pub type ApiResult<T> = Result<T, ApiError>;

/// Stable error codes exposed by every OPAP host adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    InvalidRequest,
    ProfileNotFound,
    JobNotFound,
    Conflict,
    SourceUnavailable,
    SourcePathInvalid,
    SourceNotSupported,
    SourceDataInvalid,
    SourceSizeLimitExceeded,
    CapabilityUnavailable,
    JobNotCancellable,
    StorageUnavailable,
    Internal,
}

/// Serializable service failure with a stable code and safe context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ApiError {
    pub(crate) fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            field: None,
        }
    }

    pub(crate) fn for_field(
        code: ApiErrorCode,
        message: impl Into<String>,
        field: &'static str,
    ) -> Self {
        Self {
            field: Some(field.to_owned()),
            ..Self::new(code, message)
        }
    }

    pub(crate) fn storage(error: StorageError) -> Self {
        let retryable = error.is_retryable();
        match error {
            StorageError::Io(_) | StorageError::Sqlite(_) => storage_unavailable(retryable),
            StorageError::SchemaTooNew { .. } => Self::new(
                ApiErrorCode::StorageUnavailable,
                "local storage was created by a newer OPAP version",
            ),
            StorageError::UnexpectedApplicationId { .. } => Self::new(
                ApiErrorCode::StorageUnavailable,
                "the selected data file does not belong to OPAP",
            ),
            StorageError::InvalidImportTransition { .. }
            | StorageError::ImportTimestampRegression { .. }
            | StorageError::ImportInterrupted
            | StorageError::StaleImportExecution { .. } => Self::new(
                ApiErrorCode::Conflict,
                "the requested import job update conflicts with its current state",
            ),
            StorageError::InvalidMigrationHistory { .. }
            | StorageError::InvalidMigrationName { .. }
            | StorageError::MigrationVersionMismatch { .. }
            | StorageError::InvalidSchemaFingerprint(_)
            | StorageError::ForeignKeyViolation { .. }
            | StorageError::Integrity(_) => Self::new(
                ApiErrorCode::StorageUnavailable,
                "local storage failed an integrity check",
            ),
        }
    }
}

fn storage_unavailable(retryable: bool) -> ApiError {
    ApiError {
        code: ApiErrorCode::StorageUnavailable,
        message: if retryable {
            "local storage is temporarily unavailable"
        } else {
            "local storage is unavailable"
        }
        .to_owned(),
        retryable,
        field: None,
    }
}

impl From<ImportError> for ApiError {
    fn from(error: ImportError) -> Self {
        let (code, retryable) = match error.kind {
            ImportErrorKind::Source => (ApiErrorCode::SourceUnavailable, true),
            ImportErrorKind::InvalidPath => (ApiErrorCode::SourcePathInvalid, false),
            ImportErrorKind::UnsupportedSource => (ApiErrorCode::SourceNotSupported, false),
            ImportErrorKind::InvalidData => (ApiErrorCode::SourceDataInvalid, false),
            ImportErrorKind::SizeLimitExceeded => (ApiErrorCode::SourceSizeLimitExceeded, false),
            ImportErrorKind::InventoryLimitExceeded => {
                (ApiErrorCode::SourceSizeLimitExceeded, false)
            }
            ImportErrorKind::InvalidConfiguration => (ApiErrorCode::Internal, false),
            ImportErrorKind::UnsupportedOperation => (ApiErrorCode::CapabilityUnavailable, false),
        };

        let message = match error.kind {
            ImportErrorKind::Source => "could not read the selected source",
            ImportErrorKind::InvalidPath => "the source contains an invalid or unsafe path",
            ImportErrorKind::UnsupportedSource => "the selected source is not supported",
            ImportErrorKind::InvalidData => "the source contains invalid device data",
            ImportErrorKind::SizeLimitExceeded => "a source file exceeds the safe read limit",
            ImportErrorKind::InventoryLimitExceeded => {
                "the selected source exceeds safe inspection limits"
            }
            ImportErrorKind::InvalidConfiguration => "the source inspector is misconfigured",
            ImportErrorKind::UnsupportedOperation => {
                "the requested import capability is unavailable"
            }
        };

        Self {
            code,
            message: message.to_owned(),
            retryable,
            field: None,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl fmt::Display for ApiErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_variant_name(*self);
        formatter.write_str(value)
    }
}

fn serde_variant_name(code: ApiErrorCode) -> &'static str {
    match code {
        ApiErrorCode::InvalidRequest => "invalid_request",
        ApiErrorCode::ProfileNotFound => "profile_not_found",
        ApiErrorCode::JobNotFound => "job_not_found",
        ApiErrorCode::Conflict => "conflict",
        ApiErrorCode::SourceUnavailable => "source_unavailable",
        ApiErrorCode::SourcePathInvalid => "source_path_invalid",
        ApiErrorCode::SourceNotSupported => "source_not_supported",
        ApiErrorCode::SourceDataInvalid => "source_data_invalid",
        ApiErrorCode::SourceSizeLimitExceeded => "source_size_limit_exceeded",
        ApiErrorCode::CapabilityUnavailable => "capability_unavailable",
        ApiErrorCode::JobNotCancellable => "job_not_cancellable",
        ApiErrorCode::StorageUnavailable => "storage_unavailable",
        ApiErrorCode::Internal => "internal",
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind as IoErrorKind;

    #[test]
    fn error_code_has_stable_json_and_display_forms() {
        let error = ApiError::for_field(
            ApiErrorCode::InvalidRequest,
            "display name is required",
            "display_name",
        );

        let value = serde_json::to_value(&error).expect("serialize API error");
        assert_eq!(value["code"], "invalid_request");
        assert_eq!(value["field"], "display_name");
        assert_eq!(
            error.to_string(),
            "invalid_request: display name is required"
        );
    }

    #[test]
    fn every_public_error_code_matches_its_documented_wire_value() {
        let cases = [
            (ApiErrorCode::InvalidRequest, "invalid_request"),
            (ApiErrorCode::ProfileNotFound, "profile_not_found"),
            (ApiErrorCode::JobNotFound, "job_not_found"),
            (ApiErrorCode::Conflict, "conflict"),
            (ApiErrorCode::SourceUnavailable, "source_unavailable"),
            (ApiErrorCode::SourcePathInvalid, "source_path_invalid"),
            (ApiErrorCode::SourceNotSupported, "source_not_supported"),
            (ApiErrorCode::SourceDataInvalid, "source_data_invalid"),
            (
                ApiErrorCode::SourceSizeLimitExceeded,
                "source_size_limit_exceeded",
            ),
            (
                ApiErrorCode::CapabilityUnavailable,
                "capability_unavailable",
            ),
            (ApiErrorCode::JobNotCancellable, "job_not_cancellable"),
            (ApiErrorCode::StorageUnavailable, "storage_unavailable"),
            (ApiErrorCode::Internal, "internal"),
        ];

        for (code, expected) in cases {
            assert_eq!(code.to_string(), expected);
            assert_eq!(
                serde_json::to_value(code).expect("serialize code"),
                expected
            );
        }
    }

    #[test]
    fn service_retryability_matches_the_storage_error_contract() {
        for kind in [
            IoErrorKind::Interrupted,
            IoErrorKind::WouldBlock,
            IoErrorKind::TimedOut,
            IoErrorKind::PermissionDenied,
        ] {
            let storage_error =
                StorageError::Io(std::io::Error::new(kind, "private database path"));
            let expected_retryable = storage_error.is_retryable();
            let api = ApiError::storage(storage_error);
            assert_eq!(api.code, ApiErrorCode::StorageUnavailable);
            assert_eq!(api.retryable, expected_retryable);
            assert!(api.retryable, "I/O kind {kind:?}");
            assert!(!api.message.contains("private"));
        }

        for result_code in [
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_LOCKED,
            rusqlite::ffi::SQLITE_CANTOPEN,
            rusqlite::ffi::SQLITE_CANTOPEN_SYMLINK,
            rusqlite::ffi::SQLITE_IOERR,
            rusqlite::ffi::SQLITE_IOERR_AUTH,
            rusqlite::ffi::SQLITE_IOERR_DATA,
            rusqlite::ffi::SQLITE_IOERR_CORRUPTFS,
        ] {
            let sqlite = rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(result_code),
                Some("private SQLite details".to_owned()),
            );
            let storage_error = StorageError::Sqlite(sqlite);
            let expected_retryable = storage_error.is_retryable();
            let api = ApiError::storage(storage_error);
            assert_eq!(api.code, ApiErrorCode::StorageUnavailable);
            assert_eq!(api.retryable, expected_retryable);
            assert!(api.retryable, "SQLite result code {result_code}");
            assert!(!api.message.contains("private"));
        }
    }

    #[test]
    fn non_retryable_sqlite_schema_and_integrity_failures_are_sanitized() {
        let sqlite_permission = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_PERM),
            Some("private SQLite details".to_owned()),
        );
        assert!(!ApiError::storage(StorageError::Sqlite(sqlite_permission)).retryable);

        for result_code in [
            rusqlite::ffi::SQLITE_INTERRUPT,
            rusqlite::ffi::SQLITE_PROTOCOL,
        ] {
            let sqlite = rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(result_code),
                Some("private SQLite details".to_owned()),
            );
            let api = ApiError::storage(StorageError::Sqlite(sqlite));
            assert_eq!(api.code, ApiErrorCode::StorageUnavailable);
            assert!(!api.retryable, "SQLite result code {result_code}");
            assert!(!api.message.contains("private"));
        }

        let schema = ApiError::storage(StorageError::SchemaTooNew {
            found: 99,
            supported: 6,
        });
        assert_eq!(schema.code, ApiErrorCode::StorageUnavailable);
        assert!(!schema.retryable);
        assert!(schema.message.contains("newer OPAP version"));

        let integrity = ApiError::storage(StorageError::Integrity(
            "corrupt row at /Users/private".to_owned(),
        ));
        assert_eq!(integrity.code, ApiErrorCode::StorageUnavailable);
        assert!(!integrity.retryable);
        assert!(!integrity.message.contains("/Users"));
    }

    #[test]
    fn interrupted_and_stale_import_executions_are_safe_conflicts() {
        for error in [
            StorageError::ImportInterrupted,
            StorageError::StaleImportExecution { id: 42 },
        ] {
            let api = ApiError::storage(error);
            assert_eq!(api.code, ApiErrorCode::Conflict);
            assert!(!api.retryable);
            assert!(!api.message.contains("42"));
            assert!(!api.message.contains("opap-execution:"));
        }
    }

    #[test]
    fn invalid_job_transitions_are_permanent_conflicts() {
        let error = ApiError::storage(StorageError::InvalidImportTransition {
            id: 42,
            from: "completed".to_owned(),
            operation: "cancel",
        });

        assert_eq!(error.code, ApiErrorCode::Conflict);
        assert!(!error.retryable);
        assert!(!error.message.contains("42"));
    }
}
