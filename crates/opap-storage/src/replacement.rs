use crate::repository::{Events, SessionSnapshots, Sessions, Waveforms};
use crate::{
    Database, Error, NewEvent, NewSession, NewWaveformChunk, NewWaveformMetadata, Result,
    SessionDataReplacement, SessionReplacementResult, SessionReplacementStats, SessionSettingInput,
    SessionSnapshotReplacement, SessionSnapshotReplacementResult, SessionSnapshotReplacementStats,
};
use rusqlite::Connection;
use std::collections::HashSet;

impl Database {
    /// Atomically replaces the authoritative derived data for one session.
    /// Existing records absent from `replacement` are pruned, including chunks
    /// cascading from a removed waveform. Any validation or database failure
    /// rolls the entire replacement back.
    #[deprecated(
        note = "use replace_session so session metadata and derived data share one transaction"
    )]
    pub fn replace_session_data(
        &mut self,
        session_id: i64,
        replacement: &SessionDataReplacement<'_>,
    ) -> Result<SessionReplacementStats> {
        validate_replacement(replacement)?;
        let transaction = self.transaction()?;
        let stats = replace_session_data_on(&transaction, session_id, replacement)?;
        transaction.commit()?;
        Ok(stats)
    }

    /// Atomically upserts authoritative session metadata and replaces every
    /// event, waveform, and chunk owned by that session. This is the import path
    /// to use when a parser produces a complete session snapshot.
    pub fn replace_session(
        &mut self,
        session: &NewSession<'_>,
        replacement: &SessionDataReplacement<'_>,
    ) -> Result<SessionReplacementResult> {
        validate_replacement(replacement)?;
        let transaction = self.transaction()?;
        let session = Sessions::new(&transaction).upsert(session)?;
        let stats = replace_session_data_on(&transaction, session.id, replacement)?;
        transaction.commit()?;
        Ok(SessionReplacementResult { session, stats })
    }

    /// Atomically upserts session metadata and authoritatively replaces its
    /// complete v8 snapshot: events, waveforms/chunks, provenance, slices,
    /// summary/metrics, and typed settings. Validation is completed before the
    /// immediate transaction begins; any later database failure rolls back all
    /// old and new child state together.
    pub fn replace_session_snapshot(
        &mut self,
        session: &NewSession<'_>,
        replacement: &SessionSnapshotReplacement<'_>,
    ) -> Result<SessionSnapshotReplacementResult> {
        validate_session_snapshot(session, replacement)?;
        let transaction = self.transaction()?;
        let result = replace_session_snapshot_on(&transaction, session, replacement)?;
        transaction.commit()?;
        Ok(result)
    }
}

pub(crate) fn replace_session_snapshot_on(
    connection: &Connection,
    session: &NewSession<'_>,
    replacement: &SessionSnapshotReplacement<'_>,
) -> Result<SessionSnapshotReplacementResult> {
    let session = Sessions::new(connection).upsert(session)?;
    let session_data = replace_session_data_on(connection, session.id, &replacement.data)?;
    let children = SessionSnapshots::new(connection).replace(session.id, replacement)?;
    Ok(SessionSnapshotReplacementResult {
        session,
        stats: SessionSnapshotReplacementStats {
            session_data,
            slices_written: children.slices_written,
            slices_pruned: children.slices_pruned,
            summary_metrics_written: children.summary_metrics_written,
            summary_metrics_pruned: children.summary_metrics_pruned,
            settings_written: children.settings_written,
            settings_pruned: children.settings_pruned,
        },
    })
}

pub(crate) fn replace_session_snapshot_on_with_checkpoint(
    connection: &Connection,
    session: &NewSession<'_>,
    replacement: &SessionSnapshotReplacement<'_>,
    checkpoint: &mut dyn FnMut() -> Result<()>,
) -> Result<SessionSnapshotReplacementResult> {
    checkpoint()?;
    let session = Sessions::new(connection).upsert(session)?;
    checkpoint()?;
    let session_data = replace_session_data_on_with_checkpoint(
        connection,
        session.id,
        &replacement.data,
        checkpoint,
    )?;
    checkpoint()?;
    let children = SessionSnapshots::new(connection).replace_with_checkpoint(
        session.id,
        replacement,
        checkpoint,
    )?;
    checkpoint()?;
    Ok(SessionSnapshotReplacementResult {
        session,
        stats: SessionSnapshotReplacementStats {
            session_data,
            slices_written: children.slices_written,
            slices_pruned: children.slices_pruned,
            summary_metrics_written: children.summary_metrics_written,
            summary_metrics_pruned: children.summary_metrics_pruned,
            settings_written: children.settings_written,
            settings_pruned: children.settings_pruned,
        },
    })
}

fn replace_session_data_on(
    connection: &Connection,
    session_id: i64,
    replacement: &SessionDataReplacement<'_>,
) -> Result<SessionReplacementStats> {
    let mut checkpoint = || Ok(());
    replace_session_data_on_with_checkpoint(connection, session_id, replacement, &mut checkpoint)
}

fn replace_session_data_on_with_checkpoint(
    connection: &Connection,
    session_id: i64,
    replacement: &SessionDataReplacement<'_>,
    checkpoint: &mut dyn FnMut() -> Result<()>,
) -> Result<SessionReplacementStats> {
    if Sessions::new(connection).get(session_id)?.is_none() {
        return Err(Error::Integrity(format!(
            "cannot replace data for missing session {session_id}"
        )));
    }

    let existing_events = Events::new(connection).list_by_session(session_id)?;
    let event_keys = replacement
        .events
        .iter()
        .map(|event| event.source_key)
        .collect::<HashSet<_>>();
    for event in replacement.events {
        checkpoint()?;
        Events::new(connection).upsert(&NewEvent {
            session_id,
            source_key: event.source_key,
            channel_key: event.channel_key,
            event_type: event.event_type,
            starts_at_ms: event.starts_at_ms,
            duration_ms: event.duration_ms,
            value: event.value,
            unit: event.unit,
            created_at_ms: event.created_at_ms,
        })?;
    }
    let stale_events = existing_events
        .iter()
        .filter(|event| !event_keys.contains(event.source_key.as_str()))
        .collect::<Vec<_>>();
    for event in &stale_events {
        checkpoint()?;
        if !Events::new(connection).delete(event.id)? {
            return Err(Error::Integrity(
                "authoritative event deletion affected no row".to_owned(),
            ));
        }
    }

    let existing_waveforms = Waveforms::new(connection).list_metadata_by_session(session_id)?;
    let waveform_keys = replacement
        .waveforms
        .iter()
        .map(|waveform| waveform.source_key)
        .collect::<HashSet<_>>();
    let mut chunks_written = 0;
    for waveform in replacement.waveforms {
        checkpoint()?;
        let repository = Waveforms::new(connection);
        if let Some(existing) =
            repository.find_metadata_by_source_key(session_id, waveform.source_key)?
        {
            let expected_chunks = repository.list_chunks(existing.id)?.len();
            let deleted_chunks = repository.delete_chunks(existing.id)?;
            if deleted_chunks != expected_chunks {
                return Err(Error::Integrity(format!(
                    "waveform chunk deletion affected {deleted_chunks} rows; expected {expected_chunks}"
                )));
            }
        }
        let metadata = repository.upsert_metadata(&NewWaveformMetadata {
            session_id,
            source_key: waveform.source_key,
            channel_key: waveform.channel_key,
            unit: waveform.unit,
            started_at_ms: waveform.started_at_ms,
            sample_interval_us: waveform.sample_interval_us,
            sample_count: waveform.sample_count,
            encoding: waveform.encoding,
            min_value: waveform.min_value,
            max_value: waveform.max_value,
            created_at_ms: waveform.created_at_ms,
        })?;
        for chunk in waveform.chunks {
            checkpoint()?;
            repository.upsert_chunk(&NewWaveformChunk {
                waveform_id: metadata.id,
                chunk_index: chunk.chunk_index,
                start_sample: chunk.start_sample,
                sample_count: chunk.sample_count,
                payload: chunk.payload,
                min_value: chunk.min_value,
                max_value: chunk.max_value,
            })?;
            chunks_written += 1;
        }
        repository.validate_complete(metadata.id)?;
    }
    let stale_waveforms = existing_waveforms
        .iter()
        .filter(|waveform| !waveform_keys.contains(waveform.source_key.as_str()))
        .collect::<Vec<_>>();
    for waveform in &stale_waveforms {
        checkpoint()?;
        if !Waveforms::new(connection).delete_metadata(waveform.id)? {
            return Err(Error::Integrity(
                "authoritative waveform deletion affected no row".to_owned(),
            ));
        }
    }
    checkpoint()?;

    let stats = SessionReplacementStats {
        events_written: replacement.events.len(),
        events_pruned: stale_events.len(),
        waveforms_written: replacement.waveforms.len(),
        waveforms_pruned: stale_waveforms.len(),
        waveform_chunks_written: chunks_written,
    };
    Ok(stats)
}

fn validate_replacement(replacement: &SessionDataReplacement<'_>) -> Result<()> {
    let mut event_keys = HashSet::new();
    for event in replacement.events {
        if !event_keys.insert(event.source_key) {
            return Err(Error::Integrity(format!(
                "duplicate event source key {:?} in session replacement",
                event.source_key
            )));
        }
    }

    let mut waveform_keys = HashSet::new();
    for waveform in replacement.waveforms {
        if !waveform_keys.insert(waveform.source_key) {
            return Err(Error::Integrity(format!(
                "duplicate waveform source key {:?} in session replacement",
                waveform.source_key
            )));
        }
        let mut chunk_indices = HashSet::new();
        for chunk in waveform.chunks {
            if !chunk_indices.insert(chunk.chunk_index) {
                return Err(Error::Integrity(format!(
                    "duplicate chunk index {} for waveform {:?}",
                    chunk.chunk_index, waveform.source_key
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_session_snapshot(
    session: &NewSession<'_>,
    replacement: &SessionSnapshotReplacement<'_>,
) -> Result<()> {
    validate_replacement(&replacement.data)?;
    let ended_at_ms = session.ended_at_ms.ok_or_else(|| {
        Error::Integrity("session snapshot requires a known session end".to_owned())
    })?;
    let duration_ms = ended_at_ms
        .checked_sub(session.started_at_ms)
        .filter(|duration| *duration > 0)
        .ok_or_else(|| {
            Error::Integrity("session snapshot end must be later than its start".to_owned())
        })?;

    validate_provenance(session, replacement)?;
    validate_events(session.started_at_ms, ended_at_ms, &replacement.data)?;
    validate_waveforms(session.started_at_ms, ended_at_ms, &replacement.data)?;
    validate_slices(session.started_at_ms, ended_at_ms, replacement)?;
    validate_summary(duration_ms, replacement)?;
    validate_settings(replacement.settings)?;
    Ok(())
}

fn validate_provenance(
    session: &NewSession<'_>,
    replacement: &SessionSnapshotReplacement<'_>,
) -> Result<()> {
    let provenance = replacement.provenance;
    parse_date(provenance.therapy_day).ok_or_else(|| {
        Error::Integrity("session therapy day must be a valid YYYY-MM-DD date".to_owned())
    })?;
    let start_local_ms = parse_local_epoch_ms(provenance.start_local_wall).ok_or_else(|| {
        Error::Integrity(
            "session start local wall time must be a valid YYYY-MM-DDTHH:MM:SS.mmm value"
                .to_owned(),
        )
    })?;
    let end_local_ms = parse_local_epoch_ms(provenance.end_local_wall).ok_or_else(|| {
        Error::Integrity(
            "session end local wall time must be a valid YYYY-MM-DDTHH:MM:SS.mmm value".to_owned(),
        )
    })?;
    validate_offset(
        "start",
        provenance.start_utc_offset_seconds,
        start_local_ms,
        provenance.start_clock_correction_ms,
        session.started_at_ms,
    )?;
    validate_offset(
        "end",
        provenance.end_utc_offset_seconds,
        end_local_ms,
        provenance.end_clock_correction_ms,
        session.ended_at_ms.expect("validated session end"),
    )?;
    for (field, value, maximum) in [
        ("importer name", provenance.importer_name, 128),
        ("importer schema", provenance.importer_schema, 128),
        ("session id algorithm", provenance.id_algorithm, 128),
    ] {
        validate_text(field, value, maximum)?;
    }
    for (field, digest) in [
        ("session source digest", provenance.source_digest),
        ("session content digest", provenance.content_digest),
    ] {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(Error::Integrity(format!(
                "{field} must be exactly 64 lowercase hexadecimal characters"
            )));
        }
    }
    Ok(())
}

fn validate_offset(
    boundary: &str,
    offset_seconds: Option<i32>,
    local_epoch_ms: i64,
    correction_ms: i64,
    normalized_utc_ms: i64,
) -> Result<()> {
    let Some(offset_seconds) = offset_seconds else {
        return Ok(());
    };
    if !(-64_800..=64_800).contains(&offset_seconds) {
        return Err(Error::Integrity(format!(
            "session {boundary} UTC offset must be between -64800 and 64800 seconds"
        )));
    }
    let interpreted = local_epoch_ms
        .checked_sub(i64::from(offset_seconds) * 1_000)
        .and_then(|value| value.checked_add(correction_ms))
        .ok_or_else(|| {
            Error::Integrity(format!(
                "session {boundary} local-time normalization overflow"
            ))
        })?;
    if interpreted != normalized_utc_ms {
        return Err(Error::Integrity(format!(
            "session {boundary} local wall time, offset, and clock correction do not match its normalized UTC boundary"
        )));
    }
    Ok(())
}

fn validate_events(
    session_start_ms: i64,
    session_end_ms: i64,
    replacement: &SessionDataReplacement<'_>,
) -> Result<()> {
    for event in replacement.events {
        validate_text("event source key", event.source_key, 256)?;
        validate_text("event channel key", event.channel_key, 256)?;
        validate_text("event type", event.event_type, 256)?;
        validate_optional_text("event unit", event.unit, 256)?;
        if event.value.is_some_and(|value| !value.is_finite()) {
            return Err(Error::Integrity(format!(
                "event {:?} has a non-finite value",
                event.source_key
            )));
        }
        let duration_ms = event.duration_ms.unwrap_or(0);
        if duration_ms < 0 {
            return Err(Error::Integrity(format!(
                "event {:?} has a negative duration",
                event.source_key
            )));
        }
        let event_end_ms = event
            .starts_at_ms
            .checked_add(duration_ms)
            .ok_or_else(|| Error::Integrity("event time range overflow".to_owned()))?;
        if event.starts_at_ms < session_start_ms || event_end_ms > session_end_ms {
            return Err(Error::Integrity(format!(
                "event {:?} lies outside the session bounds",
                event.source_key
            )));
        }
    }
    Ok(())
}

fn validate_waveforms(
    session_start_ms: i64,
    session_end_ms: i64,
    replacement: &SessionDataReplacement<'_>,
) -> Result<()> {
    let session_end_us = session_end_ms
        .checked_mul(1_000)
        .ok_or_else(|| Error::Integrity("session microsecond boundary overflow".to_owned()))?;
    for waveform in replacement.waveforms {
        validate_text("waveform source key", waveform.source_key, 256)?;
        validate_text("waveform channel key", waveform.channel_key, 256)?;
        validate_optional_text("waveform unit", waveform.unit, 256)?;
        if waveform.sample_interval_us <= 0 || waveform.sample_count < 0 {
            return Err(Error::Integrity(format!(
                "waveform {:?} has an invalid sample interval or count",
                waveform.source_key
            )));
        }
        validate_finite_range(
            "waveform",
            waveform.source_key,
            waveform.min_value,
            waveform.max_value,
        )?;
        if waveform.started_at_ms < session_start_ms || waveform.started_at_ms > session_end_ms {
            return Err(Error::Integrity(format!(
                "waveform {:?} starts outside the session bounds",
                waveform.source_key
            )));
        }
        if waveform.sample_count > 0 {
            let last_sample_offset_us = (waveform.sample_count - 1)
                .checked_mul(waveform.sample_interval_us)
                .ok_or_else(|| Error::Integrity("waveform time range overflow".to_owned()))?;
            let last_sample_us = waveform
                .started_at_ms
                .checked_mul(1_000)
                .and_then(|value| value.checked_add(last_sample_offset_us))
                .ok_or_else(|| Error::Integrity("waveform time range overflow".to_owned()))?;
            if last_sample_us > session_end_us {
                return Err(Error::Integrity(format!(
                    "waveform {:?} extends beyond the session end",
                    waveform.source_key
                )));
            }
        }
        let width = bytes_per_sample(waveform.encoding).ok_or_else(|| {
            Error::Integrity(format!(
                "unsupported waveform encoding {:?}",
                waveform.encoding
            ))
        })?;
        let mut next_sample = 0_i64;
        for (expected_index, chunk) in waveform.chunks.iter().enumerate() {
            if chunk.chunk_index != expected_index as i64 || chunk.start_sample != next_sample {
                return Err(Error::Integrity(format!(
                    "waveform {:?} chunks must have contiguous indices and sample ranges",
                    waveform.source_key
                )));
            }
            if chunk.sample_count <= 0 {
                return Err(Error::Integrity(format!(
                    "waveform {:?} chunk {} must contain samples",
                    waveform.source_key, chunk.chunk_index
                )));
            }
            validate_finite_range(
                "waveform chunk",
                waveform.source_key,
                chunk.min_value,
                chunk.max_value,
            )?;
            let sample_count = usize::try_from(chunk.sample_count).map_err(|_| {
                Error::Integrity("waveform chunk sample count is too large".to_owned())
            })?;
            let expected_bytes = sample_count
                .checked_mul(width)
                .ok_or_else(|| Error::Integrity("waveform payload length overflow".to_owned()))?;
            if chunk.payload.len() != expected_bytes {
                return Err(Error::Integrity(format!(
                    "waveform {:?} chunk {} payload length does not match its sample count",
                    waveform.source_key, chunk.chunk_index
                )));
            }
            next_sample = next_sample
                .checked_add(chunk.sample_count)
                .ok_or_else(|| Error::Integrity("waveform sample range overflow".to_owned()))?;
        }
        if next_sample != waveform.sample_count {
            return Err(Error::Integrity(format!(
                "waveform {:?} chunks do not cover its declared sample count",
                waveform.source_key
            )));
        }
    }
    Ok(())
}

fn validate_slices(
    session_start_ms: i64,
    session_end_ms: i64,
    replacement: &SessionSnapshotReplacement<'_>,
) -> Result<()> {
    let mut source_keys = HashSet::new();
    let mut previous_end = None;
    for (expected_sequence, slice) in replacement.slices.iter().enumerate() {
        validate_text("session slice source key", slice.source_key, 256)?;
        if slice.sequence != expected_sequence as i64 {
            return Err(Error::Integrity(
                "session slice sequences must be contiguous and start at zero".to_owned(),
            ));
        }
        if !source_keys.insert(slice.source_key) {
            return Err(Error::Integrity(format!(
                "duplicate session slice source key {:?}",
                slice.source_key
            )));
        }
        if slice.started_at_ms < session_start_ms
            || slice.ended_at_ms > session_end_ms
            || slice.ended_at_ms <= slice.started_at_ms
        {
            return Err(Error::Integrity(format!(
                "session slice {:?} has invalid session bounds",
                slice.source_key
            )));
        }
        if previous_end.is_some_and(|end| slice.started_at_ms < end) {
            return Err(Error::Integrity(format!(
                "session slice {:?} overlaps the previous slice",
                slice.source_key
            )));
        }
        previous_end = Some(slice.ended_at_ms);
    }
    Ok(())
}

fn validate_summary(
    session_duration_ms: i64,
    replacement: &SessionSnapshotReplacement<'_>,
) -> Result<()> {
    if replacement.summary.usage_ms < 0 || replacement.summary.usage_ms > session_duration_ms {
        return Err(Error::Integrity(
            "session usage must be non-negative and no greater than its duration".to_owned(),
        ));
    }
    let mut keys = HashSet::new();
    for metric in replacement.summary.metrics {
        validate_text("summary metric key", metric.key, 256)?;
        validate_optional_text("summary metric unit", metric.unit, 256)?;
        if !keys.insert(metric.key) {
            return Err(Error::Integrity(format!(
                "duplicate summary metric key {:?}",
                metric.key
            )));
        }
        if !metric.value.is_finite() {
            return Err(Error::Integrity(format!(
                "summary metric {:?} has a non-finite value",
                metric.key
            )));
        }
    }
    Ok(())
}

fn validate_settings(settings: &[SessionSettingInput<'_>]) -> Result<()> {
    let mut keys = HashSet::new();
    for setting in settings {
        validate_text("session setting key", setting.key, 256)?;
        validate_text("session setting origin", setting.origin, 256)?;
        validate_optional_text("session setting unit", setting.unit, 256)?;
        if !keys.insert(setting.key) {
            return Err(Error::Integrity(format!(
                "duplicate session setting key {:?}",
                setting.key
            )));
        }
        let populated = usize::from(setting.integer_value.is_some())
            + usize::from(setting.real_value.is_some())
            + usize::from(setting.text_value.is_some())
            + usize::from(setting.boolean_value.is_some());
        if populated != 1 {
            return Err(Error::Integrity(format!(
                "session setting {:?} must have exactly one typed value",
                setting.key
            )));
        }
        if setting.real_value.is_some_and(|value| !value.is_finite()) {
            return Err(Error::Integrity(format!(
                "session setting {:?} has a non-finite real value",
                setting.key
            )));
        }
        if let Some(value) = setting.text_value {
            validate_text("session setting text value", value, usize::MAX)?;
        }
    }
    Ok(())
}

fn validate_finite_range(
    kind: &str,
    source_key: &str,
    minimum: Option<f64>,
    maximum: Option<f64>,
) -> Result<()> {
    if minimum.is_some_and(|value| !value.is_finite())
        || maximum.is_some_and(|value| !value.is_finite())
    {
        return Err(Error::Integrity(format!(
            "{kind} {source_key:?} has a non-finite range"
        )));
    }
    if minimum.zip(maximum).is_some_and(|(min, max)| min > max) {
        return Err(Error::Integrity(format!(
            "{kind} {source_key:?} has a reversed range"
        )));
    }
    Ok(())
}

fn validate_optional_text(field: &str, value: Option<&str>, maximum: usize) -> Result<()> {
    if let Some(value) = value {
        if value.contains('\0') || value.len() > maximum {
            return Err(Error::Integrity(format!("{field} is not persistable")));
        }
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, maximum: usize) -> Result<()> {
    if value.is_empty() || value.contains('\0') || value.len() > maximum {
        return Err(Error::Integrity(format!("{field} is not persistable")));
    }
    Ok(())
}

fn bytes_per_sample(encoding: &str) -> Option<usize> {
    match encoding {
        "i8" | "u8" => Some(1),
        "i16-le" | "i16-be" => Some(2),
        "i32-le" | "i32-be" | "f32-le" | "f32-be" => Some(4),
        "f64-le" | "f64-be" => Some(8),
        _ => None,
    }
}

fn parse_local_epoch_ms(value: &str) -> Option<i64> {
    if !value.is_ascii() {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes.len() != 23
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
    {
        return None;
    }
    let (year, month, day) = parse_date(&value[..10])?;
    let hour = parse_ascii_number(&bytes[11..13])?;
    let minute = parse_ascii_number(&bytes[14..16])?;
    let second = parse_ascii_number(&bytes[17..19])?;
    let millisecond = parse_ascii_number(&bytes[20..23])?;
    if hour > 23 || minute > 59 || second > 59 || millisecond > 999 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    days.checked_mul(86_400_000)?
        .checked_add(i64::from(hour) * 3_600_000)?
        .checked_add(i64::from(minute) * 60_000)?
        .checked_add(i64::from(second) * 1_000)?
        .checked_add(i64::from(millisecond))
}

fn parse_date(value: &str) -> Option<(i32, u32, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = i32::try_from(parse_ascii_number(&bytes[..4])?).ok()?;
    let month = parse_ascii_number(&bytes[5..7])?;
    let day = parse_ascii_number(&bytes[8..10])?;
    if year == 0 || !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (day >= 1 && day <= days_in_month).then_some((year, month, day))
}

fn parse_ascii_number(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(byte - b'0'))
    })
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    year -= i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
