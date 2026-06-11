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

/// Strip terminal escape sequences (CSI `ESC[…m`-style, OSC `ESC]…BEL/ST`,
/// single-char escapes) and remaining control bytes (tabs survive) from one
/// log line. Worker logs are written by tools that assume a TTY (npm's
/// colored output, braille spinners, cursor-rewrite frames); over the bus
/// they reach LLM/automation callers where the escapes are pure token noise
/// and can even rewrite the reader's terminal when echoed.
pub fn strip_terminal_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                // CSI: ESC [ … final byte in @..=~
                Some('[') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c2) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] … terminated by BEL or ST (ESC \)
                Some(']') => {
                    chars.next();
                    while let Some(c2) = chars.next() {
                        if c2 == '\u{7}' {
                            break;
                        }
                        if c2 == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Single-char escape (ESC X)
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    out
}

/// True when a stripped line carries no information for a non-TTY reader:
/// blank, or nothing but braille spinner glyphs (U+2800..=U+28FF) and
/// whitespace — the residue of progress-spinner redraw frames.
fn is_spinner_noise(stripped: &str) -> bool {
    stripped
        .chars()
        .all(|c| c.is_whitespace() || ('\u{2800}'..='\u{28ff}').contains(&c))
}

/// Default (non-`raw`) presentation: strip terminal controls and drop
/// spinner-residue frames.
pub fn sanitize_lines(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| strip_terminal_controls(&l))
        .filter(|l| !is_spinner_noise(l))
        .collect()
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

    let stdout = tail_lines(&dir.join("stdout.log"), tail);
    let stderr = tail_lines(&dir.join("stderr.log"), tail);
    let (stdout, stderr) = if opts.raw {
        (stdout, stderr)
    } else {
        (sanitize_lines(stdout), sanitize_lines(stderr))
    };

    Ok(LogsOutcome {
        stdout,
        stderr,
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
                raw: false,
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

    #[test]
    fn strip_terminal_controls_removes_csi_osc_and_controls() {
        // npm-style colored line
        assert_eq!(
            strip_terminal_controls("\u{1b}[1mnpm\u{1b}[22m \u{1b}[31merror\u{1b}[39m 404"),
            "npm error 404"
        );
        // OSC title-set terminated by BEL, then text
        assert_eq!(strip_terminal_controls("\u{1b}]2;evil\u{7}ok"), "ok");
        // cursor-rewrite spinner frame residue
        assert_eq!(strip_terminal_controls("⠼\u{1b}[1G\u{1b}[0K"), "⠼");
        // tabs survive, other C0 controls don't
        assert_eq!(strip_terminal_controls("a\tb\u{8}c"), "a\tbc");
    }

    #[test]
    fn strip_terminal_controls_terminates_osc_on_st_terminator() {
        assert_eq!(
            strip_terminal_controls("\u{1b}]0;set title\u{1b}\\visible"),
            "visible"
        );
    }

    #[test]
    fn strip_terminal_controls_consumes_single_char_escapes() {
        // ESC M (reverse index) and ESC 7 (save cursor): the char after
        // ESC is swallowed, surrounding text survives.
        assert_eq!(strip_terminal_controls("\u{1b}Mup"), "up");
        assert_eq!(strip_terminal_controls("a\u{1b}7b"), "ab");
    }

    #[test]
    fn strip_terminal_controls_drops_trailing_escape() {
        assert_eq!(strip_terminal_controls("tail\u{1b}"), "tail");
    }

    #[test]
    fn sanitize_lines_drops_spinner_residue_and_blanks() {
        let lines = vec![
            "⠙\u{1b}[1G\u{1b}[0K⠹\u{1b}[1G\u{1b}[0K".to_string(), // pure spinner
            "".to_string(),
            "\u{1b}[32mreal output\u{1b}[0m".to_string(),
        ];
        assert_eq!(sanitize_lines(lines), vec!["real output".to_string()]);
    }

    #[tokio::test]
    async fn run_caps_tail_at_max() {
        // Unknown-but-valid worker: empty outcome, no error — the cap and
        // the no-logs path are both exercised without touching real $HOME
        // state for an unlikely name.
        let outcome = run(LogsOptions {
            name: "definitely-not-a-real-worker-name-xyz".to_string(),
            tail: usize::MAX,
            raw: false,
        })
        .await
        .unwrap();
        assert!(outcome.logs_dir.is_none());
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.is_empty());
    }

    /// Restores the original `HOME` on drop so the override can't leak
    /// into sibling tests. Mirrors `cli::status::tests::ProbeEnvGuard`;
    /// callers must hold `test_support::lock_home` for the whole body.
    struct HomeGuard {
        original: Option<std::ffi::OsString>,
    }

    impl HomeGuard {
        fn set(home: &Path) -> Self {
            let original = std::env::var_os("HOME");
            // SAFETY: test-only, serialized via test_support::lock_home.
            unsafe { std::env::set_var("HOME", home) };
            Self { original }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: test-only, serialized via test_support::lock_home.
            unsafe {
                match &self.original {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    // lock_home is intentionally held across run().await: it serializes the
    // process-global HOME mutation against sibling tests.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn run_sanitizes_logs_from_best_dir_by_default() {
        let _g = crate::cli::test_support::lock_home();
        let tmp = TempDir::new().unwrap();
        let _home = HomeGuard::set(tmp.path());
        let logs_dir = tmp.path().join(".iii/logs/sanitize-target");
        write(
            &logs_dir,
            "stdout.log",
            "\u{1b}[32mready\u{1b}[0m\n⠙\u{1b}[1G\u{1b}[0K\n",
        );
        write(&logs_dir, "stderr.log", "\u{1b}[31moops\u{1b}[39m\n");

        let outcome = run(LogsOptions {
            name: "sanitize-target".to_string(),
            tail: 10,
            raw: false,
        })
        .await
        .unwrap();

        assert_eq!(outcome.stdout, vec!["ready".to_string()]);
        assert_eq!(outcome.stderr, vec!["oops".to_string()]);
        assert_eq!(outcome.logs_dir, Some(logs_dir.display().to_string()));
        assert_eq!(outcome.name, "sanitize-target");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn run_raw_preserves_escapes_and_spinner_frames() {
        let _g = crate::cli::test_support::lock_home();
        let tmp = TempDir::new().unwrap();
        let _home = HomeGuard::set(tmp.path());
        let logs_dir = tmp.path().join(".iii/logs/raw-target");
        write(
            &logs_dir,
            "stdout.log",
            "\u{1b}[32mready\u{1b}[0m\n⠙\u{1b}[1G\u{1b}[0K\n",
        );

        let outcome = run(LogsOptions {
            name: "raw-target".to_string(),
            tail: 10,
            raw: true,
        })
        .await
        .unwrap();

        assert_eq!(
            outcome.stdout,
            vec![
                "\u{1b}[32mready\u{1b}[0m".to_string(),
                "⠙\u{1b}[1G\u{1b}[0K".to_string(),
            ]
        );
        assert!(outcome.stderr.is_empty());
        assert_eq!(outcome.logs_dir, Some(logs_dir.display().to_string()));
    }
}
