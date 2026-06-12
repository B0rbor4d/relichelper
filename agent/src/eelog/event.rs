//! Event model for parsed EE.log lines.
//!
//! Grammar verified empirically against a real Warframe EE.log (2026-06-12).
//! Only the lines relevant to the relic workflow are modelled; everything else
//! parses to `None`.

use serde::{Deserialize, Serialize};

/// Relic refinement tier, as named in the in-game refine dialog.
///
/// `Intact` is the unrefined base state and never appears in the refine dialog
/// (you only ever *refine up to* the higher tiers), but it is part of the drop
/// table model, so it is included here for completeness.
///
/// Declaration order is the natural refinement order, so derived `Ord` sorts
/// `Intact < Exceptional < Flawless < Radiant`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RefinementTier {
    Intact,
    Exceptional,
    Flawless,
    Radiant,
}

impl RefinementTier {
    /// Parses the upper-case tier word used in the refine dialog
    /// (e.g. `RADIANT`). Case-insensitive.
    pub fn from_dialog_word(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "INTACT" => Some(Self::Intact),
            "EXCEPTIONAL" => Some(Self::Exceptional),
            "FLAWLESS" => Some(Self::Flawless),
            "RADIANT" => Some(Self::Radiant),
            _ => None,
        }
    }

    /// Stable lower-case identifier (matches the serde representation), suitable
    /// for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intact => "intact",
            Self::Exceptional => "exceptional",
            Self::Flawless => "flawless",
            Self::Radiant => "radiant",
        }
    }
}

/// A single meaningful event extracted from one EE.log line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogEvent {
    /// Local account logged in. Gives the account id used to recognise the
    /// player's own reward roll later in the session.
    LoggedIn { name: String, account_id: String },

    /// Player refined a relic via the refine dialog (also reveals which relic
    /// and target tier the player is about to take into a fissure).
    ///
    /// `trace_cost` is best-effort: in real logs the Void Trace amount is often
    /// rendered with private-use-area font glyphs rather than ASCII digits (and
    /// the line may even be truncated there), so it is `None` when it cannot be
    /// read as plain digits. The relic and tier are always reliable.
    RelicRefine {
        relic: String,
        tier: RefinementTier,
        trace_cost: Option<u32>,
    },

    /// The void-fissure reward selection screen opened. Primary trigger for the
    /// live overlay.
    RewardScreenOpen,

    /// The local player's own relic roll, with the exact internal item path.
    /// Language-independent. Only the local player's roll carries a path in the
    /// log; squadmates do not (see [`LogEvent::SquadmateRewardInfo`]).
    OwnReward { account_id: String, item_path: String },

    /// A squadmate's reward arrived, but without an item path. Useful only to
    /// know how many of the four rewards are in; the actual item must come from
    /// OCR.
    SquadmateRewardInfo { player_id: String },

    /// Reward-screen decision countdown (seconds). `15` marks the start of the
    /// decision window; `0` marks reset/teardown.
    Countdown { seconds: u32 },

    /// The decision countdown expired.
    CountdownExpired,

    /// The reward selection screen was torn down.
    RewardScreenClosed,

    /// Mission completed successfully.
    MissionSucceeded,
}
