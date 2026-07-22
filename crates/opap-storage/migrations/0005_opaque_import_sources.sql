-- Early v4 development databases allowed terminal jobs to retain absolute paths.
-- Temporarily remove terminal protection, redact them, then install strict guards.
DROP TRIGGER IF EXISTS import_history_protect_terminal_state;
DROP TRIGGER IF EXISTS import_history_validate_source_insert;
DROP TRIGGER IF EXISTS import_history_validate_source_update;
DROP TRIGGER IF EXISTS import_history_validate_state_transition;
DROP TRIGGER IF EXISTS import_history_monotonic_update_time;

UPDATE import_history
SET source_uri = 'opap-source:legacy-' || CAST(id AS TEXT)
WHERE NOT (
    (
        length(source_uri) = 44
        AND substr(source_uri, 1, 12) = 'opap-source:'
        AND substr(source_uri, 13) NOT GLOB '*[^0-9a-f]*'
    )
    OR (
        length(source_uri) BETWEEN 20 AND 38
        AND substr(source_uri, 1, 19) = 'opap-source:legacy-'
        AND substr(source_uri, 20, 1) BETWEEN '1' AND '9'
        AND substr(source_uri, 20) NOT GLOB '*[^0-9]*'
    )
);

CREATE TRIGGER import_history_validate_source_insert
BEFORE INSERT ON import_history
WHEN NOT (
    (
        length(NEW.source_uri) = 44
        AND substr(NEW.source_uri, 1, 12) = 'opap-source:'
        AND substr(NEW.source_uri, 13) NOT GLOB '*[^0-9a-f]*'
    )
    OR (
        length(NEW.source_uri) BETWEEN 20 AND 38
        AND substr(NEW.source_uri, 1, 19) = 'opap-source:legacy-'
        AND substr(NEW.source_uri, 20, 1) BETWEEN '1' AND '9'
        AND substr(NEW.source_uri, 20) NOT GLOB '*[^0-9]*'
    )
)
BEGIN
    SELECT RAISE(ABORT, 'import source must be an opaque OPAP source identifier');
END;

CREATE TRIGGER import_history_validate_source_update
BEFORE UPDATE OF source_uri ON import_history
WHEN NOT (
    (
        length(NEW.source_uri) = 44
        AND substr(NEW.source_uri, 1, 12) = 'opap-source:'
        AND substr(NEW.source_uri, 13) NOT GLOB '*[^0-9a-f]*'
    )
    OR (
        length(NEW.source_uri) BETWEEN 20 AND 38
        AND substr(NEW.source_uri, 1, 19) = 'opap-source:legacy-'
        AND substr(NEW.source_uri, 20, 1) BETWEEN '1' AND '9'
        AND substr(NEW.source_uri, 20) NOT GLOB '*[^0-9]*'
    )
)
BEGIN
    SELECT RAISE(ABORT, 'import source must be an opaque OPAP source identifier');
END;

CREATE TRIGGER import_history_validate_state_transition
BEFORE UPDATE OF status ON import_history
WHEN OLD.status <> NEW.status
 AND NOT (
    (OLD.status = 'blocked' AND NEW.status IN ('running', 'cancelled'))
    OR (
        OLD.status = 'running'
        AND NEW.status IN ('blocked', 'completed', 'failed', 'cancelled')
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'invalid import job state transition');
END;

CREATE TRIGGER import_history_monotonic_update_time
BEFORE UPDATE OF updated_at_ms ON import_history
WHEN NEW.updated_at_ms < OLD.updated_at_ms
BEGIN
    SELECT RAISE(ABORT, 'import job update time cannot move backwards');
END;

CREATE TRIGGER import_history_protect_terminal_state
BEFORE UPDATE OF
    profile_id, import_key, source_uri, loader_name, attempt, status, state_message,
    created_at_ms, updated_at_ms, started_at_ms, completed_at_ms,
    sessions_created, sessions_updated, events_written,
    waveform_chunks_written, error_message
ON import_history
WHEN OLD.status IN ('completed', 'failed', 'cancelled')
BEGIN
    SELECT RAISE(ABORT, 'terminal import job cannot be changed');
END;
