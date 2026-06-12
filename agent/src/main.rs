//! RelichHelper agent CLI.
//!
//! Subcommands:
//!   locate                 Print the detected EE.log path and probed candidates.
//!   parse [FILE]           Batch-parse a log (defaults to the located one) and
//!                          print each recognised event as a JSON line.
//!   watch [FILE]           Follow the log live, printing events as JSON lines.
//!
//! With no arguments it locates the log and starts watching.

use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use relichelper_agent::eelog::{self, watcher::LogWatcher};
use relichelper_agent::paths;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("watch");

    match cmd {
        "locate" => cmd_locate(),
        "parse" => cmd_parse(args.get(1).map(PathBuf::from)),
        "watch" => cmd_watch(args.get(1).map(PathBuf::from)),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("usage: relichelper-agent [locate|parse|watch] [FILE]");
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
