use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub const APPLICATION_ID: i32 = i32::from_be_bytes(*b"OPAP");
pub const LATEST_SCHEMA_VERSION: i64 = 8;

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
    Migration {
        version: 4,
        name: "import_job_states",
        sql: include_str!("../migrations/0004_import_job_states.sql"),
    },
    Migration {
        version: 5,
        name: "opaque_import_sources",
        sql: include_str!("../migrations/0005_opaque_import_sources.sql"),
    },
    Migration {
        version: 6,
        name: "import_history_link_integrity",
        sql: include_str!("../migrations/0006_import_history_link_integrity.sql"),
    },
    Migration {
        version: 7,
        name: "opaque_import_keys",
        sql: include_str!("../migrations/0007_opaque_import_keys.sql"),
    },
    Migration {
        version: 8,
        name: "session_snapshots",
        sql: include_str!("../migrations/0008_session_snapshots.sql"),
    },
];

pub(crate) fn migrate(connection: &mut Connection) -> Result<()> {
    validate_application_id(connection)?;
    let applied = applied_migrations(connection)?;
    let current = applied.last().map_or(0, |record| record.version);
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

    if current > LATEST_SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            found: current,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if user_version != current {
        return Err(Error::MigrationVersionMismatch {
            user_version,
            history_version: current,
        });
    }

    validate_foreign_keys(connection)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version       INTEGER PRIMARY KEY,
             name          TEXT NOT NULL,
             applied_at_ms INTEGER NOT NULL
         ) STRICT;",
    )?;

    for migration in MIGRATIONS.iter().filter(|item| item.version > current) {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name, applied_at_ms)
             VALUES (?1, ?2, unixepoch() * 1000)",
            params![migration.version, migration.name],
        )?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        validate_foreign_keys(&transaction)?;
        transaction.commit()?;
    }

    validate_schema_fingerprint(connection)?;
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    validate_foreign_keys(connection)?;
    truncate_wal(connection)?;

    Ok(())
}

fn truncate_wal(connection: &Connection) -> Result<()> {
    let busy: i64 =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    if busy != 0 {
        return Err(Error::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database WAL is busy during privacy checkpoint".to_owned()),
        )));
    }
    Ok(())
}

fn validate_schema_fingerprint(connection: &Connection) -> Result<()> {
    const TABLES: &[(&str, &str)] = &[
        (
            "schema_migrations",
            "SELECT version, name, applied_at_ms FROM schema_migrations LIMIT 0",
        ),
        (
            "profiles",
            "SELECT id, display_name, created_at_ms, updated_at_ms FROM profiles LIMIT 0",
        ),
        (
            "machines",
            "SELECT id, profile_id, source_key, device_type, manufacturer, model, \
             model_number, serial_number, first_seen_at_ms, last_seen_at_ms \
             FROM machines LIMIT 0",
        ),
        (
            "sessions",
            "SELECT id, machine_id, source_key, started_at_ms, ended_at_ms, \
             timezone_offset_minutes, created_at_ms, updated_at_ms FROM sessions LIMIT 0",
        ),
        (
            "events",
            "SELECT id, session_id, source_key, channel_key, event_type, starts_at_ms, \
             duration_ms, value, unit, created_at_ms FROM events LIMIT 0",
        ),
        (
            "waveforms",
            "SELECT id, session_id, source_key, channel_key, unit, started_at_ms, \
             sample_interval_us, sample_count, encoding, min_value, max_value, created_at_ms \
             FROM waveforms LIMIT 0",
        ),
        (
            "waveform_chunks",
            "SELECT waveform_id, chunk_index, start_sample, sample_count, payload, \
             min_value, max_value FROM waveform_chunks LIMIT 0",
        ),
        (
            "import_history",
            "SELECT id, profile_id, machine_id, import_key, source_uri, loader_name, attempt, \
             retry_of_id, status, state_message, created_at_ms, updated_at_ms, started_at_ms, \
             completed_at_ms, sessions_created, sessions_updated, events_written, \
             waveform_chunks_written, error_message FROM import_history LIMIT 0",
        ),
        (
            "session_provenance",
            "SELECT session_id, therapy_day, start_local_wall, end_local_wall, \
             start_utc_offset_seconds, end_utc_offset_seconds, start_clock_correction_ms, \
             end_clock_correction_ms, data_kind, importer_name, importer_schema, id_algorithm, \
             source_digest, content_digest FROM session_provenance LIMIT 0",
        ),
        (
            "session_slices",
            "SELECT session_id, sequence, source_key, state, started_at_ms, ended_at_ms \
             FROM session_slices LIMIT 0",
        ),
        (
            "session_summary",
            "SELECT session_id, usage_ms FROM session_summary LIMIT 0",
        ),
        (
            "summary_metrics",
            "SELECT session_id, metric_key, value, unit FROM summary_metrics LIMIT 0",
        ),
        (
            "session_settings",
            "SELECT session_id, setting_key, value_kind, integer_value, real_value, \
             text_value, boolean_value, unit, origin FROM session_settings LIMIT 0",
        ),
    ];
    const INDEXES: &[(&str, &str)] = &[
        (
            "sessions_by_start",
            "onsessions(machine_id,started_at_msdesc)",
        ),
        (
            "events_by_time",
            "onevents(session_id,starts_at_ms,channel_key)",
        ),
        (
            "waveforms_by_channel",
            "onwaveforms(session_id,channel_key,started_at_ms)",
        ),
        (
            "imports_by_start",
            "onimport_history(profile_id,created_at_msdesc,attemptdesc)",
        ),
        (
            "imports_by_logical_key",
            "onimport_history(profile_id,import_key,attemptdesc)",
        ),
        (
            "session_slices_by_time",
            "onsession_slices(session_id,started_at_ms,sequence)",
        ),
    ];
    const TRIGGERS: &[&str] = &[
        "waveforms_validate_encoding_insert",
        "waveforms_validate_encoding_update",
        "waveforms_protect_chunk_layout",
        "waveform_chunks_validate_insert",
        "waveform_chunks_validate_update",
        "import_history_validate_machine_insert",
        "import_history_validate_machine_update",
        "import_history_validate_source_insert",
        "import_history_validate_source_update",
        "import_history_validate_request_key_insert",
        "import_history_validate_request_key_update",
        "import_history_protect_import_identity",
        "import_history_validate_state_transition",
        "import_history_monotonic_update_time",
        "import_history_protect_terminal_state",
        "import_history_validate_retry_time_insert",
        "import_history_protect_terminal_links",
    ];

    for (table, probe) in TABLES {
        require_schema_object(connection, "table", table)?;
        let strict = connection
            .query_row(
                "SELECT strict FROM pragma_table_list
                 WHERE schema = 'main' AND name = ?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if strict != Some(1) || connection.prepare(probe).is_err() {
            return Err(Error::InvalidSchemaFingerprint(format!(
                "strict table {table:?} does not match its required columns"
            )));
        }
    }

    for (index, required_fragment) in INDEXES {
        let sql = require_schema_object(connection, "index", index)?;
        if !compact_schema_sql(&sql).contains(required_fragment) {
            return Err(Error::InvalidSchemaFingerprint(format!(
                "index {index:?} does not match its required columns"
            )));
        }
    }

    for trigger in TRIGGERS {
        let sql = require_schema_object(connection, "trigger", trigger)?;
        if !compact_schema_sql(&sql).contains("raise(abort") {
            return Err(Error::InvalidSchemaFingerprint(format!(
                "trigger {trigger:?} is not an enforcing OPAP guard"
            )));
        }
    }

    let import_sql = compact_schema_sql(&require_schema_object(
        connection,
        "table",
        "import_history",
    )?);
    for required in [
        "length(cast(import_keyasblob))",
        "opap-request:legacy-",
        "unique(profile_id,import_key,attempt)",
        "retry_of_idintegeruniquereferencesimport_history(id)ondeletesetnull",
    ] {
        if !import_sql.contains(required) {
            return Err(Error::InvalidSchemaFingerprint(
                "import_history privacy or lineage constraints are missing".to_owned(),
            ));
        }
    }

    let identity_sql = compact_schema_sql(&require_schema_object(
        connection,
        "trigger",
        "import_history_protect_import_identity",
    )?);
    if !identity_sql.contains("beforeupdateofid,profile_id,import_key,attempt,retry_of_id") {
        return Err(Error::InvalidSchemaFingerprint(
            "import history identity guard is incomplete".to_owned(),
        ));
    }

    for (table, expected_count) in [
        ("profiles", 0_i64),
        ("machines", 1),
        ("sessions", 1),
        ("events", 1),
        ("waveforms", 1),
        ("waveform_chunks", 1),
        ("import_history", 3),
        ("session_provenance", 1),
        ("session_slices", 1),
        ("session_summary", 1),
        ("summary_metrics", 1),
        ("session_settings", 1),
    ] {
        let sql = format!("SELECT COUNT(*) FROM pragma_foreign_key_list('{table}')");
        let count: i64 = connection.query_row(&sql, [], |row| row.get(0))?;
        if count != expected_count {
            return Err(Error::InvalidSchemaFingerprint(format!(
                "table {table:?} has {count} foreign keys; expected {expected_count}"
            )));
        }
    }

    let stale_table = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema
             WHERE type = 'table' AND name = 'import_history_v6'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if stale_table {
        return Err(Error::InvalidSchemaFingerprint(
            "stale import_history_v6 table remains".to_owned(),
        ));
    }

    let provenance_sql = compact_schema_sql(&require_schema_object(
        connection,
        "table",
        "session_provenance",
    )?);
    for required in [
        "source_digesttextnotnullcheck(length(cast(source_digestasblob))=64",
        "content_digesttextnotnullcheck(length(cast(content_digestasblob))=64",
        "data_kindin('detailed','summary_only','partial')",
    ] {
        if !provenance_sql.contains(required) {
            return Err(Error::InvalidSchemaFingerprint(
                "session provenance constraints are incomplete".to_owned(),
            ));
        }
    }

    let settings_sql = compact_schema_sql(&require_schema_object(
        connection,
        "table",
        "session_settings",
    )?);
    for required in [
        "value_kindin('integer','real','text','boolean')",
        "value_kind='integer'andinteger_valueisnotnull",
        "value_kind='real'andinteger_valueisnullandreal_valueisnotnull",
        "value_kind='text'andinteger_valueisnullandreal_valueisnullandtext_valueisnotnull",
        "value_kind='boolean'andinteger_valueisnullandreal_valueisnullandtext_valueisnullandboolean_valueisnotnull",
    ] {
        if !settings_sql.contains(required) {
            return Err(Error::InvalidSchemaFingerprint(
                "session setting typed-value constraints are incomplete".to_owned(),
            ));
        }
    }
    Ok(())
}

fn require_schema_object(connection: &Connection, kind: &str, name: &str) -> Result<String> {
    connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            params![kind, name],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            Error::InvalidSchemaFingerprint(format!("required {kind} {name:?} is missing"))
        })
}

fn compact_schema_sql(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn validate_foreign_keys(connection: &Connection) -> Result<()> {
    let violation = connection
        .query_row(
            "SELECT \"table\", rowid, parent, fkid
             FROM pragma_foreign_key_check
             ORDER BY \"table\", rowid, parent, fkid
             LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some((table, row_id, parent, foreign_key_index)) = violation {
        return Err(Error::ForeignKeyViolation {
            table,
            row_id,
            parent,
            foreign_key_index,
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_key_validation_sees_uncommitted_deferred_violations() -> Result<()> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE parents (id INTEGER PRIMARY KEY);
             CREATE TABLE children (
                 id INTEGER PRIMARY KEY,
                 parent_id INTEGER NOT NULL,
                 FOREIGN KEY (parent_id) REFERENCES parents(id)
                     DEFERRABLE INITIALLY DEFERRED
             );",
        )?;

        {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute("INSERT INTO children VALUES (1, 999)", [])?;
            let error = validate_foreign_keys(&transaction)
                .expect_err("uncommitted foreign-key violation must be visible");
            assert!(matches!(
                error,
                Error::ForeignKeyViolation {
                    table,
                    row_id: Some(1),
                    parent,
                    foreign_key_index: 0,
                } if table == "children" && parent == "parents"
            ));
        }

        let remaining: i64 =
            connection.query_row("SELECT COUNT(*) FROM children", [], |row| row.get(0))?;
        assert_eq!(remaining, 0, "dropping the failed migration rolls it back");
        Ok(())
    }
}
