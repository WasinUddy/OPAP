use crate::repository::{Events, Sessions, Waveforms};
use crate::{
    Database, Error, NewEvent, NewWaveformChunk, NewWaveformMetadata, Result,
    SessionDataReplacement, SessionReplacementStats,
};
use std::collections::HashSet;

impl Database {
    /// Atomically replaces the authoritative derived data for one session.
    /// Existing records absent from `replacement` are pruned, including chunks
    /// cascading from a removed waveform. Any validation or database failure
    /// rolls the entire replacement back.
    pub fn replace_session_data(
        &mut self,
        session_id: i64,
        replacement: &SessionDataReplacement<'_>,
    ) -> Result<SessionReplacementStats> {
        validate_replacement(replacement)?;
        let transaction = self.transaction()?;
        if Sessions::new(&transaction).get(session_id)?.is_none() {
            return Err(Error::Integrity(format!(
                "cannot replace data for missing session {session_id}"
            )));
        }

        let existing_events = Events::new(&transaction).list_by_session(session_id)?;
        let event_keys = replacement
            .events
            .iter()
            .map(|event| event.source_key)
            .collect::<HashSet<_>>();
        for event in replacement.events {
            Events::new(&transaction).upsert(&NewEvent {
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
            Events::new(&transaction).delete(event.id)?;
        }

        let existing_waveforms =
            Waveforms::new(&transaction).list_metadata_by_session(session_id)?;
        let waveform_keys = replacement
            .waveforms
            .iter()
            .map(|waveform| waveform.source_key)
            .collect::<HashSet<_>>();
        let mut chunks_written = 0;
        for waveform in replacement.waveforms {
            let repository = Waveforms::new(&transaction);
            if let Some(existing) =
                repository.find_metadata_by_source_key(session_id, waveform.source_key)?
            {
                repository.delete_chunks(existing.id)?;
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
            Waveforms::new(&transaction).delete_metadata(waveform.id)?;
        }

        let stats = SessionReplacementStats {
            events_written: replacement.events.len(),
            events_pruned: stale_events.len(),
            waveforms_written: replacement.waveforms.len(),
            waveforms_pruned: stale_waveforms.len(),
            waveform_chunks_written: chunks_written,
        };
        transaction.commit()?;
        Ok(stats)
    }
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
