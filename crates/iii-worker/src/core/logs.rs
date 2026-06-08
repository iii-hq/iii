// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `worker::logs` orchestrator.
//!
//! Unlike the lifecycle ops this is a pure host-filesystem read: no project
//! lock, no events, no host shim. The log directories live under the
//! daemon's `~/.iii`, not the project root, and reading them has no side
//! effects worth fanning out to `worker` trigger subscribers.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::core::error::WorkerOpError;
use crate::core::types::{LogsOptions, LogsOutcome, validate_worker_name};

/// Hard cap on `tail` so one call can't flood the bus.
pub const LOGS_TAIL_MAX: usize = 1000;

/// Only the trailing window of each log file is scanned, so a multi-GB log
/// can't blow up the daemon's memory or stall the dispatch loop.
const LOGS_READ_BYTES_MAX: u64 = 1024 * 1024;

/// Candidate log directories for a worker, newest layout first: the unified
/// location, then the legacy libkrun/OCI layout, then the legacy binary
/// layout. Mirrors what `iii worker logs` checks on the CLI.
pub fn candidate_log_dirs(home: &Path, name: &str) -> [PathBuf; 3] {
    [
        home.join(".iii/logs").join(name),
        home.join(".iii/managed").join(name).join("logs"),
        home.join(".iii/workers/logs").join(name),
    ]
}

/// The candidate whose `stdout.log`/`stderr.log` was modified most recently
/// (empty files don't count). Avoids picking a stale directory (e.g.
/// `~/.iii/logs/` from a binary worker) over the active one (e.g.
/// `~/.iii/managed/` from a libkrun OCI worker).
pub fn pick_best_logs_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;

    for dir in candidates {
        let latest = ["stdout.log", "stderr.log"]
            .iter()
            .map(|f| dir.join(f))
            .filter_map(|p| std::fs::metadata(&p).ok().map(|m| (p, m)))
            .filter(|(_, m)| m.len() > 0)
            .filter_map(|(_, m)| m.modified().ok())
            .max();

        if let Some(modified) = latest
            && best.as_ref().is_none_or(|(_, t)| modified > *t)
        {
            best = Some((dir.clone(), modified));
        }
    }

    best.map(|(dir, _)| dir)
}

/// Last `tail` lines of `path`, scanning at most the trailing
/// [`LOGS_READ_BYTES_MAX`] bytes. Missing/unreadable files yield no lines.
fn tail_lines(path: &Path, tail: usize) -> Vec<String> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(LOGS_READ_BYTES_MAX);
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        // The window almost certainly opened mid-line; drop the partial.
        lines.remove(0);
    }
    let skip = lines.len().saturating_sub(tail);
    lines[skip..].iter().map(|s| s.to_string()).collect()
}

pub async fn run(opts: LogsOptions) -> Result<LogsOutcome, WorkerOpError> {
    validate_worker_name(&opts.name).map_err(|reason| WorkerOpError::BadRequest {
        function_id: "worker::logs".into(),
        reason,
    })?;
    let tail = opts.tail.min(LOGS_TAIL_MAX);
    let home = dirs::home_dir().unwrap_or_default();

    let Some(dir) = pick_best_logs_dir(&candidate_log_dirs(&home, &opts.name)) else {
        return Ok(LogsOutcome {
            name: opts.name,
            logs_dir: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
    };

    Ok(LogsOutcome {
        stdout: tail_lines(&dir.join("stdout.log"), tail),
        stderr: tail_lines(&dir.join("stderr.log"), tail),
        logs_dir: Some(dir.display().to_string()),
        name: opts.name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::WorkerOpErrorKind;
    use tempfile::TempDir;

    fn write(dir: &Path, file: &str, contents: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(file), contents).unwrap();
    }

    #[test]
    fn pick_best_logs_dir_prefers_most_recent() {
        let tmp = TempDir::new().unwrap();
        let stale_dir = tmp.path().join("stale");
        let fresh_dir = tmp.path().join("fresh");
        write(&stale_dir, "stdout.log", "old\n");
        write(&fresh_dir, "stdout.log", "new\n");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let f = std::fs::File::options()
            .write(true)
            .open(stale_dir.join("stdout.log"))
            .unwrap();
        f.set_modified(old).unwrap();

        let result = pick_best_logs_dir(&[stale_dir, fresh_dir.clone()]).unwrap();
        assert_eq!(result, fresh_dir);
    }

    #[test]
    fn pick_best_logs_dir_skips_empty_files() {
        let tmp = TempDir::new().unwrap();
        let empty_dir = tmp.path().join("empty");
        let content_dir = tmp.path().join("content");
        write(&empty_dir, "stdout.log", "");
        write(&content_dir, "stderr.log", "boot\n");

        let result = pick_best_logs_dir(&[empty_dir, content_dir.clone()]).unwrap();
        assert_eq!(result, content_dir);
    }

    #[test]
    fn pick_best_logs_dir_returns_none_when_no_content() {
        let tmp = TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        write(&dir_a, "stdout.log", "");
        std::fs::create_dir_all(&dir_b).unwrap();
        assert!(pick_best_logs_dir(&[dir_a, dir_b]).is_none());
    }

    #[test]
    fn tail_lines_returns_last_n() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        std::fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
        assert_eq!(tail_lines(&path, 2), vec!["three", "four"]);
        assert_eq!(tail_lines(&path, 100).len(), 4);
        assert!(tail_lines(&tmp.path().join("missing.log"), 5).is_empty());
    }

    #[tokio::test]
    async fn run_rejects_traversal_names_with_bad_request() {
        for name in ["../escape", "", ".hidden", "a/b"] {
            let err = run(LogsOptions {
                name: name.to_string(),
                tail: 10,
            })
            .await
            .unwrap_err();
            assert_eq!(
                err.kind(),
                WorkerOpErrorKind::BadRequest,
                "name {name:?} must be rejected as W105"
            );
        }
    }

    #[tokio::test]
    async fn run_caps_tail_at_max() {
        // Unknown-but-valid worker: empty outcome, no error — the cap and
        // the no-logs path are both exercised without touching real $HOME
        // state for an unlikely name.
        let outcome = run(LogsOptions {
            name: "definitely-not-a-real-worker-name-xyz".to_string(),
            tail: usize::MAX,
        })
        .await
        .unwrap();
        assert!(outcome.logs_dir.is_none());
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.is_empty());
    }
}
