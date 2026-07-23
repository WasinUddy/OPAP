CREATE TRIGGER import_history_validate_initial_insert
BEFORE INSERT ON import_history
WHEN NOT (
    length(CAST(NEW.loader_name AS BLOB)) BETWEEN 1 AND 128
    AND instr(CAST(NEW.loader_name AS BLOB), x'00') = 0
    AND NEW.status IN ('blocked', 'running')
    AND NEW.source_fingerprint = ''
    AND NEW.input_digest = ''
    AND NEW.options_digest = ''
    AND NEW.execution_generation = 0
    AND NEW.execution_token IS NULL
    AND NEW.sessions_created = 0
    AND NEW.sessions_updated = 0
    AND NEW.events_written = 0
    AND NEW.waveform_chunks_written = 0
    AND NEW.completed_at_ms IS NULL
    AND NEW.error_message IS NULL
    AND (
        (NEW.status = 'blocked' AND NEW.started_at_ms IS NULL)
        OR (NEW.status = 'running' AND NEW.started_at_ms IS NOT NULL)
    )
)
BEGIN
    SELECT RAISE(ABORT, 'invalid initial import job state');
END;

CREATE TRIGGER import_history_validate_generation_artifacts
BEFORE UPDATE OF
    status, state_message, completed_at_ms, sessions_created, sessions_updated,
    events_written, waveform_chunks_written, error_message,
    execution_generation, execution_token
ON import_history
WHEN NEW.execution_generation > 0
 AND (
    (
        NEW.status IN ('blocked', 'failed', 'cancelled')
        AND NOT (
            NEW.sessions_created = 0
            AND NEW.sessions_updated = 0
            AND NEW.events_written = 0
            AND NEW.waveform_chunks_written = 0
            AND NOT EXISTS (
                SELECT 1
                FROM import_session_results
                WHERE import_id = NEW.id
            )
        )
    )
    OR (
        NEW.status = 'completed'
        AND NOT (
            NEW.machine_id IS NOT NULL
            AND NOT EXISTS (
                SELECT 1
                FROM import_session_results AS result
                LEFT JOIN sessions AS session
                    ON session.id = result.session_id
                LEFT JOIN machines AS machine
                    ON machine.id = session.machine_id
                LEFT JOIN session_provenance AS provenance
                    ON provenance.session_id = session.id
                LEFT JOIN session_summary AS summary
                    ON summary.session_id = session.id
                WHERE result.import_id = NEW.id
                  AND (
                      session.id IS NULL
                      OR session.machine_id IS NOT NEW.machine_id
                      OR machine.id IS NULL
                      OR machine.profile_id IS NOT NEW.profile_id
                      OR provenance.session_id IS NULL
                      OR provenance.importer_name IS NOT NEW.loader_name
                      OR summary.session_id IS NULL
                  )
            )
        )
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation-scoped import artifacts');
END;

CREATE TRIGGER import_session_results_validate_snapshot_insert
BEFORE INSERT ON import_session_results
WHEN NOT EXISTS (
    SELECT 1
    FROM import_history AS history
    JOIN sessions AS session
        ON session.id = NEW.session_id
    JOIN machines AS machine
        ON machine.id = session.machine_id
    JOIN session_provenance AS provenance
        ON provenance.session_id = session.id
    JOIN session_summary AS summary
        ON summary.session_id = session.id
    WHERE history.id = NEW.import_id
      AND history.status = 'running'
      AND history.execution_generation > 0
      AND history.execution_token IS NOT NULL
      AND history.state_message = 'committing_result'
      AND history.machine_id = session.machine_id
      AND history.profile_id = machine.profile_id
      AND history.loader_name = provenance.importer_name
)
BEGIN
    SELECT RAISE(ABORT, 'import session result requires a complete matching snapshot');
END;
