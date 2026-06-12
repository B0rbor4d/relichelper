//! RelichHelper agent CLI.
//!
//! Subcommands:
//!   locate                 Print the detected EE.log path and probed candidates.
//!   parse [FILE]           Batch-parse a log (defaults to the located one) and
//!                          print each recognised event as a JSON line.
//!   watch [FILE]           Follow the log live, printing events as JSON lines.
//!   sync HTML [DB]         Parse the official drop-table HTML into the SQLite
//!                          reference cache (DB defaults to data/refdata.sqlite).
//!   resolve PATH [DB]      Resolve an EE.log reward path to its item, vault
//!                          status and relic sources (JSON).
//!   relic NAME [TIER] [DB] Print a relic's drop table at TIER (default radiant)
//!                          with per-drop vault/owned annotation (JSON).
//!   own list               List owned items.
//!   own add ITEM [COUNT]   Manually record owning COUNT (default 1) of ITEM.
//!   own remove ITEM        Forget an owned item.
//!   own from-log [LOG] [DB] Scan a log for own-reward rolls and record them.
//!
//! Ownership is stored in data/inventory.sqlite (override via
//! RELICHELPER_INVENTORY_DB). With no arguments it locates the log and watches.

use std::collections::HashSet;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use relichelper_agent::eelog::{self, watcher::LogWatcher, LogEvent};
use relichelper_agent::inventory;
use relichelper_agent::refdata::RefinementTier;
use relichelper_agent::{paths, refdata};

const DEFAULT_INVENTORY_DB: &str = "data/inventory.sqlite";

/// Inventory DB path, overridable via `RELICHELPER_INVENTORY_DB` for testing.
fn inventory_path() -> PathBuf {
    std::env::var("RELICHELPER_INVENTORY_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_INVENTORY_DB))
}

/// Loads the owned-item set for annotating views. Returns `None` (ownership
/// unknown) when no inventory has been started yet.
fn load_owned() -> Option<HashSet<String>> {
    let path = inventory_path();
    if !path.exists() {
        return None;
    }
    inventory::store::open(&path)
        .ok()
        .and_then(|c| inventory::store::owned_set(&c).ok())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("watch");

    match cmd {
        "locate" => cmd_locate(),
        "parse" => cmd_parse(args.get(1).map(PathBuf::from)),
        "watch" => cmd_watch(args.get(1).map(PathBuf::from)),
        "sync" => cmd_sync(args.get(1).map(PathBuf::from), args.get(2).map(PathBuf::from)),
        "resolve" => cmd_resolve(args.get(1).cloned(), args.get(2).map(PathBuf::from)),
        "relic" => cmd_relic(
            args.get(1).cloned(),
            args.get(2).cloned(),
            args.get(3).map(PathBuf::from),
        ),
        "own" => cmd_own(&args[1..]),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!(
                "usage: relichelper-agent [locate|parse|watch|sync|resolve|relic|own] [ARGS]"
            );
            ExitCode::FAILURE
        }
    }
}

fn cmd_locate() -> ExitCode {
    println!("Probed candidates (priority order):");
    for c in paths::candidates() {
        let mark = if c.is_file() { "FOUND" } else { "     " };
        println!("  [{mark}] {}", c.display());
    }
    match paths::locate() {
        Some(p) => {
            println!("\nResolved EE.log: {}", p.display());
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("\nEE.log not found automatically — a manual path picker is needed.");
            ExitCode::FAILURE
        }
    }
}

fn resolve_or_exit(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(paths::locate).or_else(|| {
        eprintln!("EE.log not found. Pass the path explicitly or set an override.");
        None
    })
}

fn cmd_parse(file: Option<PathBuf>) -> ExitCode {
    let Some(path) = resolve_or_exit(file) else {
        return ExitCode::FAILURE;
    };
    let f = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let events = eelog::parse_reader(BufReader::new(f));
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for ev in &events {
        let _ = writeln!(out, "{}", serde_json::to_string(&ev.event).unwrap());
    }
    eprintln!("parsed {} relic-workflow events from {}", events.len(), path.display());
    ExitCode::SUCCESS
}

fn cmd_sync(html: Option<PathBuf>, db: Option<PathBuf>) -> ExitCode {
    let Some(html_path) = html else {
        eprintln!("usage: relichelper-agent sync HTML [DB]");
        return ExitCode::FAILURE;
    };
    let db_path = db.unwrap_or_else(|| PathBuf::from("data/refdata.sqlite"));

    let html = match std::fs::read_to_string(&html_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", html_path.display());
            return ExitCode::FAILURE;
        }
    };

    let relics = refdata::parse_drop_data(&html);
    if relics.is_empty() {
        eprintln!("no relics parsed from {} — is this the drop-table HTML?", html_path.display());
        return ExitCode::FAILURE;
    }
    let vaulted = relics.iter().filter(|r| r.vaulted).count();

    let mut conn = match refdata::store::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open db {}: {e}", db_path.display());
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = refdata::store::persist(&mut conn, &relics) {
        eprintln!("persist failed: {e}");
        return ExitCode::FAILURE;
    }

    let (rc, dc) = refdata::store::counts(&conn).unwrap_or((0, 0));
    eprintln!(
        "synced {} relics ({} vaulted) / {} drops -> {}",
        rc,
        vaulted,
        dc,
        db_path.display()
    );
    ExitCode::SUCCESS
}

fn cmd_resolve(path: Option<String>, db: Option<PathBuf>) -> ExitCode {
    let Some(item_path) = path else {
        eprintln!("usage: relichelper-agent resolve PATH [DB]");
        return ExitCode::FAILURE;
    };
    let db_path = db.unwrap_or_else(|| PathBuf::from("data/refdata.sqlite"));
    let conn = match refdata::store::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open db {}: {e}", db_path.display());
            return ExitCode::FAILURE;
        }
    };
    match refdata::resolve_reward(&conn, &item_path, load_owned().as_ref()) {
        Ok(Some(view)) => {
            println!("{}", serde_json::to_string_pretty(&view).unwrap());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("no match for {item_path} (not a known relic reward, or run `sync` first)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("query failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_relic(name: Option<String>, tier: Option<String>, db: Option<PathBuf>) -> ExitCode {
    let Some(relic) = name else {
        eprintln!("usage: relichelper-agent relic NAME [TIER] [DB]");
        return ExitCode::FAILURE;
    };
    let tier = match tier.as_deref() {
        None => RefinementTier::Radiant,
        Some(w) => match RefinementTier::from_dialog_word(w) {
            Some(t) => t,
            None => {
                eprintln!("unknown tier '{w}' (intact|exceptional|flawless|radiant)");
                return ExitCode::FAILURE;
            }
        },
    };
    let db_path = db.unwrap_or_else(|| PathBuf::from("data/refdata.sqlite"));
    let conn = match refdata::store::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open db {}: {e}", db_path.display());
            return ExitCode::FAILURE;
        }
    };
    match refdata::relic_view(&conn, &relic, tier, load_owned().as_ref()) {
        Ok(Some(view)) => {
            println!("{}", serde_json::to_string_pretty(&view).unwrap());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("no data for relic '{relic}' at {} (or run `sync` first)", tier.as_str());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("query failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_own(args: &[String]) -> ExitCode {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    let inv_path = inventory_path();
    let conn = match inventory::store::open(&inv_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open inventory {}: {e}", inv_path.display());
            return ExitCode::FAILURE;
        }
    };

    match sub {
        "list" => match inventory::store::list(&conn) {
            Ok(items) => {
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("query failed: {e}");
                ExitCode::FAILURE
            }
        },
        "add" => {
            let Some(item) = args.get(1) else {
                eprintln!("usage: own add ITEM [COUNT]");
                return ExitCode::FAILURE;
            };
            let delta: i64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
            match inventory::store::add(&conn, item, delta, inventory::Source::Manual) {
                Ok(n) => {
                    eprintln!("{item}: now {n}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "remove" => {
            let Some(item) = args.get(1) else {
                eprintln!("usage: own remove ITEM");
                return ExitCode::FAILURE;
            };
            match inventory::store::remove(&conn, item) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => {
                    eprintln!("{item} was not tracked");
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "from-log" => cmd_own_from_log(
            &conn,
            args.get(1).map(PathBuf::from),
            args.get(2).map(PathBuf::from),
        ),
        other => {
            eprintln!("unknown own subcommand: {other} (list|add|remove|from-log)");
            ExitCode::FAILURE
        }
    }
}

/// Scans a log for the player's own reward rolls, resolves each item path via
/// the reference cache, and records it (log-derived ownership).
fn cmd_own_from_log(
    inv: &rusqlite::Connection,
    log: Option<PathBuf>,
    refdb: Option<PathBuf>,
) -> ExitCode {
    let Some(log_path) = log.or_else(paths::locate) else {
        eprintln!("EE.log not found; pass it explicitly");
        return ExitCode::FAILURE;
    };
    let refdb = refdb.unwrap_or_else(|| PathBuf::from("data/refdata.sqlite"));
    let refconn = match refdata::store::open(&refdb) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open reference db {}: {e}", refdb.display());
            return ExitCode::FAILURE;
        }
    };
    let file = match std::fs::File::open(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open {}: {e}", log_path.display());
            return ExitCode::FAILURE;
        }
    };

    let (mut recorded, mut unresolved) = (0u32, 0u32);
    for parsed in eelog::parse_reader(BufReader::new(file)) {
        if let LogEvent::OwnReward { item_path, .. } = parsed.event {
            match refdata::store::resolve_item_path(&refconn, &item_path) {
                Ok(Some(name)) => {
                    if inventory::store::record_reward(inv, &name).is_ok() {
                        recorded += 1;
                        eprintln!("  + {name}");
                    }
                }
                _ => unresolved += 1,
            }
        }
    }
    eprintln!("recorded {recorded} log-derived roll(s), {unresolved} unresolved");
    ExitCode::SUCCESS
}

fn cmd_watch(file: Option<PathBuf>) -> ExitCode {
    let Some(path) = resolve_or_exit(file) else {
        return ExitCode::FAILURE;
    };
    eprintln!("watching {} (Ctrl-C to stop)", path.display());
    let mut watcher = match LogWatcher::new(&path) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("cannot watch {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let result = watcher.watch(|ev| {
        println!("{}", serde_json::to_string(&ev.event).unwrap());
    });
    if let Err(e) = result {
        eprintln!("watch error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
