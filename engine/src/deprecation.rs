// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Rate-limited deprecation warnings for engine entry points scheduled for
//! removal (MOT-3619: iii-stream).
//!
//! A warning is keyed by `(entry_point, caller_key)` and logged at most once
//! per [`WARN_INTERVAL`] per key, so a hot call path costs one map lookup.
//! The key set is bounded by [`MAX_KEYS`]; once full (after dropping expired
//! keys), new callers share the [`FALLBACK_CALLER_KEY`] bucket.
//!
//! Warnings carry only caller metadata (worker id, name, runtime, SDK version,
//! or the remote IP for raw WebSocket clients). They never include payloads,
//! stream/group/item identifiers or values, headers, or query strings.

use std::{
    net::IpAddr,
    sync::LazyLock,
    time::{Duration, Instant},
};

use dashmap::{DashMap, DashSet, mapref::entry::Entry};
use uuid::Uuid;

use crate::worker_connections::WorkerConnectionRegistry;

/// Migration guide linked from every iii-stream deprecation message.
pub(crate) const STREAM_MIGRATION_GUIDE: &str =
    "https://iii.dev/docs/upgrading/migrate-from-streams";

/// Minimum time between two warnings for the same `(entry_point, caller)`.
pub(crate) const WARN_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Upper bound on tracked `(entry_point, caller)` keys.
pub(crate) const MAX_KEYS: usize = 1024;

/// Caller key shared by every new caller once [`MAX_KEYS`] is reached.
pub(crate) const FALLBACK_CALLER_KEY: &str = "*";

/// Caller key for in-process calls and engine-local events (startup).
pub(crate) const ENGINE_CALLER_KEY: &str = "engine";

/// Placeholder logged for caller fields that do not apply or are unknown.
const ABSENT: &str = "-";

/// The standard iii-stream deprecation sentence for `entry` (a function id,
/// trigger type, or entry-point name).
pub(crate) fn stream_deprecation_message(entry: &str) -> String {
    format!(
        "{entry} is deprecated (iii-stream) and will be removed in an upcoming release. \
         Behavior is unchanged for now. Migration guide: \
         {STREAM_MIGRATION_GUIDE}"
    )
}

/// Who reached a deprecated entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caller {
    /// A connected worker, identified by its connection UUID.
    Worker(Uuid),
    /// A raw client of a dedicated listener, identified by IP only (never the
    /// port, which would make the key set unbounded).
    Remote(IpAddr),
    /// In-process call or engine-local event.
    Engine,
}

impl Caller {
    /// Resolve the engine-injected `_caller_worker_id`. A missing or malformed
    /// value is attributed to the engine; the raw value is never logged.
    pub(crate) fn from_worker_id(id: Option<&str>) -> Self {
        id.and_then(|id| Uuid::parse_str(id).ok())
            .map_or(Self::Engine, Self::Worker)
    }

    /// Resolve an optional worker UUID (e.g. `Trigger::worker_id`).
    pub(crate) fn from_worker_uuid(id: Option<Uuid>) -> Self {
        id.map_or(Self::Engine, Self::Worker)
    }

    /// The rate-limit key for this caller.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Worker(id) => id.to_string(),
            Self::Remote(ip) => ip.to_string(),
            Self::Engine => ENGINE_CALLER_KEY.to_string(),
        }
    }
}

/// Per-key interval limiter with a bounded key set.
pub(crate) struct RateLimiter {
    last: DashMap<(&'static str, String), Instant>,
    interval: Duration,
    max_keys: usize,
}

impl RateLimiter {
    pub(crate) fn new(interval: Duration, max_keys: usize) -> Self {
        Self {
            last: DashMap::new(),
            interval,
            max_keys,
        }
    }

    /// True when `(entry, caller_key)` has not warned within the interval;
    /// records `now` for the key in that case. Check-then-insert happens under
    /// the map entry lock, so concurrent callers warn at most once.
    pub(crate) fn should_warn(&self, entry: &'static str, caller_key: &str, now: Instant) -> bool {
        let mut key = (entry, caller_key.to_string());
        if !self.last.contains_key(&key) && self.last.len() >= self.max_keys {
            // Make room by dropping expired keys before falling back.
            self.last
                .retain(|_, at| now.saturating_duration_since(*at) < self.interval);
            if self.last.len() >= self.max_keys {
                key.1 = FALLBACK_CALLER_KEY.to_string();
            }
        }
        match self.last.entry(key) {
            Entry::Occupied(mut slot) => {
                if now.saturating_duration_since(*slot.get()) >= self.interval {
                    slot.insert(now);
                    true
                } else {
                    false
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(now);
                true
            }
        }
    }

    /// Forget every key.
    #[cfg(test)]
    pub(crate) fn reset(&self) {
        self.last.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.last.len()
    }
}

static LIMITER: LazyLock<RateLimiter> = LazyLock::new(|| RateLimiter::new(WARN_INTERVAL, MAX_KEYS));
static WARNED_ONCE: LazyLock<DashSet<&'static str>> = LazyLock::new(DashSet::new);

/// Log the iii-stream deprecation warning for `entry` reached by `caller`,
/// at most once per [`WARN_INTERVAL`] per `(entry, caller)`. Worker details
/// are looked up in `registry` only when the warning is actually logged.
/// Returns true when it logged.
pub(crate) fn warn_stream_deprecated(
    entry: &'static str,
    caller: Caller,
    registry: &WorkerConnectionRegistry,
) -> bool {
    if !LIMITER.should_warn(entry, &caller.key(), Instant::now()) {
        return false;
    }
    emit(entry, caller, Some(registry));
    true
}

/// Log the iii-stream deprecation warning for an engine-local `entry` once per
/// process (worker startup, listener startup); a reload does not repeat it.
/// Returns true when it logged.
pub(crate) fn warn_stream_deprecated_once(entry: &'static str) -> bool {
    if !WARNED_ONCE.insert(entry) {
        return false;
    }
    emit(entry, Caller::Engine, None);
    true
}

/// Every field a deprecation warning carries besides the guide and the
/// sentence. Built only from caller metadata, never from call input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WarningFields {
    pub(crate) entry_point: &'static str,
    pub(crate) caller_worker_id: String,
    pub(crate) caller_worker_name: String,
    pub(crate) caller_runtime: String,
    pub(crate) caller_sdk_version: String,
    pub(crate) caller_ip: String,
}

impl WarningFields {
    fn resolve(
        entry: &'static str,
        caller: Caller,
        registry: Option<&WorkerConnectionRegistry>,
    ) -> Self {
        let absent = || ABSENT.to_string();
        match caller {
            Caller::Worker(id) => {
                let (name, runtime, version) = registry
                    .and_then(|registry| registry.workers.get(&id))
                    .map(|worker| {
                        (
                            worker.name.clone(),
                            worker.runtime.clone(),
                            worker.version.clone(),
                        )
                    })
                    .unwrap_or_default();
                let unknown = || "unknown".to_string();
                Self {
                    entry_point: entry,
                    caller_worker_id: id.to_string(),
                    caller_worker_name: name.unwrap_or_else(unknown),
                    caller_runtime: runtime.unwrap_or_else(unknown),
                    caller_sdk_version: version.unwrap_or_else(unknown),
                    caller_ip: absent(),
                }
            }
            Caller::Remote(ip) => Self {
                entry_point: entry,
                caller_worker_id: absent(),
                caller_worker_name: absent(),
                caller_runtime: absent(),
                caller_sdk_version: absent(),
                caller_ip: ip.to_string(),
            },
            Caller::Engine => Self {
                entry_point: entry,
                caller_worker_id: ENGINE_CALLER_KEY.to_string(),
                caller_worker_name: ENGINE_CALLER_KEY.to_string(),
                caller_runtime: absent(),
                caller_sdk_version: absent(),
                caller_ip: absent(),
            },
        }
    }
}

fn emit(entry: &'static str, caller: Caller, registry: Option<&WorkerConnectionRegistry>) {
    let fields = WarningFields::resolve(entry, caller, registry);
    tracing::warn!(
        target: "iii::deprecation",
        entry_point = %fields.entry_point,
        caller_worker_id = %fields.caller_worker_id,
        caller_worker_name = %fields.caller_worker_name,
        caller_runtime = %fields.caller_runtime,
        caller_sdk_version = %fields.caller_sdk_version,
        caller_ip = %fields.caller_ip,
        guide = %STREAM_MIGRATION_GUIDE,
        "{}",
        stream_deprecation_message(entry)
    );
    #[cfg(test)]
    test_support::record(fields);
}

/// Clear the global rate limiter (tests only). The once-per-process set is
/// deliberately kept: it models process lifetime, not an interval.
#[cfg(test)]
pub(crate) fn reset_for_tests() {
    LIMITER.reset();
}

/// Thread-local log capture for tests that assert on emitted warnings.
#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    /// Serializes tests that call [`super::reset_for_tests`] or count the
    /// global limiter's output, so a reset never lands between their calls.
    pub(crate) static GLOBAL_LIMITER_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    static EMITTED: std::sync::LazyLock<Mutex<Vec<super::WarningFields>>> =
        std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

    pub(super) fn record(fields: super::WarningFields) {
        EMITTED.lock().expect("emitted poisoned").push(fields);
    }

    /// Warnings emitted so far (process-wide) for one caller worker id.
    /// Thread-independent, unlike log capture through a scoped subscriber.
    pub(crate) fn emitted_for(caller_worker_id: &str) -> Vec<super::WarningFields> {
        EMITTED
            .lock()
            .expect("emitted poisoned")
            .iter()
            .filter(|fields| fields.caller_worker_id == caller_worker_id)
            .cloned()
            .collect()
    }

    #[derive(Clone, Default)]
    pub(crate) struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl LogBuffer {
        pub(crate) fn contents(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("log buffer poisoned")).into_owned()
        }

        /// Lines logged under the `iii::deprecation` target.
        pub(crate) fn deprecation_lines(&self) -> Vec<String> {
            self.contents()
                .lines()
                .filter(|line| line.contains("iii::deprecation"))
                .map(str::to_string)
                .collect()
        }
    }

    impl io::Write for LogBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("log buffer poisoned")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Capture WARN+ logs on the current thread until the guard drops. Use a
    /// current-thread runtime for async tests.
    pub(crate) fn capture() -> (tracing::subscriber::DefaultGuard, LogBuffer) {
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        (tracing::subscriber::set_default(subscriber), buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_connections::WorkerConnection;

    #[test]
    fn message_is_the_standard_sentence() {
        assert_eq!(
            stream_deprecation_message("stream::set"),
            "stream::set is deprecated (iii-stream) and will be removed in an upcoming release. \
             Behavior is unchanged for now. Migration guide: \
             https://iii.dev/docs/upgrading/migrate-from-streams"
        );
    }

    #[test]
    fn second_warning_within_interval_is_suppressed() {
        let limiter = RateLimiter::new(WARN_INTERVAL, MAX_KEYS);
        let t0 = Instant::now();
        assert!(limiter.should_warn("stream::set", "worker-a", t0));
        assert!(!limiter.should_warn("stream::set", "worker-a", t0 + Duration::from_secs(1)));
        assert!(!limiter.should_warn(
            "stream::set",
            "worker-a",
            t0 + WARN_INTERVAL - Duration::from_millis(1)
        ));
        assert!(limiter.should_warn("stream::set", "worker-a", t0 + WARN_INTERVAL));
    }

    #[test]
    fn distinct_callers_and_entries_each_warn() {
        let limiter = RateLimiter::new(WARN_INTERVAL, MAX_KEYS);
        let t0 = Instant::now();
        assert!(limiter.should_warn("stream::set", "worker-a", t0));
        assert!(limiter.should_warn("stream::set", "worker-b", t0));
        assert!(limiter.should_warn("stream::get", "worker-a", t0));
        assert!(!limiter.should_warn("stream::get", "worker-a", t0));
        assert_eq!(limiter.len(), 3);
    }

    #[test]
    fn cap_falls_back_to_shared_key_and_recovers_after_expiry() {
        let limiter = RateLimiter::new(WARN_INTERVAL, 3);
        let t0 = Instant::now();
        for caller in ["a", "b", "c"] {
            assert!(limiter.should_warn("stream::set", caller, t0));
        }
        // Full: a new caller lands in the shared "*" bucket and warns once...
        assert!(limiter.should_warn("stream::set", "d", t0));
        // ...and every further new caller shares that bucket.
        assert!(!limiter.should_warn("stream::set", "e", t0));
        assert_eq!(limiter.len(), 4);
        assert!(
            limiter
                .last
                .contains_key(&("stream::set", FALLBACK_CALLER_KEY.to_string()))
        );
        // Known callers keep their own key.
        assert!(!limiter.should_warn("stream::set", "a", t0));

        // After the interval expired keys are dropped and new callers are
        // tracked individually again.
        let later = t0 + WARN_INTERVAL;
        assert!(limiter.should_warn("stream::set", "f", later));
        assert!(limiter.last.contains_key(&("stream::set", "f".to_string())));
        assert_eq!(limiter.len(), 1);

        limiter.reset();
        assert_eq!(limiter.len(), 0);
    }

    #[test]
    fn caller_resolution() {
        let id = Uuid::new_v4();
        assert_eq!(
            Caller::from_worker_id(Some(&id.to_string())),
            Caller::Worker(id)
        );
        assert_eq!(Caller::from_worker_id(Some("not-a-uuid")), Caller::Engine);
        assert_eq!(Caller::from_worker_id(None), Caller::Engine);
        assert_eq!(Caller::from_worker_uuid(Some(id)), Caller::Worker(id));
        assert_eq!(Caller::from_worker_uuid(None), Caller::Engine);
        assert_eq!(Caller::Worker(id).key(), id.to_string());
        assert_eq!(Caller::Engine.key(), ENGINE_CALLER_KEY);
        let ip: IpAddr = "10.1.2.3".parse().unwrap();
        assert_eq!(Caller::Remote(ip).key(), "10.1.2.3");
    }

    #[test]
    fn global_warning_logs_caller_fields_once_and_reset_rearms_it() {
        let registry = WorkerConnectionRegistry::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut worker = WorkerConnection::new(tx);
        worker.name = Some("orders-worker".to_string());
        worker.runtime = Some("node".to_string());
        worker.version = Some("0.24.0".to_string());
        let id = worker.id;
        registry.workers.insert(id, worker);
        let caller = Caller::Worker(id);

        let _lock = test_support::GLOBAL_LIMITER_LOCK.blocking_lock();
        let (_guard, logs) = test_support::capture();
        assert!(warn_stream_deprecated("stream::set", caller, &registry));
        assert!(!warn_stream_deprecated("stream::set", caller, &registry));

        let lines = logs.deprecation_lines();
        assert_eq!(lines.len(), 1, "expected exactly one warning: {lines:?}");
        let line = &lines[0];
        assert!(line.contains("WARN"), "{line}");
        assert!(line.contains("entry_point=stream::set"), "{line}");
        assert!(line.contains(&format!("caller_worker_id={id}")), "{line}");
        assert!(line.contains("caller_worker_name=orders-worker"), "{line}");
        assert!(line.contains("caller_runtime=node"), "{line}");
        assert!(line.contains("caller_sdk_version=0.24.0"), "{line}");
        assert!(
            line.contains(&format!("guide={STREAM_MIGRATION_GUIDE}")),
            "{line}"
        );
        assert!(
            line.contains(&stream_deprecation_message("stream::set")),
            "{line}"
        );

        assert_eq!(
            test_support::emitted_for(&id.to_string()),
            vec![WarningFields {
                entry_point: "stream::set",
                caller_worker_id: id.to_string(),
                caller_worker_name: "orders-worker".to_string(),
                caller_runtime: "node".to_string(),
                caller_sdk_version: "0.24.0".to_string(),
                caller_ip: "-".to_string(),
            }]
        );

        reset_for_tests();
        assert!(warn_stream_deprecated("stream::set", caller, &registry));
    }

    #[test]
    fn function_not_found_hint_uses_the_same_sentence_and_guide() {
        let message = crate::legacy_worker_functions::migration_message("stream::set")
            .expect("stream::set needs an actionable hint");
        assert!(message.contains(&stream_deprecation_message("stream::set")));
    }

    #[test]
    fn once_per_process_warning_does_not_repeat() {
        assert!(warn_stream_deprecated_once("test-only once entry"));
        assert!(!warn_stream_deprecated_once("test-only once entry"));
    }
}
