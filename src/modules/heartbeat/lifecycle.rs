// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

const MAX_EVENTS: usize = 10_000;
const MAX_FAILURES_PER_FUNCTION_PER_MINUTE: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    FirstConnection,
    FirstSuccessfulCall,
    RegisterFunction,
    RegisterTrigger,
    FailureWithin24h,
    InstallFailed,
    InstallSuccess,
    ProjectCreated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub kind: EventKind,
    pub timestamp: String,
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

struct FailureRateLimit {
    timestamps: VecDeque<Instant>,
}

struct TrackerState {
    events: VecDeque<LifecycleEvent>,
    pending: Vec<LifecycleEvent>,
    failure_rate_limits: HashMap<String, FailureRateLimit>,
}

pub struct LifecycleTracker {
    start_time: Instant,
    first_connection: AtomicBool,
    first_successful_call: AtomicBool,
    state: parking_lot::RwLock<TrackerState>,
    instance_id: OnceLock<String>,
}

impl LifecycleTracker {
    pub(crate) fn new() -> Self {
        Self {
            start_time: Instant::now(),
            first_connection: AtomicBool::new(false),
            first_successful_call: AtomicBool::new(false),
            state: parking_lot::RwLock::new(TrackerState {
                events: VecDeque::new(),
                pending: Vec::new(),
                failure_rate_limits: HashMap::new(),
            }),
            instance_id: OnceLock::new(),
        }
    }

    pub fn set_instance_id(&self, id: String) {
        let _ = self.instance_id.set(id);
    }

    fn instance_id(&self) -> String {
        self.instance_id
            .get()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn make_event(&self, kind: EventKind, metadata: Option<Value>) -> LifecycleEvent {
        LifecycleEvent {
            kind,
            timestamp: chrono::Utc::now().to_rfc3339(),
            instance_id: self.instance_id(),
            metadata,
        }
    }

    fn push_event(&self, event: LifecycleEvent) {
        let mut state = self.state.write();
        if state.events.len() >= MAX_EVENTS {
            state.events.pop_front();
        }
        state.events.push_back(event.clone());
        state.pending.push(event);
    }

    pub fn record_first(&self, kind: EventKind, metadata: Option<Value>) {
        let flag = match kind {
            EventKind::FirstConnection => &self.first_connection,
            EventKind::FirstSuccessfulCall => &self.first_successful_call,
            _ => return,
        };

        if flag
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            let event = self.make_event(kind, metadata);
            self.push_event(event);
        }
    }

    pub fn record_registration(&self, kind: EventKind, metadata: Option<Value>) {
        match kind {
            EventKind::RegisterFunction | EventKind::RegisterTrigger => {}
            _ => return,
        }
        let event = self.make_event(kind, metadata);
        self.push_event(event);
    }

    pub fn record_failure(&self, metadata: Option<Value>) {
        let elapsed = self.start_time.elapsed();
        if elapsed.as_secs() > 86400 {
            return;
        }

        let function_id = metadata
            .as_ref()
            .and_then(|m| m.get("function_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let now = Instant::now();
        let one_minute_ago = now - std::time::Duration::from_secs(60);

        {
            let mut state = self.state.write();
            let rate_limit = state
                .failure_rate_limits
                .entry(function_id)
                .or_insert_with(|| FailureRateLimit {
                    timestamps: VecDeque::new(),
                });

            while rate_limit
                .timestamps
                .front()
                .is_some_and(|t| *t < one_minute_ago)
            {
                rate_limit.timestamps.pop_front();
            }

            if rate_limit.timestamps.len() >= MAX_FAILURES_PER_FUNCTION_PER_MINUTE {
                return;
            }

            rate_limit.timestamps.push_back(now);
        }

        let event = self.make_event(EventKind::FailureWithin24h, metadata);
        self.push_event(event);
    }

    pub fn record_sdk_event(&self, kind: EventKind, metadata: Option<Value>) {
        match kind {
            EventKind::InstallFailed | EventKind::InstallSuccess | EventKind::ProjectCreated => {}
            _ => return,
        }
        let event = self.make_event(kind, metadata);
        self.push_event(event);
    }

    pub fn take_pending(&self) -> Vec<LifecycleEvent> {
        let mut state = self.state.write();
        std::mem::take(&mut state.pending)
    }

    pub fn requeue_pending(&self, events: Vec<LifecycleEvent>) {
        if events.is_empty() {
            return;
        }
        let mut state = self.state.write();
        let mut restored = events;
        restored.append(&mut state.pending);
        state.pending = restored;
    }

    pub fn all_events(&self) -> Vec<LifecycleEvent> {
        let state = self.state.read();
        state.events.iter().cloned().collect()
    }
}

static LIFECYCLE_TRACKER: OnceLock<LifecycleTracker> = OnceLock::new();

pub fn get_lifecycle_tracker() -> &'static LifecycleTracker {
    LIFECYCLE_TRACKER.get_or_init(LifecycleTracker::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> LifecycleTracker {
        let t = LifecycleTracker::new();
        t.set_instance_id("test-instance-id".to_string());
        t
    }

    #[test]
    fn first_connection_recorded_once() {
        let t = tracker();
        t.record_first(EventKind::FirstConnection, None);
        t.record_first(EventKind::FirstConnection, None);
        let events = t.all_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::FirstConnection);
    }

    #[test]
    fn first_successful_call_recorded_once() {
        let t = tracker();
        t.record_first(EventKind::FirstSuccessfulCall, None);
        t.record_first(EventKind::FirstSuccessfulCall, None);
        assert_eq!(t.all_events().len(), 1);
    }

    #[test]
    fn record_registration_filters_invalid_kinds() {
        let t = tracker();
        t.record_registration(EventKind::FirstConnection, None);
        assert_eq!(t.all_events().len(), 0);
        t.record_registration(EventKind::RegisterFunction, None);
        assert_eq!(t.all_events().len(), 1);
    }

    #[test]
    fn failure_rate_limiting() {
        let t = tracker();
        let meta = Some(serde_json::json!({"function_id": "my.func"}));
        for _ in 0..20 {
            t.record_failure(meta.clone());
        }
        let events: Vec<_> = t
            .all_events()
            .into_iter()
            .filter(|e| e.kind == EventKind::FailureWithin24h)
            .collect();
        assert_eq!(events.len(), MAX_FAILURES_PER_FUNCTION_PER_MINUTE);
    }

    #[test]
    fn events_bounded_by_max() {
        let t = tracker();
        for i in 0..(MAX_EVENTS + 100) {
            t.record_registration(
                EventKind::RegisterFunction,
                Some(serde_json::json!({"i": i})),
            );
        }
        assert_eq!(t.all_events().len(), MAX_EVENTS);
    }

    #[test]
    fn take_pending_and_requeue() {
        let t = tracker();
        t.record_registration(EventKind::RegisterFunction, None);
        t.record_registration(EventKind::RegisterTrigger, None);
        let pending = t.take_pending();
        assert_eq!(pending.len(), 2);
        assert_eq!(t.take_pending().len(), 0);

        t.requeue_pending(pending);
        let restored = t.take_pending();
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn sdk_event_filters_invalid_kinds() {
        let t = tracker();
        t.record_sdk_event(EventKind::FirstConnection, None);
        assert_eq!(t.all_events().len(), 0);
        t.record_sdk_event(EventKind::InstallSuccess, None);
        assert_eq!(t.all_events().len(), 1);
    }

    #[test]
    fn event_kind_serde_roundtrip() {
        let kind = EventKind::FirstSuccessfulCall;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"first_successful_call\"");
        let back: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, EventKind::FirstSuccessfulCall);
    }
}
