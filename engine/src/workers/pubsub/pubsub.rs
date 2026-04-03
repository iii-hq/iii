// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use futures::Future;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PubSubAdapter, config::PubSubWorkerConfig};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    workers::module::{AdapterFactory, ConfigurableWorker, Worker},
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Clone)]
pub struct PubSubWorker {
    adapter: Arc<dyn PubSubAdapter>,
    engine: Arc<Engine>,
    _config: PubSubWorkerConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub struct PubSubInput {
    pub topic: String,
    pub data: Value,
}

#[service(name = "pubsub")]
impl PubSubWorker {
    #[function(id = "publish", description = "Publishes an event")]
    pub async fn publish(&self, input: PubSubInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
                stacktrace: None,
            });
        }

        tracing::debug!(topic = %topic, event_data = %event_data, "Publishing event");
        let _ = adapter.publish(&topic, event_data).await;
        crate::workers::telemetry::collector::track_pubsub_publish();

        FunctionResult::Success(None)
    }
}

impl TriggerRegistrator for PubSubWorker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let topic = trigger
            .clone()
            .config
            .get("topic")
            .unwrap_or_default()
            .as_str()
            .unwrap_or("")
            .to_string();

        tracing::info!(
            "{} PubSub subscription {} → {}",
            "[REGISTERED]".green(),
            topic.purple(),
            trigger.function_id.cyan()
        );

        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            if !topic.is_empty() {
                adapter
                    .subscribe(&topic, &trigger.id, &trigger.function_id)
                    .await;
                crate::workers::telemetry::collector::track_pubsub_subscribe();
            } else {
                tracing::warn!(
                    function_id = %trigger.function_id.purple(),
                    "Topic is not set for trigger"
                );
            }

            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            tracing::debug!(trigger = %trigger.id, "Unregistering trigger");
            adapter
                .unsubscribe(
                    trigger
                        .config
                        .get("topic")
                        .unwrap_or_default()
                        .as_str()
                        .unwrap_or(""),
                    &trigger.id,
                )
                .await;
            Ok(())
        })
    }
}

#[async_trait]
impl Worker for PubSubWorker {
    fn name(&self) -> &'static str {
        "PubSubWorker"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing PubSubWorker");

        let trigger_type = TriggerType::new(
            "subscribe",
            "Subscribe to a topic",
            Box::new(self.clone()),
            None,
        );

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }
}

#[async_trait]
impl ConfigurableWorker for PubSubWorker {
    type Config = PubSubWorkerConfig;
    type Adapter = dyn PubSubAdapter;
    type AdapterRegistration = super::registry::PubSubAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = "local";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn PubSubAdapter>>>> =
            Lazy::new(|| RwLock::new(PubSubWorker::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            engine,
            _config: config,
            adapter,
        }
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

crate::register_worker!("iii-pubsub", PubSubWorker, enabled_by_default = true);

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::{
        engine::Engine,
        workers::{
            worker::{AdapterEntry, ConfigurableWorker},
            observability::metrics::ensure_default_meter,
        },
    };

    #[derive(Default)]
    struct RecordingPubSubAdapter {
        published: Mutex<Vec<(String, Value)>>,
        subscribed: Mutex<Vec<(String, String, String)>>,
        unsubscribed: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl PubSubAdapter for RecordingPubSubAdapter {
        async fn publish(&self, topic: &str, pubsub_data: Value) {
            self.published
                .lock()
                .expect("lock published")
                .push((topic.to_string(), pubsub_data));
        }

        async fn subscribe(&self, topic: &str, id: &str, function_id: &str) {
            self.subscribed.lock().expect("lock subscribed").push((
                topic.to_string(),
                id.to_string(),
                function_id.to_string(),
            ));
        }

        async fn unsubscribe(&self, topic: &str, id: &str) {
            self.unsubscribed
                .lock()
                .expect("lock unsubscribed")
                .push((topic.to_string(), id.to_string()));
        }
    }

    fn build_worker() -> (Arc<Engine>, PubSubWorker, Arc<RecordingPubSubAdapter>) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(RecordingPubSubAdapter::default());
        let worker = PubSubWorker::build(
            engine.clone(),
            PubSubWorkerConfig::default(),
            adapter.clone(),
        );
        (engine, worker, adapter)
    }

    #[tokio::test]
    async fn publish_rejects_empty_topic() {
        let (_engine, worker, adapter) = build_worker();

        let result = worker
            .publish(PubSubInput {
                topic: String::new(),
                data: json!({ "ignored": true }),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "topic_not_set"),
            _ => panic!("expected topic_not_set failure"),
        }
        assert!(adapter.published.lock().expect("lock published").is_empty());
    }

    #[tokio::test]
    async fn publish_delegates_to_adapter_and_returns_success() {
        let (_engine, worker, adapter) = build_worker();

        let result = worker
            .publish(PubSubInput {
                topic: "orders".to_string(),
                data: json!({ "id": 1 }),
            })
            .await;

        assert!(matches!(result, FunctionResult::Success(None)));
        let published = adapter.published.lock().expect("lock published");
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "orders");
        assert_eq!(published[0].1, json!({ "id": 1 }));
    }

    #[tokio::test]
    async fn register_and_unregister_trigger_delegate_to_adapter() {
        let (_engine, worker, adapter) = build_worker();
        let trigger = Trigger {
            id: "sub-1".to_string(),
            trigger_type: "subscribe".to_string(),
            function_id: "test::listener".to_string(),
            config: json!({ "topic": "orders" }),
            worker_id: None,
        };

        worker
            .register_trigger(trigger.clone())
            .await
            .expect("register pubsub trigger");
        worker
            .unregister_trigger(trigger)
            .await
            .expect("unregister pubsub trigger");

        let subscribed = adapter.subscribed.lock().expect("lock subscribed");
        assert_eq!(
            subscribed.as_slice(),
            &[(
                "orders".to_string(),
                "sub-1".to_string(),
                "test::listener".to_string(),
            )]
        );
        let unsubscribed = adapter.unsubscribed.lock().expect("lock unsubscribed");
        assert_eq!(
            unsubscribed.as_slice(),
            &[("orders".to_string(), "sub-1".to_string())]
        );
    }

    #[tokio::test]
    async fn register_trigger_without_topic_skips_subscription() {
        let (_engine, worker, adapter) = build_worker();

        worker
            .register_trigger(Trigger {
                id: "sub-empty".to_string(),
                trigger_type: "subscribe".to_string(),
                function_id: "test::listener".to_string(),
                config: json!({}),
                worker_id: None,
            })
            .await
            .expect("register trigger without topic");

        assert!(
            adapter
                .subscribed
                .lock()
                .expect("lock subscribed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn initialize_registers_subscribe_trigger_type() {
        let (engine, worker, _adapter) = build_worker();

        worker.initialize().await.expect("initialize pubsub worker");

        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key("subscribe")
        );
        assert_eq!(worker.name(), "PubSubWorker");
    }

    #[test]
    fn adapter_name_and_config_are_read_from_config() {
        let config = PubSubWorkerConfig {
            adapter: Some(AdapterEntry {
                name: "custom::PubSub".to_string(),
                config: Some(json!({ "url": "redis://example" })),
            }),
        };

        assert_eq!(
            PubSubWorker::adapter_name_from_config(&config).as_deref(),
            Some("custom::PubSub")
        );
        assert_eq!(
            PubSubWorker::adapter_config_from_config(&config),
            Some(json!({ "url": "redis://example" }))
        );
    }
}
