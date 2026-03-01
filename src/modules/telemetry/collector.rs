// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

/// Global telemetry collector with atomic counters for all module operations.
/// All counters use `Ordering::Relaxed` for zero overhead.
pub struct TelemetryCollector {
    // Cron
    pub cron_executions: AtomicU64,

    // Queue
    pub queue_emits: AtomicU64,
    pub queue_consumes: AtomicU64,

    // State
    pub state_sets: AtomicU64,
    pub state_gets: AtomicU64,
    pub state_deletes: AtomicU64,
    pub state_updates: AtomicU64,

    // Stream
    pub stream_sets: AtomicU64,
    pub stream_gets: AtomicU64,
    pub stream_deletes: AtomicU64,
    pub stream_lists: AtomicU64,
    pub stream_updates: AtomicU64,

    // PubSub
    pub pubsub_publishes: AtomicU64,
    pub pubsub_subscribes: AtomicU64,

    // KV
    pub kv_sets: AtomicU64,
    pub kv_gets: AtomicU64,
    pub kv_deletes: AtomicU64,

    // API
    pub api_requests: AtomicU64,

    // Registrations
    pub function_registrations: AtomicU64,
    pub trigger_registrations: AtomicU64,

    // Workers
    pub peak_active_workers: AtomicU64,
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self {
            cron_executions: AtomicU64::new(0),
            queue_emits: AtomicU64::new(0),
            queue_consumes: AtomicU64::new(0),
            state_sets: AtomicU64::new(0),
            state_gets: AtomicU64::new(0),
            state_deletes: AtomicU64::new(0),
            state_updates: AtomicU64::new(0),
            stream_sets: AtomicU64::new(0),
            stream_gets: AtomicU64::new(0),
            stream_deletes: AtomicU64::new(0),
            stream_lists: AtomicU64::new(0),
            stream_updates: AtomicU64::new(0),
            pubsub_publishes: AtomicU64::new(0),
            pubsub_subscribes: AtomicU64::new(0),
            kv_sets: AtomicU64::new(0),
            kv_gets: AtomicU64::new(0),
            kv_deletes: AtomicU64::new(0),
            api_requests: AtomicU64::new(0),
            function_registrations: AtomicU64::new(0),
            trigger_registrations: AtomicU64::new(0),
            peak_active_workers: AtomicU64::new(0),
        }
    }
}

impl TelemetryCollector {
    /// Returns all counter values as a JSON snapshot.
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "cron": {
                "executions": self.cron_executions.load(Ordering::Relaxed),
            },
            "queue": {
                "emits": self.queue_emits.load(Ordering::Relaxed),
                "consumes": self.queue_consumes.load(Ordering::Relaxed),
            },
            "state": {
                "sets": self.state_sets.load(Ordering::Relaxed),
                "gets": self.state_gets.load(Ordering::Relaxed),
                "deletes": self.state_deletes.load(Ordering::Relaxed),
                "updates": self.state_updates.load(Ordering::Relaxed),
            },
            "stream": {
                "sets": self.stream_sets.load(Ordering::Relaxed),
                "gets": self.stream_gets.load(Ordering::Relaxed),
                "deletes": self.stream_deletes.load(Ordering::Relaxed),
                "lists": self.stream_lists.load(Ordering::Relaxed),
                "updates": self.stream_updates.load(Ordering::Relaxed),
            },
            "pubsub": {
                "publishes": self.pubsub_publishes.load(Ordering::Relaxed),
                "subscribes": self.pubsub_subscribes.load(Ordering::Relaxed),
            },
            "kv": {
                "sets": self.kv_sets.load(Ordering::Relaxed),
                "gets": self.kv_gets.load(Ordering::Relaxed),
                "deletes": self.kv_deletes.load(Ordering::Relaxed),
            },
            "api": {
                "requests": self.api_requests.load(Ordering::Relaxed),
            },
            "registrations": {
                "functions": self.function_registrations.load(Ordering::Relaxed),
                "triggers": self.trigger_registrations.load(Ordering::Relaxed),
            },
            "workers": {
                "peak_active": self.peak_active_workers.load(Ordering::Relaxed),
            },
        })
    }
}

/// Global telemetry collector instance.
static TELEMETRY_COLLECTOR: OnceLock<TelemetryCollector> = OnceLock::new();

/// Get the global telemetry collector instance.
pub fn collector() -> &'static TelemetryCollector {
    TELEMETRY_COLLECTOR.get_or_init(TelemetryCollector::default)
}

// Convenience tracking functions

pub fn track_cron_execution() {
    collector().cron_executions.fetch_add(1, Ordering::Relaxed);
}

pub fn track_queue_emit() {
    collector().queue_emits.fetch_add(1, Ordering::Relaxed);
}

pub fn track_queue_consume() {
    collector().queue_consumes.fetch_add(1, Ordering::Relaxed);
}

pub fn track_state_set() {
    collector().state_sets.fetch_add(1, Ordering::Relaxed);
}

pub fn track_state_get() {
    collector().state_gets.fetch_add(1, Ordering::Relaxed);
}

pub fn track_state_delete() {
    collector().state_deletes.fetch_add(1, Ordering::Relaxed);
}

pub fn track_state_update() {
    collector().state_updates.fetch_add(1, Ordering::Relaxed);
}

pub fn track_stream_set() {
    collector().stream_sets.fetch_add(1, Ordering::Relaxed);
}

pub fn track_stream_get() {
    collector().stream_gets.fetch_add(1, Ordering::Relaxed);
}

pub fn track_stream_delete() {
    collector().stream_deletes.fetch_add(1, Ordering::Relaxed);
}

pub fn track_stream_list() {
    collector().stream_lists.fetch_add(1, Ordering::Relaxed);
}

pub fn track_stream_update() {
    collector().stream_updates.fetch_add(1, Ordering::Relaxed);
}

pub fn track_pubsub_publish() {
    collector().pubsub_publishes.fetch_add(1, Ordering::Relaxed);
}

pub fn track_pubsub_subscribe() {
    collector()
        .pubsub_subscribes
        .fetch_add(1, Ordering::Relaxed);
}

pub fn track_kv_set() {
    collector().kv_sets.fetch_add(1, Ordering::Relaxed);
}

pub fn track_kv_get() {
    collector().kv_gets.fetch_add(1, Ordering::Relaxed);
}

pub fn track_kv_delete() {
    collector().kv_deletes.fetch_add(1, Ordering::Relaxed);
}

pub fn track_api_request() {
    collector().api_requests.fetch_add(1, Ordering::Relaxed);
}

pub fn track_function_registered() {
    collector()
        .function_registrations
        .fetch_add(1, Ordering::Relaxed);
}

pub fn track_trigger_registered() {
    collector()
        .trigger_registrations
        .fetch_add(1, Ordering::Relaxed);
}

pub fn track_peak_workers(current_active: u64) {
    let peak = &collector().peak_active_workers;
    loop {
        let prev = peak.load(Ordering::Relaxed);
        if current_active <= prev {
            break;
        }
        if peak
            .compare_exchange_weak(prev, current_active, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // TelemetryCollector::default
    // =========================================================================

    #[test]
    fn test_default_all_counters_are_zero() {
        let c = TelemetryCollector::default();
        assert_eq!(c.cron_executions.load(Ordering::Relaxed), 0);
        assert_eq!(c.queue_emits.load(Ordering::Relaxed), 0);
        assert_eq!(c.queue_consumes.load(Ordering::Relaxed), 0);
        assert_eq!(c.state_sets.load(Ordering::Relaxed), 0);
        assert_eq!(c.state_gets.load(Ordering::Relaxed), 0);
        assert_eq!(c.state_deletes.load(Ordering::Relaxed), 0);
        assert_eq!(c.state_updates.load(Ordering::Relaxed), 0);
        assert_eq!(c.stream_sets.load(Ordering::Relaxed), 0);
        assert_eq!(c.stream_gets.load(Ordering::Relaxed), 0);
        assert_eq!(c.stream_deletes.load(Ordering::Relaxed), 0);
        assert_eq!(c.stream_lists.load(Ordering::Relaxed), 0);
        assert_eq!(c.stream_updates.load(Ordering::Relaxed), 0);
        assert_eq!(c.pubsub_publishes.load(Ordering::Relaxed), 0);
        assert_eq!(c.pubsub_subscribes.load(Ordering::Relaxed), 0);
        assert_eq!(c.kv_sets.load(Ordering::Relaxed), 0);
        assert_eq!(c.kv_gets.load(Ordering::Relaxed), 0);
        assert_eq!(c.kv_deletes.load(Ordering::Relaxed), 0);
        assert_eq!(c.api_requests.load(Ordering::Relaxed), 0);
        assert_eq!(c.function_registrations.load(Ordering::Relaxed), 0);
        assert_eq!(c.trigger_registrations.load(Ordering::Relaxed), 0);
        assert_eq!(c.peak_active_workers.load(Ordering::Relaxed), 0);
    }

    // =========================================================================
    // TelemetryCollector::snapshot
    // =========================================================================

    #[test]
    fn test_snapshot_default_all_zeros() {
        let c = TelemetryCollector::default();
        let snap = c.snapshot();

        assert_eq!(snap["cron"]["executions"], 0);
        assert_eq!(snap["queue"]["emits"], 0);
        assert_eq!(snap["queue"]["consumes"], 0);
        assert_eq!(snap["state"]["sets"], 0);
        assert_eq!(snap["state"]["gets"], 0);
        assert_eq!(snap["state"]["deletes"], 0);
        assert_eq!(snap["state"]["updates"], 0);
        assert_eq!(snap["stream"]["sets"], 0);
        assert_eq!(snap["stream"]["gets"], 0);
        assert_eq!(snap["stream"]["deletes"], 0);
        assert_eq!(snap["stream"]["lists"], 0);
        assert_eq!(snap["stream"]["updates"], 0);
        assert_eq!(snap["pubsub"]["publishes"], 0);
        assert_eq!(snap["pubsub"]["subscribes"], 0);
        assert_eq!(snap["kv"]["sets"], 0);
        assert_eq!(snap["kv"]["gets"], 0);
        assert_eq!(snap["kv"]["deletes"], 0);
        assert_eq!(snap["api"]["requests"], 0);
        assert_eq!(snap["registrations"]["functions"], 0);
        assert_eq!(snap["registrations"]["triggers"], 0);
        assert_eq!(snap["workers"]["peak_active"], 0);
    }

    #[test]
    fn test_snapshot_reflects_incremented_counters() {
        let c = TelemetryCollector::default();

        c.cron_executions.fetch_add(5, Ordering::Relaxed);
        c.queue_emits.fetch_add(10, Ordering::Relaxed);
        c.queue_consumes.fetch_add(8, Ordering::Relaxed);
        c.state_sets.fetch_add(3, Ordering::Relaxed);
        c.state_gets.fetch_add(7, Ordering::Relaxed);
        c.api_requests.fetch_add(42, Ordering::Relaxed);
        c.function_registrations.fetch_add(2, Ordering::Relaxed);
        c.trigger_registrations.fetch_add(4, Ordering::Relaxed);
        c.peak_active_workers.store(6, Ordering::Relaxed);
        c.kv_sets.fetch_add(11, Ordering::Relaxed);
        c.kv_gets.fetch_add(20, Ordering::Relaxed);
        c.kv_deletes.fetch_add(1, Ordering::Relaxed);
        c.pubsub_publishes.fetch_add(15, Ordering::Relaxed);
        c.pubsub_subscribes.fetch_add(9, Ordering::Relaxed);
        c.stream_sets.fetch_add(13, Ordering::Relaxed);
        c.stream_updates.fetch_add(2, Ordering::Relaxed);

        let snap = c.snapshot();

        assert_eq!(snap["cron"]["executions"], 5);
        assert_eq!(snap["queue"]["emits"], 10);
        assert_eq!(snap["queue"]["consumes"], 8);
        assert_eq!(snap["state"]["sets"], 3);
        assert_eq!(snap["state"]["gets"], 7);
        assert_eq!(snap["api"]["requests"], 42);
        assert_eq!(snap["registrations"]["functions"], 2);
        assert_eq!(snap["registrations"]["triggers"], 4);
        assert_eq!(snap["workers"]["peak_active"], 6);
        assert_eq!(snap["kv"]["sets"], 11);
        assert_eq!(snap["kv"]["gets"], 20);
        assert_eq!(snap["kv"]["deletes"], 1);
        assert_eq!(snap["pubsub"]["publishes"], 15);
        assert_eq!(snap["pubsub"]["subscribes"], 9);
        assert_eq!(snap["stream"]["sets"], 13);
        assert_eq!(snap["stream"]["updates"], 2);
    }

    #[test]
    fn test_snapshot_has_all_top_level_keys() {
        let c = TelemetryCollector::default();
        let snap = c.snapshot();
        let expected_keys = [
            "cron",
            "queue",
            "state",
            "stream",
            "pubsub",
            "kv",
            "api",
            "registrations",
            "workers",
        ];
        for key in &expected_keys {
            assert!(
                snap.get(key).is_some(),
                "snapshot should have top-level key '{}'",
                key
            );
        }
    }

    #[test]
    fn test_snapshot_is_valid_json_object() {
        let c = TelemetryCollector::default();
        let snap = c.snapshot();
        assert!(snap.is_object(), "snapshot should be a JSON object");
    }

    // =========================================================================
    // Global collector singleton
    // =========================================================================

    #[test]
    fn test_global_collector_returns_same_instance() {
        let c1 = collector() as *const TelemetryCollector;
        let c2 = collector() as *const TelemetryCollector;
        assert_eq!(c1, c2, "collector() should always return the same instance");
    }

    // =========================================================================
    // Convenience tracking functions
    // =========================================================================

    #[test]
    fn test_track_cron_execution_increments() {
        let before = collector().cron_executions.load(Ordering::Relaxed);
        track_cron_execution();
        let after = collector().cron_executions.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_queue_emit_increments() {
        let before = collector().queue_emits.load(Ordering::Relaxed);
        track_queue_emit();
        let after = collector().queue_emits.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_queue_consume_increments() {
        let before = collector().queue_consumes.load(Ordering::Relaxed);
        track_queue_consume();
        let after = collector().queue_consumes.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_state_set_increments() {
        let before = collector().state_sets.load(Ordering::Relaxed);
        track_state_set();
        let after = collector().state_sets.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_state_get_increments() {
        let before = collector().state_gets.load(Ordering::Relaxed);
        track_state_get();
        let after = collector().state_gets.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_state_delete_increments() {
        let before = collector().state_deletes.load(Ordering::Relaxed);
        track_state_delete();
        let after = collector().state_deletes.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_state_update_increments() {
        let before = collector().state_updates.load(Ordering::Relaxed);
        track_state_update();
        let after = collector().state_updates.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_stream_set_increments() {
        let before = collector().stream_sets.load(Ordering::Relaxed);
        track_stream_set();
        let after = collector().stream_sets.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_stream_get_increments() {
        let before = collector().stream_gets.load(Ordering::Relaxed);
        track_stream_get();
        let after = collector().stream_gets.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_stream_delete_increments() {
        let before = collector().stream_deletes.load(Ordering::Relaxed);
        track_stream_delete();
        let after = collector().stream_deletes.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_stream_list_increments() {
        let before = collector().stream_lists.load(Ordering::Relaxed);
        track_stream_list();
        let after = collector().stream_lists.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_stream_update_increments() {
        let before = collector().stream_updates.load(Ordering::Relaxed);
        track_stream_update();
        let after = collector().stream_updates.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_pubsub_publish_increments() {
        let before = collector().pubsub_publishes.load(Ordering::Relaxed);
        track_pubsub_publish();
        let after = collector().pubsub_publishes.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_pubsub_subscribe_increments() {
        let before = collector().pubsub_subscribes.load(Ordering::Relaxed);
        track_pubsub_subscribe();
        let after = collector().pubsub_subscribes.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_kv_set_increments() {
        let before = collector().kv_sets.load(Ordering::Relaxed);
        track_kv_set();
        let after = collector().kv_sets.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_kv_get_increments() {
        let before = collector().kv_gets.load(Ordering::Relaxed);
        track_kv_get();
        let after = collector().kv_gets.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_kv_delete_increments() {
        let before = collector().kv_deletes.load(Ordering::Relaxed);
        track_kv_delete();
        let after = collector().kv_deletes.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_api_request_increments() {
        let before = collector().api_requests.load(Ordering::Relaxed);
        track_api_request();
        let after = collector().api_requests.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_function_registered_increments() {
        let before = collector().function_registrations.load(Ordering::Relaxed);
        track_function_registered();
        let after = collector().function_registrations.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn test_track_trigger_registered_increments() {
        let before = collector().trigger_registrations.load(Ordering::Relaxed);
        track_trigger_registered();
        let after = collector().trigger_registrations.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    // =========================================================================
    // track_peak_workers (CAS logic)
    // =========================================================================

    #[test]
    fn test_track_peak_workers_updates_when_higher() {
        let c = TelemetryCollector::default();
        c.peak_active_workers.store(0, Ordering::Relaxed);

        // Simulate tracking via the global collector
        // We test the standalone TelemetryCollector CAS logic directly.
        let peak = &c.peak_active_workers;

        // Set peak to 5 manually
        peak.store(5, Ordering::Relaxed);

        // 10 > 5 => should update
        let new_val = 10u64;
        loop {
            let prev = peak.load(Ordering::Relaxed);
            if new_val <= prev {
                break;
            }
            if peak
                .compare_exchange_weak(prev, new_val, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
        assert_eq!(peak.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn test_track_peak_workers_does_not_decrease() {
        let c = TelemetryCollector::default();
        c.peak_active_workers.store(20, Ordering::Relaxed);

        let peak = &c.peak_active_workers;
        // 5 < 20 => should NOT update
        let new_val = 5u64;
        loop {
            let prev = peak.load(Ordering::Relaxed);
            if new_val <= prev {
                break;
            }
            if peak
                .compare_exchange_weak(prev, new_val, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
        assert_eq!(peak.load(Ordering::Relaxed), 20);
    }

    #[test]
    fn test_track_peak_workers_equal_value_no_change() {
        let c = TelemetryCollector::default();
        c.peak_active_workers.store(15, Ordering::Relaxed);

        let peak = &c.peak_active_workers;
        let new_val = 15u64;
        loop {
            let prev = peak.load(Ordering::Relaxed);
            if new_val <= prev {
                break;
            }
            if peak
                .compare_exchange_weak(prev, new_val, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
        assert_eq!(peak.load(Ordering::Relaxed), 15);
    }
}
