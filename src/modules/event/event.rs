// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
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

use super::{EventAdapter, SubscriberQueueConfig, config::EventModuleConfig};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::module::{AdapterFactory, ConfigurableModule, Module},
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
};

#[derive(Clone)]
pub struct EventCoreModule {
    adapter: Arc<dyn EventAdapter>,
    engine: Arc<Engine>,
    _config: EventModuleConfig,
}

#[derive(Deserialize)]
pub struct EventInput {
    topic: String,
    data: Value,
}

#[service(name = "event")]
impl EventCoreModule {
    #[function(name = "emit", description = "Emit an event")]
    pub async fn emit(&self, input: EventInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
            });
        }

        tracing::debug!(topic = %topic, event_data = %event_data, "Emitting event");
        let _ = adapter.emit(&topic, event_data).await;

        FunctionResult::Success(None)
    }
}

impl TriggerRegistrator for EventCoreModule {
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
            trigger.function_path.cyan()
        );

        // Get adapter reference before async block
        let adapter = self.adapter.clone();

        Box::pin(async move {
            if !topic.is_empty() {
                let condition_function_path = trigger
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
                        &trigger.function_path,
                        condition_function_path,
                        queue_config,
                    )
                    .await;
            } else {
                tracing::warn!(
                    function_path = %trigger.function_path.purple(),
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
impl Module for EventCoreModule {
    fn name(&self) -> &'static str {
        "EventModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing EventModule");

        let trigger_type = TriggerType {
            id: "event".to_string(),
            _description: "Event core module".to_string(),
            registrator: Box::new(self.clone()),
            worker_id: None,
        };

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }
}

#[async_trait]
impl ConfigurableModule for EventCoreModule {
    type Config = EventModuleConfig;
    type Adapter = dyn EventAdapter;
    type AdapterRegistration = super::registry::EventAdapterRegistration;
    const DEFAULT_ADAPTER_CLASS: &'static str = "modules::event::RedisAdapter";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn EventAdapter>>>> =
            Lazy::new(|| RwLock::new(EventCoreModule::build_registry()));
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
    "modules::event::EventModule",
    EventCoreModule,
    enabled_by_default = true
);
