// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What the children printed, kept per container.
//!
//! The console shows every line as it arrives, prefixed and coloured, which is
//! the right thing for someone watching a terminal and useless for anyone
//! arriving afterwards. This keeps a bounded copy so `compose::logs` can answer
//! for one container without the operator having to have been there.
//!
//! Bounded, not durable: a project that runs for a week must not grow a log in
//! the daemon's heap, and a daemon restart legitimately starts a new story.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};

/// Lines retained per container. Enough for a crash and its run-up; a worker
/// that needs its whole history should be writing a file.
const CAPACITY: usize = 500;

/// Which stream a line came from. Workers put failures on stderr, and losing
/// that distinction is losing the reason the operator came looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
    /// Written by compose itself, not by the worker. Used to explain a gap the
    /// reader would otherwise have to guess at — an adopted container's earlier
    /// output, for instance, went to a daemon that no longer exists.
    Compose,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    /// Position in this container's output, counted from its first line and
    /// never reused.
    ///
    /// A follower needs to know where it stopped, and content cannot say: a
    /// worker prints the same startup banner every time it restarts, so
    /// matching on text finds the newest copy and silently skips everything
    /// before it. The counter keeps growing while the buffer drops from the
    /// front, which is also what lets a follower notice it missed something.
    pub seq: u64,
    pub stream: Stream,
    pub text: String,
}

/// Per-container ring buffers, shared between the output pumps and the remote
/// surface.
#[derive(Debug, Default)]
pub struct LogStore {
    containers: Mutex<BTreeMap<String, Container>>,
}

/// One container's buffer plus the counter that outlives it.
#[derive(Debug, Default)]
struct Container {
    lines: VecDeque<LogLine>,
    /// Next sequence number. Kept across drops from the front, so a follower
    /// can tell "nothing new" from "I missed some".
    next_seq: u64,
}

impl LogStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one line, dropping the oldest once the buffer is full.
    pub fn append(&self, container: &str, stream: Stream, text: String) {
        let Ok(mut containers) = self.containers.lock() else {
            // A poisoned lock means a pump panicked mid-write. Logs are
            // diagnostics: losing a line beats taking the daemon down with it.
            return;
        };
        let entry = containers.entry(container.to_string()).or_default();
        if entry.lines.len() == CAPACITY {
            entry.lines.pop_front();
        }
        let seq = entry.next_seq;
        entry.next_seq += 1;
        entry.lines.push_back(LogLine { seq, stream, text });
    }

    /// The last `tail` lines for one container, oldest first. An unknown
    /// container answers empty — it may simply not have started yet.
    pub fn tail(&self, container: &str, tail: usize) -> Vec<LogLine> {
        let Ok(containers) = self.containers.lock() else {
            return Vec::new();
        };
        let Some(entry) = containers.get(container) else {
            return Vec::new();
        };
        entry
            .lines
            .iter()
            .skip(entry.lines.len().saturating_sub(tail))
            .cloned()
            .collect()
    }

    /// Every container that has produced output, with its tail.
    pub fn tail_all(&self, tail: usize) -> BTreeMap<String, Vec<LogLine>> {
        let Ok(containers) = self.containers.lock() else {
            return BTreeMap::new();
        };
        containers
            .keys()
            .map(|key| (key.clone(), self.tail_locked(&containers, key, tail)))
            .collect()
    }

    fn tail_locked(
        &self,
        containers: &BTreeMap<String, Container>,
        container: &str,
        tail: usize,
    ) -> Vec<LogLine> {
        containers
            .get(container)
            .map(|entry| {
                entry
                    .lines
                    .iter()
                    .skip(entry.lines.len().saturating_sub(tail))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Forgets a container's output. Called when it is torn down for good, so
    /// a long-lived daemon does not hold the logs of containers that no longer
    /// exist in the file.
    pub fn forget(&self, container: &str) {
        if let Ok(mut containers) = self.containers.lock() {
            containers.remove(container);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_come_back_oldest_first() {
        let logs = LogStore::new();
        for text in ["one", "two", "three"] {
            logs.append("api", Stream::Stdout, text.to_string());
        }

        let lines = logs.tail("api", 10);
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
    }

    #[test]
    fn tail_returns_the_end_not_the_beginning() {
        let logs = LogStore::new();
        for n in 0..10 {
            logs.append("api", Stream::Stdout, n.to_string());
        }

        let lines = logs.tail("api", 3);
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["7", "8", "9"]
        );
    }

    #[test]
    fn the_buffer_is_bounded_and_drops_the_oldest() {
        let logs = LogStore::new();
        for n in 0..(CAPACITY + 50) {
            logs.append("api", Stream::Stdout, n.to_string());
        }

        let lines = logs.tail("api", usize::MAX);
        assert_eq!(lines.len(), CAPACITY, "a long-running worker must not grow");
        assert_eq!(lines[0].text, "50", "the oldest lines are the ones dropped");
    }

    #[test]
    fn streams_stay_apart() {
        let logs = LogStore::new();
        logs.append("api", Stream::Stdout, "listening".to_string());
        logs.append("api", Stream::Stderr, "config missing".to_string());

        let lines = logs.tail("api", 10);
        assert_eq!(lines[0].stream, Stream::Stdout);
        assert_eq!(lines[1].stream, Stream::Stderr);
    }

    #[test]
    fn containers_do_not_share_a_buffer() {
        let logs = LogStore::new();
        logs.append("api", Stream::Stdout, "from api".to_string());
        logs.append("web", Stream::Stdout, "from web".to_string());

        assert_eq!(logs.tail("api", 10).len(), 1);
        assert_eq!(logs.tail("web", 10)[0].text, "from web");
        assert!(
            logs.tail("database", 10).is_empty(),
            "a container that never printed is empty, not an error"
        );
    }

    #[test]
    fn forgetting_one_container_leaves_the_others() {
        let logs = LogStore::new();
        logs.append("api", Stream::Stdout, "a".to_string());
        logs.append("web", Stream::Stdout, "b".to_string());

        logs.forget("api");
        assert!(logs.tail("api", 10).is_empty());
        assert_eq!(logs.tail("web", 10).len(), 1);
    }

    #[test]
    fn sequence_numbers_survive_the_buffer_dropping_lines() {
        let logs = LogStore::new();
        for n in 0..(CAPACITY + 10) {
            logs.append("api", Stream::Stdout, n.to_string());
        }

        let lines = logs.tail("api", usize::MAX);
        assert_eq!(lines[0].seq, 10, "the counter does not restart at the front");
        assert_eq!(lines.last().unwrap().seq, (CAPACITY + 9) as u64);
    }

    #[test]
    fn a_repeated_line_still_gets_its_own_number() {
        // A worker prints the same banner on every restart. Content cannot
        // tell a follower where it stopped; the counter can.
        let logs = LogStore::new();
        for _ in 0..3 {
            logs.append("api", Stream::Stdout, "listening".to_string());
        }

        let seqs: Vec<_> = logs.tail("api", 10).iter().map(|l| l.seq).collect();
        assert_eq!(seqs, [0, 1, 2]);
    }

    #[test]
    fn tail_all_covers_every_container_that_printed() {
        let logs = LogStore::new();
        logs.append("api", Stream::Stdout, "a".to_string());
        logs.append("web", Stream::Stderr, "b".to_string());

        let all = logs.tail_all(10);
        assert_eq!(all.keys().cloned().collect::<Vec<_>>(), ["api", "web"]);
        assert_eq!(all["web"][0].stream, Stream::Stderr);
    }
}
