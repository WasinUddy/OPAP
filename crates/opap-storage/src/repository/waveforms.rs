use crate::{
    Error, NewWaveformChunk, NewWaveformMetadata, Result, WaveformChunk, WaveformMetadata,
};
use rusqlite::{Connection, OptionalExtension, params};

const METADATA_COLUMNS: &str = "id, session_id, source_key, channel_key, unit, started_at_ms, \
    sample_interval_us, sample_count, encoding, min_value, max_value, created_at_ms";

#[derive(Clone, Copy)]
pub struct Waveforms<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Waveforms<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn upsert_metadata(&self, input: &NewWaveformMetadata<'_>) -> Result<WaveformMetadata> {
        bytes_per_sample(input.encoding).ok_or_else(|| {
            Error::Integrity(format!(
                "unsupported waveform encoding {:?}",
                input.encoding
            ))
        })?;
        let sql = format!(
            "INSERT INTO waveforms (
                 session_id, source_key, channel_key, unit, started_at_ms,
                 sample_interval_us, sample_count, encoding, min_value, max_value, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(session_id, source_key) DO UPDATE SET
                 channel_key = excluded.channel_key,
                 unit = excluded.unit,
                 started_at_ms = excluded.started_at_ms,
                 sample_interval_us = excluded.sample_interval_us,
                 sample_count = excluded.sample_count,
                 encoding = excluded.encoding,
                 min_value = excluded.min_value,
                 max_value = excluded.max_value
             RETURNING {METADATA_COLUMNS}"
        );
        Ok(self.connection.query_row(
            &sql,
            params![
                input.session_id,
                input.source_key,
                input.channel_key,
                input.unit,
                input.started_at_ms,
                input.sample_interval_us,
                input.sample_count,
                input.encoding,
                input.min_value,
                input.max_value,
                input.created_at_ms,
            ],
            map_metadata,
        )?)
    }

    pub fn get_metadata(&self, id: i64) -> Result<Option<WaveformMetadata>> {
        let sql = format!("SELECT {METADATA_COLUMNS} FROM waveforms WHERE id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_metadata)
            .optional()?)
    }

    pub fn list_metadata_by_session(&self, session_id: i64) -> Result<Vec<WaveformMetadata>> {
        let sql = format!(
            "SELECT {METADATA_COLUMNS} FROM waveforms
             WHERE session_id = ?1 ORDER BY started_at_ms, channel_key, id"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([session_id], map_metadata)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn find_metadata_by_source_key(
        &self,
        session_id: i64,
        source_key: &str,
    ) -> Result<Option<WaveformMetadata>> {
        let sql = format!(
            "SELECT {METADATA_COLUMNS} FROM waveforms
             WHERE session_id = ?1 AND source_key = ?2"
        );
        Ok(self
            .connection
            .query_row(&sql, params![session_id, source_key], map_metadata)
            .optional()?)
    }

    pub fn upsert_chunk(&self, input: &NewWaveformChunk<'_>) -> Result<WaveformChunk> {
        self.validate_chunk(input)?;
        Ok(self.connection.query_row(
            "INSERT INTO waveform_chunks (
                 waveform_id, chunk_index, start_sample, sample_count, payload, min_value, max_value
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(waveform_id, chunk_index) DO UPDATE SET
                 start_sample = excluded.start_sample,
                 sample_count = excluded.sample_count,
                 payload = excluded.payload,
                 min_value = excluded.min_value,
                 max_value = excluded.max_value
             RETURNING waveform_id, chunk_index, start_sample, sample_count,
                       payload, min_value, max_value",
            params![
                input.waveform_id,
                input.chunk_index,
                input.start_sample,
                input.sample_count,
                input.payload,
                input.min_value,
                input.max_value,
            ],
            map_chunk,
        )?)
    }

    pub fn list_chunks(&self, waveform_id: i64) -> Result<Vec<WaveformChunk>> {
        let mut statement = self.connection.prepare(
            "SELECT waveform_id, chunk_index, start_sample, sample_count,
                    payload, min_value, max_value
             FROM waveform_chunks WHERE waveform_id = ?1 ORDER BY chunk_index",
        )?;
        let rows = statement.query_map([waveform_id], map_chunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_chunks(&self, waveform_id: i64) -> Result<usize> {
        Ok(self.connection.execute(
            "DELETE FROM waveform_chunks WHERE waveform_id = ?1",
            [waveform_id],
        )?)
    }

    pub fn delete_metadata(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM waveforms WHERE id = ?1", [id])?
            == 1)
    }

    /// Verifies that chunks form an ordered, gap-free representation of every
    /// sample declared by the waveform metadata.
    pub fn validate_complete(&self, waveform_id: i64) -> Result<()> {
        let metadata = self
            .get_metadata(waveform_id)?
            .ok_or_else(|| Error::Integrity(format!("waveform {waveform_id} does not exist")))?;
        let chunks = self.list_chunks(waveform_id)?;
        if metadata.sample_count == 0 {
            return if chunks.is_empty() {
                Ok(())
            } else {
                Err(Error::Integrity(format!(
                    "zero-length waveform {waveform_id} contains chunks"
                )))
            };
        }
        if chunks.is_empty() {
            return Err(Error::Integrity(format!(
                "waveform {waveform_id} has no sample chunks"
            )));
        }

        let mut next_sample = 0_i64;
        for (expected_index, chunk) in chunks.iter().enumerate() {
            if chunk.chunk_index != expected_index as i64 {
                return Err(Error::Integrity(format!(
                    "waveform {waveform_id} chunk indices are not contiguous"
                )));
            }
            if chunk.start_sample != next_sample {
                return Err(Error::Integrity(format!(
                    "waveform {waveform_id} has a gap before sample {}",
                    chunk.start_sample
                )));
            }
            next_sample = next_sample.checked_add(chunk.sample_count).ok_or_else(|| {
                Error::Integrity(format!("waveform {waveform_id} sample range overflow"))
            })?;
        }
        if next_sample != metadata.sample_count {
            return Err(Error::Integrity(format!(
                "waveform {waveform_id} chunks cover {next_sample} of {} samples",
                metadata.sample_count
            )));
        }
        Ok(())
    }

    fn validate_chunk(&self, input: &NewWaveformChunk<'_>) -> Result<()> {
        let metadata = self.get_metadata(input.waveform_id)?.ok_or_else(|| {
            Error::Integrity(format!("waveform {} does not exist", input.waveform_id))
        })?;
        if input.chunk_index < 0 || input.start_sample < 0 || input.sample_count <= 0 {
            return Err(Error::Integrity(
                "waveform chunk index/start must be non-negative and count must be positive"
                    .to_owned(),
            ));
        }
        let end_sample = input
            .start_sample
            .checked_add(input.sample_count)
            .ok_or_else(|| Error::Integrity("waveform chunk sample range overflow".to_owned()))?;
        if end_sample > metadata.sample_count {
            return Err(Error::Integrity(format!(
                "waveform chunk ends at {end_sample}, beyond {} samples",
                metadata.sample_count
            )));
        }

        let sample_count = usize::try_from(input.sample_count)
            .map_err(|_| Error::Integrity("waveform chunk sample count is too large".to_owned()))?;
        let width = bytes_per_sample(&metadata.encoding).ok_or_else(|| {
            Error::Integrity(format!(
                "unsupported waveform encoding {:?}",
                metadata.encoding
            ))
        })?;
        let expected_length = sample_count
            .checked_mul(width)
            .ok_or_else(|| Error::Integrity("waveform payload length overflow".to_owned()))?;
        if input.payload.len() != expected_length {
            return Err(Error::Integrity(format!(
                "{} samples encoded as {} require {expected_length} bytes, received {}",
                input.sample_count,
                metadata.encoding,
                input.payload.len()
            )));
        }

        let overlaps: bool = self.connection.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM waveform_chunks
                 WHERE waveform_id = ?1
                   AND chunk_index <> ?2
                   AND ?3 < start_sample + sample_count
                   AND start_sample < ?4
             )",
            params![
                input.waveform_id,
                input.chunk_index,
                input.start_sample,
                end_sample
            ],
            |row| row.get(0),
        )?;
        if overlaps {
            return Err(Error::Integrity(format!(
                "waveform {} chunk {} overlaps an existing chunk",
                input.waveform_id, input.chunk_index
            )));
        }
        Ok(())
    }
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

fn map_metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<WaveformMetadata> {
    Ok(WaveformMetadata {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source_key: row.get(2)?,
        channel_key: row.get(3)?,
        unit: row.get(4)?,
        started_at_ms: row.get(5)?,
        sample_interval_us: row.get(6)?,
        sample_count: row.get(7)?,
        encoding: row.get(8)?,
        min_value: row.get(9)?,
        max_value: row.get(10)?,
        created_at_ms: row.get(11)?,
    })
}

fn map_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<WaveformChunk> {
    Ok(WaveformChunk {
        waveform_id: row.get(0)?,
        chunk_index: row.get(1)?,
        start_sample: row.get(2)?,
        sample_count: row.get(3)?,
        payload: row.get(4)?,
        min_value: row.get(5)?,
        max_value: row.get(6)?,
    })
}
