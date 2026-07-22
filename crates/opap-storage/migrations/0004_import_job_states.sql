DROP TRIGGER IF EXISTS import_history_validate_machine_insert;
DROP TRIGGER IF EXISTS import_history_validate_machine_update;
DROP TRIGGER IF EXISTS import_history_protect_terminal_state;
DROP INDEX IF EXISTS imports_by_start;

ALTER TABLE import_history RENAME TO import_history_v3;

CREATE TABLE import_history (
    id                      INTEGER PRIMARY KEY,
    profile_id              INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    machine_id              INTEGER REFERENCES machines(id) ON DELETE SET NULL,
    import_key              TEXT NOT NULL CHECK (length(import_key) > 0),
    source_uri              TEXT NOT NULL CHECK (
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
    ),
    loader_name             TEXT NOT NULL CHECK (length(loader_name) > 0),
    attempt                 INTEGER NOT NULL CHECK (attempt > 0),
    retry_of_id             INTEGER UNIQUE REFERENCES import_history(id) ON DELETE SET NULL,
    status                  TEXT NOT NULL CHECK (
        status IN ('blocked', 'running', 'completed', 'failed', 'cancelled')
    ),
    state_message           TEXT,
    created_at_ms           INTEGER NOT NULL,
    updated_at_ms           INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    started_at_ms           INTEGER CHECK (started_at_ms IS NULL OR started_at_ms >= created_at_ms),
    completed_at_ms         INTEGER CHECK (
        completed_at_ms IS NULL OR completed_at_ms >= COALESCE(started_at_ms, created_at_ms)
    ),
    sessions_created        INTEGER NOT NULL DEFAULT 0 CHECK (sessions_created >= 0),
    sessions_updated        INTEGER NOT NULL DEFAULT 0 CHECK (sessions_updated >= 0),
    events_written          INTEGER NOT NULL DEFAULT 0 CHECK (events_written >= 0),
    waveform_chunks_written INTEGER NOT NULL DEFAULT 0 CHECK (waveform_chunks_written >= 0),
    error_message           TEXT,
    CHECK (
        (status IN ('blocked', 'running') AND completed_at_ms IS NULL)
        OR (status IN ('completed', 'failed', 'cancelled') AND completed_at_ms IS NOT NULL)
    ),
    CHECK (status NOT IN ('running', 'completed', 'failed') OR started_at_ms IS NOT NULL),
    CHECK (updated_at_ms >= COALESCE(completed_at_ms, started_at_ms, created_at_ms)),
    CHECK (
        (status = 'failed' AND error_message IS NOT NULL AND length(error_message) > 0)
        OR (status <> 'failed' AND error_message IS NULL)
    ),
    UNIQUE (profile_id, import_key, attempt)
) STRICT;

INSERT INTO import_history (
    id, profile_id, machine_id, import_key, source_uri, loader_name,
    attempt, retry_of_id, status, state_message, created_at_ms, updated_at_ms,
    started_at_ms, completed_at_ms, sessions_created, sessions_updated,
    events_written, waveform_chunks_written, error_message
)
SELECT
    id, profile_id, machine_id, import_key,
    CASE
        WHEN (
            length(source_uri) = 44
            AND substr(source_uri, 1, 12) = 'opap-source:'
            AND substr(source_uri, 13) NOT GLOB '*[^0-9a-f]*'
        ) OR (
            length(source_uri) BETWEEN 20 AND 38
            AND substr(source_uri, 1, 19) = 'opap-source:legacy-'
            AND substr(source_uri, 20, 1) BETWEEN '1' AND '9'
            AND substr(source_uri, 20) NOT GLOB '*[^0-9]*'
        ) THEN source_uri
        ELSE 'opap-source:legacy-' || CAST(id AS TEXT)
    END,
    loader_name,
    1, NULL,
    CASE
        WHEN status = 'in_progress' THEN 'running'
        WHEN status = 'failed' AND error_message = 'opap.service.cancelled.v1' THEN 'cancelled'
        ELSE status
    END,
    CASE
        WHEN status = 'failed' AND error_message = 'opap.service.cancelled.v1'
            THEN 'cancelled before import-job state migration'
        ELSE NULL
    END,
    started_at_ms,
    CASE
        WHEN status IN ('completed', 'failed')
            THEN max(started_at_ms, COALESCE(completed_at_ms, started_at_ms))
        ELSE started_at_ms
    END,
    started_at_ms,
    CASE
        WHEN status IN ('completed', 'failed')
            THEN max(started_at_ms, COALESCE(completed_at_ms, started_at_ms))
        ELSE NULL
    END,
    sessions_created, sessions_updated, events_written, waveform_chunks_written,
    CASE
        WHEN status = 'failed' AND error_message = 'opap.service.cancelled.v1' THEN NULL
        WHEN status = 'failed' AND (error_message IS NULL OR length(error_message) = 0)
            THEN 'legacy import failed without an error message'
        ELSE error_message
    END
FROM import_history_v3;

DROP TABLE import_history_v3;

CREATE INDEX imports_by_start
    ON import_history(profile_id, created_at_ms DESC, attempt DESC);

CREATE INDEX imports_by_logical_key
    ON import_history(profile_id, import_key, attempt DESC);

CREATE TRIGGER import_history_validate_machine_insert
BEFORE INSERT ON import_history
WHEN NEW.machine_id IS NOT NULL
 AND NOT EXISTS (
    SELECT 1 FROM machines
    WHERE id = NEW.machine_id AND profile_id = NEW.profile_id
 )
BEGIN
    SELECT RAISE(ABORT, 'import machine belongs to a different profile');
END;

CREATE TRIGGER import_history_validate_machine_update
BEFORE UPDATE OF profile_id, machine_id ON import_history
WHEN NEW.machine_id IS NOT NULL
 AND NOT EXISTS (
    SELECT 1 FROM machines
    WHERE id = NEW.machine_id AND profile_id = NEW.profile_id
 )
BEGIN
    SELECT RAISE(ABORT, 'import machine belongs to a different profile');
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
