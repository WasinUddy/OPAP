DROP TRIGGER IF EXISTS import_history_validate_retry_time_insert;
DROP TRIGGER IF EXISTS import_history_protect_terminal_links;

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

-- SQLite removes a parent before running its ON DELETE SET NULL child action.
-- That lets this trigger distinguish FK cleanup from a direct unlink: cleanup
-- may clear a missing parent, while user-issued updates cannot rewrite history.
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
