// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use dashmap::DashMap;
use std::sync::Arc;

use crate::protocol::{MiddlewarePhase, MiddlewareScope};

#[derive(Debug, Clone)]
pub struct MiddlewareEntry {
    pub middleware_id: String,
    pub phase: MiddlewarePhase,
    pub scope: Option<MiddlewareScope>,
    pub priority: u16,
    pub function_id: String,
    pub worker_id: String,
}

#[derive(Default, Clone)]
pub struct MiddlewareRegistry {
    entries: Arc<DashMap<MiddlewarePhase, Vec<MiddlewareEntry>>>,
}

impl MiddlewareRegistry {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
        }
    }

    pub fn register(&self, entry: MiddlewareEntry) {
        let mut phase_entries = self.entries.entry(entry.phase.clone()).or_default();

        // Remove existing entry with same middleware_id if it exists
        phase_entries.retain(|e| e.middleware_id != entry.middleware_id);

        phase_entries.push(entry);
        phase_entries.sort_by_key(|e| e.priority);
    }

    pub fn deregister(&self, middleware_id: &str) {
        for mut phase_entries in self.entries.iter_mut() {
            phase_entries.retain(|e| e.middleware_id != middleware_id);
        }
    }

    pub fn deregister_by_worker(&self, worker_id: &str) {
        for mut phase_entries in self.entries.iter_mut() {
            phase_entries.retain(|e| e.worker_id != worker_id);
        }
    }

    pub fn get_for_phase(&self, phase: &MiddlewarePhase) -> Vec<MiddlewareEntry> {
        self.entries
            .get(phase)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }
}
