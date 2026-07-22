/// Errors produced while opening, migrating, or accessing OPAP storage.
#[derive(Debug, thiserror::Error)]
pub enum Error {
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

    #[error("database application id {found} does not belong to OPAP (expected {expected})")]
    UnexpectedApplicationId { expected: i64, found: i64 },

    #[error("storage integrity violation: {0}")]
    Integrity(String),
}

pub type Result<T> = std::result::Result<T, Error>;
