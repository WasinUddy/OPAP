-- Pre-v7 callers controlled import_key and could persist a device serial,
-- filesystem path, or other identifying text. A rebuild avoids transient
-- UNIQUE collisions while every historical logical-key group is redacted.
DROP TABLE IF EXISTS temp.opap_v7_retry_lineage_validation;
CREATE TEMP TABLE opap_v7_retry_lineage_validation (
    valid INTEGER NOT NULL CHECK (valid = 1)
) STRICT;

INSERT INTO opap_v7_retry_lineage_validation (valid)
SELECT CASE WHEN EXISTS (
    SELECT 1
    FROM import_history AS child
    LEFT JOIN import_history AS parent ON parent.id = child.retry_of_id
    WHERE child.retry_of_id IS NOT NULL
      AND (
          parent.id IS NULL
          OR parent.status NOT IN ('failed', 'cancelled')
          OR child.profile_id <> parent.profile_id
          OR child.import_key <> parent.import_key
          OR parent.attempt = 9223372036854775807
          OR child.attempt <> parent.attempt + 1
          OR child.created_at_ms < max(
              parent.updated_at_ms,
              COALESCE(parent.completed_at_ms, parent.updated_at_ms)
          )
      )
) THEN 0 ELSE 1 END;

DROP TABLE temp.opap_v7_retry_lineage_validation;

DROP TRIGGER IF EXISTS import_history_validate_machine_insert;
DROP TRIGGER IF EXISTS import_history_validate_machine_update;
DROP TRIGGER IF EXISTS import_history_validate_source_insert;
DROP TRIGGER IF EXISTS import_history_validate_source_update;
DROP TRIGGER IF EXISTS import_history_validate_state_transition;
DROP TRIGGER IF EXISTS import_history_monotonic_update_time;
DROP TRIGGER IF EXISTS import_history_protect_terminal_state;
DROP TRIGGER IF EXISTS import_history_validate_retry_time_insert;
DROP TRIGGER IF EXISTS import_history_protect_terminal_links;
DROP TRIGGER IF EXISTS import_history_validate_request_key_insert;
DROP TRIGGER IF EXISTS import_history_validate_request_key_update;
DROP TRIGGER IF EXISTS import_history_protect_import_identity;
DROP INDEX IF EXISTS imports_by_start;
DROP INDEX IF EXISTS imports_by_logical_key;

ALTER TABLE import_history RENAME TO import_history_v6;

CREATE TABLE import_history (
    id                      INTEGER PRIMARY KEY,
    profile_id              INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    machine_id              INTEGER REFERENCES machines(id) ON DELETE SET NULL,
    import_key              TEXT NOT NULL CHECK (
        length(import_key) = length(CAST(import_key AS BLOB))
        AND (
          (
            length(import_key) = 45
            AND substr(import_key, 1, 13) = 'opap-request:'
            AND substr(import_key, 14) NOT GLOB '*[^0-9a-f]*'
          )
          OR (
            length(import_key) BETWEEN 21 AND 39
            AND substr(import_key, 1, 20) = 'opap-request:legacy-'
            AND substr(import_key, 21, 1) BETWEEN '1' AND '9'
            AND substr(import_key, 21) NOT GLOB '*[^0-9]*'
            AND (
                length(import_key) < 39
                OR (substr(import_key, 21) COLLATE BINARY) <= '9223372036854775807'
            )
          )
        )
    ),
    source_uri              TEXT NOT NULL CHECK (
        length(source_uri) = length(CAST(source_uri AS BLOB))
        AND (
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

-- Preserve logical attempt grouping by anchoring each old profile/key group to
-- its smallest row id. All caller text is discarded, including keys that
-- happen to resemble the new canonical format.
PRAGMA defer_foreign_keys = ON;
INSERT INTO import_history (
    id, profile_id, machine_id, import_key, source_uri, loader_name,
    attempt, retry_of_id, status, state_message, created_at_ms, updated_at_ms,
    started_at_ms, completed_at_ms, sessions_created, sessions_updated,
    events_written, waveform_chunks_written, error_message
)
SELECT
    history.id,
    history.profile_id,
    history.machine_id,
    'opap-request:legacy-' || CAST(anchor.anchor_id AS TEXT),
    CASE
        WHEN length(history.source_uri) = length(CAST(history.source_uri AS BLOB))
         AND (
            (
                length(history.source_uri) = 44
                AND substr(history.source_uri, 1, 12) = 'opap-source:'
                AND substr(history.source_uri, 13) NOT GLOB '*[^0-9a-f]*'
            )
            OR (
                length(history.source_uri) BETWEEN 20 AND 38
                AND substr(history.source_uri, 1, 19) = 'opap-source:legacy-'
                AND substr(history.source_uri, 20, 1) BETWEEN '1' AND '9'
                AND substr(history.source_uri, 20) NOT GLOB '*[^0-9]*'
            )
         ) THEN history.source_uri
        ELSE 'opap-source:legacy-' || CAST(history.id AS TEXT)
    END,
    history.loader_name,
    history.attempt,
    history.retry_of_id,
    history.status,
    history.state_message,
    history.created_at_ms,
    history.updated_at_ms,
    history.started_at_ms,
    history.completed_at_ms,
    history.sessions_created,
    history.sessions_updated,
    history.events_written,
    history.waveform_chunks_written,
    history.error_message
FROM import_history_v6 AS history
JOIN (
    SELECT profile_id, import_key, MIN(id) AS anchor_id
    FROM import_history_v6
    GROUP BY profile_id, import_key
) AS anchor
  ON anchor.profile_id = history.profile_id
 AND anchor.import_key = history.import_key;

DROP TABLE import_history_v6;

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

CREATE TRIGGER import_history_validate_source_insert
BEFORE INSERT ON import_history
WHEN NOT (
    length(NEW.source_uri) = length(CAST(NEW.source_uri AS BLOB))
    AND (
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
)
BEGIN
    SELECT RAISE(ABORT, 'import source must be an opaque OPAP source identifier');
END;

CREATE TRIGGER import_history_validate_source_update
BEFORE UPDATE OF source_uri ON import_history
WHEN NOT (
    length(NEW.source_uri) = length(CAST(NEW.source_uri AS BLOB))
    AND (
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
)
BEGIN
    SELECT RAISE(ABORT, 'import source must be an opaque OPAP source identifier');
END;

CREATE TRIGGER import_history_validate_request_key_insert
BEFORE INSERT ON import_history
WHEN NOT (
    length(NEW.import_key) = length(CAST(NEW.import_key AS BLOB))
    AND (
        (
            length(NEW.import_key) = 45
            AND substr(NEW.import_key, 1, 13) = 'opap-request:'
            AND substr(NEW.import_key, 14) NOT GLOB '*[^0-9a-f]*'
            AND NEW.retry_of_id IS NULL
            AND NEW.attempt = 1
        )
        OR (
            (
                (
                    length(NEW.import_key) = 45
                    AND substr(NEW.import_key, 1, 13) = 'opap-request:'
                    AND substr(NEW.import_key, 14) NOT GLOB '*[^0-9a-f]*'
                )
                OR (
                    length(NEW.import_key) BETWEEN 21 AND 39
                    AND substr(NEW.import_key, 1, 20) = 'opap-request:legacy-'
                    AND substr(NEW.import_key, 21, 1) BETWEEN '1' AND '9'
                    AND substr(NEW.import_key, 21) NOT GLOB '*[^0-9]*'
                    AND (
                        length(NEW.import_key) < 39
                        OR (substr(NEW.import_key, 21) COLLATE BINARY)
                            <= '9223372036854775807'
                    )
                )
            )
            AND NEW.retry_of_id IS NOT NULL
            AND EXISTS (
                SELECT 1 FROM import_history AS parent_import
                WHERE parent_import.id = NEW.retry_of_id
                  AND parent_import.profile_id = NEW.profile_id
                  AND parent_import.import_key = NEW.import_key
                  AND parent_import.status IN ('failed', 'cancelled')
                  AND NEW.attempt = parent_import.attempt + 1
            )
        )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'import key must be an opaque OPAP request identifier');
END;

CREATE TRIGGER import_history_validate_request_key_update
BEFORE UPDATE OF import_key ON import_history
WHEN NEW.import_key IS NOT OLD.import_key
BEGIN
    SELECT RAISE(ABORT, 'import key cannot be changed');
END;

CREATE TRIGGER import_history_protect_import_identity
BEFORE UPDATE OF id, profile_id, import_key, attempt, retry_of_id ON import_history
WHEN NEW.id IS NOT OLD.id
 OR NEW.profile_id IS NOT OLD.profile_id
 OR NEW.import_key IS NOT OLD.import_key
 OR NEW.attempt IS NOT OLD.attempt
 OR (
    NEW.retry_of_id IS NOT OLD.retry_of_id
    AND NOT (
        OLD.retry_of_id IS NOT NULL
        AND NEW.retry_of_id IS NULL
        AND NOT EXISTS (
            SELECT 1 FROM import_history AS parent_import
            WHERE parent_import.id = OLD.retry_of_id
        )
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'import job identity cannot be changed');
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

CREATE TRIGGER import_history_validate_retry_time_insert
BEFORE INSERT ON import_history
WHEN NEW.retry_of_id IS NOT NULL
 AND EXISTS (
    SELECT 1 FROM import_history AS parent
    WHERE parent.id = NEW.retry_of_id
      AND NEW.created_at_ms < max(
          parent.updated_at_ms,
          COALESCE(parent.completed_at_ms, parent.updated_at_ms)
      )
 )
BEGIN
    SELECT RAISE(ABORT, 'retry timestamp cannot precede its parent attempt');
END;

CREATE TRIGGER import_history_protect_terminal_links
BEFORE UPDATE OF machine_id, retry_of_id ON import_history
WHEN OLD.status IN ('completed', 'failed', 'cancelled')
 AND (
    (
        NEW.machine_id IS NOT OLD.machine_id
        AND NOT (
            OLD.machine_id IS NOT NULL
            AND NEW.machine_id IS NULL
            AND NOT EXISTS (SELECT 1 FROM machines WHERE id = OLD.machine_id)
        )
    )
    OR (
        NEW.retry_of_id IS NOT OLD.retry_of_id
        AND NOT (
            OLD.retry_of_id IS NOT NULL
            AND NEW.retry_of_id IS NULL
            AND NOT EXISTS (
                SELECT 1 FROM import_history AS parent_import
                WHERE parent_import.id = OLD.retry_of_id
            )
        )
    )
 )
BEGIN
    SELECT RAISE(ABORT, 'terminal import job links cannot be changed');
END;
