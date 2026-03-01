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
use serde::Deserialize;
use serde_json::Value;

use super::{QueueAdapter, SubscriberQueueConfig, config::QueueModuleConfig};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::{AdapterFactory, ConfigurableModule, Module},
    protocol::ErrorBody,
    telemetry::{inject_baggage_from_context, inject_traceparent_from_context},
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Clone)]
pub struct QueueCoreModule {
    adapter: Arc<dyn QueueAdapter>,
    engine: Arc<Engine>,
    _config: QueueModuleConfig,
}

#[derive(Deserialize)]
pub struct QueueInput {
    topic: String,
    data: Value,
}

#[service(name = "queue")]
impl QueueCoreModule {
    #[function(id = "enqueue", description = "Enqueue a message")]
    pub async fn enqueue(&self, input: QueueInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
            });
        }

        // Record the queue topic on the current span for trace visibility
        let current_span = tracing::Span::current();
        current_span.set_attribute("messaging.destination.name", topic.clone());
        current_span.set_attribute("messaging.operation.type", "publish".to_string());
        current_span.set_attribute("baggage.topic", topic.clone());

        let ctx = current_span.context();
        let traceparent = inject_traceparent_from_context(&ctx);
        let baggage = inject_baggage_from_context(&ctx);

        tracing::debug!(topic = %topic, traceparent = ?traceparent, baggage = ?baggage, "Enqueuing message with trace context");
        let _ = adapter
            .enqueue(&topic, event_data.clone(), traceparent, baggage)
            .await;
        crate::modules::telemetry::collector::track_queue_emit();

        FunctionResult::Success(None)
    }
}

impl TriggerRegistrator for QueueCoreModule {
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
            "{} Subscription {} → {}",
            "[REGISTERED]".green(),
            topic.purple(),
            trigger.function_id.cyan()
        );

        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            if !topic.is_empty() {
                let condition_function_id = trigger
                    .config
                    .get("_condition_path")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());

                let queue_config = trigger
                    .config
                    .get("metadata")
                    .and_then(|m| m.get("infrastructure"))
                    .and_then(|i| i.get("queue"))
                    .and_then(|q| SubscriberQueueConfig::from_value(Some(q)));

                adapter
                    .subscribe(
                        &topic,
                        &trigger.id,
                        &trigger.function_id,
                        condition_function_id,
                        queue_config,
                    )
                    .await;
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
impl Module for QueueCoreModule {
    fn name(&self) -> &'static str {
        "QueueModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing QueueModule");

        let trigger_type = TriggerType {
            id: "queue".to_string(),
            _description: "Queue core module".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for QueueCoreModule {
    type Config = QueueModuleConfig;
    type Adapter = dyn QueueAdapter;
    type AdapterRegistration = super::registry::QueueAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::queue::BuiltinQueueAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn QueueAdapter>>>> =
            Lazy::new(|| RwLock::new(QueueCoreModule::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            engine,
            _config: config,
            adapter,
        }
    }

    fn adapter_class_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.class.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

crate::register_module!(
    "modules::queue::QueueModule",
    QueueCoreModule,
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // QueueInput deserialization
    // =========================================================================

    #[test]
    fn queue_input_deserialize() {
        let json = json!({
            "topic": "my-topic",
            "data": {"key": "value"}
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "my-topic");
        assert_eq!(input.data["key"], "value");
    }

    #[test]
    fn queue_input_deserialize_empty_topic() {
        let json = json!({
            "topic": "",
            "data": null
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "");
        assert_eq!(input.data, Value::Null);
    }

    #[test]
    fn queue_input_deserialize_missing_topic_fails() {
        let json = json!({"data": "hello"});
        let result: Result<QueueInput, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn queue_input_deserialize_missing_data_fails() {
        let json = json!({"topic": "test"});
        let result: Result<QueueInput, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn queue_input_deserialize_complex_data() {
        let json = json!({
            "topic": "events",
            "data": {
                "event_type": "user.created",
                "payload": {
                    "user_id": 123,
                    "email": "test@example.com"
                }
            }
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "events");
        assert_eq!(input.data["event_type"], "user.created");
        assert_eq!(input.data["payload"]["user_id"], 123);
    }

    // =========================================================================
    // QueueCoreModule::name
    // =========================================================================
    // Note: The module itself requires an adapter and engine, so we test what
    // we can without those dependencies.

    // =========================================================================
    // ConfigurableModule trait constants
    // =========================================================================

    #[test]
    fn default_adapter_class() {
        assert_eq!(
            QueueCoreModule::DEFAULT_ADAPTER_CLASS,
            "modules::queue::BuiltinQueueAdapter"
        );
    }

    // =========================================================================
    // QueueInput additional deserialization tests
    // =========================================================================

    #[test]
    fn queue_input_deserialize_array_data() {
        let json = json!({
            "topic": "batch",
            "data": [1, 2, 3]
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "batch");
        assert!(input.data.is_array());
        assert_eq!(input.data.as_array().unwrap().len(), 3);
    }

    #[test]
    fn queue_input_deserialize_string_data() {
        let json = json!({
            "topic": "simple",
            "data": "just a string"
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "simple");
        assert_eq!(input.data, "just a string");
    }

    #[test]
    fn queue_input_deserialize_number_data() {
        let json = json!({
            "topic": "numeric",
            "data": 42
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "numeric");
        assert_eq!(input.data, 42);
    }

    #[test]
    fn queue_input_deserialize_bool_data() {
        let json = json!({
            "topic": "flags",
            "data": true
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "flags");
        assert_eq!(input.data, true);
    }

    #[test]
    fn queue_input_deserialize_extra_fields_ignored() {
        // serde by default ignores extra fields (no deny_unknown_fields)
        let json = json!({
            "topic": "t",
            "data": null,
            "extra": "ignored"
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.topic, "t");
    }

    #[test]
    fn queue_input_deserialize_nested_deeply() {
        let json = json!({
            "topic": "deep",
            "data": { "a": { "b": { "c": { "d": 99 } } } }
        });
        let input: QueueInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.data["a"]["b"]["c"]["d"], 99);
    }

    // =========================================================================
    // Mock adapter for integration tests
    // =========================================================================

    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::Mutex;

    struct MockQueueAdapter {
        enqueue_count: AtomicU64,
        subscribe_count: AtomicU64,
        unsubscribe_count: AtomicU64,
        last_topic: Mutex<String>,
        last_data: Mutex<Option<Value>>,
        last_traceparent: Mutex<Option<String>>,
        last_baggage: Mutex<Option<String>>,
    }

    impl MockQueueAdapter {
        fn new() -> Self {
            Self {
                enqueue_count: AtomicU64::new(0),
                subscribe_count: AtomicU64::new(0),
                unsubscribe_count: AtomicU64::new(0),
                last_topic: Mutex::new(String::new()),
                last_data: Mutex::new(None),
                last_traceparent: Mutex::new(None),
                last_baggage: Mutex::new(None),
            }
        }
    }

    #[async_trait::async_trait]
    impl QueueAdapter for MockQueueAdapter {
        async fn enqueue(
            &self,
            topic: &str,
            data: Value,
            traceparent: Option<String>,
            baggage: Option<String>,
        ) {
            self.enqueue_count.fetch_add(1, Ordering::SeqCst);
            *self.last_topic.lock().await = topic.to_string();
            *self.last_data.lock().await = Some(data);
            *self.last_traceparent.lock().await = traceparent;
            *self.last_baggage.lock().await = baggage;
        }

        async fn subscribe(
            &self,
            _topic: &str,
            _id: &str,
            _function_id: &str,
            _condition_function_id: Option<String>,
            _queue_config: Option<SubscriberQueueConfig>,
        ) {
            self.subscribe_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn unsubscribe(&self, _topic: &str, _id: &str) {
            self.unsubscribe_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn redrive_dlq(&self, _topic: &str) -> anyhow::Result<u64> {
            Ok(0)
        }

        async fn dlq_count(&self, _topic: &str) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    fn setup_queue_module() -> (Arc<Engine>, QueueCoreModule, Arc<MockQueueAdapter>) {
        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(MockQueueAdapter::new());
        let module = QueueCoreModule {
            adapter: adapter.clone(),
            engine: engine.clone(),
            _config: super::super::config::QueueModuleConfig::default(),
        };
        (engine, module, adapter)
    }

    // =========================================================================
    // QueueCoreModule::name
    // =========================================================================

    #[test]
    fn queue_module_name() {
        let (_engine, module, _adapter) = setup_queue_module();
        assert_eq!(Module::name(&module), "QueueModule");
    }

    // =========================================================================
    // enqueue service function tests
    // =========================================================================

    #[tokio::test]
    async fn enqueue_with_empty_topic_returns_failure() {
        let (_engine, module, _adapter) = setup_queue_module();
        let input = QueueInput {
            topic: "".to_string(),
            data: json!({"msg": "hello"}),
        };
        let result = module.enqueue(input).await;
        match result {
            FunctionResult::Failure(e) => {
                assert_eq!(e.code, "topic_not_set");
                assert_eq!(e.message, "Topic is not set");
            }
            _ => panic!("Expected Failure for empty topic"),
        }
    }

    #[tokio::test]
    async fn enqueue_success_calls_adapter() {
        let (_engine, module, adapter) = setup_queue_module();
        let input = QueueInput {
            topic: "my-topic".to_string(),
            data: json!({"msg": "hello"}),
        };
        let result = module.enqueue(input).await;
        assert!(matches!(result, FunctionResult::Success(None)));
        assert_eq!(adapter.enqueue_count.load(Ordering::SeqCst), 1);
        assert_eq!(*adapter.last_topic.lock().await, "my-topic");
        assert_eq!(
            *adapter.last_data.lock().await,
            Some(json!({"msg": "hello"}))
        );
    }

    #[tokio::test]
    async fn enqueue_multiple_calls() {
        let (_engine, module, adapter) = setup_queue_module();
        for i in 0..5 {
            let input = QueueInput {
                topic: format!("topic-{}", i),
                data: json!(i),
            };
            let result = module.enqueue(input).await;
            assert!(matches!(result, FunctionResult::Success(None)));
        }
        assert_eq!(adapter.enqueue_count.load(Ordering::SeqCst), 5);
    }

    // =========================================================================
    // TriggerRegistrator tests
    // =========================================================================

    #[tokio::test]
    async fn register_trigger_with_valid_topic_subscribes() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-1".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": "my-topic"}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn register_trigger_with_empty_topic_does_not_subscribe() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-2".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": ""}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn register_trigger_with_missing_topic_does_not_subscribe() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-3".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        // topic defaults to "" when missing, so subscribe should not be called
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn register_trigger_with_condition_path_subscribes() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-cond".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "topic": "conditioned-topic",
                "_condition_path": "test::condition_fn"
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn register_trigger_with_queue_infrastructure_metadata() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-infra".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({
                "topic": "infra-topic",
                "metadata": {
                    "infrastructure": {
                        "queue": {
                            "type": "fifo",
                            "maxRetries": 3
                        }
                    }
                }
            }),
            worker_id: None,
        };
        let result = module.register_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.subscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unregister_trigger_calls_unsubscribe() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-unsub".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({"topic": "unsub-topic"}),
            worker_id: None,
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        assert_eq!(adapter.unsubscribe_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unregister_trigger_with_no_topic() {
        let (_engine, module, adapter) = setup_queue_module();
        let trigger = crate::trigger::Trigger {
            id: "trig-no-topic".to_string(),
            trigger_type: "queue".to_string(),
            function_id: "test::handler".to_string(),
            config: json!({}),
            worker_id: None,
        };
        let result = module.unregister_trigger(trigger).await;
        assert!(result.is_ok());
        // unsubscribe is called regardless (with empty topic)
        assert_eq!(adapter.unsubscribe_count.load(Ordering::SeqCst), 1);
    }

    // =========================================================================
    // ConfigurableModule trait tests
    // =========================================================================

    #[test]
    fn adapter_class_from_config_none() {
        let config = super::super::config::QueueModuleConfig::default();
        assert!(QueueCoreModule::adapter_class_from_config(&config).is_none());
    }

    #[test]
    fn adapter_class_from_config_some() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::CustomAdapter".to_string(),
                config: None,
            }),
        };
        assert_eq!(
            QueueCoreModule::adapter_class_from_config(&config),
            Some("my::CustomAdapter".to_string())
        );
    }

    #[test]
    fn adapter_config_from_config_none() {
        let config = super::super::config::QueueModuleConfig::default();
        assert!(QueueCoreModule::adapter_config_from_config(&config).is_none());
    }

    #[test]
    fn adapter_config_from_config_some() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::Adapter".to_string(),
                config: Some(json!({"url": "redis://localhost"})),
            }),
        };
        assert_eq!(
            QueueCoreModule::adapter_config_from_config(&config),
            Some(json!({"url": "redis://localhost"}))
        );
    }

    #[test]
    fn adapter_config_from_config_adapter_without_config() {
        let config = super::super::config::QueueModuleConfig {
            adapter: Some(crate::modules::module::AdapterEntry {
                class: "my::Adapter".to_string(),
                config: None,
            }),
        };
        assert!(QueueCoreModule::adapter_config_from_config(&config).is_none());
    }

    // =========================================================================
    // Module::initialize test
    // =========================================================================

    #[tokio::test]
    async fn initialize_registers_trigger_type() {
        let (engine, module, _adapter) = setup_queue_module();
        let result = module.initialize().await;
        assert!(result.is_ok());
        assert!(engine.trigger_registry.trigger_types.contains_key("queue"));
    }

    // =========================================================================
    // build helper test
    // =========================================================================

    #[test]
    fn build_creates_module() {
        crate::modules::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn QueueAdapter> = Arc::new(MockQueueAdapter::new());
        let config = super::super::config::QueueModuleConfig::default();
        let module = QueueCoreModule::build(engine.clone(), config, adapter);
        assert_eq!(Module::name(&module), "QueueModule");
    }
}
