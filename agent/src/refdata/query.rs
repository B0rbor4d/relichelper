//! Read-side views over the reference cache, for the UI and the live overlay.
//!
//! Two shapes:
//!   - [`relic_view`] — the full drop table of one relic at one tier, each drop
//!     annotated with vault and ownership status (the data-driven half of the
//!     overlay, and the relic browser).
//!   - [`resolve_reward`] — given an EE.log reward path, the item it maps to,
//!     whether it is vaulted, and which relics currently drop it.
//!
//! `owned` is `None` everywhere for now; inventory tracking (phase 4) will fill
//! it in.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::naming;
use crate::eelog::event::RefinementTier;

/// Ownership context passed to the view builders: `None` leaves `owned`
/// unknown, `Some(set)` annotates each item against the owned-item names.
pub type Owned<'a> = Option<&'a HashSet<String>>;

fn owned_flag(owned: Owned, item: &str) -> Option<bool> {
    owned.map(|set| set.contains(item))
}

/// One possible reward within a relic, annotated for decision-making.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropView {
    pub item: String,
    pub rarity: String,
    pub chance: f32,
    /// The reward item is only obtainable from currently-vaulted relics.
    pub item_vaulted: bool,
    /// Whether the built item is already owned. `None` = inventory unknown.
    pub owned: Option<bool>,
}

/// A relic's full drop table at a given refinement tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelicView {
    pub relic: String,
    pub era: String,
    pub vaulted: bool,
    pub tier: RefinementTier,
    /// Drops, highest chance first.
    pub drops: Vec<DropView>,
}

/// A relic that drops a given item, with its vault status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelicSource {
    pub relic: String,
    pub vaulted: bool,
}

/// A resolved reward (for the overlay): the item, its vault status, ownership,
/// and the relics that currently drop it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewardView {
    pub item: String,
    pub item_vaulted: bool,
    pub owned: Option<bool>,
    /// Relics dropping this item, non-vaulted first.
    pub sources: Vec<RelicSource>,
}

/// Returns the full drop table of `relic` at `tier`, or `None` if the relic or
/// tier is unknown.
pub fn relic_view(
    conn: &Connection,
    relic: &str,
    tier: RefinementTier,
    owned: Owned,
) -> rusqlite::Result<Option<RelicView>> {
    let meta = conn
        .query_row(
            "SELECT era, vaulted FROM relics WHERE name = ?1",
            [relic],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((era, vaulted)) = meta else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT d.item, d.rarity, d.chance, i.vaulted \
         FROM drops d JOIN items i ON i.name = d.item \
         WHERE d.relic = ?1 AND d.tier = ?2 \
         ORDER BY d.chance DESC, d.item",
    )?;
    let drops: Vec<DropView> = stmt
        .query_map(params![relic, tier.as_str()], |r| {
            let item: String = r.get(0)?;
            let owned = owned_flag(owned, &item);
            Ok(DropView {
                item,
                rarity: r.get(1)?,
                chance: r.get(2)?,
                item_vaulted: r.get::<_, i64>(3)? != 0,
                owned,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    if drops.is_empty() {
        return Ok(None); // relic exists but not at this tier
    }

    Ok(Some(RelicView {
        relic: relic.to_string(),
        era,
        vaulted: vaulted != 0,
        tier,
        drops,
    }))
}

/// Resolves an EE.log reward path to a [`RewardView`], or `None` if the item is
/// not a known relic reward.
pub fn resolve_reward(
    conn: &Connection,
    path: &str,
    owned: Owned,
) -> rusqlite::Result<Option<RewardView>> {
    let key = naming::path_key(path);
    let item = conn
        .query_row(
            "SELECT name, vaulted FROM items WHERE norm = ?1",
            [key],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((name, item_vaulted)) = item else {
        return Ok(None);
    };
    let owned_flag = owned_flag(owned, &name);

    let mut stmt = conn.prepare(
        "SELECT DISTINCT d.relic, r.vaulted \
         FROM drops d JOIN relics r ON r.name = d.relic \
         WHERE d.item = ?1 \
         ORDER BY r.vaulted ASC, d.relic",
    )?;
    let sources: Vec<RelicSource> = stmt
        .query_map([&name], |r| {
            Ok(RelicSource {
                relic: r.get(0)?,
                vaulted: r.get::<_, i64>(1)? != 0,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    Ok(Some(RewardView {
        item: name,
        item_vaulted: item_vaulted != 0,
        owned: owned_flag,
        sources,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refdata::{parse::parse_drop_data, store};

    fn db() -> Connection {
        let html = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/drops_sample.html"
        ))
        .unwrap();
        let relics = parse_drop_data(&html);
        let mut conn = Connection::open_in_memory().unwrap();
        store::init_schema(&conn).unwrap();
        store::persist(&mut conn, &relics).unwrap();
        conn
    }

    #[test]
    fn relic_view_returns_full_tier_table() {
        let conn = db();
        let view = relic_view(&conn, "Axi A1", RefinementTier::Radiant, None)
            .unwrap()
            .unwrap();
        assert_eq!(view.drops.len(), 6);
        assert!(!view.vaulted, "Axi A1 is farmable in the fixture");
        // Sorted by descending chance.
        assert!(view.drops[0].chance >= view.drops[5].chance);
        // No ownership context -> unknown.
        assert!(view.drops.iter().all(|d| d.owned.is_none()));
    }

    #[test]
    fn vaulted_relic_marks_items_vaulted() {
        let conn = db();
        let view = relic_view(&conn, "Axi A10", RefinementTier::Intact, None)
            .unwrap()
            .unwrap();
        assert!(view.vaulted);
        // A10's items appear in no non-vaulted relic in the fixture.
        assert!(view.drops.iter().all(|d| d.item_vaulted));
    }

    #[test]
    fn unknown_relic_or_tier_is_none() {
        let conn = db();
        assert!(relic_view(&conn, "Axi Z99", RefinementTier::Radiant, None)
            .unwrap()
            .is_none());
        // Axi A10 has no Exceptional tier in the fixture.
        assert!(relic_view(&conn, "Axi A10", RefinementTier::Exceptional, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn resolve_reward_lists_sources_and_vault() {
        let conn = db();
        let view = resolve_reward(
            &conn,
            "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeBarrel",
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(view.item, "Akstiletto Prime Barrel");
        // Dropped by Axi A1 (non-vaulted in the fixture) -> not item-vaulted.
        assert!(!view.item_vaulted);
        assert!(view.sources.iter().any(|s| s.relic == "Axi A1" && !s.vaulted));
        assert_eq!(view.owned, None);
    }

    #[test]
    fn ownership_context_annotates_drops() {
        let conn = db();
        let owned: HashSet<String> = ["Akstiletto Prime Barrel".to_string()].into_iter().collect();

        let view = relic_view(&conn, "Axi A1", RefinementTier::Radiant, Some(&owned))
            .unwrap()
            .unwrap();
        let barrel = view
            .drops
            .iter()
            .find(|d| d.item == "Akstiletto Prime Barrel")
            .unwrap();
        assert_eq!(barrel.owned, Some(true));
        let other = view
            .drops
            .iter()
            .find(|d| d.item == "Nikana Prime Blueprint")
            .unwrap();
        assert_eq!(other.owned, Some(false));

        let reward = resolve_reward(
            &conn,
            "/x/AkstilettoPrimeBarrel",
            Some(&owned),
        )
        .unwrap()
        .unwrap();
        assert_eq!(reward.owned, Some(true));
    }
}
