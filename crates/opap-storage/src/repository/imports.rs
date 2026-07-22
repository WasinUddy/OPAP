use crate::{BeginImport, ImportCounts, ImportHistory, ImportStatus, NewImport, Result};
use rusqlite::{Connection, OptionalExtension, params, types::Type};
use std::io;

const COLUMNS: &str = "id, profile_id, machine_id, import_key, source_uri, loader_name, status, \
    started_at_ms, completed_at_ms, sessions_created, sessions_updated, events_written, \
    waveform_chunks_written, error_message";

#[derive(Clone, Copy)]
pub struct Imports<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Imports<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Creates an in-progress record once per `(profile_id, import_key)`.
    /// Repeating the same logical import returns the original row with
    /// `inserted == false`, allowing callers to skip already completed work.
    pub fn begin_or_get(&self, input: &NewImport<'_>) -> Result<BeginImport> {
        let sql = format!(
            "INSERT INTO import_history (
                 profile_id, machine_id, import_key, source_uri, loader_name,
                 status, started_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(profile_id, import_key) DO NOTHING
             RETURNING {COLUMNS}"
        );
        let inserted = self
            .connection
            .query_row(
                &sql,
                params![
                    input.profile_id,
                    input.machine_id,
                    input.import_key,
                    input.source_uri,
                    input.loader_name,
                    ImportStatus::InProgress.as_str(),
                    input.started_at_ms,
                ],
                map_import,
            )
            .optional()?;

        match inserted {
            Some(history) => Ok(BeginImport {
                history,
                inserted: true,
            }),
            None => Ok(BeginImport {
                history: self
                    .find_by_key(input.profile_id, input.import_key)?
                    .expect("unique conflicting import must exist"),
                inserted: false,
            }),
        }
    }

    pub fn get(&self, id: i64) -> Result<Option<ImportHistory>> {
        let sql = format!("SELECT {COLUMNS} FROM import_history WHERE id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_import)
            .optional()?)
    }

    pub fn find_by_key(&self, profile_id: i64, import_key: &str) -> Result<Option<ImportHistory>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM import_history WHERE profile_id = ?1 AND import_key = ?2"
        );
        Ok(self
            .connection
            .query_row(&sql, params![profile_id, import_key], map_import)
            .optional()?)
    }

    pub fn list_by_profile(&self, profile_id: i64) -> Result<Vec<ImportHistory>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM import_history
             WHERE profile_id = ?1 ORDER BY started_at_ms DESC, id DESC"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([profile_id], map_import)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn complete(
        &self,
        id: i64,
        completed_at_ms: i64,
        counts: ImportCounts,
    ) -> Result<Option<ImportHistory>> {
        let sql = format!(
            "UPDATE import_history SET
                 status = ?2,
                 completed_at_ms = ?3,
                 sessions_created = ?4,
                 sessions_updated = ?5,
                 events_written = ?6,
                 waveform_chunks_written = ?7,
                 error_message = NULL
             WHERE id = ?1 AND status = 'in_progress' RETURNING {COLUMNS}"
        );
        Ok(self
            .connection
            .query_row(
                &sql,
                params![
                    id,
                    ImportStatus::Completed.as_str(),
                    completed_at_ms,
                    counts.sessions_created,
                    counts.sessions_updated,
                    counts.events_written,
                    counts.waveform_chunks_written,
                ],
                map_import,
            )
            .optional()?)
    }

    pub fn fail(
        &self,
        id: i64,
        completed_at_ms: i64,
        message: &str,
    ) -> Result<Option<ImportHistory>> {
        let sql = format!(
            "UPDATE import_history SET status = ?2, completed_at_ms = ?3, error_message = ?4
             WHERE id = ?1 AND status = 'in_progress' RETURNING {COLUMNS}"
        );
        Ok(self
            .connection
            .query_row(
                &sql,
                params![id, ImportStatus::Failed.as_str(), completed_at_ms, message],
                map_import,
            )
            .optional()?)
    }
}

fn map_import(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImportHistory> {
    let raw_status: String = row.get(6)?;
    let status = ImportStatus::from_str(&raw_status).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            6,
            Type::Text,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown import status {raw_status:?}"),
            )),
        )
    })?;
    Ok(ImportHistory {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        machine_id: row.get(2)?,
        import_key: row.get(3)?,
        source_uri: row.get(4)?,
        loader_name: row.get(5)?,
        status,
        started_at_ms: row.get(7)?,
        completed_at_ms: row.get(8)?,
        sessions_created: row.get(9)?,
        sessions_updated: row.get(10)?,
        events_written: row.get(11)?,
        waveform_chunks_written: row.get(12)?,
        error_message: row.get(13)?,
    })
}
