CREATE INDEX sessions_by_start
    ON sessions(machine_id, started_at_ms DESC);

CREATE INDEX events_by_time
    ON events(session_id, starts_at_ms, channel_key);

CREATE INDEX waveforms_by_channel
    ON waveforms(session_id, channel_key, started_at_ms);

CREATE INDEX imports_by_start
    ON import_history(profile_id, started_at_ms DESC);
