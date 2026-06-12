//! Maps noisy OCR text to a canonical item/relic name.
//!
//! OCR of the reward screen and inventory screens is imperfect: characters get
//! confused (l/I/1, O/0, rn/m), words wrap, and extra tokens (rarity, counts)
//! creep in. Rather than trust the raw text, we match it against the
//! authoritative name corpus we already hold (from the drop tables) and snap to
//! the closest known name. This is also what makes the feature
//! language-tolerant: the corpus can be swapped for a localized one.
//!
//! Matching is on a normalized key (alphanumerics, lower-case — see
//! [`crate::refdata::naming::normalize`]) using normalized Levenshtein
//! similarity, which is robust to the small per-character errors OCR makes.

use crate::refdata::naming::normalize;

/// A matched canonical name with its similarity score in `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub name: String,
    pub score: f64,
}

/// A fuzzy matcher over a fixed corpus of canonical names.
pub struct Matcher {
    /// `(normalized_key, original_name)` pairs.
    corpus: Vec<(String, String)>,
}

impl Matcher {
    /// Builds a matcher from canonical names (e.g. item or relic display names).
    /// Empty names are ignored.
    pub fn new<I>(names: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let corpus = names
            .into_iter()
            .map(|n| (normalize(&n), n))
            .filter(|(k, _)| !k.is_empty())
            .collect();
        Self { corpus }
    }

    pub fn len(&self) -> usize {
        self.corpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corpus.is_empty()
    }

    /// Returns the best corpus match for `raw` whose similarity is at least
    /// `threshold` (try ~0.7), or `None`. Ties resolve to the first/shortest
    /// candidate scanned.
    pub fn best(&self, raw: &str, threshold: f64) -> Option<Match> {
        let key = normalize(raw);
        if key.is_empty() {
            return None;
        }
        let mut best: Option<Match> = None;
        for (cand_key, name) in &self.corpus {
            let score = strsim::normalized_levenshtein(&key, cand_key);
            if score >= threshold && best.as_ref().map_or(true, |b| score > b.score) {
                best = Some(Match {
                    name: name.clone(),
                    score,
                });
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher() -> Matcher {
        Matcher::new(
            [
                "Lex Prime Barrel",
                "Lex Prime Receiver",
                "Nikana Prime Blueprint",
                "Forma Blueprint",
            ]
            .into_iter()
            .map(String::from),
        )
    }

    #[test]
    fn exact_match_scores_one() {
        let m = matcher().best("Lex Prime Barrel", 0.7).unwrap();
        assert_eq!(m.name, "Lex Prime Barrel");
        assert!((m.score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn tolerates_ocr_character_confusion() {
        // l/I/1 confusion and a wrong char.
        let m = matcher().best("Lex Pr1me Barre1", 0.7).unwrap();
        assert_eq!(m.name, "Lex Prime Barrel");
    }

    #[test]
    fn ignores_case_and_spacing() {
        let m = matcher().best("LEXPRIMEBARREL", 0.7).unwrap();
        assert_eq!(m.name, "Lex Prime Barrel");
    }

    #[test]
    fn distinguishes_close_siblings() {
        let m = matcher().best("Lex Prime Receiver", 0.7).unwrap();
        assert_eq!(m.name, "Lex Prime Receiver");
    }

    #[test]
    fn rejects_unrelated_text_below_threshold() {
        assert!(matcher().best("Continue to extraction", 0.7).is_none());
    }

    #[test]
    fn empty_input_is_none() {
        assert!(matcher().best("   ", 0.7).is_none());
    }
}
