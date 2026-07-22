CREATE TABLE profiles (
    id            INTEGER PRIMARY KEY,
    display_name  TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE machines (
    id               INTEGER PRIMARY KEY,
    profile_id       INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    source_key       TEXT NOT NULL CHECK (length(source_key) > 0),
    device_type      TEXT NOT NULL CHECK (length(device_type) > 0),
    manufacturer     TEXT NOT NULL,
    model            TEXT NOT NULL,
    model_number     TEXT NOT NULL,
    serial_number    TEXT NOT NULL,
    first_seen_at_ms INTEGER NOT NULL,
    last_seen_at_ms  INTEGER NOT NULL,
    UNIQUE (profile_id, source_key)
) STRICT;

CREATE TABLE sessions (
    id                      INTEGER PRIMARY KEY,
    machine_id              INTEGER NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    source_key              TEXT NOT NULL CHECK (length(source_key) > 0),
    started_at_ms           INTEGER NOT NULL,
    ended_at_ms             INTEGER,
    timezone_offset_minutes INTEGER,
    created_at_ms           INTEGER NOT NULL,
    updated_at_ms           INTEGER NOT NULL,
    CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms),
    UNIQUE (machine_id, source_key)
) STRICT;

CREATE TABLE events (
    id            INTEGER PRIMARY KEY,
    session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source_key    TEXT NOT NULL CHECK (length(source_key) > 0),
    channel_key   TEXT NOT NULL CHECK (length(channel_key) > 0),
    event_type    TEXT NOT NULL CHECK (length(event_type) > 0),
    starts_at_ms  INTEGER NOT NULL,
    duration_ms   INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    value         REAL,
    unit          TEXT,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (session_id, source_key)
) STRICT;

CREATE TABLE waveforms (
    id                 INTEGER PRIMARY KEY,
    session_id         INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source_key         TEXT NOT NULL CHECK (length(source_key) > 0),
    channel_key        TEXT NOT NULL CHECK (length(channel_key) > 0),
    unit               TEXT,
    started_at_ms      INTEGER NOT NULL,
    sample_interval_us INTEGER NOT NULL CHECK (sample_interval_us > 0),
    sample_count       INTEGER NOT NULL CHECK (sample_count >= 0),
    encoding           TEXT NOT NULL CHECK (length(encoding) > 0),
    min_value          REAL,
    max_value          REAL,
    created_at_ms      INTEGER NOT NULL,
    CHECK (min_value IS NULL OR max_value IS NULL OR min_value <= max_value),
    UNIQUE (session_id, source_key)
) STRICT;

CREATE TABLE waveform_chunks (
    waveform_id INTEGER NOT NULL REFERENCES waveforms(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    start_sample INTEGER NOT NULL CHECK (start_sample >= 0),
    sample_count INTEGER NOT NULL CHECK (sample_count >= 0),
    payload      BLOB NOT NULL,
    min_value    REAL,
    max_value    REAL,
    CHECK (min_value IS NULL OR max_value IS NULL OR min_value <= max_value),
    PRIMARY KEY (waveform_id, chunk_index)
) STRICT;

CREATE TABLE import_history (
    id                      INTEGER PRIMARY KEY,
    profile_id              INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    machine_id              INTEGER REFERENCES machines(id) ON DELETE SET NULL,
    import_key              TEXT NOT NULL CHECK (length(import_key) > 0),
    source_uri              TEXT NOT NULL,
    loader_name             TEXT NOT NULL CHECK (length(loader_name) > 0),
    status                  TEXT NOT NULL CHECK (status IN ('in_progress', 'completed', 'failed')),
    started_at_ms           INTEGER NOT NULL,
    completed_at_ms         INTEGER,
    sessions_created        INTEGER NOT NULL DEFAULT 0 CHECK (sessions_created >= 0),
    sessions_updated        INTEGER NOT NULL DEFAULT 0 CHECK (sessions_updated >= 0),
    events_written          INTEGER NOT NULL DEFAULT 0 CHECK (events_written >= 0),
    waveform_chunks_written INTEGER NOT NULL DEFAULT 0 CHECK (waveform_chunks_written >= 0),
    error_message           TEXT,
    UNIQUE (profile_id, import_key)
) STRICT;
