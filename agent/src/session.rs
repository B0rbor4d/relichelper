//! Turns raw EE.log events into enriched overlay events.
//!
//! This is the agent's live glue: it tracks a little session state (who is
//! logged in, the last relic refined) and, on each relevant log event, looks up
//! the reference cache and inventory to produce a self-contained
//! [`OverlayEvent`] that the overlay (phase 6) and web app (phase 7) can render
//! directly — vault and ownership already resolved.

use std::collections::HashSet;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::eelog::event::LogEvent;
use crate::inventory;
use crate::refdata::{
    self,
    query::{RelicView, RewardView},
};

/// A render-ready event for the overlay / UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverlayEvent {
    LoggedIn { name: String, account_id: String },
    /// The player picked/refined a relic — its full annotated drop table.
    RelicSelected { view: RelicView },
    /// The reward selection screen opened (show the overlay).
    RewardScreenOpen,
    /// The local player's own roll, resolved with vault + ownership.
    OwnReward { view: RewardView },
    /// The local player's own roll, but the item path is not a known reward.
    OwnRewardUnresolved { item_path: String },
    /// A squadmate's reward arrived; the item is unknown until OCR (phase 5).
    SquadmateReward { player_id: String },
    /// Decision-window countdown, in seconds.
    Countdown { seconds: u32 },
    /// The reward screen closed (hide the overlay).
    RewardScreenClosed,
    MissionSucceeded,
}

/// Holds session state and the database handles needed to enrich events.
pub struct Session<'a> {
    refdb: &'a Connection,
    invdb: Option<&'a Connection>,
    account_id: Option<String>,
}

impl<'a> Session<'a> {
    pub fn new(refdb: &'a Connection, invdb: Option<&'a Connection>) -> Self {
        Self {
            refdb,
            invdb,
            account_id: None,
        }
    }

    /// The accountId of the logged-in player, once seen.
    pub fn account_id(&self) -> Option<&str> {
        self.account_id.as_deref()
    }

    fn owned(&self) -> Option<HashSet<String>> {
        self.invdb.and_then(|c| inventory::store::owned_set(c).ok())
    }

    /// Maps one log event to an enriched overlay event, or `None` if the event
    /// is not overlay-relevant.
    pub fn handle(&mut self, event: &LogEvent) -> rusqlite::Result<Option<OverlayEvent>> {
        let owned = self.owned();
        let out = match event {
            LogEvent::LoggedIn { name, account_id } => {
                self.account_id = Some(account_id.clone());
                Some(OverlayEvent::LoggedIn {
                    name: name.clone(),
                    account_id: account_id.clone(),
                })
            }

            LogEvent::RelicRefine { relic, tier, .. } => {
                let name = relic_lookup_name(relic);
                match refdata::relic_view(self.refdb, name, *tier, owned.as_ref())? {
                    Some(view) => Some(OverlayEvent::RelicSelected { view }),
                    None => None,
                }
            }

            LogEvent::RewardScreenOpen => Some(OverlayEvent::RewardScreenOpen),

            LogEvent::OwnReward { item_path, .. } => {
                match refdata::resolve_reward(self.refdb, item_path, owned.as_ref())? {
                    Some(view) => Some(OverlayEvent::OwnReward { view }),
                    None => Some(OverlayEvent::OwnRewardUnresolved {
                        item_path: item_path.clone(),
                    }),
                }
            }

            LogEvent::SquadmateRewardInfo { player_id } => Some(OverlayEvent::SquadmateReward {
                player_id: player_id.clone(),
            }),

            // `0` marks teardown of the timer; only surface the active window.
            LogEvent::Countdown { seconds } if *seconds > 0 => {
                Some(OverlayEvent::Countdown { seconds: *seconds })
            }
            LogEvent::Countdown { .. } | LogEvent::CountdownExpired => None,

            LogEvent::RewardScreenClosed => Some(OverlayEvent::RewardScreenClosed),
            LogEvent::MissionSucceeded => Some(OverlayEvent::MissionSucceeded),
        };
        Ok(out)
    }
}

/// The refine dialog names a relic as e.g. "Axi V14 Relic", but the reference
/// cache keys relics as "Axi V14". Strip the trailing " Relic".
fn relic_lookup_name(refine_name: &str) -> &str {
    refine_name.strip_suffix(" Relic").unwrap_or(refine_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eelog::event::RefinementTier;
    use crate::refdata::{parse::parse_drop_data, store};

    fn refdb() -> Connection {
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
    fn refine_event_yields_relic_view_stripping_relic_suffix() {
        let refdb = refdb();
        let mut s = Session::new(&refdb, None);
        let ev = s
            .handle(&LogEvent::RelicRefine {
                relic: "Axi A1 Relic".into(),
                tier: RefinementTier::Radiant,
                trace_cost: None,
            })
            .unwrap()
            .unwrap();
        match ev {
            OverlayEvent::RelicSelected { view } => {
                assert_eq!(view.relic, "Axi A1");
                assert_eq!(view.drops.len(), 6);
            }
            other => panic!("expected RelicSelected, got {other:?}"),
        }
    }

    #[test]
    fn own_reward_resolves_with_ownership() {
        let refdb = refdb();
        let invdb = Connection::open_in_memory().unwrap();
        inventory::store::init_schema(&invdb).unwrap();
        inventory::store::add(&invdb, "Akstiletto Prime Barrel", 1, inventory::Source::Manual)
            .unwrap();

        let mut s = Session::new(&refdb, Some(&invdb));
        let ev = s
            .handle(&LogEvent::OwnReward {
                account_id: "x".into(),
                item_path: "/Lotus/StoreItems/.../AkstilettoPrimeBarrel".into(),
            })
            .unwrap()
            .unwrap();
        match ev {
            OverlayEvent::OwnReward { view } => {
                assert_eq!(view.item, "Akstiletto Prime Barrel");
                assert_eq!(view.owned, Some(true));
            }
            other => panic!("expected OwnReward, got {other:?}"),
        }
    }

    #[test]
    fn login_is_tracked_and_countdown_zero_suppressed() {
        let refdb = refdb();
        let mut s = Session::new(&refdb, None);
        s.handle(&LogEvent::LoggedIn {
            name: "Tenno".into(),
            account_id: "abc".into(),
        })
        .unwrap();
        assert_eq!(s.account_id(), Some("abc"));

        assert!(s
            .handle(&LogEvent::Countdown { seconds: 0 })
            .unwrap()
            .is_none());
        assert_eq!(
            s.handle(&LogEvent::Countdown { seconds: 15 }).unwrap(),
            Some(OverlayEvent::Countdown { seconds: 15 })
        );
    }

    #[test]
    fn unresolved_own_reward_falls_back() {
        let refdb = refdb();
        let mut s = Session::new(&refdb, None);
        let ev = s
            .handle(&LogEvent::OwnReward {
                account_id: "x".into(),
                item_path: "/Lotus/.../TotallyUnknownItem".into(),
            })
            .unwrap()
            .unwrap();
        assert!(matches!(ev, OverlayEvent::OwnRewardUnresolved { .. }));
    }
}
