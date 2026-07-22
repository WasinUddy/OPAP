use crate::{NewProfile, Profile, Result};
use rusqlite::{Connection, OptionalExtension, params};

const COLUMNS: &str = "id, display_name, created_at_ms, updated_at_ms";

#[derive(Clone, Copy)]
pub struct Profiles<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Profiles<'connection> {
    pub const fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert(&self, input: &NewProfile<'_>) -> Result<Profile> {
        let sql = format!(
            "INSERT INTO profiles (display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?2) RETURNING {COLUMNS}"
        );
        Ok(self.connection.query_row(
            &sql,
            params![input.display_name, input.now_ms],
            map_profile,
        )?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Profile>> {
        let sql = format!("SELECT {COLUMNS} FROM profiles WHERE id = ?1");
        Ok(self
            .connection
            .query_row(&sql, [id], map_profile)
            .optional()?)
    }

    pub fn list(&self) -> Result<Vec<Profile>> {
        let sql = format!("SELECT {COLUMNS} FROM profiles ORDER BY display_name, id");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], map_profile)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename(&self, id: i64, display_name: &str, updated_at_ms: i64) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE profiles SET display_name = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![id, display_name, updated_at_ms],
        )? == 1)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM profiles WHERE id = ?1", [id])?
            == 1)
    }
}

fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    Ok(Profile {
        id: row.get(0)?,
        display_name: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
    })
}
