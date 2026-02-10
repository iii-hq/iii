// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::collector::HeartbeatEntry;

#[derive(Clone)]
pub struct HeartbeatStore {
    entries: Arc<RwLock<VecDeque<HeartbeatEntry>>>,
    max_size: usize,
}

impl HeartbeatStore {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(max_size.min(1024)))),
            max_size,
        }
    }

    pub async fn push(&self, entry: HeartbeatEntry) {
        let mut entries = self.entries.write().await;
        if entries.len() >= self.max_size {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub async fn history(&self, limit: Option<usize>) -> Vec<HeartbeatEntry> {
        let entries = self.entries.read().await;
        let limit = limit.unwrap_or(100).min(entries.len());
        entries.iter().rev().take(limit).cloned().collect()
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}
