/// Stable, path- and message-free classification suitable for service/UI APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCategory {
    /// A bounded retry may succeed after an I/O or database availability issue.
    Transient,
    /// A non-transient SQLite/query failure.
    Database,
    /// The database does not have trusted, supported OPAP schema metadata.
    Schema,
    /// Persisted or requested data violates an OPAP storage invariant.
    Integrity,
    /// An import command violates its state machine or chronology.
    ImportState,
}

impl ErrorCategory {
    /// Stable identifier for serialization without exposing the underlying error.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transient => "transient",
            Self::Database => "database",
            Self::Schema => "schema",
            Self::Integrity => "integrity",
            Self::ImportState => "import_state",
        }
    }
}

/// Errors produced while opening, migrating, or accessing OPAP storage.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error("database schema version {found} is newer than supported version {supported}")]
    SchemaTooNew { found: i64, supported: i64 },

    #[error("invalid migration history: expected version {expected}, found {found}")]
    InvalidMigrationHistory { expected: i64, found: i64 },

    #[error("migration {version} name mismatch: expected {expected:?}, found {found:?}")]
    InvalidMigrationName {
        version: i64,
        expected: String,
        found: String,
    },

    #[error(
        "database schema version metadata disagrees: PRAGMA user_version is {user_version}, migration history is {history_version}"
    )]
    MigrationVersionMismatch {
        user_version: i64,
        history_version: i64,
    },

    #[error(
        "foreign key violation in table {table:?}, row {row_id:?}, referencing {parent:?} (constraint {foreign_key_index})"
    )]
    ForeignKeyViolation {
        table: String,
        row_id: Option<i64>,
        parent: String,
        foreign_key_index: i64,
    },

    #[error("database application id {found} does not belong to OPAP (expected {expected})")]
    UnexpectedApplicationId { expected: i64, found: i64 },

    #[error("storage integrity violation: {0}")]
    Integrity(String),

    #[error("import job {id} cannot {operation} from state {from}")]
    InvalidImportTransition {
        id: i64,
        from: String,
        operation: &'static str,
    },

    #[error(
        "import job {id} timestamp cannot move backwards from {previous_at_ms} to {attempted_at_ms}"
    )]
    ImportTimestampRegression {
        id: i64,
        previous_at_ms: i64,
        attempted_at_ms: i64,
    },
}

impl Error {
    /// Returns a stable classification that does not expose SQLite details,
    /// filesystem paths, source identifiers, or user data.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Io(_) => ErrorCategory::Transient,
            Self::Sqlite(error) if is_retryable_sqlite(error) => ErrorCategory::Transient,
            Self::Sqlite(_) => ErrorCategory::Database,
            Self::SchemaTooNew { .. }
            | Self::InvalidMigrationHistory { .. }
            | Self::InvalidMigrationName { .. }
            | Self::MigrationVersionMismatch { .. }
            | Self::UnexpectedApplicationId { .. } => ErrorCategory::Schema,
            Self::ForeignKeyViolation { .. } | Self::Integrity(_) => ErrorCategory::Integrity,
            Self::InvalidImportTransition { .. } | Self::ImportTimestampRegression { .. } => {
                ErrorCategory::ImportState
            }
        }
    }

    /// True only for I/O failures and SQLite busy, locked, cannot-open, or
    /// system-I/O results. Callers should still use bounded retries/backoff.
    pub fn is_retryable(&self) -> bool {
        self.category() == ErrorCategory::Transient
    }
}

fn is_retryable_sqlite(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(
            rusqlite::ErrorCode::DatabaseBusy
                | rusqlite::ErrorCode::DatabaseLocked
                | rusqlite::ErrorCode::CannotOpen
                | rusqlite::ErrorCode::SystemIoFailure
        )
    )
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn sqlite_error(result_code: i32) -> Error {
        Error::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(result_code),
            None,
        ))
    }

    #[test]
    fn classifies_only_selected_availability_errors_as_retryable() {
        for result_code in [
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_BUSY_RECOVERY,
            rusqlite::ffi::SQLITE_LOCKED,
            rusqlite::ffi::SQLITE_LOCKED_SHAREDCACHE,
            rusqlite::ffi::SQLITE_CANTOPEN,
            rusqlite::ffi::SQLITE_CANTOPEN_SYMLINK,
            rusqlite::ffi::SQLITE_IOERR,
            rusqlite::ffi::SQLITE_IOERR_READ,
        ] {
            let error = sqlite_error(result_code);
            assert_eq!(error.category(), ErrorCategory::Transient);
            assert!(error.is_retryable());
        }

        let io_error = Error::Io(io::Error::new(io::ErrorKind::TimedOut, "test timeout"));
        assert_eq!(io_error.category(), ErrorCategory::Transient);
        assert!(io_error.is_retryable());
        let permission_error = Error::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "test permission failure",
        ));
        assert!(permission_error.is_retryable());

        for result_code in [
            rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY,
            rusqlite::ffi::SQLITE_CORRUPT,
            rusqlite::ffi::SQLITE_READONLY,
        ] {
            let error = sqlite_error(result_code);
            assert_eq!(error.category(), ErrorCategory::Database);
            assert!(!error.is_retryable());
        }

        let query_error = Error::Sqlite(rusqlite::Error::QueryReturnedNoRows);
        assert_eq!(query_error.category(), ErrorCategory::Database);
        assert!(!query_error.is_retryable());
    }

    #[test]
    fn categories_are_safe_and_domain_specific() {
        let schema = Error::SchemaTooNew {
            found: 7,
            supported: 6,
        };
        assert_eq!(schema.category(), ErrorCategory::Schema);
        assert_eq!(schema.category().as_str(), "schema");
        assert!(!schema.is_retryable());

        let integrity = Error::Integrity("private diagnostic".to_owned());
        assert_eq!(integrity.category(), ErrorCategory::Integrity);
        assert_eq!(integrity.category().as_str(), "integrity");

        let import_state = Error::ImportTimestampRegression {
            id: 1,
            previous_at_ms: 2,
            attempted_at_ms: 1,
        };
        assert_eq!(import_state.category(), ErrorCategory::ImportState);
        assert_eq!(import_state.category().as_str(), "import_state");
    }
}
