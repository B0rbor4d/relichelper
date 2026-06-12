//! Follows EE.log like `tail -f`, emitting parsed events as new lines are
//! flushed by the game.
//!
//! Handles log rotation/truncation: when Warframe restarts it recreates or
//! truncates EE.log, so if the file shrinks below our last read offset we reset
//! to the start.
//!
//! Caveat: Warframe buffers log writes, so reward events may be flushed only
//! after the on-screen window is already gone. Time-critical consumers should
//! treat the log as one trigger source and pair it with screen detection
//! (handled later in the OCR layer), not rely on it for sub-second latency.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use super::parser::{parse_line, ParsedLine};

/// Stateful follower over a single EE.log file.
pub struct LogWatcher {
    path: PathBuf,
    offset: u64,
}

impl LogWatcher {
    /// Creates a watcher that will emit only events written *after* creation.
    /// Use [`LogWatcher::from_start`] to replay the existing file first.
    pub fn new<P: Into<PathBuf>>(path: P) -> std::io::Result<Self> {
        let path = path.into();
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(Self { path, offset })
    }

    /// Creates a watcher positioned at the start of the file, so the first
    /// [`LogWatcher::poll`] replays all existing events.
    pub fn from_start<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            offset: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads everything appended since the last poll and returns the recognised
    /// events. Resets to the file start if the file was truncated/rotated.
    pub fn poll(&mut self) -> std::io::Result<Vec<ParsedLine>> {
        let mut file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let len = file.metadata()?.len();
        if len < self.offset {
            // File was truncated or rotated; start over.
            self.offset = 0;
        }

        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut consumed = 0u64;
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            // Only advance past complete lines (ending in '\n') so a half-written
            // buffered line is re-read on the next poll.
            if !line.ends_with('\n') {
                break;
            }
            consumed += n as u64;
            if let Some(parsed) = parse_line(&line) {
                events.push(parsed);
            }
        }

        self.offset += consumed;
        Ok(events)
    }

    /// Blocks, invoking `on_event` for each recognised event as the file grows.
    /// Combines filesystem notifications with a periodic poll so buffered writes
    /// are still picked up even if no fs event fires.
    pub fn watch<F>(&mut self, mut on_event: F) -> notify::Result<()>
    where
        F: FnMut(&ParsedLine),
    {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;

        // Watch the parent directory: the file itself may be replaced on rotation.
        let dir = self
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(_) | Err(RecvTimeoutError::Timeout) => {
                    if let Ok(events) = self.poll() {
                        for e in &events {
                            on_event(e);
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    }
}
