use crate::{NewSession, Result, Session};
use rusqlite::{Connection, OptionalExtension, params};

const COLUMNS: &str = "id, machine_id, source_key, started_at_ms, ended_at_ms, \
                       timezone_offset_minutes, created_at_ms, updated_at_ms";

#[derive(Clone, Copy)]
pub struct Sessions<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Sessions<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Upserts by `(machine_id, source_key)` so re-reading the same device data
    /// updates the existing session instead of duplicating it.
    pub fn upsert(&self, input: &NewSession<'_>) -> Result<Session> {
        let sql = format!(
            "INSERT INTO sessions (
                 machine_id, source_key, started_at_ms, ended_at_ms,
                 timezone_offset_minutes, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(machine_id, source_key) DO UPDATE SET
                 started_at_ms = excluded.started_at_ms,
                 ended_at_ms = excluded.ended_at_ms,
                 timezone_offset_minutes = excluded.timezone_offset_minutes,
                 updated_at_ms = excluded.updated_at_ms
             RETURNING {COLUMNS}"
        );
        Ok(self.connection.query_row(
            &sql,
            params![
                input.machine_id,
                input.source_key,
                input.started_at_ms,
                input.ended_at_ms,
                input.timezone_offset_minutes,
                input.now_ms,
            ],
            map_session,
        )?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Session>> {
        let sql = format!("SELECT {COLUMNS} FROM sessions WHERE id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_session)
            .optional()?)
    }

    pub fn find_by_source_key(&self, machine_id: i64, source_key: &str) -> Result<Option<Session>> {
        let sql =
            format!("SELECT {COLUMNS} FROM sessions WHERE machine_id = ?1 AND source_key = ?2");
        Ok(self
            .connection
            .query_row(&sql, params![machine_id, source_key], map_session)
            .optional()?)
    }

    pub fn list_by_machine(&self, machine_id: i64) -> Result<Vec<Session>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM sessions
             WHERE machine_id = ?1 ORDER BY started_at_ms, id"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([machine_id], map_session)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        machine_id: row.get(1)?,
        source_key: row.get(2)?,
        started_at_ms: row.get(3)?,
        ended_at_ms: row.get(4)?,
        timezone_offset_minutes: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
    })
}
