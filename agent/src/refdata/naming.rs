//! Bridges EE.log internal item paths to official display names.
//!
//! EE.log reports rewards by path, e.g.
//! `/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/LexPrimeBarrel`,
//! while the drop table uses the display name "Lex Prime Barrel".
//!
//! Every relic-reward path belongs, by definition, to an item that already
//! appears in the parsed drop table, so we bridge the two by normalising the
//! path's leaf and the display name to a common key (lower-case, alphanumerics
//! only) and matching on it. This needs no external data and has full coverage
//! for relic rewards. The `/StoreItems` segment is irrelevant because only the
//! leaf is used.

/// Returns the last `/`-separated segment of a path (the item identifier).
pub fn path_leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Collapses a path leaf or display name to a comparison key: ASCII
/// alphanumerics only, lower-cased. `"Lex Prime Barrel"` and `"LexPrimeBarrel"`
/// both become `"lexprimebarrel"`.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Convenience: the normalized key for an EE.log item path.
pub fn path_key(path: &str) -> String {
    normalize(path_leaf(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_of_full_store_path() {
        assert_eq!(
            path_leaf("/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/LexPrimeBarrel"),
            "LexPrimeBarrel"
        );
    }

    #[test]
    fn display_name_and_path_leaf_share_a_key() {
        assert_eq!(normalize("Lex Prime Barrel"), "lexprimebarrel");
        assert_eq!(path_key("/Lotus/.../LexPrimeBarrel"), "lexprimebarrel");
        assert_eq!(normalize("Lex Prime Barrel"), path_key("/x/LexPrimeBarrel"));
    }

    #[test]
    fn warframe_part_blueprints_match_too() {
        assert_eq!(
            normalize("Wukong Prime Chassis Blueprint"),
            path_key("/Lotus/Types/Recipes/Warframes/WukongPrimeChassisBlueprint")
        );
    }
}
