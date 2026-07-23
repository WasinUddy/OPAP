ALTER TABLE import_history ADD COLUMN source_fingerprint TEXT NOT NULL DEFAULT ''
CHECK (
    source_fingerprint = ''
    OR (
        length(CAST(source_fingerprint AS BLOB)) = 64
        AND source_fingerprint NOT GLOB '*[^0-9a-f]*'
    )
);

ALTER TABLE import_history ADD COLUMN input_digest TEXT NOT NULL DEFAULT ''
CHECK (
    input_digest = ''
    OR (
        length(CAST(input_digest AS BLOB)) = 64
        AND input_digest NOT GLOB '*[^0-9a-f]*'
    )
);

ALTER TABLE import_history ADD COLUMN options_digest TEXT NOT NULL DEFAULT ''
CHECK (
    options_digest = ''
    OR (
        length(CAST(options_digest AS BLOB)) = 64
        AND options_digest NOT GLOB '*[^0-9a-f]*'
    )
);

ALTER TABLE import_history ADD COLUMN execution_generation INTEGER NOT NULL DEFAULT 0
CHECK (execution_generation >= 0);

ALTER TABLE import_history ADD COLUMN execution_token TEXT
CHECK (
    execution_token IS NULL
    OR (
        status = 'running'
        AND execution_generation > 0
        AND source_fingerprint <> ''
        AND input_digest <> ''
        AND options_digest <> ''
        AND length(execution_token) = length(CAST(execution_token AS BLOB))
        AND length(execution_token) = 47
        AND substr(execution_token, 1, 15) = 'opap-execution:'
        AND substr(execution_token, 16) NOT GLOB '*[^0-9a-f]*'
    )
);

CREATE TABLE import_session_results (
    import_id  INTEGER NOT NULL
                       REFERENCES import_history(id) ON DELETE CASCADE,
    session_id INTEGER NOT NULL
                       REFERENCES sessions(id) ON DELETE CASCADE,
    outcome    TEXT NOT NULL CHECK (
        outcome IN ('created', 'updated', 'unchanged')
    ),
    PRIMARY KEY (import_id, session_id)
) STRICT;

CREATE INDEX import_session_results_by_session
    ON import_session_results(session_id, import_id);

DROP TRIGGER import_history_protect_terminal_state;

CREATE TRIGGER import_history_protect_terminal_state
BEFORE UPDATE OF
    profile_id, import_key, source_uri, loader_name, attempt, status, state_message,
    created_at_ms, updated_at_ms, started_at_ms, completed_at_ms,
    sessions_created, sessions_updated, events_written,
    waveform_chunks_written, error_message, source_fingerprint, input_digest,
    options_digest, execution_generation, execution_token
ON import_history
WHEN OLD.status IN ('completed', 'failed', 'cancelled')
BEGIN
    SELECT RAISE(ABORT, 'terminal import job cannot be changed');
END;

CREATE TRIGGER import_history_validate_execution_identity
BEFORE UPDATE OF
    source_fingerprint, input_digest, options_digest,
    execution_generation, execution_token
ON import_history
WHEN
    (OLD.source_fingerprint <> ''
     AND NEW.source_fingerprint IS NOT OLD.source_fingerprint)
 OR (OLD.input_digest <> '' AND NEW.input_digest IS NOT OLD.input_digest)
 OR (OLD.options_digest <> '' AND NEW.options_digest IS NOT OLD.options_digest)
 OR NEW.execution_generation < OLD.execution_generation
 OR NEW.execution_generation > OLD.execution_generation + 1
 OR (
    NEW.execution_generation = OLD.execution_generation + 1
    AND NOT (
        OLD.execution_token IS NULL
        AND NEW.execution_token IS NOT NULL
        AND NEW.status = 'running'
        AND NEW.source_fingerprint <> ''
        AND NEW.input_digest <> ''
        AND NEW.options_digest <> ''
    )
 )
 OR (
    NEW.execution_token IS NOT OLD.execution_token
    AND NOT (
        (
            OLD.execution_token IS NULL
            AND NEW.execution_token IS NOT NULL
            AND NEW.execution_generation = OLD.execution_generation + 1
            AND NEW.status = 'running'
        )
        OR (
            OLD.execution_token IS NOT NULL
            AND NEW.execution_token IS NULL
            AND NEW.execution_generation = OLD.execution_generation
            AND OLD.status = 'running'
            AND NEW.status IN ('blocked', 'completed', 'failed', 'cancelled')
        )
    )
 )
 OR (
    (
        OLD.source_fingerprint = ''
        OR OLD.input_digest = ''
        OR OLD.options_digest = ''
    )
    AND (
        NEW.source_fingerprint <> OLD.source_fingerprint
        OR NEW.input_digest <> OLD.input_digest
        OR NEW.options_digest <> OLD.options_digest
    )
    AND NOT (
        NEW.execution_generation = OLD.execution_generation + 1
        AND OLD.execution_token IS NULL
        AND NEW.execution_token IS NOT NULL
        AND NEW.status = 'running'
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'invalid import execution identity change');
END;

CREATE TRIGGER import_history_validate_execution_outcome_code
BEFORE UPDATE OF status, state_message, error_message, execution_token
ON import_history
WHEN OLD.execution_generation > 0
 AND OLD.status <> NEW.status
 AND NEW.status IN ('blocked', 'completed', 'failed', 'cancelled')
 AND NOT (
    (
        NEW.status = 'blocked'
        AND NEW.state_message IN (
            'worker_interrupted',
            'retry_pending',
            'source_unavailable',
            'admin_revoked',
            'recovered_after_restart'
        )
        AND NEW.error_message IS NULL
    )
    OR (
        NEW.status = 'failed'
        AND NEW.state_message IS NULL
        AND NEW.error_message IN (
            'invalid_source',
            'unsupported_source',
            'decode_failed',
            'resource_limit',
            'source_unavailable',
            'internal_failure'
        )
    )
    OR (
        NEW.status = 'cancelled'
        AND NEW.state_message = 'user_cancelled'
        AND NEW.error_message IS NULL
    )
    OR (
        NEW.status = 'completed'
        AND NEW.state_message IS NULL
        AND NEW.error_message IS NULL
        AND NEW.machine_id IS NOT NULL
        AND NEW.sessions_created = (
            SELECT COUNT(*)
            FROM import_session_results
            WHERE import_id = NEW.id AND outcome = 'created'
        )
        AND NEW.sessions_updated = (
            SELECT COUNT(*)
            FROM import_session_results
            WHERE import_id = NEW.id AND outcome = 'updated'
        )
        AND NEW.events_written = (
            SELECT COUNT(*)
            FROM import_session_results AS result
            JOIN events AS event ON event.session_id = result.session_id
            WHERE result.import_id = NEW.id
              AND result.outcome <> 'unchanged'
        )
        AND NEW.waveform_chunks_written = (
            SELECT COUNT(*)
            FROM import_session_results AS result
            JOIN waveforms AS waveform ON waveform.session_id = result.session_id
            JOIN waveform_chunks AS chunk ON chunk.waveform_id = waveform.id
            WHERE result.import_id = NEW.id
              AND result.outcome <> 'unchanged'
        )
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'invalid import execution outcome code');
END;

CREATE TRIGGER import_history_validate_execution_state
BEFORE UPDATE OF status ON import_history
WHEN OLD.execution_generation > 0
 AND OLD.status = 'blocked'
 AND NEW.status = 'running'
 AND NOT (
    NEW.execution_token IS NOT NULL
    AND NEW.execution_generation = OLD.execution_generation + 1
 )
BEGIN
    SELECT RAISE(ABORT, 'blocked import execution requires a new lease');
END;

CREATE TRIGGER import_history_validate_generation_state
BEFORE UPDATE ON import_history
WHEN NEW.execution_generation > 0
 AND NOT (
    (
        NEW.status = 'running'
        AND NEW.execution_token IS NOT NULL
        AND (
            NEW.state_message IS NULL
            OR NEW.state_message = 'committing_result'
        )
        AND NEW.error_message IS NULL
        AND NEW.completed_at_ms IS NULL
    )
    OR (
        NEW.status = 'blocked'
        AND NEW.execution_token IS NULL
        AND NEW.state_message IS NOT NULL
        AND NEW.state_message IN (
            'worker_interrupted',
            'retry_pending',
            'source_unavailable',
            'admin_revoked',
            'recovered_after_restart'
        )
        AND NEW.error_message IS NULL
        AND NEW.completed_at_ms IS NULL
    )
    OR (
        NEW.status = 'failed'
        AND NEW.execution_token IS NULL
        AND NEW.state_message IS NULL
        AND NEW.error_message IS NOT NULL
        AND NEW.error_message IN (
            'invalid_source',
            'unsupported_source',
            'decode_failed',
            'resource_limit',
            'source_unavailable',
            'internal_failure'
        )
        AND NEW.completed_at_ms IS NOT NULL
    )
    OR (
        NEW.status = 'cancelled'
        AND NEW.execution_token IS NULL
        AND NEW.state_message IS NOT NULL
        AND NEW.state_message = 'user_cancelled'
        AND NEW.error_message IS NULL
        AND NEW.completed_at_ms IS NOT NULL
    )
    OR (
        NEW.status = 'completed'
        AND NEW.execution_token IS NULL
        AND NEW.state_message IS NULL
        AND NEW.error_message IS NULL
        AND NEW.completed_at_ms IS NOT NULL
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation-scoped import state');
END;

CREATE TRIGGER import_session_results_validate_job_insert
BEFORE INSERT ON import_session_results
WHEN NOT EXISTS (
    SELECT 1
    FROM import_history AS history
    JOIN sessions AS session ON session.id = NEW.session_id
    JOIN machines AS machine ON machine.id = session.machine_id
    WHERE history.id = NEW.import_id
      AND history.status = 'running'
      AND history.execution_token IS NOT NULL
      AND history.execution_generation > 0
      AND history.state_message = 'committing_result'
      AND history.machine_id = session.machine_id
      AND history.profile_id = machine.profile_id
)
BEGIN
    SELECT RAISE(ABORT, 'import session result does not match its executing job');
END;

CREATE TRIGGER import_session_results_protect_update
BEFORE UPDATE ON import_session_results
BEGIN
    SELECT RAISE(ABORT, 'import session result cannot be changed');
END;

CREATE TRIGGER import_session_results_protect_delete
BEFORE DELETE ON import_session_results
WHEN EXISTS (
    SELECT 1 FROM import_history WHERE id = OLD.import_id
)
 AND EXISTS (
    SELECT 1 FROM sessions WHERE id = OLD.session_id
 )
BEGIN
    SELECT RAISE(ABORT, 'import session result cannot be deleted');
END;
