//! EE.log parsing and following.

pub mod event;
pub mod parser;
pub mod watcher;

pub use event::{LogEvent, RefinementTier};
pub use parser::{parse_line, ParsedLine};

use std::io::BufRead;

/// Parses every line from a reader, returning all recognised events in order.
/// Convenient for batch processing an existing log (e.g. verification, tests).
pub fn parse_reader<R: BufRead>(reader: R) -> Vec<ParsedLine> {
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| parse_line(&line))
        .collect()
}
