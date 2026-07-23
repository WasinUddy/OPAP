use crate::{
    Error, Result, SessionDataKind, SessionProvenance, SessionSetting, SessionSettingInput,
    SessionSettingValue, SessionSlice, SessionSliceInput, SessionSliceState, SessionSnapshot,
    SessionSnapshotReplacement, SessionSummary, SummaryMetric,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;

const PROVENANCE_COLUMNS: &str = "session_id, therapy_day, start_local_wall, end_local_wall, \
    start_utc_offset_seconds, end_utc_offset_seconds, start_clock_correction_ms, \
    end_clock_correction_ms, data_kind, importer_name, importer_schema, id_algorithm, \
    source_digest, content_digest";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SnapshotChildStats {
    pub slices_written: usize,
    pub slices_pruned: usize,
    pub summary_metrics_written: usize,
    pub summary_metrics_pruned: usize,
    pub settings_written: usize,
    pub settings_pruned: usize,
}

#[derive(Clone, Copy)]
pub struct SessionSnapshots<'connection> {
    connection: &'connection Connection,
}

impl<'connection> SessionSnapshots<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn provenance(&self, session_id: i64) -> Result<Option<SessionProvenance>> {
        let sql =
            format!("SELECT {PROVENANCE_COLUMNS} FROM session_provenance WHERE session_id = ?1");
        let raw = self
            .connection
            .query_row(&sql, [session_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i32>>(4)?,
                    row.get::<_, Option<i32>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                ))
            })
            .optional()?;
        raw.map(
            |(
                session_id,
                therapy_day,
                start_local_wall,
                end_local_wall,
                start_utc_offset_seconds,
                end_utc_offset_seconds,
                start_clock_correction_ms,
                end_clock_correction_ms,
                data_kind,
                importer_name,
                importer_schema,
                id_algorithm,
                source_digest,
                content_digest,
            )| {
                let data_kind = SessionDataKind::from_str(&data_kind).ok_or_else(|| {
                    Error::Integrity(format!(
                        "session {session_id} has an invalid persisted data kind"
                    ))
                })?;
                Ok(SessionProvenance {
                    session_id,
                    therapy_day,
                    start_local_wall,
                    end_local_wall,
                    start_utc_offset_seconds,
                    end_utc_offset_seconds,
                    start_clock_correction_ms,
                    end_clock_correction_ms,
                    data_kind,
                    importer_name,
                    importer_schema,
                    id_algorithm,
                    source_digest,
                    content_digest,
                })
            },
        )
        .transpose()
    }

    pub fn list_slices(&self, session_id: i64) -> Result<Vec<SessionSlice>> {
        let mut statement = self.connection.prepare(
            "SELECT session_id, sequence, source_key, state, started_at_ms, ended_at_ms
             FROM session_slices
             WHERE session_id = ?1
             ORDER BY sequence",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (session_id, sequence, source_key, state, started_at_ms, ended_at_ms) = row?;
            let state = SessionSliceState::from_str(&state).ok_or_else(|| {
                Error::Integrity(format!(
                    "session {session_id} slice {sequence} has an invalid persisted state"
                ))
            })?;
            Ok(SessionSlice {
                session_id,
                sequence,
                source_key,
                state,
                started_at_ms,
                ended_at_ms,
            })
        })
        .collect()
    }

    pub fn summary(&self, session_id: i64) -> Result<Option<SessionSummary>> {
        let usage_ms = self
            .connection
            .query_row(
                "SELECT usage_ms FROM session_summary WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(usage_ms) = usage_ms else {
            return Ok(None);
        };
        Ok(Some(SessionSummary {
            session_id,
            usage_ms,
            metrics: self.list_summary_metrics(session_id)?,
        }))
    }

    pub fn list_summary_metrics(&self, session_id: i64) -> Result<Vec<SummaryMetric>> {
        let mut statement = self.connection.prepare(
            "SELECT session_id, metric_key, value, unit
             FROM summary_metrics
             WHERE session_id = ?1
             ORDER BY metric_key",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok(SummaryMetric {
                session_id: row.get(0)?,
                key: row.get(1)?,
                value: row.get(2)?,
                unit: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_settings(&self, session_id: i64) -> Result<Vec<SessionSetting>> {
        let mut statement = self.connection.prepare(
            "SELECT session_id, setting_key, value_kind, integer_value, real_value,
                    text_value, boolean_value, unit, origin
             FROM session_settings
             WHERE session_id = ?1
             ORDER BY setting_key",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<f64>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;
        rows.map(|row| {
            let (
                session_id,
                key,
                value_kind,
                integer_value,
                real_value,
                text_value,
                boolean_value,
                unit,
                origin,
            ) = row?;
            let value = decode_setting_value(
                session_id,
                &key,
                &value_kind,
                integer_value,
                real_value,
                text_value,
                boolean_value,
            )?;
            Ok(SessionSetting {
                session_id,
                key,
                value,
                unit,
                origin,
            })
        })
        .collect()
    }

    /// Reads all v8 children for a session. `None` means no snapshot provenance
    /// has been committed. A partial persisted snapshot is reported as an
    /// integrity error rather than silently synthesized.
    pub fn get(&self, session_id: i64) -> Result<Option<SessionSnapshot>> {
        let Some(provenance) = self.provenance(session_id)? else {
            return Ok(None);
        };
        let summary = self.summary(session_id)?.ok_or_else(|| {
            Error::Integrity(format!(
                "session {session_id} has provenance but no summary row"
            ))
        })?;
        Ok(Some(SessionSnapshot {
            provenance,
            slices: self.list_slices(session_id)?,
            summary,
            settings: self.list_settings(session_id)?,
        }))
    }

    pub(crate) fn replace(
        &self,
        session_id: i64,
        replacement: &SessionSnapshotReplacement<'_>,
    ) -> Result<SnapshotChildStats> {
        let mut checkpoint = || Ok(());
        self.replace_with_checkpoint(session_id, replacement, &mut checkpoint)
    }

    pub(crate) fn replace_with_checkpoint(
        &self,
        session_id: i64,
        replacement: &SessionSnapshotReplacement<'_>,
        checkpoint: &mut dyn FnMut() -> Result<()>,
    ) -> Result<SnapshotChildStats> {
        let old_slices = self.list_slices(session_id)?;
        let old_metrics = self.list_summary_metrics(session_id)?;
        let old_settings = self.list_settings(session_id)?;

        checkpoint()?;
        self.upsert_provenance(session_id, replacement)?;

        checkpoint()?;
        self.connection.execute(
            "DELETE FROM session_slices WHERE session_id = ?1",
            [session_id],
        )?;
        for slice in replacement.slices {
            checkpoint()?;
            self.insert_slice(session_id, slice)?;
        }

        checkpoint()?;
        self.connection.execute(
            "INSERT INTO session_summary (session_id, usage_ms)
             VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET usage_ms = excluded.usage_ms",
            params![session_id, replacement.summary.usage_ms],
        )?;
        checkpoint()?;
        self.connection.execute(
            "DELETE FROM summary_metrics WHERE session_id = ?1",
            [session_id],
        )?;
        for metric in replacement.summary.metrics {
            checkpoint()?;
            self.connection.execute(
                "INSERT INTO summary_metrics (session_id, metric_key, value, unit)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, metric.key, metric.value, metric.unit],
            )?;
        }

        checkpoint()?;
        self.connection.execute(
            "DELETE FROM session_settings WHERE session_id = ?1",
            [session_id],
        )?;
        for setting in replacement.settings {
            checkpoint()?;
            self.insert_setting(session_id, setting)?;
        }

        let new_slice_keys = replacement
            .slices
            .iter()
            .map(|slice| slice.source_key)
            .collect::<HashSet<_>>();
        let new_metric_keys = replacement
            .summary
            .metrics
            .iter()
            .map(|metric| metric.key)
            .collect::<HashSet<_>>();
        let new_setting_keys = replacement
            .settings
            .iter()
            .map(|setting| setting.key)
            .collect::<HashSet<_>>();

        Ok(SnapshotChildStats {
            slices_written: replacement.slices.len(),
            slices_pruned: old_slices
                .iter()
                .filter(|slice| !new_slice_keys.contains(slice.source_key.as_str()))
                .count(),
            summary_metrics_written: replacement.summary.metrics.len(),
            summary_metrics_pruned: old_metrics
                .iter()
                .filter(|metric| !new_metric_keys.contains(metric.key.as_str()))
                .count(),
            settings_written: replacement.settings.len(),
            settings_pruned: old_settings
                .iter()
                .filter(|setting| !new_setting_keys.contains(setting.key.as_str()))
                .count(),
        })
    }

    fn upsert_provenance(
        &self,
        session_id: i64,
        replacement: &SessionSnapshotReplacement<'_>,
    ) -> Result<()> {
        let provenance = replacement.provenance;
        self.connection.execute(
            "INSERT INTO session_provenance (
                 session_id, therapy_day, start_local_wall, end_local_wall,
                 start_utc_offset_seconds, end_utc_offset_seconds,
                 start_clock_correction_ms, end_clock_correction_ms, data_kind,
                 importer_name, importer_schema, id_algorithm, source_digest, content_digest
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
             )
             ON CONFLICT(session_id) DO UPDATE SET
                 therapy_day = excluded.therapy_day,
                 start_local_wall = excluded.start_local_wall,
                 end_local_wall = excluded.end_local_wall,
                 start_utc_offset_seconds = excluded.start_utc_offset_seconds,
                 end_utc_offset_seconds = excluded.end_utc_offset_seconds,
                 start_clock_correction_ms = excluded.start_clock_correction_ms,
                 end_clock_correction_ms = excluded.end_clock_correction_ms,
                 data_kind = excluded.data_kind,
                 importer_name = excluded.importer_name,
                 importer_schema = excluded.importer_schema,
                 id_algorithm = excluded.id_algorithm,
                 source_digest = excluded.source_digest,
                 content_digest = excluded.content_digest",
            params![
                session_id,
                provenance.therapy_day,
                provenance.start_local_wall,
                provenance.end_local_wall,
                provenance.start_utc_offset_seconds,
                provenance.end_utc_offset_seconds,
                provenance.start_clock_correction_ms,
                provenance.end_clock_correction_ms,
                provenance.data_kind.as_str(),
                provenance.importer_name,
                provenance.importer_schema,
                provenance.id_algorithm,
                provenance.source_digest,
                provenance.content_digest,
            ],
        )?;
        Ok(())
    }

    fn insert_slice(&self, session_id: i64, slice: &SessionSliceInput<'_>) -> Result<()> {
        self.connection.execute(
            "INSERT INTO session_slices (
                 session_id, sequence, source_key, state, started_at_ms, ended_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                slice.sequence,
                slice.source_key,
                slice.state.as_str(),
                slice.started_at_ms,
                slice.ended_at_ms,
            ],
        )?;
        Ok(())
    }

    fn insert_setting(&self, session_id: i64, setting: &SessionSettingInput<'_>) -> Result<()> {
        let value_kind = if setting.integer_value.is_some() {
            "integer"
        } else if setting.real_value.is_some() {
            "real"
        } else if setting.text_value.is_some() {
            "text"
        } else {
            "boolean"
        };
        self.connection.execute(
            "INSERT INTO session_settings (
                 session_id, setting_key, value_kind, integer_value, real_value,
                 text_value, boolean_value, unit, origin
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                setting.key,
                value_kind,
                setting.integer_value,
                setting.real_value,
                setting.text_value,
                setting.boolean_value,
                setting.unit,
                setting.origin,
            ],
        )?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_setting_value(
    session_id: i64,
    key: &str,
    value_kind: &str,
    integer_value: Option<i64>,
    real_value: Option<f64>,
    text_value: Option<String>,
    boolean_value: Option<i64>,
) -> Result<SessionSettingValue> {
    let value = match (
        value_kind,
        integer_value,
        real_value,
        text_value,
        boolean_value,
    ) {
        ("integer", Some(value), None, None, None) => SessionSettingValue::Integer(value),
        ("real", None, Some(value), None, None) => SessionSettingValue::Real(value),
        ("text", None, None, Some(value), None) => SessionSettingValue::Text(value),
        ("boolean", None, None, None, Some(0)) => SessionSettingValue::Boolean(false),
        ("boolean", None, None, None, Some(1)) => SessionSettingValue::Boolean(true),
        _ => {
            return Err(Error::Integrity(format!(
                "session {session_id} setting {key:?} has an invalid persisted typed value"
            )));
        }
    };
    Ok(value)
}
