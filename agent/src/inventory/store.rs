//! Owned-item tracking, persisted in its own SQLite database.
//!
//! This is *user data* and lives separate from the reference cache, which is
//! rebuilt wholesale on every sync. Items are keyed by their canonical display
//! name (the same string the drop tables use), so ownership joins to drops by
//! name without any cross-database query.
//!
//! Phase 4 fills this from two sources: `manual` (the user) and `log` (the
//! player's own reward path observed in EE.log). OCR inventory sync (phase 5)
//! will add a third.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS owned (
    item       TEXT PRIMARY KEY,
    count      INTEGER NOT NULL,
    source     TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
";

/// How an owned entry was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Manual,
    Log,
    Ocr,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Log => "log",
            Self::Ocr => "ocr",
        }
    }
}

/// One owned item with its quantity and provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedItem {
    pub item: String,
    pub count: i64,
    pub source: String,
}

/// Opens (creating if needed) the inventory database and ensures its schema.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

fn now() -> String {
    // RFC3339-ish UTC timestamp without pulling in a date crate.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// Sets an item's owned count exactly (upsert). A count of 0 keeps the row but
/// marks it not owned; use [`remove`] to delete it entirely.
pub fn set_count(conn: &Connection, item: &str, count: i64, source: Source) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO owned(item, count, source, updated_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(item) DO UPDATE SET count = ?2, source = ?3, updated_at = ?4",
        params![item, count, source.as_str(), now()],
    )?;
    Ok(())
}

/// Adds `delta` to an item's count (creating the row at `delta` if absent).
/// Returns the new count.
pub fn add(conn: &Connection, item: &str, delta: i64, source: Source) -> rusqlite::Result<i64> {
    let current = count_of(conn, item)?;
    let next = (current + delta).max(0);
    set_count(conn, item, next, source)?;
    Ok(next)
}

/// Records the player's own reward (log-derived): increments by one.
pub fn record_reward(conn: &Connection, item: &str) -> rusqlite::Result<i64> {
    add(conn, item, 1, Source::Log)
}

/// Removes an item from the inventory entirely.
pub fn remove(conn: &Connection, item: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM owned WHERE item = ?1", [item])? > 0)
}

/// The current count of an item (0 if absent).
pub fn count_of(conn: &Connection, item: &str) -> rusqlite::Result<i64> {
    Ok(conn
        .query_row("SELECT count FROM owned WHERE item = ?1", [item], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(0))
}

/// All owned entries (count > 0), ordered by name.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<OwnedItem>> {
    let mut stmt = conn
        .prepare("SELECT item, count, source FROM owned WHERE count > 0 ORDER BY item")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(OwnedItem {
                item: r.get(0)?,
                count: r.get(1)?,
                source: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The set of owned item names (count > 0), for annotating views.
pub fn owned_set(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT item FROM owned WHERE count > 0")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn add_accumulates_and_lists() {
        let conn = db();
        assert_eq!(add(&conn, "Lex Prime Barrel", 1, Source::Manual).unwrap(), 1);
        assert_eq!(add(&conn, "Lex Prime Barrel", 2, Source::Manual).unwrap(), 3);
        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].count, 3);
    }

    #[test]
    fn record_reward_increments_from_log() {
        let conn = db();
        record_reward(&conn, "Lex Prime Barrel").unwrap();
        assert_eq!(count_of(&conn, "Lex Prime Barrel").unwrap(), 1);
        assert!(owned_set(&conn).unwrap().contains("Lex Prime Barrel"));
    }

    #[test]
    fn set_zero_hides_from_owned_set_and_list() {
        let conn = db();
        set_count(&conn, "Forma Blueprint", 0, Source::Manual).unwrap();
        assert!(owned_set(&conn).unwrap().is_empty());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn add_does_not_go_negative() {
        let conn = db();
        add(&conn, "Lex Prime Barrel", 1, Source::Manual).unwrap();
        assert_eq!(add(&conn, "Lex Prime Barrel", -5, Source::Manual).unwrap(), 0);
    }

    #[test]
    fn remove_deletes() {
        let conn = db();
        add(&conn, "Lex Prime Barrel", 1, Source::Manual).unwrap();
        assert!(remove(&conn, "Lex Prime Barrel").unwrap());
        assert!(!remove(&conn, "Lex Prime Barrel").unwrap());
    }
}
