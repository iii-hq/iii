// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashSet,
    sync::{Mutex, MutexGuard},
};

use chrono::Utc;
use dashmap::DashMap;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedWorkerInfo {
    pub name: String,
    pub owner_worker_id: Uuid,
    pub trusted_internal: bool,
    pub registered_at: i64,
    pub function_ids: HashSet<String>,
}

#[derive(Default)]
pub(crate) struct GeneratedWorkerRegistry {
    workers: DashMap<String, GeneratedWorkerInfo>,
    function_to_worker: DashMap<String, String>,
    mutation_lock: Mutex<()>,
}

impl GeneratedWorkerRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn claim_function(
        &self,
        worker_name: impl Into<String>,
        owner_worker_id: Uuid,
        trusted_internal: bool,
        function_id: &str,
    ) {
        let _guard = self.lock_mutation();
        let worker_name = worker_name.into();
        let function_id = function_id.to_string();

        if let Some((_, previous_worker)) = self
            .function_to_worker
            .insert(function_id.clone(), worker_name.clone())
            .map(|previous| (function_id.clone(), previous))
            && previous_worker != worker_name
        {
            self.remove_from_worker(&previous_worker, &function_id);
        }

        self.workers
            .entry(worker_name.clone())
            .and_modify(|info| {
                info.owner_worker_id = owner_worker_id;
                info.trusted_internal = trusted_internal;
                info.function_ids.insert(function_id.clone());
            })
            .or_insert_with(|| GeneratedWorkerInfo {
                name: worker_name,
                owner_worker_id,
                trusted_internal,
                registered_at: Utc::now().timestamp_millis(),
                function_ids: HashSet::from([function_id]),
            });
    }

    pub(crate) fn remove_function(&self, function_id: &str) -> Option<String> {
        let _guard = self.lock_mutation();
        let (_, worker_name) = self.function_to_worker.remove(function_id)?;
        self.remove_from_worker(&worker_name, function_id);
        Some(worker_name)
    }

    pub(crate) fn contains_function(&self, function_id: &str) -> bool {
        self.function_to_worker.contains_key(function_id)
    }

    pub(crate) fn list(&self) -> Vec<GeneratedWorkerInfo> {
        self.workers
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn get(&self, worker_name: &str) -> Option<GeneratedWorkerInfo> {
        self.workers
            .get(worker_name)
            .map(|entry| entry.value().clone())
    }

    #[cfg(test)]
    pub(crate) fn contains_worker(&self, worker_name: &str) -> bool {
        self.workers.contains_key(worker_name)
    }

    fn remove_from_worker(&self, worker_name: &str, function_id: &str) {
        let mut should_remove_worker = false;
        if let Some(mut info) = self.workers.get_mut(worker_name) {
            info.function_ids.remove(function_id);
            should_remove_worker = info.function_ids.is_empty();
        }
        if should_remove_worker {
            self.workers.remove(worker_name);
        }
    }

    fn lock_mutation(&self) -> MutexGuard<'_, ()> {
        self.mutation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub(crate) fn take_generated_worker_name(metadata: &mut Option<Value>) -> Option<String> {
    let value = metadata.as_mut()?;
    let object = value.as_object_mut()?;
    let iii = object.get_mut("iii")?;
    let iii_object = iii.as_object_mut()?;
    let generated_hint = iii_object.remove("generatedWorker");
    let virtual_hint = iii_object.remove("virtualWorker");
    let hint = generated_hint.or(virtual_hint)?;
    let name = match hint {
        Value::String(value) => non_empty(value),
        Value::Object(object) => object
            .get("name")
            .and_then(Value::as_str)
            .and_then(|value| non_empty(value.to_string()))
            .or_else(|| {
                object
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|value| non_empty(value.to_string()))
            }),
        _ => None,
    };

    if iii_object.is_empty() {
        object.remove("iii");
    }
    if object.is_empty() {
        *metadata = None;
    }

    name
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_generated_worker_name_strips_internal_metadata() {
        let mut metadata = Some(serde_json::json!({
            "spec": { "source": "https://example.com/openapi.json" },
            "iii": { "generatedWorker": { "name": "hackernews" } }
        }));

        assert_eq!(
            take_generated_worker_name(&mut metadata),
            Some("hackernews".to_string())
        );
        assert_eq!(
            metadata,
            Some(serde_json::json!({
                "spec": { "source": "https://example.com/openapi.json" }
            }))
        );
    }

    #[test]
    fn take_generated_worker_name_accepts_legacy_private_hint() {
        let mut metadata = Some(serde_json::json!({
            "spec": { "source": "https://example.com/openapi.json" },
            "iii": { "virtualWorker": { "name": "hackernews" } }
        }));

        assert_eq!(
            take_generated_worker_name(&mut metadata),
            Some("hackernews".to_string())
        );
        assert_eq!(
            metadata,
            Some(serde_json::json!({
                "spec": { "source": "https://example.com/openapi.json" }
            }))
        );
    }

    #[test]
    fn take_generated_worker_name_strips_both_private_hint_aliases() {
        let mut metadata = Some(serde_json::json!({
            "spec": { "source": "https://example.com/openapi.json" },
            "iii": {
                "generatedWorker": { "name": "hackernews" },
                "virtualWorker": { "name": "legacy" }
            }
        }));

        assert_eq!(
            take_generated_worker_name(&mut metadata),
            Some("hackernews".to_string())
        );
        assert_eq!(
            metadata,
            Some(serde_json::json!({
                "spec": { "source": "https://example.com/openapi.json" }
            }))
        );
    }

    #[test]
    fn generated_worker_registry_removes_empty_worker() {
        let registry = GeneratedWorkerRegistry::new();
        let owner = Uuid::new_v4();

        registry.claim_function("hn", owner, true, "hn::top_stories");
        assert!(registry.contains_worker("hn"));

        registry.remove_function("hn::top_stories");
        assert!(!registry.contains_worker("hn"));
    }

    #[test]
    fn generated_worker_registry_reclaim_moves_function_between_workers() {
        let registry = GeneratedWorkerRegistry::new();
        let first_owner = Uuid::new_v4();
        let second_owner = Uuid::new_v4();

        registry.claim_function("hn", first_owner, true, "shared::top_stories");
        registry.claim_function("ph", second_owner, false, "shared::top_stories");

        assert!(!registry.contains_worker("hn"));
        let worker = registry.get("ph").expect("new worker should own function");
        assert_eq!(worker.owner_worker_id, second_owner);
        assert!(!worker.trusted_internal);
        assert!(worker.registered_at > 0);
        assert!(worker.function_ids.contains("shared::top_stories"));
        assert_eq!(
            registry.remove_function("shared::top_stories"),
            Some("ph".to_string())
        );
        assert!(!registry.contains_function("shared::top_stories"));
        assert!(!registry.contains_worker("ph"));
    }
}
