use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use redis::{AsyncCommands, Client, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::config::DevToolsConfig;
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    modules::core_module::CoreModule,
    protocol::ErrorBody,
    trigger::Trigger,
};

#[derive(Clone)]
pub struct DevToolsModule {
    engine: Arc<Engine>,
    config: DevToolsConfig,
    start_time: u64,
    redis_conn: Option<Arc<Mutex<ConnectionManager>>>,
}

#[derive(Deserialize)]
pub struct EmptyInput {}

#[derive(Deserialize)]
pub struct GetTriggersInput {
    #[serde(default)]
    pub trigger_type: Option<String>,
}

#[derive(Deserialize)]
pub struct GetLogsInput {
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub since: Option<u64>,
}

#[derive(Deserialize)]
pub struct GetMetricsHistoryInput {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct StreamGroupInput {
    #[serde(default)]
    pub body: StreamGroupBody,
}

#[derive(Deserialize, Default)]
pub struct StreamGroupBody {
    #[serde(default)]
    pub stream_name: String,
    #[serde(default)]
    pub group_id: String,
}

#[derive(Deserialize)]
pub struct StreamNameInput {
    #[serde(default)]
    pub body: StreamNameBody,
}

#[derive(Deserialize, Default)]
pub struct StreamNameBody {
    #[serde(default)]
    pub stream_name: String,
}

#[derive(Serialize, Clone)]
pub struct StatusResponse {
    pub status: String,
    pub uptime_seconds: u64,
    pub uptime_formatted: String,
    pub workers: usize,
    pub functions: usize,
    pub triggers: usize,
    pub version: String,
    pub timestamp: u64,
}

#[derive(Serialize)]
pub struct FunctionInfo {
    pub path: String,
    pub description: Option<String>,
    pub metadata: Option<Value>,
    pub internal: bool, // True for system/devtools functions
}

#[derive(Serialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_path: String,
    pub config: Value,
    pub worker_id: Option<String>,
    pub internal: bool, // True for system/devtools triggers
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MetricsSnapshot {
    pub id: String,
    pub timestamp: u64,
    pub functions_count: usize,
    pub triggers_count: usize,
    pub workers_count: usize,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Clone)]
pub struct DevToolsEvent {
    pub event_type: String,
    pub timestamp: u64,
    pub data: Value,
}

fn api_response(body: Value) -> Value {
    json!({
        "status_code": 200,
        "headers": [],
        "body": body
    })
}

fn api_error(code: u16, message: &str) -> Value {
    json!({
        "status_code": code,
        "headers": [],
        "body": {
            "error": message
        }
    })
}

impl DevToolsModule {
    pub async fn emit_event(&self, event_type: &str, data: Value) {
        let topic = &self.config.event_topic;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let event = DevToolsEvent {
            event_type: event_type.to_string(),
            timestamp,
            data,
        };

        let input = json!({
            "topic": topic,
            "data": event
        });

        if let Err(e) = self.engine.invoke_function("emit", input).await {
            tracing::warn!("Failed to emit DevTools event {}: {:?}", event_type, e);
        } else {
            tracing::debug!("Emitted DevTools event: {}", event_type);
        }
    }

    pub async fn store_metrics(&self, metrics: &MetricsSnapshot) {
        let stream_name = &self.config.state_stream;
        let group_id = "metrics";
        let item_id = &metrics.id;

        let input = json!({
            "stream_name": stream_name,
            "group_id": group_id,
            "item_id": item_id,
            "data": metrics
        });

        if let Err(e) = self.engine.invoke_function("streams.set", input).await {
            tracing::warn!("Failed to store DevTools metrics: {:?}", e);
        } else {
            tracing::debug!("Stored metrics snapshot: {}", item_id);
        }
    }

    async fn fetch_metrics_history(&self, limit: Option<usize>) -> Vec<MetricsSnapshot> {
        let stream_name = &self.config.state_stream;
        let group_id = "metrics";

        let input = json!({
            "stream_name": stream_name,
            "group_id": group_id
        });

        match self.engine.invoke_function("streams.getGroup", input).await {
            Ok(Some(result)) => {
                if let Ok(metrics) = serde_json::from_value::<Vec<MetricsSnapshot>>(result) {
                    let mut sorted = metrics;
                    sorted.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                    if let Some(limit) = limit {
                        sorted.truncate(limit);
                    }
                    sorted
                } else {
                    Vec::new()
                }
            }
            Ok(None) | Err(_) => Vec::new(),
        }
    }

    pub async fn collect_and_emit_metrics(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let uptime = now.saturating_sub(self.start_time);
        let workers = self.engine.worker_registry.workers.read().await.len();
        let functions = self.engine.functions.iter().count();
        let triggers = self
            .engine
            .trigger_registry
            .triggers
            .read()
            .await
            .iter()
            .count();

        let metrics = MetricsSnapshot {
            id: Uuid::new_v4().to_string(),
            timestamp: now,
            functions_count: functions,
            triggers_count: triggers,
            workers_count: workers,
            uptime_seconds: uptime,
        };

        self.store_metrics(&metrics).await;

        self.emit_event("metrics.update", serde_json::to_value(&metrics).unwrap())
            .await;
    }
}

#[service(name = "devtools")]
impl DevToolsModule {
    #[function(name = "devtools.status", description = "Get system status and health")]
    pub async fn get_status(&self, _input: EmptyInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let uptime = now.saturating_sub(self.start_time);
        let workers = self.engine.worker_registry.workers.read().await.len();
        let functions = self.engine.functions.iter().count();
        let triggers = self
            .engine
            .trigger_registry
            .triggers
            .read()
            .await
            .iter()
            .count();

        let response = StatusResponse {
            status: "healthy".to_string(),
            uptime_seconds: uptime,
            uptime_formatted: format_uptime(uptime),
            workers,
            functions,
            triggers,
            version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: now,
        };

        FunctionResult::Success(Some(api_response(json!({ "status": response }))))
    }

    #[function(
        name = "devtools.functions",
        description = "List all registered functions"
    )]
    pub async fn list_functions(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let functions: Vec<FunctionInfo> = self
            .engine
            .functions
            .iter()
            .map(|entry| {
                let func = entry.value();
                let path = func._function_path.clone();
                let internal = path.starts_with("devtools.") 
                    || path.starts_with("iii.")
                    || path.starts_with("streams.")
                    || path.starts_with("logger.")
                    || path.starts_with("core.")
                    || path.contains("(")  // SDK internal like streams.get(todo)
                    || path == "emit";
                FunctionInfo {
                    path,
                    description: func._description.clone(),
                    metadata: func.metadata.clone(),
                    internal,
                }
            })
            .collect();

        FunctionResult::Success(Some(api_response(json!({
            "functions": functions,
            "count": functions.len()
        }))))
    }

    #[function(
        name = "devtools.triggers",
        description = "List all registered triggers"
    )]
    pub async fn list_triggers(
        &self,
        input: GetTriggersInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let triggers_guard = self.engine.trigger_registry.triggers.read().await;

        let triggers: Vec<TriggerInfo> = triggers_guard
            .iter()
            .filter(|entry| {
                if let Some(ref filter_type) = input.trigger_type {
                    entry.value().trigger_type == *filter_type
                } else {
                    true
                }
            })
            .map(|entry| {
                let t = entry.value();
                let internal = t.function_path.starts_with("devtools.")
                    || t.function_path.starts_with("iii.")
                    || t.function_path.starts_with("streams.")
                    || t.id.starts_with("devtools-");
                TriggerInfo {
                    id: t.id.clone(),
                    trigger_type: t.trigger_type.clone(),
                    function_path: t.function_path.clone(),
                    config: t.config.clone(),
                    worker_id: t.worker_id.map(|id| id.to_string()),
                    internal,
                }
            })
            .collect();

        FunctionResult::Success(Some(api_response(json!({
            "triggers": triggers,
            "count": triggers.len()
        }))))
    }

    #[function(
        name = "devtools.trigger_types",
        description = "List all registered trigger types"
    )]
    pub async fn list_trigger_types(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let types_guard = self.engine.trigger_registry.trigger_types.read().await;

        let types: Vec<Value> = types_guard
            .iter()
            .map(|entry| {
                json!({
                    "id": entry.key(),
                    "description": entry.value()._description
                })
            })
            .collect();

        FunctionResult::Success(Some(api_response(json!({
            "trigger_types": types,
            "count": types.len()
        }))))
    }

    #[function(name = "devtools.config", description = "Get engine configuration")]
    pub async fn get_config(&self, _input: EmptyInput) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(api_response(json!({
            "devtools": {
                "enabled": self.config.enabled,
                "api_prefix": self.config.api_prefix,
                "metrics_enabled": self.config.metrics_enabled,
                "metrics_interval": self.config.metrics_interval,
                "state_stream": self.config.state_stream,
                "event_topic": self.config.event_topic,
            },
            "engine": {
                "version": env!("CARGO_PKG_VERSION"),
            }
        }))))
    }

    #[function(
        name = "devtools.metrics",
        description = "Get current metrics snapshot"
    )]
    pub async fn get_metrics(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let uptime = now.saturating_sub(self.start_time);
        let workers = self.engine.worker_registry.workers.read().await.len();
        let functions = self.engine.functions.iter().count();
        let triggers = self
            .engine
            .trigger_registry
            .triggers
            .read()
            .await
            .iter()
            .count();

        let metrics = MetricsSnapshot {
            id: Uuid::new_v4().to_string(),
            timestamp: now,
            functions_count: functions,
            triggers_count: triggers,
            workers_count: workers,
            uptime_seconds: uptime,
        };

        let self_clone = self.clone();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            self_clone.store_metrics(&metrics_clone).await;
            self_clone
                .emit_event(
                    "metrics.update",
                    serde_json::to_value(&metrics_clone).unwrap(),
                )
                .await;
        });

        FunctionResult::Success(Some(api_response(serde_json::to_value(&metrics).unwrap())))
    }

    #[function(name = "devtools.metrics_history", description = "Get metrics history")]
    pub async fn get_metrics_history(
        &self,
        input: GetMetricsHistoryInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let history = self.fetch_metrics_history(input.limit).await;

        FunctionResult::Success(Some(api_response(json!({
            "history": history,
            "count": history.len()
        }))))
    }

    #[function(name = "devtools.workers", description = "List all connected workers")]
    pub async fn list_workers(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let workers_guard = self.engine.worker_registry.workers.read().await;

        let mut workers: Vec<Value> = Vec::new();
        for entry in workers_guard.iter() {
            let worker = entry.value();
            let function_paths: Vec<String> =
                worker.function_paths.read().await.iter().cloned().collect();

            workers.push(json!({
                "id": worker.id.to_string(),
                "functions": function_paths,
            }));
        }

        FunctionResult::Success(Some(api_response(json!({
            "workers": workers,
            "count": workers.len()
        }))))
    }

    #[function(name = "devtools.health", description = "Health check")]
    pub async fn health_check(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(api_response(json!({
            "status": "ok",
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        }))))
    }

    #[function(
        name = "devtools.events_info",
        description = "Get events subscription info"
    )]
    pub async fn get_events_info(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        FunctionResult::Success(Some(api_response(json!({
            "topic": self.config.event_topic,
            "stream": self.config.state_stream,
            "description": "Subscribe to this topic for real-time DevTools updates"
        }))))
    }

    #[function(name = "devtools.streams", description = "List all streams")]
    pub async fn list_streams(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let mut streams = Vec::new();
        let mut seen_streams: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Ok(Some(result)) = self
            .engine
            .invoke_function("streams.listStreams", json!({}))
            .await
        {
            if let Some(stream_names) = result.get("streams").and_then(|s| s.as_array()) {
                for stream_name_value in stream_names {
                    if let Some(stream_name) = stream_name_value.as_str() {
                        if seen_streams.contains(stream_name) {
                            continue;
                        }
                        seen_streams.insert(stream_name.to_string());

                        let internal = stream_name == "iii"
                            || stream_name.starts_with("iii:")
                            || stream_name.starts_with("iii.");

                        let mut group_ids = Vec::new();
                        let mut total_count: usize = 0;
                        let stream_type = if stream_name == "iii.logs" {
                            "logs"
                        } else {
                            "state"
                        };

                        if let Ok(Some(groups_result)) = self
                            .engine
                            .invoke_function(
                                "streams.listGroups",
                                json!({"stream_name": stream_name}),
                            )
                            .await
                        {
                            if let Some(groups) =
                                groups_result.get("groups").and_then(|g| g.as_array())
                            {
                                for g in groups {
                                    if let Some(id) = g.get("id").and_then(|id| id.as_str()) {
                                        group_ids.push(id.to_string());
                                    }
                                    if let Some(count) = g.get("count").and_then(|c| c.as_u64()) {
                                        total_count += count as usize;
                                    }
                                }
                            }
                        }

                        let description = if internal {
                            if stream_name == "iii.logs" {
                                format!("Application logs ({} entries)", total_count)
                            } else {
                                format!("System stream: {}", stream_name)
                            }
                        } else {
                            format!("User stream ({} items)", total_count)
                        };

                        streams.push(json!({
                            "id": stream_name,
                            "type": stream_type,
                            "description": description,
                            "groups": group_ids,
                            "status": "active",
                            "internal": internal
                        }));
                    }
                }
            }
        }

        if !seen_streams.contains(&self.config.state_stream) {
            streams.push(json!({
                "id": self.config.state_stream.clone(),
                "type": "state",
                "description": "DevTools metrics and state data",
                "groups": ["metrics"],
                "status": "active",
                "internal": true
            }));
        }

        if !seen_streams.contains(&self.config.event_topic) {
            streams.push(json!({
                "id": self.config.event_topic.clone(),
                "type": "events",
                "description": "DevTools real-time events",
                "groups": ["events"],
                "status": "active",
                "internal": true
            }));
        }

        FunctionResult::Success(Some(api_response(json!({
            "streams": streams,
            "count": streams.len(),
            "websocket_port": 31112
        }))))
    }

    #[function(
        name = "devtools.stream_group",
        description = "Get contents of a stream group"
    )]
    pub async fn get_stream_group(
        &self,
        input: StreamGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let stream_name = input.body.stream_name;
        let group_id = input.body.group_id;

        let invoke_input = json!({
            "stream_name": stream_name,
            "group_id": group_id
        });

        match self
            .engine
            .invoke_function("streams.getGroup", invoke_input)
            .await
        {
            Ok(Some(result)) => {
                let items: Vec<Value> = if let Ok(items) = serde_json::from_value(result.clone()) {
                    items
                } else {
                    vec![result]
                };

                FunctionResult::Success(Some(api_response(json!({
                    "stream_name": stream_name,
                    "group_id": group_id,
                    "items": items,
                    "count": items.len()
                }))))
            }
            Ok(None) => FunctionResult::Success(Some(api_response(json!({
                "stream_name": stream_name,
                "group_id": group_id,
                "items": [],
                "count": 0
            })))),
            Err(e) => FunctionResult::Success(Some(api_response(json!({
                "stream_name": stream_name,
                "group_id": group_id,
                "items": [],
                "count": 0,
                "error": format!("{:?}", e)
            })))),
        }
    }

    #[function(
        name = "devtools.stream_groups",
        description = "List all groups in a stream"
    )]
    pub async fn list_stream_groups(
        &self,
        input: StreamNameInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let stream_name = input.body.stream_name;

        let invoke_input = json!({
            "stream_name": stream_name
        });

        match self
            .engine
            .invoke_function("streams.listGroups", invoke_input)
            .await
        {
            Ok(Some(result)) => FunctionResult::Success(Some(api_response(result))),
            Ok(None) => FunctionResult::Success(Some(api_response(json!({
                "stream_name": stream_name,
                "groups": [],
                "count": 0
            })))),
            Err(e) => FunctionResult::Success(Some(api_response(json!({
                "stream_name": stream_name,
                "groups": [],
                "count": 0,
                "error": format!("{:?}", e)
            })))),
        }
    }

    #[function(name = "devtools.logs", description = "Get recent logs")]
    pub async fn get_logs(&self, input: GetLogsInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let limit = input.limit.unwrap_or(50) as isize;
        let level = input.level.unwrap_or_else(|| "all".to_string());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(ref redis_conn) = self.redis_conn {
            let mut conn = redis_conn.lock().await;
            let log_strings: Vec<String> =
                conn.lrange("logs", -limit, -1).await.unwrap_or_default();

            let mut logs: Vec<Value> = Vec::new();
            for log_str in log_strings.iter().rev() {
                if let Ok(entry) = serde_json::from_str::<Value>(log_str) {
                    if level != "all" {
                        if let Some(log_level) = entry.get("level").and_then(|l| l.as_str()) {
                            if log_level != level {
                                continue;
                            }
                        }
                    }
                    logs.push(entry);
                }
            }

            return FunctionResult::Success(Some(api_response(json!({
                "logs": logs,
                "count": logs.len(),
                "filter": {
                    "level": level,
                    "limit": limit,
                    "since": input.since.unwrap_or(now - 3600)
                }
            }))));
        }

        FunctionResult::Success(Some(api_response(json!({
            "logs": [],
            "count": 0,
            "filter": {
                "level": level,
                "limit": limit,
                "since": input.since.unwrap_or(now - 3600)
            },
            "info": {
                "message": "Logs are collected by the ObservabilityModule. Configure file or redis adapter for log storage.",
                "adapters": ["file_logger", "redis_logger"]
            }
        }))))
    }

    #[function(
        name = "devtools.adapters",
        description = "List all adapters and modules"
    )]
    pub async fn list_adapters(
        &self,
        _input: EmptyInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let mut adapters = Vec::new();

        let trigger_types = self.engine.trigger_registry.trigger_types.read().await;
        for entry in trigger_types.iter() {
            adapters.push(json!({
                "id": entry.key().clone(),
                "type": "trigger",
                "status": "active",
                "health": "healthy",
                "description": entry.value()._description.clone(),
                "internal": false  // Trigger types are user-facing
            }));
        }

        let workers_count = self.engine.worker_registry.workers.read().await.len();
        adapters.push(json!({
            "id": "workers",
            "type": "worker_pool",
            "status": "active",
            "health": "healthy",
            "count": workers_count,
            "description": "Connected worker processes",
            "internal": false  // Workers are user-facing
        }));

        adapters.push(json!({
            "id": "devtools",
            "type": "module",
            "status": "active",
            "health": "healthy",
            "description": "Developer Console backend",
            "internal": true
        }));

        adapters.push(json!({
            "id": "streams",
            "type": "module",
            "status": "active",
            "health": "healthy",
            "port": 31112,
            "description": "WebSocket streams for real-time data",
            "internal": false  // Streams are user-facing
        }));

        FunctionResult::Success(Some(api_response(json!({
            "adapters": adapters,
            "count": adapters.len()
        }))))
    }
}

#[async_trait]
impl CoreModule for DevToolsModule {
    fn name(&self) -> &'static str {
        "DevToolsModule"
    }

    async fn create(
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn CoreModule>> {
        let config: DevToolsConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let redis_conn = match Client::open("redis://localhost:6379") {
            Ok(client) => match client.get_connection_manager().await {
                Ok(manager) => Some(Arc::new(Mutex::new(manager))),
                Err(e) => {
                    tracing::warn!("DevTools: Failed to connect to Redis for logs: {}", e);
                    None
                }
            },
            Err(e) => {
                tracing::warn!("DevTools: Failed to create Redis client: {}", e);
                None
            }
        };

        Ok(Box::new(Self {
            engine,
            config,
            start_time,
            redis_conn,
        }))
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        if !self.config.enabled {
            tracing::info!("DevTools module is disabled");
            return Ok(());
        }

        tracing::info!(
            "{} DevTools module on /{}/...",
            "[INITIALIZING]".cyan(),
            self.config.api_prefix
        );

        self.register_api_triggers().await?;

        if self.config.metrics_enabled {
            self.register_metrics_cron().await?;
        }

        self.register_event_triggers().await?;

        self.emit_event(
            "engine.started",
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "api_prefix": self.config.api_prefix,
            }),
        )
        .await;

        tracing::info!(
            "{} DevTools module ready - Events: {}, Stream: {}",
            "[READY]".green(),
            self.config.event_topic.purple(),
            self.config.state_stream.purple()
        );

        Ok(())
    }
}

impl DevToolsModule {
    async fn register_api_triggers(&self) -> anyhow::Result<()> {
        let prefix = &self.config.api_prefix;

        let endpoints = vec![
            ("GET", "status", "devtools.status"),
            ("GET", "health", "devtools.health"),
            ("GET", "functions", "devtools.functions"),
            ("GET", "triggers", "devtools.triggers"),
            ("GET", "trigger-types", "devtools.trigger_types"),
            ("GET", "workers", "devtools.workers"),
            ("GET", "config", "devtools.config"),
            ("GET", "metrics", "devtools.metrics"),
            ("GET", "metrics/history", "devtools.metrics_history"),
            ("GET", "events", "devtools.events_info"),
            ("GET", "streams", "devtools.streams"),
            ("POST", "streams/group", "devtools.stream_group"),
            ("POST", "streams/groups", "devtools.stream_groups"),
            ("GET", "logs", "devtools.logs"),
            ("GET", "adapters", "devtools.adapters"),
        ];

        for (method, path, function_path) in endpoints {
            let api_path = format!("{}/{}", prefix, path);

            let trigger = Trigger {
                id: format!("devtools-api-{}", path.replace("/", "-")),
                trigger_type: "api".to_string(),
                function_path: function_path.to_string(),
                config: json!({
                    "api_path": api_path,
                    "http_method": method
                }),
                worker_id: None,
            };

            if let Err(e) = self.engine.trigger_registry.register_trigger(trigger).await {
                tracing::warn!(
                    "Failed to register DevTools API trigger for {}: {}",
                    path,
                    e
                );
            }
        }

        Ok(())
    }

    async fn register_metrics_cron(&self) -> anyhow::Result<()> {
        let interval = self.config.metrics_interval;
        let cron_expr = format!("*/{} * * * * *", interval.min(59).max(1));

        let trigger = Trigger {
            id: "devtools-metrics-cron".to_string(),
            trigger_type: "cron".to_string(),
            function_path: "devtools.metrics".to_string(),
            config: json!({
                "expression": cron_expr
            }),
            worker_id: None,
        };

        if let Err(e) = self.engine.trigger_registry.register_trigger(trigger).await {
            tracing::warn!("Failed to register DevTools metrics cron: {}", e);
        } else {
            tracing::debug!("Registered metrics cron with expression: {}", cron_expr);
        }

        Ok(())
    }

    async fn register_event_triggers(&self) -> anyhow::Result<()> {
        tracing::debug!(
            "DevTools events available on topic: {}",
            self.config.event_topic
        );

        Ok(())
    }
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, minutes, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

crate::register_module!(
    "modules::devtools::DevToolsModule",
    <DevToolsModule as CoreModule>::make_module,
    enabled_by_default = true
);
