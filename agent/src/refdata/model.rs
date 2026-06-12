//! Data model for the parsed reference drop tables.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::eelog::event::RefinementTier;

/// Relic era (the prefix of a relic name, e.g. `Axi` in "Axi A1").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Era {
    Lith,
    Meso,
    Neo,
    Axi,
    Requiem,
    /// Any era we don't explicitly know (forward-compatible with new tiers).
    Other(String),
}

impl Era {
    pub fn parse(word: &str) -> Self {
        match word {
            "Lith" => Self::Lith,
            "Meso" => Self::Meso,
            "Neo" => Self::Neo,
            "Axi" => Self::Axi,
            "Requiem" => Self::Requiem,
            other => Self::Other(other.to_string()),
        }
    }
}

/// A single possible reward within a relic at a given tier.
///
/// `rarity` is kept as the raw label from the official table (e.g. "Uncommon").
/// `chance` is the authoritative percentage and is what optimization should use
/// — the official labels do not map cleanly onto the percentage buckets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drop {
    pub item: String,
    pub rarity: String,
    pub chance: f32,
}

/// A relic with its full per-tier drop table and derived vault status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relic {
    /// Full display name, e.g. "Axi A1".
    pub name: String,
    pub era: Era,
    /// The short code, e.g. "A1".
    pub code: String,
    /// Drops keyed by tier; `Ord` on the tier keeps them in refinement order.
    pub tiers: BTreeMap<RefinementTier, Vec<Drop>>,
    /// Derived: present in the relic table but in no current mission source.
    pub vaulted: bool,
}

impl Relic {
    pub fn new(era: Era, code: String) -> Self {
        let name = format!("{} {}", era_word(&era), code);
        Self {
            name,
            era,
            code,
            tiers: BTreeMap::new(),
            vaulted: false,
        }
    }
}

/// The display word for an era (inverse of [`Era::parse`]).
pub fn era_word(era: &Era) -> String {
    match era {
        Era::Lith => "Lith".into(),
        Era::Meso => "Meso".into(),
        Era::Neo => "Neo".into(),
        Era::Axi => "Axi".into(),
        Era::Requiem => "Requiem".into(),
        Era::Other(s) => s.clone(),
    }
}
