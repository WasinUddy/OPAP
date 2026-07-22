use crate::{
    BeginImport, Error, ImportCounts, ImportHistory, ImportStatus, ImportTransition, NewImport,
    Result, RetryImport, is_persistable_source_id,
};
use rusqlite::{Connection, OptionalExtension, params, types::Type};
use std::io;

const COLUMNS: &str = "id, profile_id, machine_id, import_key, source_uri, loader_name, \
    attempt, retry_of_id, status, state_message, created_at_ms, updated_at_ms, started_at_ms, \
    completed_at_ms, sessions_created, sessions_updated, events_written, \
    waveform_chunks_written, error_message";

#[derive(Clone, Copy)]
pub struct Imports<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Imports<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Creates the first attempt for a logical import. The initial status is
    /// explicitly blocked or running, so preparation never masquerades as work.
    /// Repeating the same logical import returns its latest attempt.
    pub fn begin_or_get(&self, input: &NewImport<'_>) -> Result<BeginImport> {
        if !is_persistable_source_id(input.source_uri) {
            return Err(Error::Integrity(
                "import source must be an opaque OPAP source identifier".to_owned(),
            ));
        }
        let status = input.initial_status.status();
        let started_at_ms = (status == ImportStatus::Running).then_some(input.created_at_ms);
        let sql = format!(
            "INSERT INTO import_history (
                 profile_id, machine_id, import_key, source_uri, loader_name,
                 attempt, retry_of_id, status, state_message, created_at_ms,
                 updated_at_ms, started_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, 1, NULL, ?6, ?7, ?8, ?8, ?9)
             ON CONFLICT(profile_id, import_key, attempt) DO NOTHING
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
                    status.as_str(),
                    input.state_message,
                    input.created_at_ms,
                    started_at_ms,
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
                    .ok_or_else(|| {
                        Error::Integrity(
                            "conflicting logical import disappeared while being read".to_owned(),
                        )
                    })?,
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

    /// Returns the newest attempt for a logical import key.
    pub fn find_by_key(&self, profile_id: i64, import_key: &str) -> Result<Option<ImportHistory>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM import_history
             WHERE profile_id = ?1 AND import_key = ?2
             ORDER BY attempt DESC LIMIT 1"
        );
        Ok(self
            .connection
            .query_row(&sql, params![profile_id, import_key], map_import)
            .optional()?)
    }

    pub fn list_by_profile(&self, profile_id: i64) -> Result<Vec<ImportHistory>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM import_history
             WHERE profile_id = ?1 ORDER BY created_at_ms DESC, attempt DESC, id DESC"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([profile_id], map_import)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Applies a typed state-machine command. A missing id returns `None`; a row
    /// in an illegal source state returns `InvalidImportTransition`.
    pub fn transition(
        &self,
        id: i64,
        transition: ImportTransition<'_>,
    ) -> Result<Option<ImportHistory>> {
        let (updated, operation, attempted_at_ms) = match transition {
            ImportTransition::Block { at_ms, reason } => (
                self.update_returning(
                    id,
                    "block",
                    &format!(
                        "UPDATE import_history SET
                             status = 'blocked', state_message = ?2, updated_at_ms = ?3
                         WHERE id = ?1 AND status = 'running' AND ?3 >= updated_at_ms
                         RETURNING {COLUMNS}"
                    ),
                    params![id, reason, at_ms],
                )?,
                "block",
                at_ms,
            ),
            ImportTransition::Start { at_ms } => (
                self.update_returning(
                    id,
                    "start",
                    &format!(
                        "UPDATE import_history SET
                             status = 'running', state_message = NULL,
                             updated_at_ms = ?2,
                             started_at_ms = COALESCE(started_at_ms, ?2)
                         WHERE id = ?1 AND status = 'blocked' AND ?2 >= updated_at_ms
                         RETURNING {COLUMNS}"
                    ),
                    params![id, at_ms],
                )?,
                "start",
                at_ms,
            ),
            ImportTransition::Complete { at_ms, counts } => (
                self.update_returning(
                    id,
                    "complete",
                    &format!(
                        "UPDATE import_history SET
                             status = 'completed', state_message = NULL,
                             updated_at_ms = ?2, completed_at_ms = ?2,
                             sessions_created = ?3, sessions_updated = ?4,
                             events_written = ?5, waveform_chunks_written = ?6,
                             error_message = NULL
                         WHERE id = ?1 AND status = 'running' AND ?2 >= updated_at_ms
                         RETURNING {COLUMNS}"
                    ),
                    params![
                        id,
                        at_ms,
                        counts.sessions_created,
                        counts.sessions_updated,
                        counts.events_written,
                        counts.waveform_chunks_written,
                    ],
                )?,
                "complete",
                at_ms,
            ),
            ImportTransition::Fail { at_ms, error } => (
                self.update_returning(
                    id,
                    "fail",
                    &format!(
                        "UPDATE import_history SET
                             status = 'failed', state_message = NULL,
                             updated_at_ms = ?2, completed_at_ms = ?2,
                             error_message = ?3
                         WHERE id = ?1 AND status = 'running' AND ?2 >= updated_at_ms
                         RETURNING {COLUMNS}"
                    ),
                    params![id, at_ms, error],
                )?,
                "fail",
                at_ms,
            ),
            ImportTransition::Cancel { at_ms, reason } => (
                self.update_returning(
                    id,
                    "cancel",
                    &format!(
                        "UPDATE import_history SET
                             status = 'cancelled', state_message = ?2,
                             updated_at_ms = ?3, completed_at_ms = ?3,
                             error_message = NULL
                         WHERE id = ?1 AND status IN ('blocked', 'running')
                           AND ?3 >= updated_at_ms
                         RETURNING {COLUMNS}"
                    ),
                    params![id, reason, at_ms],
                )?,
                "cancel",
                at_ms,
            ),
        };

        if updated.is_some() {
            return Ok(updated);
        }
        match self.get(id)? {
            None => Ok(None),
            Some(history) if attempted_at_ms < history.updated_at_ms => {
                Err(Error::ImportTimestampRegression {
                    id,
                    previous_at_ms: history.updated_at_ms,
                    attempted_at_ms,
                })
            }
            Some(history) => Err(Error::InvalidImportTransition {
                id,
                from: history.status.as_str().to_owned(),
                operation,
            }),
        }
    }

    pub fn block(&self, id: i64, at_ms: i64, reason: &str) -> Result<Option<ImportHistory>> {
        self.transition(id, ImportTransition::Block { at_ms, reason })
    }

    pub fn start(&self, id: i64, at_ms: i64) -> Result<Option<ImportHistory>> {
        self.transition(id, ImportTransition::Start { at_ms })
    }

    pub fn complete(
        &self,
        id: i64,
        completed_at_ms: i64,
        counts: ImportCounts,
    ) -> Result<Option<ImportHistory>> {
        self.transition(
            id,
            ImportTransition::Complete {
                at_ms: completed_at_ms,
                counts,
            },
        )
    }

    pub fn fail(
        &self,
        id: i64,
        completed_at_ms: i64,
        message: &str,
    ) -> Result<Option<ImportHistory>> {
        self.transition(
            id,
            ImportTransition::Fail {
                at_ms: completed_at_ms,
                error: message,
            },
        )
    }

    pub fn cancel(
        &self,
        id: i64,
        completed_at_ms: i64,
        reason: Option<&str>,
    ) -> Result<Option<ImportHistory>> {
        self.transition(
            id,
            ImportTransition::Cancel {
                at_ms: completed_at_ms,
                reason,
            },
        )
    }

    /// Converts every job left running after an application interruption into
    /// blocked state so the service can present an explicit resume decision.
    pub fn recover_running(&self, at_ms: i64, reason: &str) -> Result<Vec<ImportHistory>> {
        let sql = format!(
            "UPDATE import_history SET
                 status = 'blocked', state_message = ?1,
                 updated_at_ms = max(updated_at_ms, ?2)
             WHERE status = 'running' RETURNING {COLUMNS}"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params![reason, at_ms], map_import)?;
        let mut histories = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        histories.sort_by_key(|history| history.id);
        Ok(histories)
    }

    /// Creates one linked next attempt from a failed or cancelled job. Repeating
    /// the call returns the already-created retry without duplicating history.
    pub fn retry_or_get(&self, id: i64, retry: &RetryImport<'_>) -> Result<Option<BeginImport>> {
        let Some(source) = self.get(id)? else {
            return Ok(None);
        };
        if !matches!(
            source.status,
            ImportStatus::Failed | ImportStatus::Cancelled
        ) {
            return Err(Error::InvalidImportTransition {
                id,
                from: source.status.as_str().to_owned(),
                operation: "retry",
            });
        }
        let previous_at_ms = source
            .updated_at_ms
            .max(source.completed_at_ms.unwrap_or(source.updated_at_ms));
        if retry.created_at_ms < previous_at_ms {
            return Err(Error::ImportTimestampRegression {
                id,
                previous_at_ms,
                attempted_at_ms: retry.created_at_ms,
            });
        }
        if let Some(history) = self.find_retry_of(id)? {
            return Ok(Some(BeginImport {
                history,
                inserted: false,
            }));
        }

        let attempt = source
            .attempt
            .checked_add(1)
            .ok_or_else(|| Error::Integrity("import attempt number overflow".to_owned()))?;
        let status = retry.initial_status.status();
        let started_at_ms = (status == ImportStatus::Running).then_some(retry.created_at_ms);
        let sql = format!(
            "INSERT INTO import_history (
                 profile_id, machine_id, import_key, source_uri, loader_name,
                 attempt, retry_of_id, status, state_message, created_at_ms,
                 updated_at_ms, started_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11)
             ON CONFLICT DO NOTHING RETURNING {COLUMNS}"
        );
        let inserted = self
            .connection
            .query_row(
                &sql,
                params![
                    source.profile_id,
                    source.machine_id,
                    source.import_key,
                    source.source_uri,
                    source.loader_name,
                    attempt,
                    source.id,
                    status.as_str(),
                    retry.state_message,
                    retry.created_at_ms,
                    started_at_ms,
                ],
                map_import,
            )
            .optional()?;
        match inserted {
            Some(history) => Ok(Some(BeginImport {
                history,
                inserted: true,
            })),
            None => Ok(self.find_retry_of(id)?.map(|history| BeginImport {
                history,
                inserted: false,
            })),
        }
    }

    fn find_retry_of(&self, id: i64) -> Result<Option<ImportHistory>> {
        let sql = format!("SELECT {COLUMNS} FROM import_history WHERE retry_of_id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_import)
            .optional()?)
    }

    fn update_returning<P: rusqlite::Params>(
        &self,
        _id: i64,
        _operation: &'static str,
        sql: &str,
        parameters: P,
    ) -> Result<Option<ImportHistory>> {
        Ok(self
            .connection
            .query_row(sql, parameters, map_import)
            .optional()?)
    }
}

fn map_import(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImportHistory> {
    let raw_status: String = row.get(8)?;
    let status = ImportStatus::from_str(&raw_status).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            8,
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
        attempt: row.get(6)?,
        retry_of_id: row.get(7)?,
        status,
        state_message: row.get(9)?,
        created_at_ms: row.get(10)?,
        updated_at_ms: row.get(11)?,
        started_at_ms: row.get(12)?,
        completed_at_ms: row.get(13)?,
        sessions_created: row.get(14)?,
        sessions_updated: row.get(15)?,
        events_written: row.get(16)?,
        waveform_chunks_written: row.get(17)?,
        error_message: row.get(18)?,
    })
}
