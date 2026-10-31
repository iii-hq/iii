// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! In-process listeners of configuration changes: how the engine's own
//! workers (`iii-observability`, `iii-stream`) hot-apply their configuration
//! entry. Workers outside the engine observe `configuration::on-updated`.
//!
//! The listeners belong to the engine ([`crate::engine::Engine`]), not to the
//! configuration worker, so a worker that listens keeps listening across a
//! reload of the configuration worker.

use std::sync::{Arc, RwLock};

use super::structs::{ConfigurationEventData, ConfigurationEventType};

/// Called with each configuration change, on the task that made it: it must
/// return at once (spawn whatever it does).
pub type ConfigurationListener = Arc<dyn Fn(&ConfigurationEventData) + Send + Sync>;

struct Listener {
    key: String,
    configuration_id: String,
    event_types: Vec<ConfigurationEventType>,
    listener: ConfigurationListener,
}

#[derive(Default)]
pub struct ConfigurationListeners {
    listeners: RwLock<Vec<Listener>>,
}

impl ConfigurationListeners {
    /// Calls `listener` for every change of `configuration_id` whose type is
    /// one of `event_types`, from now on. A listener already registered under
    /// `key` is replaced, so a worker that reloads listens once.
    pub fn listen(
        &self,
        key: &str,
        configuration_id: &str,
        event_types: &[ConfigurationEventType],
        listener: ConfigurationListener,
    ) {
        let mut listeners = self.listeners.write().unwrap_or_else(|e| e.into_inner());
        listeners.retain(|existing| existing.key != key);
        listeners.push(Listener {
            key: key.to_string(),
            configuration_id: configuration_id.to_string(),
            event_types: event_types.to_vec(),
            listener,
        });
    }

    /// Stops the listener registered under `key`, if any.
    pub fn unlisten(&self, key: &str) {
        self.listeners
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|existing| existing.key != key);
    }

    /// Tells every listener that takes `event`.
    pub(crate) fn notify(&self, event: &ConfigurationEventData) {
        let matching: Vec<ConfigurationListener> = self
            .listeners
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|listener| {
                listener.configuration_id == event.id
                    && listener.event_types.contains(&event.event_type)
            })
            .map(|listener| listener.listener.clone())
            .collect();
        for listener in matching {
            listener(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;

    fn event(id: &str, event_type: ConfigurationEventType) -> ConfigurationEventData {
        ConfigurationEventData {
            message_type: "configuration".to_string(),
            event_type,
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            schema: json!({}),
            old_value: None,
            new_value: Some(json!({})),
            metadata: None,
        }
    }

    /// A listener hears its entry's changes of the types it named, once per
    /// key however many times it registered, and nothing after it stopped.
    #[test]
    fn a_listener_hears_its_entry_and_types_once() {
        let listeners = ConfigurationListeners::default();
        let heard = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let heard = heard.clone();
            listeners.listen(
                "test::watch",
                "iii-test",
                &[ConfigurationEventType::Updated],
                Arc::new(move |_| {
                    heard.fetch_add(1, Ordering::SeqCst);
                }),
            );
        }
        listeners.notify(&event("iii-test", ConfigurationEventType::Updated));
        listeners.notify(&event("iii-test", ConfigurationEventType::Registered));
        listeners.notify(&event("iii-other", ConfigurationEventType::Updated));
        assert_eq!(heard.load(Ordering::SeqCst), 1);

        listeners.unlisten("test::watch");
        listeners.notify(&event("iii-test", ConfigurationEventType::Updated));
        assert_eq!(heard.load(Ordering::SeqCst), 1);
    }
}
