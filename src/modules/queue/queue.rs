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

const MAX_QUEUE_NAME_LEN: usize = 128;
const MAX_JOB_ID_LEN: usize = 256;
const VALID_JOB_STATES: &[&str] = &["waiting", "active", "delayed", "dlq"];
const MAX_LIMIT: usize = 500;

fn validate_queue_name(name: &str) -> Result<(), ErrorBody> {
    if name.is_empty() {
        return Err(ErrorBody {
            code: "topic_not_set".into(),
            message: "Topic is not set".into(),
        });
    }
    if name.len() > MAX_QUEUE_NAME_LEN {
        return Err(ErrorBody {
            code: "invalid_topic".into(),
            message: format!(
                "Topic name exceeds maximum length of {} characters",
                MAX_QUEUE_NAME_LEN
            ),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err(ErrorBody {
            code: "invalid_topic".into(),
            message: "Topic name contains invalid characters (allowed: alphanumeric, -, _, ., :)"
                .into(),
        });
    }
    Ok(())
}

fn validate_job_id(job_id: &str) -> Result<(), ErrorBody> {
    if job_id.is_empty() {
        return Err(ErrorBody {
            code: "missing_params".into(),
            message: "job_id is required".into(),
        });
    }
    if job_id.len() > MAX_JOB_ID_LEN {
        return Err(ErrorBody {
            code: "invalid_job_id".into(),
            message: format!(
                "job_id exceeds maximum length of {} characters",
                MAX_JOB_ID_LEN
            ),
        });
    }
    if !job_id
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err(ErrorBody {
            code: "invalid_job_id".into(),
            message: "job_id contains invalid characters (allowed: alphanumeric, -, _, ., :)"
                .into(),
        });
    }
    Ok(())
}

fn validate_job_state(state: &str) -> Result<(), ErrorBody> {
    if !VALID_JOB_STATES.contains(&state) {
        return Err(ErrorBody {
            code: "invalid_state".into(),
            message: format!(
                "Invalid job state '{}'. Must be one of: {}",
                state,
                VALID_JOB_STATES.join(", ")
            ),
        });
    }
    Ok(())
}

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

#[derive(Deserialize)]
pub struct QueueStatsInput {
    topic: String,
}

#[derive(Deserialize)]
pub struct QueueJobsInput {
    topic: String,
    state: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Deserialize)]
pub struct QueueJobInput {
    topic: String,
    job_id: String,
}

#[service(name = "queue")]
impl QueueCoreModule {
    #[function(id = "enqueue", description = "Enqueue a message")]
    pub async fn enqueue(&self, input: QueueInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter.clone();
        let data = input.data;
        let topic = input.topic;

        if let Err(e) = validate_queue_name(&topic) {
            return FunctionResult::Failure(e);
        }

        let ctx = tracing::Span::current().context();
        let traceparent = inject_traceparent_from_context(&ctx);
        let baggage = inject_baggage_from_context(&ctx);

        tracing::debug!(topic = %topic, traceparent = ?traceparent, baggage = ?baggage, "Enqueuing message with trace context");
        let _ = adapter.enqueue(&topic, data, traceparent, baggage).await;
        crate::modules::telemetry::collector::track_queue_emit();

        FunctionResult::Success(None)
    }

    #[function(id = "list_queues", description = "List all queues with stats")]
    pub async fn list_queues(&self, _input: Value) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list_queues().await {
            Ok(queues) => FunctionResult::Success(Some(serde_json::json!({ "queues": queues }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "list_queues_failed".into(),
                message: format!("{:?}", e),
            }),
        }
    }

    #[function(id = "stats", description = "Get queue depth stats")]
    pub async fn stats(&self, input: QueueStatsInput) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Err(e) = validate_queue_name(&input.topic) {
            return FunctionResult::Failure(e);
        }

        match self.adapter.queue_stats(&input.topic).await {
            Ok(stats) => FunctionResult::Success(Some(stats)),
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "stats_failed".into(),
                message: format!("{:?}", e),
            }),
        }
    }

    #[function(id = "jobs", description = "List jobs by state")]
    pub async fn jobs(&self, input: QueueJobsInput) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Err(e) = validate_queue_name(&input.topic) {
            return FunctionResult::Failure(e);
        }
        if let Err(e) = validate_job_state(&input.state) {
            return FunctionResult::Failure(e);
        }

        let limit = input.limit.min(MAX_LIMIT);

        match self
            .adapter
            .list_jobs(&input.topic, &input.state, input.offset, limit)
            .await
        {
            Ok(jobs) => FunctionResult::Success(Some(serde_json::json!({
                "jobs": jobs,
                "count": jobs.len(),
                "offset": input.offset,
                "limit": input.limit,
            }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "jobs_failed".into(),
                message: format!("{:?}", e),
            }),
        }
    }

    #[function(id = "job", description = "Get single job detail")]
    pub async fn job(&self, input: QueueJobInput) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Err(e) = validate_queue_name(&input.topic) {
            return FunctionResult::Failure(e);
        }
        if let Err(e) = validate_job_id(&input.job_id) {
            return FunctionResult::Failure(e);
        }

        match self.adapter.get_job(&input.topic, &input.job_id).await {
            Ok(Some(job)) => FunctionResult::Success(Some(job)),
            Ok(None) => FunctionResult::Failure(ErrorBody {
                code: "not_found".into(),
                message: format!("Job {} not found in queue {}", input.job_id, input.topic),
            }),
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "job_failed".into(),
                message: format!("{:?}", e),
            }),
        }
    }

    #[function(
        id = "redrive_dlq",
        description = "Redrive all DLQ jobs back to waiting"
    )]
    pub async fn redrive_dlq(
        &self,
        input: QueueStatsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Err(e) = validate_queue_name(&input.topic) {
            return FunctionResult::Failure(e);
        }

        tracing::warn!(queue = %input.topic, "Redriving DLQ jobs back to waiting");

        match self.adapter.redrive_dlq(&input.topic).await {
            Ok(count) => FunctionResult::Success(Some(serde_json::json!({
                "queue": input.topic,
                "redriven": count,
            }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "redrive_failed".into(),
                message: format!("{:?}", e),
            }),
        }
    }

    #[function(id = "dlq_count", description = "Get DLQ count for a queue")]
    pub async fn dlq_count(
        &self,
        input: QueueStatsInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        if let Err(e) = validate_queue_name(&input.topic) {
            return FunctionResult::Failure(e);
        }

        match self.adapter.dlq_count(&input.topic).await {
            Ok(count) => FunctionResult::Success(Some(serde_json::json!({
                "queue": input.topic,
                "dlq_count": count,
            }))),
            Err(e) => FunctionResult::Failure(ErrorBody {
                code: "dlq_count_failed".into(),
                message: format!("{:?}", e),
            }),
        }
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
