//! End-to-end parse of a fixture EE.log covering one full relic-reward cycle.

use std::io::BufReader;

use relichelper_agent::eelog::{self, LogEvent, RefinementTier};

fn fixture_events() -> Vec<LogEvent> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample_ee.log");
    let file = std::fs::File::open(path).expect("fixture log present");
    eelog::parse_reader(BufReader::new(file))
        .into_iter()
        .map(|p| p.event)
        .collect()
}

#[test]
fn parses_full_relic_reward_cycle_in_order() {
    let events = fixture_events();
    let own = "aaaaaaaaaaaaaaaaaaaaaaaa".to_string();

    let expected = vec![
        LogEvent::LoggedIn {
            name: "TestTenno".into(),
            account_id: own.clone(),
        },
        LogEvent::RelicRefine {
            relic: "Axi V14 Relic".into(),
            tier: RefinementTier::Radiant,
            trace_cost: Some(100),
        },
        LogEvent::RewardScreenOpen,
        LogEvent::OwnReward {
            account_id: own.clone(),
            item_path: "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/LexPrimeBarrel".into(),
        },
        LogEvent::SquadmateRewardInfo { player_id: own },
        LogEvent::SquadmateRewardInfo {
            player_id: "bbbbbbbbbbbbbbbbbbbbbbbb".into(),
        },
        LogEvent::SquadmateRewardInfo {
            player_id: "cccccccccccccccccccccccc".into(),
        },
        LogEvent::SquadmateRewardInfo {
            player_id: "dddddddddddddddddddddddd".into(),
        },
        LogEvent::Countdown { seconds: 15 },
        LogEvent::CountdownExpired,
        LogEvent::Countdown { seconds: 0 },
        LogEvent::RewardScreenClosed,
        LogEvent::MissionSucceeded,
    ];

    assert_eq!(events, expected);
}

#[test]
fn own_reward_is_the_only_event_with_an_item_path() {
    let with_paths: Vec<_> = fixture_events()
        .into_iter()
        .filter(|e| matches!(e, LogEvent::OwnReward { .. }))
        .collect();
    assert_eq!(with_paths.len(), 1, "exactly one own-reward roll has a path");
}
