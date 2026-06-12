//! Parser for the official Digital Extremes drop-table HTML.
//!
//! The document is one big page split into `<h3 id="...">` sections. We only
//! need two:
//!   - `relicRewards` ("Relics:") — every relic at all four tiers, each with 6
//!     drops and exact percentages.
//!   - `missionRewards` ("Missions:") — where relics are farmed; used only to
//!     derive vault status.
//!
//! Rows are extremely regular, so we extract them with a single ordered regex
//! pass rather than pulling in a full HTML parser.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use super::model::{Drop, Era, Relic};
use crate::eelog::event::RefinementTier;

/// Parses the full reference data: relics with their per-tier drops, with
/// `vaulted` derived from the mission sources.
pub fn parse_drop_data(html: &str) -> Vec<Relic> {
    let farmable = farmable_relics(html);
    let mut relics = parse_relics(html);
    for relic in &mut relics {
        relic.vaulted = !farmable.contains(&relic.name);
    }
    relics
}

/// Returns the slice of `html` belonging to the section with the given id:
/// from the `<h3 id="...">` opener up to the next `<h3` (or end of document).
fn section<'a>(html: &'a str, id: &str) -> Option<&'a str> {
    let marker = format!("id=\"{id}\"");
    let start = html.find(&marker)?;
    let rest = &html[start..];
    let end = rest[1..].find("<h3").map(|i| i + 1).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn row_regex() -> &'static Regex {
    // Matches, in document order, either a relic-tier header row or a drop row.
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?:<th colspan="2">([A-Za-z]+) ([A-Z0-9]+) Relic \((\w+)\)</th>)|(?:<td>([^<]+)</td><td>(\w+) \(([\d.]+)%\)</td>)"#,
        )
        .unwrap()
    })
}

/// Parses every relic and its per-tier drops from the `relicRewards` section.
/// `vaulted` is left at its default (`false`) — call [`parse_drop_data`] for the
/// derived value.
pub fn parse_relics(html: &str) -> Vec<Relic> {
    let Some(section) = section(html, "relicRewards") else {
        return Vec::new();
    };

    // Preserve first-seen order of relics while accumulating their tiers.
    let mut order: Vec<String> = Vec::new();
    let mut by_name: BTreeMap<String, Relic> = BTreeMap::new();
    let mut current: Option<(String, RefinementTier)> = None;

    for caps in row_regex().captures_iter(section) {
        if let (Some(era), Some(code), Some(tier)) = (caps.get(1), caps.get(2), caps.get(3)) {
            let Some(tier) = RefinementTier::from_dialog_word(tier.as_str()) else {
                current = None;
                continue;
            };
            let era = Era::parse(era.as_str());
            let relic = Relic::new(era, code.as_str().to_string());
            let name = relic.name.clone();
            by_name.entry(name.clone()).or_insert_with(|| {
                order.push(name.clone());
                relic
            });
            by_name
                .get_mut(&name)
                .unwrap()
                .tiers
                .entry(tier)
                .or_default();
            current = Some((name, tier));
        } else if let (Some(item), Some(rarity), Some(pct)) =
            (caps.get(4), caps.get(5), caps.get(6))
        {
            if let Some((name, tier)) = &current {
                let drop = Drop {
                    item: item.as_str().to_string(),
                    rarity: rarity.as_str().to_string(),
                    chance: pct.as_str().parse().unwrap_or(0.0),
                };
                if let Some(relic) = by_name.get_mut(name) {
                    relic.tiers.entry(*tier).or_default().push(drop);
                }
            }
        }
    }

    order
        .into_iter()
        .filter_map(|name| by_name.remove(&name))
        .collect()
}

/// Collects the set of relic display names (e.g. "Lith C14") that appear as a
/// farmable reward *anywhere* in the document — missions, bounties, caches, etc.
///
/// The whole document is scanned on purpose: relics drop from many sections
/// (Cetus/Vallis/Deimos/Zariman bounties, dynamic caches, …), not just
/// `missionRewards`. Relics as *farming sources* are always `<td>…Relic…</td>`
/// rows, whereas in the `relicRewards` section they are `<th>` headers, so this
/// regex never picks up the relic-content section itself. Any relic with at
/// least one such source is currently obtainable, hence not vaulted.
pub fn farmable_relics(html: &str) -> HashSet<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // The refinement tier in parens is optional: most sources list relics
    // tier-less (`<td>Axi C11 Relic</td>`), only ESO-style sources add a tier
    // (`<td>Axi V14 Relic (Radiant)</td>`). Both mean "obtainable".
    let re = RE.get_or_init(|| {
        Regex::new(r#"<td>([A-Za-z]+) ([A-Z0-9]+) Relic(?: \(\w+\))?</td>"#).unwrap()
    });

    re.captures_iter(html)
        .map(|c| format!("{} {}", &c[1], &c[2]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/drops_sample.html"
        ))
        .expect("fixture present")
    }

    #[test]
    fn parses_all_relics_and_tiers() {
        let relics = parse_relics(&fixture());
        let names: Vec<_> = relics.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Axi A1", "Axi A10"]);

        let a1 = &relics[0];
        assert_eq!(a1.tiers.len(), 4, "all four tiers present");
        let radiant = &a1.tiers[&RefinementTier::Radiant];
        assert_eq!(radiant.len(), 6, "six drops per tier");
    }

    #[test]
    fn captures_exact_percentages_and_items() {
        let relics = parse_relics(&fixture());
        let intact = &relics[0].tiers[&RefinementTier::Intact];
        let nikana = intact
            .iter()
            .find(|d| d.item == "Nikana Prime Blueprint")
            .expect("rare drop present");
        assert_eq!(nikana.rarity, "Rare");
        assert!((nikana.chance - 2.00).abs() < f32::EPSILON);
    }

    #[test]
    fn farmable_set_from_missions() {
        let farmable = farmable_relics(&fixture());
        assert!(farmable.contains("Axi A1"));
        assert!(!farmable.contains("Axi A10"));
    }

    #[test]
    fn farmable_accepts_both_tiered_and_tierless_rows() {
        // Most sources are tier-less; ESO-style sources carry a tier.
        let html = "<td>Lith C14 Relic</td><td>Rare (1%)</td>\
                    <td>Axi V14 Relic (Radiant)</td><td>Rare (1%)</td>";
        let farmable = farmable_relics(html);
        assert!(farmable.contains("Lith C14"), "tier-less row counted");
        assert!(farmable.contains("Axi V14"), "tiered row counted");
        assert_eq!(farmable.len(), 2);
    }

    #[test]
    fn derives_vaulted_status() {
        let relics = parse_drop_data(&fixture());
        let a1 = relics.iter().find(|r| r.name == "Axi A1").unwrap();
        let a10 = relics.iter().find(|r| r.name == "Axi A10").unwrap();
        assert!(!a1.vaulted, "Axi A1 has a mission source -> not vaulted");
        assert!(a10.vaulted, "Axi A10 has no mission source -> vaulted");
    }
}
