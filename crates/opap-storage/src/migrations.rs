use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub const APPLICATION_ID: i32 = i32::from_be_bytes(*b"OPAP");
pub const LATEST_SCHEMA_VERSION: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRecord {
    pub version: i64,
    pub name: String,
    pub applied_at_ms: i64,
}

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_storage",
        sql: include_str!("../migrations/0001_initial_storage.sql"),
    },
    Migration {
        version: 2,
        name: "query_indexes",
        sql: include_str!("../migrations/0002_query_indexes.sql"),
    },
    Migration {
        version: 3,
        name: "storage_integrity",
        sql: include_str!("../migrations/0003_storage_integrity.sql"),
    },
];

pub(crate) fn migrate(connection: &mut Connection) -> Result<()> {
    validate_application_id(connection)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version       INTEGER PRIMARY KEY,
             name          TEXT NOT NULL,
             applied_at_ms INTEGER NOT NULL
         ) STRICT;",
    )?;

    let applied = applied_migrations(connection)?;
    for (index, record) in applied.iter().enumerate() {
        let expected = index as i64 + 1;
        if record.version != expected {
            return Err(Error::InvalidMigrationHistory {
                expected,
                found: record.version,
            });
        }
        let Some(expected_migration) = MIGRATIONS.get(index) else {
            return Err(Error::SchemaTooNew {
                found: record.version,
                supported: LATEST_SCHEMA_VERSION,
            });
        };
        if record.name != expected_migration.name {
            return Err(Error::InvalidMigrationName {
                version: record.version,
                expected: expected_migration.name.to_owned(),
                found: record.name.clone(),
            });
        }
    }

    let current = applied.last().map_or(0, |record| record.version);
    if current > LATEST_SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            found: current,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    for migration in MIGRATIONS.iter().filter(|item| item.version > current) {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name, applied_at_ms)
             VALUES (?1, ?2, unixepoch() * 1000)",
            params![migration.version, migration.name],
        )?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        transaction.commit()?;
    }

    connection.pragma_update(None, "application_id", APPLICATION_ID)?;

    Ok(())
}

pub(crate) fn validate_application_id(connection: &Connection) -> Result<()> {
    let found: i64 = connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let expected = i64::from(APPLICATION_ID);
    if found != 0 && found != expected {
        return Err(Error::UnexpectedApplicationId { expected, found });
    }
    Ok(())
}

pub(crate) fn schema_version(connection: &Connection) -> Result<i64> {
    let version = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0);
    Ok(version)
}

pub(crate) fn applied_migrations(connection: &Connection) -> Result<Vec<MigrationRecord>> {
    let table_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !table_exists {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare("SELECT version, name, applied_at_ms FROM schema_migrations ORDER BY version")?;
    let rows = statement.query_map([], |row| {
        Ok(MigrationRecord {
            version: row.get(0)?,
            name: row.get(1)?,
            applied_at_ms: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
