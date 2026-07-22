use crate::{Event, NewEvent, Result};
use rusqlite::{Connection, OptionalExtension, params};

const COLUMNS: &str = "id, session_id, source_key, channel_key, event_type, starts_at_ms, \
                       duration_ms, value, unit, created_at_ms";

#[derive(Clone, Copy)]
pub struct Events<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Events<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn upsert(&self, input: &NewEvent<'_>) -> Result<Event> {
        let sql = format!(
            "INSERT INTO events (
                 session_id, source_key, channel_key, event_type, starts_at_ms,
                 duration_ms, value, unit, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(session_id, source_key) DO UPDATE SET
                 channel_key = excluded.channel_key,
                 event_type = excluded.event_type,
                 starts_at_ms = excluded.starts_at_ms,
                 duration_ms = excluded.duration_ms,
                 value = excluded.value,
                 unit = excluded.unit
             RETURNING {COLUMNS}"
        );
        Ok(self.connection.query_row(
            &sql,
            params![
                input.session_id,
                input.source_key,
                input.channel_key,
                input.event_type,
                input.starts_at_ms,
                input.duration_ms,
                input.value,
                input.unit,
                input.created_at_ms,
            ],
            map_event,
        )?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Event>> {
        let sql = format!("SELECT {COLUMNS} FROM events WHERE id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_event)
            .optional()?)
    }

    pub fn list_by_session(&self, session_id: i64) -> Result<Vec<Event>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events
             WHERE session_id = ?1 ORDER BY starts_at_ms, source_key"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([session_id], map_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM events WHERE id = ?1", [id])?
            == 1)
    }
}

fn map_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source_key: row.get(2)?,
        channel_key: row.get(3)?,
        event_type: row.get(4)?,
        starts_at_ms: row.get(5)?,
        duration_ms: row.get(6)?,
        value: row.get(7)?,
        unit: row.get(8)?,
        created_at_ms: row.get(9)?,
    })
}
