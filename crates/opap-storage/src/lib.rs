//! Local SQLite persistence for OPAP.
//!
//! The public DTOs intentionally use only primitive Rust types. Device-specific
//! parsers can map into these records without creating a dependency from this
//! crate back to an importer or analysis engine.

mod error;
mod import_commit;
mod migrations;
mod model;
mod replacement;
pub mod repository;

use std::path::{Path, PathBuf};

pub use error::{Error, ErrorCategory, Result};
pub use migrations::{APPLICATION_ID, LATEST_SCHEMA_VERSION, MigrationRecord};
pub use model::*;
use repository::{Events, Imports, Machines, Profiles, SessionSnapshots, Sessions, Waveforms};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

/// An initialized OPAP SQLite database.
pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens or creates a database, configures it for local use, and applies all
    /// available schema migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = canonicalize_database_parent(path.as_ref())?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        Self::from_connection(connection)
    }

    /// Creates a migrated in-memory database, primarily for callers' tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Takes ownership of an existing connection, configures it, and migrates it.
    pub fn from_connection(mut connection: Connection) -> Result<Self> {
        migrations::validate_application_id(&connection)?;
        configure(&connection)?;
        migrations::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64> {
        migrations::schema_version(&self.connection)
    }

    pub fn applied_migrations(&self) -> Result<Vec<MigrationRecord>> {
        migrations::applied_migrations(&self.connection)
    }

    /// Starts an immediate transaction. Construct repositories with `&transaction`
    /// and commit only after a complete import has succeeded; dropping it rolls back.
    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn profiles(&self) -> Profiles<'_> {
        Profiles::new(&self.connection)
    }

    pub fn machines(&self) -> Machines<'_> {
        Machines::new(&self.connection)
    }

    pub fn sessions(&self) -> Sessions<'_> {
        Sessions::new(&self.connection)
    }

    pub fn session_snapshots(&self) -> SessionSnapshots<'_> {
        SessionSnapshots::new(&self.connection)
    }

    pub fn events(&self) -> Events<'_> {
        Events::new(&self.connection)
    }

    pub fn waveforms(&self) -> Waveforms<'_> {
        Waveforms::new(&self.connection)
    }

    pub fn imports(&self) -> Imports<'_> {
        Imports::new(&self.connection)
    }
}

fn canonicalize_database_parent(path: &Path) -> Result<PathBuf> {
    let file_name = path.file_name().ok_or_else(|| {
        Error::Integrity("database path must include a final file name".to_owned())
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.canonicalize()?.join(file_name))
}

fn configure(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA secure_delete = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}
