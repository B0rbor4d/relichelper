//! SQLite persistence for the parsed reference data.
//!
//! The cache is rebuilt wholesale on each sync (the official table changes with
//! patches), so persistence is a simple delete-and-reinsert inside one
//! transaction.

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::model::{era_word, Relic};
use super::naming;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS relics (
    name    TEXT PRIMARY KEY,
    era     TEXT NOT NULL,
    code    TEXT NOT NULL,
    vaulted INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS drops (
    relic  TEXT NOT NULL REFERENCES relics(name),
    tier   TEXT NOT NULL,
    item   TEXT NOT NULL,
    rarity TEXT NOT NULL,
    chance REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_drops_relic ON drops(relic);
CREATE INDEX IF NOT EXISTS idx_drops_item  ON drops(item);
CREATE INDEX IF NOT EXISTS idx_relics_vaulted ON relics(vaulted);
CREATE TABLE IF NOT EXISTS items (
    name TEXT PRIMARY KEY,
    norm TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_items_norm ON items(norm);
";

/// Opens (creating if needed) the reference-data database at `path` and ensures
/// the schema exists.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Replaces all cached relics/drops with the given set in a single transaction.
pub fn persist(conn: &mut Connection, relics: &[Relic]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM drops", [])?;
    tx.execute("DELETE FROM relics", [])?;
    tx.execute("DELETE FROM items", [])?;

    // Distinct reward items, for the path -> display-name bridge.
    let mut items: BTreeSet<&str> = BTreeSet::new();

    for relic in relics {
        tx.execute(
            "INSERT INTO relics(name, era, code, vaulted) VALUES (?1, ?2, ?3, ?4)",
            params![relic.name, era_word(&relic.era), relic.code, relic.vaulted as i32],
        )?;
        for (tier, drops) in &relic.tiers {
            for d in drops {
                tx.execute(
                    "INSERT INTO drops(relic, tier, item, rarity, chance) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![relic.name, tier.as_str(), d.item, d.rarity, d.chance],
                )?;
                items.insert(d.item.as_str());
            }
        }
    }

    for item in items {
        tx.execute(
            "INSERT OR REPLACE INTO items(name, norm) VALUES (?1, ?2)",
            params![item, naming::normalize(item)],
        )?;
    }

    tx.commit()
}

/// Resolves an EE.log item path to its official display name, e.g.
/// `/Lotus/StoreItems/.../LexPrimeBarrel` -> `"Lex Prime Barrel"`. Returns
/// `None` if the item is not a known relic reward.
pub fn resolve_item_path(conn: &Connection, path: &str) -> rusqlite::Result<Option<String>> {
    let key = naming::path_key(path);
    conn.query_row("SELECT name FROM items WHERE norm = ?1", [key], |r| r.get(0))
        .optional()
}

/// Number of relics and drops currently stored — handy for verification.
pub fn counts(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    let relics = conn.query_row("SELECT COUNT(*) FROM relics", [], |r| r.get(0))?;
    let drops = conn.query_row("SELECT COUNT(*) FROM drops", [], |r| r.get(0))?;
    Ok((relics, drops))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refdata::parse::parse_drop_data;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/drops_sample.html"
        ))
        .unwrap()
    }

    #[test]
    fn persists_and_reads_back_counts() {
        let relics = parse_drop_data(&fixture());
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        persist(&mut conn, &relics).unwrap();

        let (relic_count, drop_count) = counts(&conn).unwrap();
        assert_eq!(relic_count, 2);
        // Axi A1: 4 tiers * 6 + Axi A10: 2 tiers * 6 = 36
        assert_eq!(drop_count, 36);
    }

    #[test]
    fn persist_is_idempotent() {
        let relics = parse_drop_data(&fixture());
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        persist(&mut conn, &relics).unwrap();
        persist(&mut conn, &relics).unwrap();
        assert_eq!(counts(&conn).unwrap(), (2, 36));
    }

    #[test]
    fn resolves_ee_log_path_to_display_name() {
        let relics = parse_drop_data(&fixture());
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        persist(&mut conn, &relics).unwrap();

        // Item present in the fixture's Axi A1 drop table.
        let resolved = resolve_item_path(
            &conn,
            "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeBarrel",
        )
        .unwrap();
        assert_eq!(resolved.as_deref(), Some("Akstiletto Prime Barrel"));

        // Unknown item -> None.
        let missing =
            resolve_item_path(&conn, "/Lotus/StoreItems/.../SomethingNotInTable").unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn vaulted_flag_round_trips() {
        let relics = parse_drop_data(&fixture());
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        persist(&mut conn, &relics).unwrap();

        let vaulted: i64 = conn
            .query_row(
                "SELECT vaulted FROM relics WHERE name = 'Axi A10'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(vaulted, 1);
    }
}
