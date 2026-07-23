CREATE TABLE session_provenance (
    session_id                INTEGER PRIMARY KEY
                                      REFERENCES sessions(id) ON DELETE CASCADE,
    therapy_day               TEXT NOT NULL CHECK (
        length(therapy_day) = 10
        AND therapy_day GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    start_local_wall          TEXT NOT NULL CHECK (
        length(start_local_wall) = 23
        AND start_local_wall GLOB
            '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9].[0-9][0-9][0-9]'
    ),
    end_local_wall            TEXT NOT NULL CHECK (
        length(end_local_wall) = 23
        AND end_local_wall GLOB
            '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9].[0-9][0-9][0-9]'
    ),
    start_utc_offset_seconds  INTEGER CHECK (
        start_utc_offset_seconds IS NULL
        OR start_utc_offset_seconds BETWEEN -64800 AND 64800
    ),
    end_utc_offset_seconds    INTEGER CHECK (
        end_utc_offset_seconds IS NULL
        OR end_utc_offset_seconds BETWEEN -64800 AND 64800
    ),
    start_clock_correction_ms INTEGER NOT NULL,
    end_clock_correction_ms   INTEGER NOT NULL,
    data_kind                 TEXT NOT NULL CHECK (
        data_kind IN ('detailed', 'summary_only', 'partial')
    ),
    importer_name             TEXT NOT NULL CHECK (
        length(CAST(importer_name AS BLOB)) BETWEEN 1 AND 128
        AND instr(importer_name, char(0)) = 0
    ),
    importer_schema           TEXT NOT NULL CHECK (
        length(CAST(importer_schema AS BLOB)) BETWEEN 1 AND 128
        AND instr(importer_schema, char(0)) = 0
    ),
    id_algorithm              TEXT NOT NULL CHECK (
        length(CAST(id_algorithm AS BLOB)) BETWEEN 1 AND 128
        AND instr(id_algorithm, char(0)) = 0
    ),
    source_digest             TEXT NOT NULL CHECK (
        length(CAST(source_digest AS BLOB)) = 64
        AND source_digest NOT GLOB '*[^0-9a-f]*'
    ),
    content_digest            TEXT NOT NULL CHECK (
        length(CAST(content_digest AS BLOB)) = 64
        AND content_digest NOT GLOB '*[^0-9a-f]*'
    )
) STRICT;

CREATE TABLE session_slices (
    session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence      INTEGER NOT NULL CHECK (sequence >= 0),
    source_key    TEXT NOT NULL CHECK (
        length(CAST(source_key AS BLOB)) BETWEEN 1 AND 256
        AND instr(source_key, char(0)) = 0
    ),
    state         TEXT NOT NULL CHECK (
        state IN ('mask_on', 'mask_off', 'equipment_off')
    ),
    started_at_ms INTEGER NOT NULL,
    ended_at_ms   INTEGER NOT NULL CHECK (ended_at_ms > started_at_ms),
    PRIMARY KEY (session_id, sequence),
    UNIQUE (session_id, source_key)
) STRICT;

CREATE INDEX session_slices_by_time
    ON session_slices(session_id, started_at_ms, sequence);

CREATE TABLE session_summary (
    session_id INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    usage_ms   INTEGER NOT NULL CHECK (usage_ms >= 0)
) STRICT;

CREATE TABLE summary_metrics (
    session_id INTEGER NOT NULL
                       REFERENCES session_summary(session_id) ON DELETE CASCADE,
    metric_key TEXT NOT NULL CHECK (
        length(CAST(metric_key AS BLOB)) BETWEEN 1 AND 256
        AND instr(metric_key, char(0)) = 0
    ),
    value      REAL NOT NULL,
    unit       TEXT CHECK (unit IS NULL OR instr(unit, char(0)) = 0),
    PRIMARY KEY (session_id, metric_key)
) STRICT;

CREATE TABLE session_settings (
    session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    setting_key   TEXT NOT NULL CHECK (
        length(CAST(setting_key AS BLOB)) BETWEEN 1 AND 256
        AND instr(setting_key, char(0)) = 0
    ),
    value_kind    TEXT NOT NULL CHECK (
        value_kind IN ('integer', 'real', 'text', 'boolean')
    ),
    integer_value INTEGER,
    real_value    REAL,
    text_value    TEXT,
    boolean_value INTEGER CHECK (boolean_value IS NULL OR boolean_value IN (0, 1)),
    unit          TEXT CHECK (unit IS NULL OR instr(unit, char(0)) = 0),
    origin        TEXT NOT NULL CHECK (
        length(CAST(origin AS BLOB)) BETWEEN 1 AND 256
        AND instr(origin, char(0)) = 0
    ),
    CHECK (
        (
            value_kind = 'integer'
            AND integer_value IS NOT NULL
            AND real_value IS NULL
            AND text_value IS NULL
            AND boolean_value IS NULL
        )
        OR (
            value_kind = 'real'
            AND integer_value IS NULL
            AND real_value IS NOT NULL
            AND text_value IS NULL
            AND boolean_value IS NULL
        )
        OR (
            value_kind = 'text'
            AND integer_value IS NULL
            AND real_value IS NULL
            AND text_value IS NOT NULL
            AND boolean_value IS NULL
        )
        OR (
            value_kind = 'boolean'
            AND integer_value IS NULL
            AND real_value IS NULL
            AND text_value IS NULL
            AND boolean_value IS NOT NULL
        )
    ),
    PRIMARY KEY (session_id, setting_key)
) STRICT;
