use crate::{
    BeginImport, Error, ImportBlockCode, ImportCounts, ImportExecutionClaim, ImportExecutionLease,
    ImportFailureCode, ImportHistory, ImportSessionOutcome, ImportSessionResult, ImportStatus,
    ImportTransition, NewImport, Result, RetryImport, is_canonical_execution_token,
    is_canonical_request_id, is_canonical_sha256, is_persistable_import_key,
    is_persistable_source_id,
};
use rusqlite::{Connection, OptionalExtension, params, types::Type};
use std::io;

const COLUMNS: &str = "id, profile_id, machine_id, import_key, source_uri, loader_name, \
    attempt, retry_of_id, status, state_message, created_at_ms, updated_at_ms, started_at_ms, \
    completed_at_ms, sessions_created, sessions_updated, events_written, \
    waveform_chunks_written, error_message, source_fingerprint, input_digest, options_digest, \
    execution_generation, execution_token";

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
        validate_importer_name(input.loader_name)?;
        if !is_canonical_request_id(input.import_key) {
            return Err(Error::Integrity(
                "new import key must be a service-generated OPAP request identifier".to_owned(),
            ));
        }
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

    /// Acquires a generation-scoped execution lease for a blocked job, or for a
    /// legacy running job that does not yet have a lease. Immutable digests make
    /// later commit compare-and-set checks independent of process-local state.
    pub fn claim_execution(
        &self,
        id: i64,
        claim: &ImportExecutionClaim<'_>,
    ) -> Result<Option<ImportHistory>> {
        validate_execution_claim(claim)?;
        let sql = format!(
            "UPDATE import_history SET
                 status = 'running',
                 state_message = NULL,
                 updated_at_ms = ?2,
                 started_at_ms = COALESCE(started_at_ms, ?2),
                 source_fingerprint = ?3,
                 input_digest = ?4,
                 options_digest = ?5,
                 execution_generation = execution_generation + 1,
                 execution_token = ?6
             WHERE id = ?1
               AND status IN ('blocked', 'running')
               AND execution_token IS NULL
               AND ?2 >= updated_at_ms
               AND (source_fingerprint = '' OR source_fingerprint = ?3)
               AND (input_digest = '' OR input_digest = ?4)
               AND (options_digest = '' OR options_digest = ?5)
               AND profile_id = ?7
               AND loader_name = ?8
             RETURNING {COLUMNS}"
        );
        let claimed = self
            .connection
            .query_row(
                &sql,
                params![
                    id,
                    claim.claimed_at_ms,
                    claim.source_fingerprint,
                    claim.input_digest,
                    claim.options_digest,
                    claim.execution_token,
                    claim.profile_id,
                    claim.importer_name,
                ],
                map_import,
            )
            .optional()?;
        if claimed.is_some() {
            return Ok(claimed);
        }
        match self.get(id)? {
            None => Ok(None),
            Some(history) if claim.claimed_at_ms < history.updated_at_ms => {
                Err(Error::ImportTimestampRegression {
                    id,
                    previous_at_ms: history.updated_at_ms,
                    attempted_at_ms: claim.claimed_at_ms,
                })
            }
            Some(history) => Err(Error::InvalidImportTransition {
                id,
                from: history.status.as_str().to_owned(),
                operation: "claim execution",
            }),
        }
    }

    pub fn list_session_results(&self, import_id: i64) -> Result<Vec<ImportSessionResult>> {
        let mut statement = self.connection.prepare(
            "SELECT import_id, session_id, outcome
             FROM import_session_results
             WHERE import_id = ?1
             ORDER BY session_id",
        )?;
        let rows = statement.query_map([import_id], |row| {
            let import_id = row.get(0)?;
            let session_id = row.get(1)?;
            let raw_outcome: String = row.get(2)?;
            let outcome = ImportSessionOutcome::from_str(&raw_outcome).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    Type::Text,
                    Box::new(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unknown import session outcome {raw_outcome:?}"),
                    )),
                )
            })?;
            Ok(ImportSessionResult {
                import_id,
                session_id,
                outcome,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Blocks only the exact running execution lease owned by a worker. Generic
    /// [`Self::block`] remains available for recovery or administrative
    /// revocation when deliberately invalidating whichever lease is current.
    pub fn block_execution(
        &self,
        id: i64,
        lease: &ImportExecutionLease<'_>,
        at_ms: i64,
        code: ImportBlockCode,
    ) -> Result<Option<ImportHistory>> {
        validate_execution_lease(lease)?;
        let sql = format!(
            "UPDATE import_history SET
                 status = 'blocked',
                 state_message = ?2,
                 updated_at_ms = ?3,
                 execution_token = NULL
             WHERE id = ?1
               AND profile_id = ?4
               AND loader_name = ?5
               AND status = 'running'
               AND execution_token = ?6
               AND execution_generation = ?7
               AND ?3 >= updated_at_ms
             RETURNING {COLUMNS}"
        );
        let updated = self
            .connection
            .query_row(
                &sql,
                params![
                    id,
                    code.as_str(),
                    at_ms,
                    lease.profile_id,
                    lease.importer_name,
                    lease.execution_token,
                    lease.execution_generation,
                ],
                map_import,
            )
            .optional()?;
        self.resolve_execution_update(id, lease, at_ms, updated)
    }

    /// Fails only the exact running execution lease owned by a worker.
    pub fn fail_execution(
        &self,
        id: i64,
        lease: &ImportExecutionLease<'_>,
        at_ms: i64,
        code: ImportFailureCode,
    ) -> Result<Option<ImportHistory>> {
        validate_execution_lease(lease)?;
        let sql = format!(
            "UPDATE import_history SET
                 status = 'failed',
                 state_message = NULL,
                 updated_at_ms = ?2,
                 completed_at_ms = ?2,
                 error_message = ?3,
                 execution_token = NULL
             WHERE id = ?1
               AND profile_id = ?4
               AND loader_name = ?5
               AND status = 'running'
               AND execution_token = ?6
               AND execution_generation = ?7
               AND ?2 >= updated_at_ms
             RETURNING {COLUMNS}"
        );
        let updated = self
            .connection
            .query_row(
                &sql,
                params![
                    id,
                    at_ms,
                    code.as_str(),
                    lease.profile_id,
                    lease.importer_name,
                    lease.execution_token,
                    lease.execution_generation,
                ],
                map_import,
            )
            .optional()?;
        self.resolve_execution_update(id, lease, at_ms, updated)
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
                             status = 'blocked',
                             state_message = CASE
                                 WHEN execution_generation > 0 THEN 'admin_revoked'
                                 ELSE ?2
                             END,
                             updated_at_ms = ?3,
                             execution_token = NULL
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
                           AND execution_generation = 0
                           AND execution_token IS NULL
                           AND source_fingerprint = ''
                           AND input_digest = ''
                           AND options_digest = ''
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
                             error_message = NULL, execution_token = NULL
                         WHERE id = ?1 AND status = 'running'
                           AND execution_generation = 0
                           AND execution_token IS NULL
                           AND source_fingerprint = ''
                           AND input_digest = ''
                           AND options_digest = ''
                           AND ?2 >= updated_at_ms
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
                             error_message = ?3, execution_token = NULL
                         WHERE id = ?1 AND status = 'running'
                           AND execution_generation = 0
                           AND execution_token IS NULL
                           AND source_fingerprint = ''
                           AND input_digest = ''
                           AND options_digest = ''
                           AND ?2 >= updated_at_ms
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
                             status = 'cancelled',
                             state_message = CASE
                                 WHEN execution_generation > 0 THEN 'user_cancelled'
                                 ELSE ?2
                             END,
                             updated_at_ms = ?3, completed_at_ms = ?3,
                             error_message = NULL, execution_token = NULL
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
                 status = 'blocked',
                 state_message = CASE
                     WHEN execution_generation > 0 THEN 'recovered_after_restart'
                     ELSE ?1
                 END,
                 updated_at_ms = max(updated_at_ms, ?2),
                 execution_token = NULL
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
        if !is_persistable_import_key(&source.import_key) {
            return Err(Error::Integrity(
                "stored import key is not an opaque OPAP request identifier".to_owned(),
            ));
        }
        validate_importer_name(&source.loader_name)?;
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
             ON CONFLICT(retry_of_id) DO NOTHING
             RETURNING {COLUMNS}"
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

    fn resolve_execution_update(
        &self,
        id: i64,
        lease: &ImportExecutionLease<'_>,
        attempted_at_ms: i64,
        updated: Option<ImportHistory>,
    ) -> Result<Option<ImportHistory>> {
        if updated.is_some() {
            return Ok(updated);
        }
        match self.get(id)? {
            None => Ok(None),
            Some(history)
                if history.profile_id == lease.profile_id
                    && history.loader_name == lease.importer_name
                    && history.status == ImportStatus::Running
                    && history.execution_token.as_deref() == Some(lease.execution_token)
                    && history.execution_generation == lease.execution_generation
                    && attempted_at_ms < history.updated_at_ms =>
            {
                Err(Error::ImportTimestampRegression {
                    id,
                    previous_at_ms: history.updated_at_ms,
                    attempted_at_ms,
                })
            }
            Some(_) => Err(Error::StaleImportExecution { id }),
        }
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
        source_fingerprint: row.get(19)?,
        input_digest: row.get(20)?,
        options_digest: row.get(21)?,
        execution_generation: row.get(22)?,
        execution_token: row.get(23)?,
    })
}

fn validate_execution_claim(claim: &ImportExecutionClaim<'_>) -> Result<()> {
    validate_execution_lease(&ImportExecutionLease {
        profile_id: claim.profile_id,
        importer_name: claim.importer_name,
        execution_token: claim.execution_token,
        execution_generation: 1,
    })?;
    for (field, value) in [
        ("source fingerprint", claim.source_fingerprint),
        ("input digest", claim.input_digest),
        ("options digest", claim.options_digest),
    ] {
        if !is_canonical_sha256(value) {
            return Err(Error::Integrity(format!(
                "{field} must be exactly 64 lowercase hexadecimal characters"
            )));
        }
    }
    Ok(())
}

fn validate_execution_lease(lease: &ImportExecutionLease<'_>) -> Result<()> {
    if lease.profile_id <= 0 || lease.execution_generation <= 0 {
        return Err(Error::Integrity(
            "execution lease profile id and generation must be positive".to_owned(),
        ));
    }
    validate_importer_name(lease.importer_name)?;
    if !is_canonical_execution_token(lease.execution_token) {
        return Err(Error::Integrity(
            "execution token must be a service-generated OPAP identifier".to_owned(),
        ));
    }
    Ok(())
}

fn validate_importer_name(importer_name: &str) -> Result<()> {
    if importer_name.is_empty()
        || importer_name.len() > 128
        || importer_name.as_bytes().contains(&0)
    {
        return Err(Error::Integrity(
            "importer name must be non-empty and at most 128 bytes without NUL characters"
                .to_owned(),
        ));
    }
    Ok(())
}
