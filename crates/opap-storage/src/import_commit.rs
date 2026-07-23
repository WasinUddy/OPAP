use crate::replacement::{replace_session_snapshot_on_with_checkpoint, validate_session_snapshot};
use crate::repository::{
    Events, Imports, Machines, Profiles, SessionSnapshots, Sessions, Waveforms,
};
use crate::{
    Database, Error, ImportCommitInput, ImportCommitResult, ImportCounts, ImportSessionOutcome,
    ImportSessionResult, ImportStatus, NewMachine, NewSession, Result, Session,
    SessionProvenanceInput, SessionSettingInput, SessionSettingValue, SessionSnapshot,
    is_canonical_execution_token, is_canonical_sha256,
};
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, HashSet};

impl Database {
    /// Commits one importer run as a single compare-and-set transaction.
    ///
    /// The callback is invoked before the transaction, at each durable boundary,
    /// and immediately before commit. Returning an error from any checkpoint
    /// rolls back the machine, every session snapshot, result links, and job
    /// completion together.
    pub fn commit_import_result<F>(
        &mut self,
        input: &ImportCommitInput<'_>,
        mut checkpoint: F,
    ) -> Result<ImportCommitResult>
    where
        F: FnMut() -> Result<()>,
    {
        validate_import_commit(input)?;
        let expected_session_results = usize_to_i64(input.sessions.len())?;
        checkpoint()?;

        let transaction = self.transaction()?;
        verify_commit_expectation(&transaction, input)?;
        checkpoint()?;

        let machine = Machines::new(&transaction).upsert(&NewMachine {
            profile_id: input.profile_id,
            source_key: input.machine.source_key,
            device_type: input.machine.device_type,
            manufacturer: input.machine.manufacturer,
            model: input.machine.model,
            model_number: input.machine.model_number,
            serial_number: input.machine.serial_number,
            seen_at_ms: input.machine.seen_at_ms,
        })?;
        link_import_machine(&transaction, input, machine.id)?;
        checkpoint()?;

        let mut session_results = Vec::with_capacity(input.sessions.len());
        let mut counts = ImportCounts::default();
        for imported in input.sessions {
            checkpoint()?;
            let existing =
                Sessions::new(&transaction).find_by_source_key(machine.id, imported.source_key)?;
            let outcome = classify_session_outcome(&transaction, existing.as_ref(), imported)?;
            let (session_id, events_written, waveform_chunks_written) = match outcome {
                ImportSessionOutcome::Created => {
                    counts.sessions_created = counts
                        .sessions_created
                        .checked_add(1)
                        .ok_or_else(count_overflow)?;
                    let replacement = replace_imported_session(
                        &transaction,
                        machine.id,
                        imported,
                        &mut checkpoint,
                    )?;
                    (
                        replacement.session.id,
                        replacement.stats.session_data.events_written,
                        replacement.stats.session_data.waveform_chunks_written,
                    )
                }
                ImportSessionOutcome::Updated => {
                    counts.sessions_updated = counts
                        .sessions_updated
                        .checked_add(1)
                        .ok_or_else(count_overflow)?;
                    let replacement = replace_imported_session(
                        &transaction,
                        machine.id,
                        imported,
                        &mut checkpoint,
                    )?;
                    (
                        replacement.session.id,
                        replacement.stats.session_data.events_written,
                        replacement.stats.session_data.waveform_chunks_written,
                    )
                }
                ImportSessionOutcome::Unchanged => (
                    existing
                        .as_ref()
                        .expect("unchanged outcome requires an existing session")
                        .id,
                    0,
                    0,
                ),
            };
            counts.events_written = counts
                .events_written
                .checked_add(usize_to_i64(events_written)?)
                .ok_or_else(count_overflow)?;
            counts.waveform_chunks_written = counts
                .waveform_chunks_written
                .checked_add(usize_to_i64(waveform_chunks_written)?)
                .ok_or_else(count_overflow)?;

            let inserted = transaction.execute(
                "INSERT INTO import_session_results (import_id, session_id, outcome)
                 VALUES (?1, ?2, ?3)",
                params![input.import_id, session_id, outcome.as_str(),],
            )?;
            if inserted != 1 {
                return Err(Error::Integrity(
                    "import session result insertion affected no row".to_owned(),
                ));
            }
            session_results.push(ImportSessionResult {
                import_id: input.import_id,
                session_id,
                outcome,
            });
            checkpoint()?;
        }

        checkpoint()?;
        let updated = transaction.execute(
            "UPDATE import_history SET
                 status = 'completed',
                 state_message = NULL,
                 updated_at_ms = ?1,
                 completed_at_ms = ?1,
                 sessions_created = ?2,
                 sessions_updated = ?3,
                 events_written = ?4,
                 waveform_chunks_written = ?5,
                 error_message = NULL,
                 execution_token = NULL
             WHERE id = ?6
               AND profile_id = ?7
               AND status = 'running'
               AND loader_name = ?8
               AND source_fingerprint = ?9
               AND input_digest = ?10
               AND options_digest = ?11
               AND execution_token = ?12
               AND execution_generation = ?13
               AND machine_id = ?14
               AND state_message = 'committing_result'
               AND (
                   SELECT COUNT(*) FROM import_session_results
                   WHERE import_id = ?6
               ) = ?15
               AND ?1 >= updated_at_ms",
            params![
                input.finished_at_ms,
                counts.sessions_created,
                counts.sessions_updated,
                counts.events_written,
                counts.waveform_chunks_written,
                input.import_id,
                input.profile_id,
                input.importer_name,
                input.source_fingerprint,
                input.input_digest,
                input.options_digest,
                input.execution_token,
                input.execution_generation,
                machine.id,
                expected_session_results,
            ],
        )?;
        if updated != 1 {
            return Err(commit_cas_error(&transaction, input)?);
        }
        checkpoint()?;
        let history = Imports::new(&transaction).get(input.import_id)?.ok_or(
            Error::StaleImportExecution {
                id: input.import_id,
            },
        )?;
        verify_final_import_sweep(
            &transaction,
            input,
            machine.id,
            &history,
            &counts,
            &session_results,
        )?;
        checkpoint()?;
        transaction.commit()?;

        Ok(ImportCommitResult {
            history,
            machine,
            sessions: session_results,
        })
    }
}

fn link_import_machine(
    connection: &rusqlite::Connection,
    input: &ImportCommitInput<'_>,
    machine_id: i64,
) -> Result<()> {
    let updated = connection.execute(
        "UPDATE import_history SET
             machine_id = ?1,
             state_message = 'committing_result'
         WHERE id = ?2
           AND profile_id = ?3
           AND status = 'running'
           AND loader_name = ?4
           AND source_fingerprint = ?5
           AND input_digest = ?6
           AND options_digest = ?7
           AND execution_token = ?8
           AND execution_generation = ?9
           AND state_message IS NULL
           AND (machine_id IS NULL OR machine_id = ?1)
           AND NOT EXISTS (
               SELECT 1 FROM import_session_results WHERE import_id = ?2
           )
           AND ?10 >= updated_at_ms",
        params![
            machine_id,
            input.import_id,
            input.profile_id,
            input.importer_name,
            input.source_fingerprint,
            input.input_digest,
            input.options_digest,
            input.execution_token,
            input.execution_generation,
            input.finished_at_ms,
        ],
    )?;
    if updated != 1 {
        return Err(commit_cas_error(connection, input)?);
    }
    Ok(())
}

fn replace_imported_session(
    connection: &rusqlite::Connection,
    machine_id: i64,
    imported: &crate::ImportSessionSnapshotInput<'_>,
    checkpoint: &mut dyn FnMut() -> Result<()>,
) -> Result<crate::SessionSnapshotReplacementResult> {
    replace_session_snapshot_on_with_checkpoint(
        connection,
        &NewSession {
            machine_id,
            source_key: imported.source_key,
            started_at_ms: imported.started_at_ms,
            ended_at_ms: imported.ended_at_ms,
            timezone_offset_minutes: imported.timezone_offset_minutes,
            now_ms: imported.now_ms,
        },
        &imported.snapshot,
        checkpoint,
    )
}

fn validate_import_commit(input: &ImportCommitInput<'_>) -> Result<()> {
    if input.profile_id <= 0 || input.import_id <= 0 || input.execution_generation <= 0 {
        return Err(Error::Integrity(
            "import commit identifiers and execution generation must be positive".to_owned(),
        ));
    }
    validate_bounded_text("importer name", input.importer_name, 128, false)?;
    for (field, digest) in [
        ("source fingerprint", input.source_fingerprint),
        ("input digest", input.input_digest),
        ("options digest", input.options_digest),
    ] {
        if !is_canonical_sha256(digest) {
            return Err(Error::Integrity(format!(
                "{field} must be exactly 64 lowercase hexadecimal characters"
            )));
        }
    }
    if !is_canonical_execution_token(input.execution_token) {
        return Err(Error::Integrity(
            "execution token must be a service-generated OPAP identifier".to_owned(),
        ));
    }

    validate_bounded_text("machine source key", input.machine.source_key, 256, false)?;
    validate_bounded_text("machine device type", input.machine.device_type, 128, false)?;
    validate_bounded_text(
        "machine manufacturer",
        input.machine.manufacturer,
        256,
        true,
    )?;
    validate_bounded_text("machine model", input.machine.model, 256, true)?;
    validate_bounded_text(
        "machine model number",
        input.machine.model_number,
        256,
        true,
    )?;
    validate_bounded_text(
        "machine serial number",
        input.machine.serial_number,
        256,
        true,
    )?;

    let mut session_keys = HashSet::with_capacity(input.sessions.len());
    for imported in input.sessions {
        validate_bounded_text("session source key", imported.source_key, 256, false)?;
        if !session_keys.insert(imported.source_key) {
            return Err(Error::Integrity(
                "import commit contains duplicate logical session identities".to_owned(),
            ));
        }
        if imported.snapshot.provenance.importer_name != input.importer_name {
            return Err(Error::Integrity(
                "session provenance importer does not match its import job".to_owned(),
            ));
        }
        validate_session_snapshot(
            &NewSession {
                machine_id: 1,
                source_key: imported.source_key,
                started_at_ms: imported.started_at_ms,
                ended_at_ms: imported.ended_at_ms,
                timezone_offset_minutes: imported.timezone_offset_minutes,
                now_ms: imported.now_ms,
            },
            &imported.snapshot,
        )?;
    }
    Ok(())
}

fn verify_commit_expectation(
    connection: &rusqlite::Connection,
    input: &ImportCommitInput<'_>,
) -> Result<()> {
    let matches = connection
        .query_row(
            "SELECT 1
             FROM import_history
             WHERE id = ?1
               AND profile_id = ?2
               AND status = 'running'
               AND loader_name = ?3
               AND source_fingerprint = ?4
               AND input_digest = ?5
               AND options_digest = ?6
               AND execution_token = ?7
               AND execution_generation = ?8
               AND state_message IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM import_session_results WHERE import_id = ?1
               )
               AND ?9 >= updated_at_ms",
            params![
                input.import_id,
                input.profile_id,
                input.importer_name,
                input.source_fingerprint,
                input.input_digest,
                input.options_digest,
                input.execution_token,
                input.execution_generation,
                input.finished_at_ms,
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !matches {
        return Err(commit_cas_error(connection, input)?);
    }
    if Profiles::new(connection).get(input.profile_id)?.is_none() {
        return Err(Error::Integrity(
            "import commit profile does not exist".to_owned(),
        ));
    }
    Ok(())
}

fn commit_cas_error(
    connection: &rusqlite::Connection,
    input: &ImportCommitInput<'_>,
) -> Result<Error> {
    let Some(history) = Imports::new(connection).get(input.import_id)? else {
        return Ok(Error::StaleImportExecution {
            id: input.import_id,
        });
    };
    let lease_is_current = history.profile_id == input.profile_id
        && history.status == ImportStatus::Running
        && history.loader_name == input.importer_name
        && history.source_fingerprint == input.source_fingerprint
        && history.input_digest == input.input_digest
        && history.options_digest == input.options_digest
        && history.execution_token.as_deref() == Some(input.execution_token)
        && history.execution_generation == input.execution_generation
        && history.state_message.is_none()
        && connection.query_row(
            "SELECT NOT EXISTS (
                 SELECT 1 FROM import_session_results WHERE import_id = ?1
             )",
            [input.import_id],
            |row| row.get::<_, bool>(0),
        )?;
    if lease_is_current && input.finished_at_ms < history.updated_at_ms {
        return Ok(Error::ImportTimestampRegression {
            id: input.import_id,
            previous_at_ms: history.updated_at_ms,
            attempted_at_ms: input.finished_at_ms,
        });
    }
    Ok(Error::StaleImportExecution {
        id: input.import_id,
    })
}

fn classify_session_outcome(
    connection: &rusqlite::Connection,
    existing: Option<&Session>,
    imported: &crate::ImportSessionSnapshotInput<'_>,
) -> Result<ImportSessionOutcome> {
    let Some(existing) = existing else {
        return Ok(ImportSessionOutcome::Created);
    };
    if existing.started_at_ms != imported.started_at_ms
        || existing.ended_at_ms != imported.ended_at_ms
        || existing.timezone_offset_minutes != imported.timezone_offset_minutes
    {
        return Ok(ImportSessionOutcome::Updated);
    }
    if persisted_session_matches(connection, existing.id, imported)? {
        Ok(ImportSessionOutcome::Unchanged)
    } else {
        Ok(ImportSessionOutcome::Updated)
    }
}

fn persisted_session_matches(
    connection: &rusqlite::Connection,
    session_id: i64,
    imported: &crate::ImportSessionSnapshotInput<'_>,
) -> Result<bool> {
    let stored_snapshot = match SessionSnapshots::new(connection).get(session_id) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) | Err(Error::Integrity(_)) => return Ok(false),
        Err(error) => return Err(error),
    };
    if !snapshot_matches(&stored_snapshot, &imported.snapshot) {
        return Ok(false);
    }

    let stored_events = Events::new(connection).list_by_session(session_id)?;
    let stored_events_by_key = stored_events
        .iter()
        .map(|event| (event.source_key.as_str(), event))
        .collect::<HashMap<_, _>>();
    if stored_events_by_key.len() != stored_events.len()
        || stored_events.len() != imported.snapshot.data.events.len()
    {
        return Ok(false);
    }
    for expected in imported.snapshot.data.events {
        let Some(stored) = stored_events_by_key.get(expected.source_key) else {
            return Ok(false);
        };
        if stored.channel_key != expected.channel_key
            || stored.event_type != expected.event_type
            || stored.starts_at_ms != expected.starts_at_ms
            || stored.duration_ms != expected.duration_ms
            || stored.value != expected.value
            || stored.unit.as_deref() != expected.unit
        {
            return Ok(false);
        }
    }

    let stored_waveforms = Waveforms::new(connection).list_metadata_by_session(session_id)?;
    let stored_waveforms_by_key = stored_waveforms
        .iter()
        .map(|waveform| (waveform.source_key.as_str(), waveform))
        .collect::<HashMap<_, _>>();
    if stored_waveforms_by_key.len() != stored_waveforms.len()
        || stored_waveforms.len() != imported.snapshot.data.waveforms.len()
    {
        return Ok(false);
    }
    for expected in imported.snapshot.data.waveforms {
        let Some(stored) = stored_waveforms_by_key.get(expected.source_key) else {
            return Ok(false);
        };
        if stored.channel_key != expected.channel_key
            || stored.unit.as_deref() != expected.unit
            || stored.started_at_ms != expected.started_at_ms
            || stored.sample_interval_us != expected.sample_interval_us
            || stored.sample_count != expected.sample_count
            || stored.encoding != expected.encoding
            || stored.min_value != expected.min_value
            || stored.max_value != expected.max_value
        {
            return Ok(false);
        }
        let stored_chunks = Waveforms::new(connection).list_chunks(stored.id)?;
        let stored_chunks_by_index = stored_chunks
            .iter()
            .map(|chunk| (chunk.chunk_index, chunk))
            .collect::<HashMap<_, _>>();
        if stored_chunks_by_index.len() != stored_chunks.len()
            || stored_chunks.len() != expected.chunks.len()
        {
            return Ok(false);
        }
        for expected_chunk in expected.chunks {
            let Some(stored_chunk) = stored_chunks_by_index.get(&expected_chunk.chunk_index) else {
                return Ok(false);
            };
            if stored_chunk.start_sample != expected_chunk.start_sample
                || stored_chunk.sample_count != expected_chunk.sample_count
                || stored_chunk.payload != expected_chunk.payload
                || stored_chunk.min_value != expected_chunk.min_value
                || stored_chunk.max_value != expected_chunk.max_value
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn snapshot_matches(
    stored: &SessionSnapshot,
    expected: &crate::SessionSnapshotReplacement<'_>,
) -> bool {
    if !provenance_matches(&stored.provenance, &expected.provenance)
        || stored.summary.usage_ms != expected.summary.usage_ms
    {
        return false;
    }

    let stored_slices_by_sequence = stored
        .slices
        .iter()
        .map(|slice| (slice.sequence, slice))
        .collect::<HashMap<_, _>>();
    if stored_slices_by_sequence.len() != stored.slices.len()
        || stored.slices.len() != expected.slices.len()
    {
        return false;
    }
    for expected_slice in expected.slices {
        let Some(stored_slice) = stored_slices_by_sequence.get(&expected_slice.sequence) else {
            return false;
        };
        if stored_slice.source_key != expected_slice.source_key
            || stored_slice.state != expected_slice.state
            || stored_slice.started_at_ms != expected_slice.started_at_ms
            || stored_slice.ended_at_ms != expected_slice.ended_at_ms
        {
            return false;
        }
    }

    let stored_metrics_by_key = stored
        .summary
        .metrics
        .iter()
        .map(|metric| (metric.key.as_str(), metric))
        .collect::<HashMap<_, _>>();
    if stored_metrics_by_key.len() != stored.summary.metrics.len()
        || stored.summary.metrics.len() != expected.summary.metrics.len()
    {
        return false;
    }
    for expected_metric in expected.summary.metrics {
        let Some(stored_metric) = stored_metrics_by_key.get(expected_metric.key) else {
            return false;
        };
        if stored_metric.value != expected_metric.value
            || stored_metric.unit.as_deref() != expected_metric.unit
        {
            return false;
        }
    }

    let stored_settings_by_key = stored
        .settings
        .iter()
        .map(|setting| (setting.key.as_str(), setting))
        .collect::<HashMap<_, _>>();
    if stored_settings_by_key.len() != stored.settings.len()
        || stored.settings.len() != expected.settings.len()
    {
        return false;
    }
    for expected_setting in expected.settings {
        let Some(stored_setting) = stored_settings_by_key.get(expected_setting.key) else {
            return false;
        };
        if stored_setting.unit.as_deref() != expected_setting.unit
            || stored_setting.origin != expected_setting.origin
            || !setting_value_matches(&stored_setting.value, expected_setting)
        {
            return false;
        }
    }
    true
}

fn setting_value_matches(stored: &SessionSettingValue, expected: &SessionSettingInput<'_>) -> bool {
    match stored {
        SessionSettingValue::Integer(value) => {
            expected.integer_value == Some(*value)
                && expected.real_value.is_none()
                && expected.text_value.is_none()
                && expected.boolean_value.is_none()
        }
        SessionSettingValue::Real(value) => {
            expected.integer_value.is_none()
                && expected.real_value == Some(*value)
                && expected.text_value.is_none()
                && expected.boolean_value.is_none()
        }
        SessionSettingValue::Text(value) => {
            expected.integer_value.is_none()
                && expected.real_value.is_none()
                && expected.text_value == Some(value.as_str())
                && expected.boolean_value.is_none()
        }
        SessionSettingValue::Boolean(value) => {
            expected.integer_value.is_none()
                && expected.real_value.is_none()
                && expected.text_value.is_none()
                && expected.boolean_value == Some(*value)
        }
    }
}

fn provenance_matches(
    stored: &crate::SessionProvenance,
    imported: &SessionProvenanceInput<'_>,
) -> bool {
    stored.therapy_day == imported.therapy_day
        && stored.start_local_wall == imported.start_local_wall
        && stored.end_local_wall == imported.end_local_wall
        && stored.start_utc_offset_seconds == imported.start_utc_offset_seconds
        && stored.end_utc_offset_seconds == imported.end_utc_offset_seconds
        && stored.start_clock_correction_ms == imported.start_clock_correction_ms
        && stored.end_clock_correction_ms == imported.end_clock_correction_ms
        && stored.data_kind == imported.data_kind
        && stored.importer_name == imported.importer_name
        && stored.importer_schema == imported.importer_schema
        && stored.id_algorithm == imported.id_algorithm
        && stored.source_digest == imported.source_digest
        && stored.content_digest == imported.content_digest
}

fn verify_final_import_sweep(
    connection: &rusqlite::Connection,
    input: &ImportCommitInput<'_>,
    machine_id: i64,
    history: &crate::ImportHistory,
    counts: &ImportCounts,
    expected_results: &[ImportSessionResult],
) -> Result<()> {
    if history.status != crate::ImportStatus::Completed
        || history.profile_id != input.profile_id
        || history.machine_id != Some(machine_id)
        || history.loader_name != input.importer_name
        || history.execution_generation != input.execution_generation
        || history.execution_token.is_some()
        || history.sessions_created != counts.sessions_created
        || history.sessions_updated != counts.sessions_updated
        || history.events_written != counts.events_written
        || history.waveform_chunks_written != counts.waveform_chunks_written
    {
        return Err(Error::Integrity(
            "final import history sweep found an inconsistent completion".to_owned(),
        ));
    }

    let stored_results = Imports::new(connection).list_session_results(input.import_id)?;
    let stored_results_by_session = stored_results
        .iter()
        .map(|result| (result.session_id, result))
        .collect::<HashMap<_, _>>();
    let expected_results_by_session = expected_results
        .iter()
        .map(|result| (result.session_id, result))
        .collect::<HashMap<_, _>>();
    if stored_results_by_session.len() != stored_results.len()
        || expected_results_by_session.len() != expected_results.len()
        || stored_results_by_session.len() != expected_results_by_session.len()
        || expected_results_by_session
            .iter()
            .any(|(session_id, expected)| {
                stored_results_by_session.get(session_id).copied() != Some(*expected)
            })
    {
        return Err(Error::Integrity(
            "final import result sweep found inconsistent session links".to_owned(),
        ));
    }

    for imported in input.sessions {
        let Some(session) =
            Sessions::new(connection).find_by_source_key(machine_id, imported.source_key)?
        else {
            return Err(Error::Integrity(
                "final import sweep found a missing session".to_owned(),
            ));
        };
        if !stored_results_by_session.contains_key(&session.id)
            || !persisted_session_matches(connection, session.id, imported)?
        {
            return Err(Error::Integrity(
                "final import sweep found an incomplete session snapshot".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_bounded_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
    allow_empty: bool,
) -> Result<()> {
    if (!allow_empty && value.is_empty())
        || value.len() > maximum_bytes
        || value.as_bytes().contains(&0)
    {
        return Err(Error::Integrity(format!(
            "{field} must be {} and at most {maximum_bytes} bytes without NUL characters",
            if allow_empty { "bounded" } else { "non-empty" }
        )));
    }
    Ok(())
}

fn usize_to_i64(value: usize) -> Result<i64> {
    i64::try_from(value).map_err(|_| count_overflow())
}

fn count_overflow() -> Error {
    Error::Integrity("import result count overflow".to_owned())
}
