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
        self.with_coherent_read(|snapshots| {
            snapshots.summary_in_current_transaction(session_id, || Ok(()))
        })
    }

    fn summary_in_current_transaction<F>(
        &self,
        session_id: i64,
        after_summary_row: F,
    ) -> Result<Option<SessionSummary>>
    where
        F: FnOnce() -> Result<()>,
    {
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
        after_summary_row()?;
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
        self.with_coherent_read(|snapshots| {
            snapshots.get_in_current_transaction(session_id, || Ok(()))
        })
    }

    fn get_in_current_transaction<F>(
        &self,
        session_id: i64,
        after_provenance: F,
    ) -> Result<Option<SessionSnapshot>>
    where
        F: FnOnce() -> Result<()>,
    {
        let Some(provenance) = self.provenance(session_id)? else {
            return Ok(None);
        };
        after_provenance()?;
        let summary = self
            .summary_in_current_transaction(session_id, || Ok(()))?
            .ok_or_else(|| {
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

    fn with_coherent_read<T>(
        &self,
        read: impl FnOnce(&SessionSnapshots<'_>) -> Result<T>,
    ) -> Result<T> {
        if !self.connection.is_autocommit() {
            return read(self);
        }
        let transaction = self.connection.unchecked_transaction()?;
        let value = {
            let snapshots = SessionSnapshots::new(&transaction);
            read(&snapshots)?
        };
        transaction.commit()?;
        Ok(value)
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
        let deleted_slices = self.connection.execute(
            "DELETE FROM session_slices WHERE session_id = ?1",
            [session_id],
        )?;
        require_affected_rows("delete session slices", deleted_slices, old_slices.len())?;
        for slice in replacement.slices {
            checkpoint()?;
            self.insert_slice(session_id, slice)?;
        }

        checkpoint()?;
        let summary_rows = self.connection.execute(
            "INSERT INTO session_summary (session_id, usage_ms)
             VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET usage_ms = excluded.usage_ms",
            params![session_id, replacement.summary.usage_ms],
        )?;
        require_affected_rows("upsert session summary", summary_rows, 1)?;
        checkpoint()?;
        let deleted_metrics = self.connection.execute(
            "DELETE FROM summary_metrics WHERE session_id = ?1",
            [session_id],
        )?;
        require_affected_rows("delete summary metrics", deleted_metrics, old_metrics.len())?;
        for metric in replacement.summary.metrics {
            checkpoint()?;
            let inserted = self.connection.execute(
                "INSERT INTO summary_metrics (session_id, metric_key, value, unit)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, metric.key, metric.value, metric.unit],
            )?;
            require_affected_rows("insert summary metric", inserted, 1)?;
        }

        checkpoint()?;
        let deleted_settings = self.connection.execute(
            "DELETE FROM session_settings WHERE session_id = ?1",
            [session_id],
        )?;
        require_affected_rows(
            "delete session settings",
            deleted_settings,
            old_settings.len(),
        )?;
        for setting in replacement.settings {
            checkpoint()?;
            self.insert_setting(session_id, setting)?;
        }
        checkpoint()?;

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
        let affected = self.connection.execute(
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
        require_affected_rows("upsert session provenance", affected, 1)?;
        Ok(())
    }

    fn insert_slice(&self, session_id: i64, slice: &SessionSliceInput<'_>) -> Result<()> {
        let affected = self.connection.execute(
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
        require_affected_rows("insert session slice", affected, 1)?;
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
        let affected = self.connection.execute(
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
        require_affected_rows("insert session setting", affected, 1)?;
        Ok(())
    }
}

fn require_affected_rows(operation: &str, actual: usize, expected: usize) -> Result<()> {
    if actual != expected {
        return Err(Error::Integrity(format!(
            "{operation} affected {actual} rows; expected {expected}"
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn wal_snapshot_fixture() -> Result<(TempDir, Database, Database)> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("coherent-snapshot.sqlite3");
        let reader = Database::open(&path)?;
        reader.connection().execute_batch(&format!(
            "INSERT INTO profiles (id, display_name, created_at_ms, updated_at_ms)
             VALUES (1, 'Coherent reader', 1, 1);
             INSERT INTO machines (
                 id, profile_id, source_key, device_type, manufacturer, model,
                 model_number, serial_number, first_seen_at_ms, last_seen_at_ms
             ) VALUES (1, 1, 'machine:one', 'pap', '', '', '', '', 1, 1);
             INSERT INTO sessions (
                 id, machine_id, source_key, started_at_ms, ended_at_ms,
                 timezone_offset_minutes, created_at_ms, updated_at_ms
             ) VALUES (1, 1, 'session:one', 1000, 2000, 0, 1, 1);
             INSERT INTO session_provenance (
                 session_id, therapy_day, start_local_wall, end_local_wall,
                 start_utc_offset_seconds, end_utc_offset_seconds,
                 start_clock_correction_ms, end_clock_correction_ms, data_kind,
                 importer_name, importer_schema, id_algorithm, source_digest, content_digest
             ) VALUES (
                 1, '1970-01-01', '1970-01-01T00:00:01.000',
                 '1970-01-01T00:00:02.000', 0, 0, 0, 0, 'detailed',
                 'resmed', 'schema:v1', 'id:v1', '{DIGEST_A}', '{DIGEST_A}'
             );
             INSERT INTO session_slices (
                 session_id, sequence, source_key, state, started_at_ms, ended_at_ms
             ) VALUES (1, 0, 'slice:one', 'mask_on', 1000, 2000);
             INSERT INTO session_summary (session_id, usage_ms) VALUES (1, 1000);
             INSERT INTO summary_metrics (session_id, metric_key, value, unit)
             VALUES (1, 'ahi', 1.0, '1/h');
             INSERT INTO session_settings (
                 session_id, setting_key, value_kind, integer_value, origin
             ) VALUES (1, 'level', 'integer', 1, 'device_reported');"
        ))?;
        let writer = Database::open(&path)?;
        Ok((directory, reader, writer))
    }

    #[test]
    fn summary_read_is_one_wal_snapshot_across_parent_and_metric_queries() -> Result<()> {
        let (_directory, reader, writer) = wal_snapshot_fixture()?;
        let summary = reader
            .session_snapshots()
            .with_coherent_read(|snapshots| {
                snapshots.summary_in_current_transaction(1, || {
                    writer.connection().execute_batch(
                        "BEGIN IMMEDIATE;
                         UPDATE session_summary SET usage_ms = 2000 WHERE session_id = 1;
                         UPDATE summary_metrics SET value = 2.0
                         WHERE session_id = 1 AND metric_key = 'ahi';
                         COMMIT;",
                    )?;
                    Ok(())
                })
            })?
            .expect("summary");
        assert_eq!(summary.usage_ms, 1000);
        assert_eq!(summary.metrics[0].value, 1.0);

        let latest = reader
            .session_snapshots()
            .summary(1)?
            .expect("latest summary");
        assert_eq!(latest.usage_ms, 2000);
        assert_eq!(latest.metrics[0].value, 2.0);
        Ok(())
    }

    #[test]
    fn complete_snapshot_read_is_one_wal_snapshot_across_all_child_queries() -> Result<()> {
        let (_directory, reader, writer) = wal_snapshot_fixture()?;
        let snapshot = reader
            .session_snapshots()
            .with_coherent_read(|snapshots| {
                snapshots.get_in_current_transaction(1, || {
                    writer.connection().execute_batch(&format!(
                        "BEGIN IMMEDIATE;
                         UPDATE session_provenance SET content_digest = '{DIGEST_B}'
                         WHERE session_id = 1;
                         UPDATE session_slices SET state = 'mask_off' WHERE session_id = 1;
                         UPDATE session_summary SET usage_ms = 2000 WHERE session_id = 1;
                         UPDATE session_settings SET integer_value = 2 WHERE session_id = 1;
                         COMMIT;"
                    ))?;
                    Ok(())
                })
            })?
            .expect("snapshot");
        assert_eq!(snapshot.provenance.content_digest, DIGEST_A);
        assert_eq!(snapshot.slices[0].state, SessionSliceState::MaskOn);
        assert_eq!(snapshot.summary.usage_ms, 1000);
        assert_eq!(snapshot.settings[0].value, SessionSettingValue::Integer(1));

        let latest = reader.session_snapshots().get(1)?.expect("latest snapshot");
        assert_eq!(latest.provenance.content_digest, DIGEST_B);
        assert_eq!(latest.slices[0].state, SessionSliceState::MaskOff);
        assert_eq!(latest.summary.usage_ms, 2000);
        assert_eq!(latest.settings[0].value, SessionSettingValue::Integer(2));
        Ok(())
    }
}
