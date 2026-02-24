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
