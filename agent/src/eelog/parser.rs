//! Line-by-line parser for EE.log.
//!
//! Each line looks like:
//! `<elapsed_seconds> <source> [<level>]: <message>`
//! e.g. `33341.395 Sys [Info]: VoidProjections: <id> gets reward /Lotus/...`
//!
//! We only recognise the relic-workflow lines; everything else yields `None`.

use std::sync::OnceLock;

use regex::Regex;

use super::event::{LogEvent, RefinementTier};

/// A parsed line: the elapsed timestamp plus the recognised event.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLine {
    /// Seconds since game start, as logged in the line prefix.
    pub elapsed: f64,
    pub event: LogEvent,
}

struct Patterns {
    logged_in: Regex,
    refine: Regex,
    refine_cost: Regex,
    own_reward: Regex,
    squadmate: Regex,
    countdown: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        // `Logged in B0rbor4d (51573ba51a4d80ac10000086)`
        logged_in: Regex::new(r"Logged in (\S+) \(([0-9a-fA-F]+)\)").unwrap(),
        // `Refine Axi V14 Relic to RADIANT?` — relic + tier are reliable.
        refine: Regex::new(r"Refine (.+?) to (\w+)\?").unwrap(),
        // `It will cost 100.` — best-effort; absent when rendered as a glyph.
        refine_cost: Regex::new(r"It will cost (\d+)").unwrap(),
        // `VoidProjections: <id> gets reward /Lotus/StoreItems/.../LexPrimeBarrel`
        own_reward: Regex::new(r"VoidProjections: ([0-9a-fA-F]+) gets reward (\S+)").unwrap(),
        // `VoidProjections: Client got reward info from <id>`
        squadmate: Regex::new(r"VoidProjections: Client got reward info from ([0-9a-fA-F]+)")
            .unwrap(),
        // `ProjectionsCountdown.lua: Initialize timer nil\t15`
        countdown: Regex::new(r"Initialize timer \S+\s+(\d+)").unwrap(),
    })
}

/// Splits the `<elapsed> <rest>` prefix, returning the elapsed seconds and the
/// remainder of the line (source, level and message). Returns `None` if the
/// line does not start with a float timestamp.
fn split_prefix(line: &str) -> Option<(f64, &str)> {
    let line = line.trim_end();
    let (ts, rest) = line.split_once(' ')?;
    let elapsed: f64 = ts.parse().ok()?;
    Some((elapsed, rest))
}

/// Parses a single EE.log line into a [`ParsedLine`], or `None` if the line is
/// not one of the recognised relic-workflow events.
pub fn parse_line(line: &str) -> Option<ParsedLine> {
    let (elapsed, rest) = split_prefix(line)?;
    let event = parse_message(rest)?;
    Some(ParsedLine { elapsed, event })
}

/// Recognises an event from the post-timestamp remainder of a line.
fn parse_message(rest: &str) -> Option<LogEvent> {
    let p = patterns();

    // Cheap substring guards before the regex work, ordered by frequency of the
    // distinguishing token.
    if rest.contains("gets reward") {
        if let Some(c) = p.own_reward.captures(rest) {
            return Some(LogEvent::OwnReward {
                account_id: c[1].to_string(),
                item_path: c[2].to_string(),
            });
        }
    }

    if rest.contains("Client got reward info from") {
        if let Some(c) = p.squadmate.captures(rest) {
            return Some(LogEvent::SquadmateRewardInfo {
                player_id: c[1].to_string(),
            });
        }
    }

    if rest.contains("OpenVoidProjectionRewardScreenRMI") {
        return Some(LogEvent::RewardScreenOpen);
    }

    if rest.contains("Relic reward screen shut down") {
        return Some(LogEvent::RewardScreenClosed);
    }

    if rest.contains("Countdown timer expired") {
        return Some(LogEvent::CountdownExpired);
    }

    if rest.contains("Initialize timer") {
        if let Some(c) = p.countdown.captures(rest) {
            return Some(LogEvent::Countdown {
                seconds: c[1].parse().ok()?,
            });
        }
    }

    if rest.contains("Mission Succeeded") {
        return Some(LogEvent::MissionSucceeded);
    }

    if rest.contains("Logged in") {
        if let Some(c) = p.logged_in.captures(rest) {
            return Some(LogEvent::LoggedIn {
                name: c[1].to_string(),
                account_id: c[2].to_string(),
            });
        }
    }

    if rest.contains("Refine") {
        if let Some(c) = p.refine.captures(rest) {
            if let Some(tier) = RefinementTier::from_dialog_word(&c[2]) {
                let trace_cost = p
                    .refine_cost
                    .captures(rest)
                    .and_then(|c| c[1].parse().ok());
                return Some(LogEvent::RelicRefine {
                    relic: c[1].trim().to_string(),
                    tier,
                    trace_cost,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_login() {
        let line = "12.368 Sys [Info]: Logged in B0rbor4d (51573ba51a4d80ac10000086)";
        let parsed = parse_line(line).unwrap();
        assert_eq!(parsed.elapsed, 12.368);
        assert_eq!(
            parsed.event,
            LogEvent::LoggedIn {
                name: "B0rbor4d".into(),
                account_id: "51573ba51a4d80ac10000086".into(),
            }
        );
    }

    #[test]
    fn parses_refine_with_tier_and_cost() {
        let line = "33202.582 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Refine Axi V14 Relic to RADIANT? It will cost 100., title= leftItem=/Menu/Confirm_Item_Yes, rightItem=/Menu/Confirm_Item_No)";
        let parsed = parse_line(line).unwrap();
        assert_eq!(
            parsed.event,
            LogEvent::RelicRefine {
                relic: "Axi V14 Relic".into(),
                tier: RefinementTier::Radiant,
                trace_cost: Some(100),
            }
        );
    }

    #[test]
    fn parses_refine_when_cost_is_a_glyph_or_truncated() {
        // Real logs render the Void Trace amount as a private-use-area glyph and
        // may truncate the line there. Relic + tier must still parse; cost is None.
        let line = "33202.582 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Refine Axi V14 Relic to RADIANT? It will cost \u{e0f1}";
        assert_eq!(
            parse_line(line).unwrap().event,
            LogEvent::RelicRefine {
                relic: "Axi V14 Relic".into(),
                tier: RefinementTier::Radiant,
                trace_cost: None,
            }
        );
    }

    #[test]
    fn parses_reward_screen_open() {
        let line = "33341.215 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreenRMI";
        assert_eq!(parse_line(line).unwrap().event, LogEvent::RewardScreenOpen);
    }

    #[test]
    fn parses_own_reward_path() {
        let line = "33341.395 Sys [Info]: VoidProjections: 51573ba51a4d80ac10000086 gets reward /Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/LexPrimeBarrel";
        assert_eq!(
            parse_line(line).unwrap().event,
            LogEvent::OwnReward {
                account_id: "51573ba51a4d80ac10000086".into(),
                item_path: "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/LexPrimeBarrel"
                    .into(),
            }
        );
    }

    #[test]
    fn parses_squadmate_reward_info_without_path() {
        let line =
            "33341.613 Sys [Info]: VoidProjections: Client got reward info from 539570b43846322c04c496b7";
        assert_eq!(
            parse_line(line).unwrap().event,
            LogEvent::SquadmateRewardInfo {
                player_id: "539570b43846322c04c496b7".into(),
            }
        );
    }

    #[test]
    fn parses_countdown_start_and_reset() {
        let start = "33341.906 Script [Info]: ProjectionsCountdown.lua: Initialize timer nil\t15";
        let reset = "33356.910 Script [Info]: ProjectionsCountdown.lua: Initialize timer nil\t0";
        assert_eq!(
            parse_line(start).unwrap().event,
            LogEvent::Countdown { seconds: 15 }
        );
        assert_eq!(
            parse_line(reset).unwrap().event,
            LogEvent::Countdown { seconds: 0 }
        );
    }

    #[test]
    fn parses_countdown_expired_and_screen_close() {
        let expired =
            "33356.910 Script [Info]: ProjectionsCountdown.lua: Countdown timer expired";
        let closed =
            "33356.910 Script [Info]: ProjectionRewardChoice.lua: Relic reward screen shut down";
        assert_eq!(
            parse_line(expired).unwrap().event,
            LogEvent::CountdownExpired
        );
        assert_eq!(
            parse_line(closed).unwrap().event,
            LogEvent::RewardScreenClosed
        );
    }

    #[test]
    fn parses_mission_succeeded() {
        let line = "33357.031 Script [Info]: EndOfMatch.lua: Mission Succeeded";
        assert_eq!(
            parse_line(line).unwrap().event,
            LogEvent::MissionSucceeded
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        let line = "33336.484 Sys [Info]: client received full cache!";
        assert!(parse_line(line).is_none());
    }

    #[test]
    fn ignores_line_without_timestamp() {
        assert!(parse_line("not a log line").is_none());
    }
}
