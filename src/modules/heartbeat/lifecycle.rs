// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    FirstConnection,
    FirstCall,
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

struct TrackerState {
    events: Vec<LifecycleEvent>,
    pending: Vec<LifecycleEvent>,
}

pub struct LifecycleTracker {
    start_time: Instant,
    first_connection: AtomicBool,
    first_call: AtomicBool,
    state: RwLock<TrackerState>,
    instance_id: OnceLock<String>,
}

impl LifecycleTracker {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            first_connection: AtomicBool::new(false),
            first_call: AtomicBool::new(false),
            state: RwLock::new(TrackerState {
                events: Vec::new(),
                pending: Vec::new(),
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
        match self.state.write() {
            Ok(mut state) => {
                state.events.push(event.clone());
                state.pending.push(event);
            }
            Err(e) => {
                tracing::error!("Failed to record lifecycle event: lock poisoned");
                let mut state = e.into_inner();
                state.events.push(event.clone());
                state.pending.push(event);
            }
        }
    }

    pub fn record_first(&self, kind: EventKind, metadata: Option<Value>) {
        let flag = match kind {
            EventKind::FirstConnection => &self.first_connection,
            EventKind::FirstCall => &self.first_call,
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
        match self.state.write() {
            Ok(mut state) => std::mem::take(&mut state.pending),
            Err(e) => {
                tracing::error!("Failed to take pending lifecycle events: lock poisoned");
                let mut state = e.into_inner();
                std::mem::take(&mut state.pending)
            }
        }
    }

    pub fn requeue_pending(&self, events: Vec<LifecycleEvent>) {
        if events.is_empty() {
            return;
        }
        match self.state.write() {
            Ok(mut state) => {
                let mut restored = events;
                restored.append(&mut state.pending);
                state.pending = restored;
            }
            Err(e) => {
                tracing::error!("Failed to requeue pending lifecycle events: lock poisoned");
                let mut state = e.into_inner();
                let mut restored = events;
                restored.append(&mut state.pending);
                state.pending = restored;
            }
        }
    }

    pub fn all_events(&self) -> Vec<LifecycleEvent> {
        self.state
            .read()
            .map(|s| s.events.clone())
            .unwrap_or_default()
    }
}

static LIFECYCLE_TRACKER: OnceLock<LifecycleTracker> = OnceLock::new();

pub fn get_lifecycle_tracker() -> &'static LifecycleTracker {
    LIFECYCLE_TRACKER.get_or_init(LifecycleTracker::new)
}
