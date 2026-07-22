use crate::{Machine, NewMachine, Result};
use rusqlite::{Connection, OptionalExtension, params};

const COLUMNS: &str = "id, profile_id, source_key, device_type, manufacturer, model, \
                       model_number, serial_number, first_seen_at_ms, last_seen_at_ms";

#[derive(Clone, Copy)]
pub struct Machines<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Machines<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Inserts a machine or refreshes its mutable metadata. The stable row id is
    /// preserved when a device is encountered in a later import.
    pub fn upsert(&self, input: &NewMachine<'_>) -> Result<Machine> {
        let sql = format!(
            "INSERT INTO machines (
                 profile_id, source_key, device_type, manufacturer, model,
                 model_number, serial_number, first_seen_at_ms, last_seen_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(profile_id, source_key) DO UPDATE SET
                 device_type = excluded.device_type,
                 manufacturer = excluded.manufacturer,
                 model = excluded.model,
                 model_number = excluded.model_number,
                 serial_number = excluded.serial_number,
                 first_seen_at_ms = min(machines.first_seen_at_ms, excluded.first_seen_at_ms),
                 last_seen_at_ms = max(machines.last_seen_at_ms, excluded.last_seen_at_ms)
             RETURNING {COLUMNS}"
        );
        Ok(self.connection.query_row(
            &sql,
            params![
                input.profile_id,
                input.source_key,
                input.device_type,
                input.manufacturer,
                input.model,
                input.model_number,
                input.serial_number,
                input.seen_at_ms,
            ],
            map_machine,
        )?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Machine>> {
        let sql = format!("SELECT {COLUMNS} FROM machines WHERE id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_machine)
            .optional()?)
    }

    pub fn find_by_source_key(&self, profile_id: i64, source_key: &str) -> Result<Option<Machine>> {
        let sql =
            format!("SELECT {COLUMNS} FROM machines WHERE profile_id = ?1 AND source_key = ?2");
        Ok(self
            .connection
            .query_row(&sql, params![profile_id, source_key], map_machine)
            .optional()?)
    }

    pub fn list_by_profile(&self, profile_id: i64) -> Result<Vec<Machine>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM machines
             WHERE profile_id = ?1 ORDER BY manufacturer, model, serial_number, id"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([profile_id], map_machine)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn map_machine(row: &rusqlite::Row<'_>) -> rusqlite::Result<Machine> {
    Ok(Machine {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        source_key: row.get(2)?,
        device_type: row.get(3)?,
        manufacturer: row.get(4)?,
        model: row.get(5)?,
        model_number: row.get(6)?,
        serial_number: row.get(7)?,
        first_seen_at_ms: row.get(8)?,
        last_seen_at_ms: row.get(9)?,
    })
}
