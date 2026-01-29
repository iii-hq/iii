use std::sync::Arc;

use dashmap::DashMap;
use serde_json::Value;

use crate::invocation::method::HttpAuth;

#[derive(Debug, Clone)]
pub struct HttpTrigger {
    pub function_path: String,
    pub trigger_type: String,
    pub trigger_id: String,
    pub url: String,
    pub auth: Option<HttpAuth>,
    pub config: Value,
}

#[derive(Default)]
pub struct HttpTriggerRegistry {
    triggers: Arc<DashMap<String, HttpTrigger>>,
}

impl HttpTriggerRegistry {
    pub fn new() -> Self {
        Self {
            triggers: Arc::new(DashMap::new()),
        }
    }

    pub fn register(&self, trigger: HttpTrigger) {
        self.triggers.insert(trigger.trigger_id.clone(), trigger);
    }

    pub fn remove(&self, trigger_id: &str) {
        self.triggers.remove(trigger_id);
    }

    pub fn list_by_type(&self, trigger_type: &str) -> Vec<HttpTrigger> {
        self.triggers
            .iter()
            .filter(|entry| entry.value().trigger_type == trigger_type)
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn list_cron_triggers(&self, trigger_id: &str) -> Vec<HttpTrigger> {
        self.triggers
            .iter()
            .filter(|entry| entry.value().trigger_type == "cron" && entry.value().trigger_id == trigger_id)
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn list_event_triggers(&self, topic: &str) -> Vec<HttpTrigger> {
        self.triggers
            .iter()
            .filter(|entry| {
                let trigger = entry.value();
                if trigger.trigger_type != "event" {
                    return false;
                }
                trigger
                    .config
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .map(|value| value == topic)
                    .unwrap_or(false)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }
}
