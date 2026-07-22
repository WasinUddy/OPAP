-- Refuse to bless incompatible data written under the less restrictive v1 schema.
CREATE TABLE migration_0003_validation (
    valid INTEGER NOT NULL CHECK (valid = 1)
) STRICT;

INSERT INTO migration_0003_validation
SELECT CASE WHEN NOT EXISTS (
    SELECT 1
    FROM import_history AS history
    WHERE history.machine_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM machines AS machine
          WHERE machine.id = history.machine_id
            AND machine.profile_id = history.profile_id
      )
) THEN 1 ELSE 0 END;

INSERT INTO migration_0003_validation
SELECT CASE WHEN NOT EXISTS (
    SELECT 1 FROM waveforms
    WHERE encoding NOT IN (
        'i8', 'u8', 'i16-le', 'i16-be', 'i32-le', 'i32-be',
        'f32-le', 'f32-be', 'f64-le', 'f64-be'
    )
) THEN 1 ELSE 0 END;

INSERT INTO migration_0003_validation
SELECT CASE WHEN NOT EXISTS (
    SELECT 1
    FROM waveform_chunks AS chunk
    JOIN waveforms AS waveform ON waveform.id = chunk.waveform_id
    WHERE chunk.sample_count <= 0
       OR chunk.sample_count > waveform.sample_count
       OR chunk.start_sample > waveform.sample_count - chunk.sample_count
       OR length(chunk.payload) <> chunk.sample_count * CASE waveform.encoding
            WHEN 'i8' THEN 1 WHEN 'u8' THEN 1
            WHEN 'i16-le' THEN 2 WHEN 'i16-be' THEN 2
            WHEN 'i32-le' THEN 4 WHEN 'i32-be' THEN 4
            WHEN 'f32-le' THEN 4 WHEN 'f32-be' THEN 4
            WHEN 'f64-le' THEN 8 WHEN 'f64-be' THEN 8
          END
) THEN 1 ELSE 0 END;

INSERT INTO migration_0003_validation
SELECT CASE WHEN NOT EXISTS (
    SELECT 1
    FROM waveform_chunks AS first
    JOIN waveform_chunks AS second
      ON second.waveform_id = first.waveform_id
     AND second.chunk_index > first.chunk_index
     AND first.start_sample < second.start_sample + second.sample_count
     AND second.start_sample < first.start_sample + first.sample_count
) THEN 1 ELSE 0 END;

DROP TABLE migration_0003_validation;

CREATE TRIGGER waveforms_validate_encoding_insert
BEFORE INSERT ON waveforms
WHEN NEW.encoding NOT IN (
    'i8', 'u8', 'i16-le', 'i16-be', 'i32-le', 'i32-be',
    'f32-le', 'f32-be', 'f64-le', 'f64-be'
)
BEGIN
    SELECT RAISE(ABORT, 'unsupported waveform encoding');
END;

CREATE TRIGGER waveforms_validate_encoding_update
BEFORE UPDATE OF encoding ON waveforms
WHEN NEW.encoding NOT IN (
    'i8', 'u8', 'i16-le', 'i16-be', 'i32-le', 'i32-be',
    'f32-le', 'f32-be', 'f64-le', 'f64-be'
)
BEGIN
    SELECT RAISE(ABORT, 'unsupported waveform encoding');
END;

CREATE TRIGGER waveforms_protect_chunk_layout
BEFORE UPDATE OF sample_count, encoding ON waveforms
WHEN (NEW.sample_count <> OLD.sample_count OR NEW.encoding <> OLD.encoding)
 AND EXISTS (SELECT 1 FROM waveform_chunks WHERE waveform_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'delete waveform chunks before changing layout');
END;

CREATE TRIGGER waveform_chunks_validate_insert
BEFORE INSERT ON waveform_chunks
BEGIN
    SELECT CASE WHEN NEW.sample_count <= 0
        THEN RAISE(ABORT, 'waveform chunk must contain samples') END;
    SELECT CASE WHEN NEW.sample_count > (SELECT sample_count FROM waveforms WHERE id = NEW.waveform_id)
                      OR NEW.start_sample > (SELECT sample_count FROM waveforms WHERE id = NEW.waveform_id) - NEW.sample_count
        THEN RAISE(ABORT, 'waveform chunk exceeds metadata bounds') END;
    SELECT CASE WHEN length(NEW.payload) <> NEW.sample_count * CASE (SELECT encoding FROM waveforms WHERE id = NEW.waveform_id)
            WHEN 'i8' THEN 1 WHEN 'u8' THEN 1
            WHEN 'i16-le' THEN 2 WHEN 'i16-be' THEN 2
            WHEN 'i32-le' THEN 4 WHEN 'i32-be' THEN 4
            WHEN 'f32-le' THEN 4 WHEN 'f32-be' THEN 4
            WHEN 'f64-le' THEN 8 WHEN 'f64-be' THEN 8
          END
        THEN RAISE(ABORT, 'waveform chunk payload length mismatch') END;
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM waveform_chunks AS existing
        WHERE existing.waveform_id = NEW.waveform_id
          AND existing.chunk_index <> NEW.chunk_index
          AND NEW.start_sample < existing.start_sample + existing.sample_count
          AND existing.start_sample < NEW.start_sample + NEW.sample_count
    ) THEN RAISE(ABORT, 'overlapping waveform chunks') END;
END;

CREATE TRIGGER waveform_chunks_validate_update
BEFORE UPDATE ON waveform_chunks
BEGIN
    SELECT CASE WHEN NEW.sample_count <= 0
        THEN RAISE(ABORT, 'waveform chunk must contain samples') END;
    SELECT CASE WHEN NEW.sample_count > (SELECT sample_count FROM waveforms WHERE id = NEW.waveform_id)
                      OR NEW.start_sample > (SELECT sample_count FROM waveforms WHERE id = NEW.waveform_id) - NEW.sample_count
        THEN RAISE(ABORT, 'waveform chunk exceeds metadata bounds') END;
    SELECT CASE WHEN length(NEW.payload) <> NEW.sample_count * CASE (SELECT encoding FROM waveforms WHERE id = NEW.waveform_id)
            WHEN 'i8' THEN 1 WHEN 'u8' THEN 1
            WHEN 'i16-le' THEN 2 WHEN 'i16-be' THEN 2
            WHEN 'i32-le' THEN 4 WHEN 'i32-be' THEN 4
            WHEN 'f32-le' THEN 4 WHEN 'f32-be' THEN 4
            WHEN 'f64-le' THEN 8 WHEN 'f64-be' THEN 8
          END
        THEN RAISE(ABORT, 'waveform chunk payload length mismatch') END;
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM waveform_chunks AS existing
        WHERE existing.waveform_id = NEW.waveform_id
          AND NOT (
              existing.waveform_id = OLD.waveform_id
              AND existing.chunk_index = OLD.chunk_index
          )
          AND NEW.start_sample < existing.start_sample + existing.sample_count
          AND existing.start_sample < NEW.start_sample + NEW.sample_count
    ) THEN RAISE(ABORT, 'overlapping waveform chunks') END;
END;

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

CREATE TRIGGER import_history_protect_terminal_state
BEFORE UPDATE OF status ON import_history
WHEN OLD.status IN ('completed', 'failed') AND NEW.status <> OLD.status
BEGIN
    SELECT RAISE(ABORT, 'terminal import status cannot be changed');
END;
