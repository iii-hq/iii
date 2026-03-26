use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::RegisterTriggerInput;

// ── HTTP ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpTriggerConfig {
    /// HTTP endpoint path (e.g. `/users/:id`)
    pub api_path: String,
    /// HTTP method (defaults to GET)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_method: Option<HttpMethod>,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl HttpTriggerConfig {
    pub fn new(api_path: impl Into<String>) -> Self {
        Self {
            api_path: api_path.into(),
            http_method: None,
            condition_function_id: None,
        }
    }

    pub fn method(mut self, method: HttpMethod) -> Self {
        self.http_method = Some(method);
        self
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }
}

// ── Cron ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CronTriggerConfig {
    /// Cron expression (6-field format: sec min hour day month weekday)
    pub expression: String,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
}

impl CronTriggerConfig {
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            condition_function_id: None,
        }
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }
}

// ── Queue ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueueTriggerConfig {
    /// Queue topic to subscribe to
    pub topic: String,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
    /// Queue-specific subscriber configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_config: Option<Value>,
}

impl QueueTriggerConfig {
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            topic: topic.into(),
            condition_function_id: None,
            queue_config: None,
        }
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }

    pub fn queue_config(mut self, config: impl Serialize) -> Result<Self, serde_json::Error> {
        self.queue_config = Some(serde_json::to_value(config)?);
        Ok(self)
    }
}

// ── PubSub (subscribe) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubscribeTriggerConfig {
    /// Topic to subscribe to
    pub topic: String,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
}

impl SubscribeTriggerConfig {
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            topic: topic.into(),
            condition_function_id: None,
        }
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }
}

// ── State ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateTriggerConfig {
    /// State scope to watch (exact match filter)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// State key to watch (exact match filter)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum StateEventType {
    #[serde(rename = "state:created")]
    Created,
    #[serde(rename = "state:updated")]
    Updated,
    #[serde(rename = "state:deleted")]
    Deleted,
}

impl StateTriggerConfig {
    pub fn new() -> Self {
        Self {
            scope: None,
            key: None,
            condition_function_id: None,
        }
    }

    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }
}

impl Default for StateTriggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ── Stream ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamJoinLeaveTriggerConfig {
    /// Stream name to watch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
}

impl StreamJoinLeaveTriggerConfig {
    pub fn new() -> Self {
        Self {
            stream_name: None,
            condition_function_id: None,
        }
    }

    pub fn stream_name(mut self, name: impl Into<String>) -> Self {
        self.stream_name = Some(name.into());
        self
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }
}

impl Default for StreamJoinLeaveTriggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamTriggerConfig {
    /// Stream name to watch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,
    /// Group ID filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Item ID filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    /// Optional function ID to evaluate before invoking handler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_function_id: Option<String>,
}

impl StreamTriggerConfig {
    pub fn new() -> Self {
        Self {
            stream_name: None,
            group_id: None,
            item_id: None,
            condition_function_id: None,
        }
    }

    pub fn stream_name(mut self, name: impl Into<String>) -> Self {
        self.stream_name = Some(name.into());
        self
    }

    pub fn group_id(mut self, id: impl Into<String>) -> Self {
        self.group_id = Some(id.into());
        self
    }

    pub fn item_id(mut self, id: impl Into<String>) -> Self {
        self.item_id = Some(id.into());
        self
    }

    pub fn condition(mut self, function_id: impl Into<String>) -> Self {
        self.condition_function_id = Some(function_id.into());
        self
    }
}

impl Default for StreamTriggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ── Log ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogTriggerConfig {
    /// Minimum log level to trigger on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<LogLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    All,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogTriggerConfig {
    pub fn new() -> Self {
        Self { level: None }
    }

    pub fn level(mut self, level: LogLevel) -> Self {
        self.level = Some(level);
        self
    }
}

impl Default for LogTriggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ── Call request types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpCallRequest {
    pub query_params: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub path: String,
    pub method: String,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CronCallRequest {
    pub trigger: String,
    pub job_id: String,
    pub scheduled_time: String,
    pub actual_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateCallRequest {
    #[serde(rename = "type")]
    pub message_type: String,
    pub event_type: StateEventType,
    pub scope: String,
    pub key: String,
    pub old_value: Option<Value>,
    pub new_value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamJoinLeaveCallRequest {
    pub event_type: String,
    pub timestamp: i64,
    pub stream_name: String,
    pub group_id: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamCallRequest {
    pub event_type: String,
    pub timestamp: i64,
    pub stream_name: String,
    pub group_id: String,
    pub id: Option<String>,
    pub event: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogCallRequest {
    pub timestamp_unix_nano: u64,
    pub observed_timestamp_unix_nano: u64,
    pub severity_number: u32,
    pub severity_text: String,
    pub body: String,
    pub attributes: Value,
    pub trace_id: String,
    pub span_id: String,
    pub resource: Value,
    pub service_name: String,
    pub instrumentation_scope_name: String,
    pub instrumentation_scope_version: String,
}

// ── IIITrigger enum ────────────────────────────────────────────────────

/// Enum of all built-in trigger types with typed configuration.
///
/// Use `.for_function()` to create a [`RegisterTriggerInput`]:
/// ```rust,no_run
/// # use iii_sdk::builtin_triggers::*;
/// let input = IIITrigger::Cron(CronTriggerConfig::new("0 * * * * *"))
///     .for_function("my::handler");
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum IIITrigger {
    Http(HttpTriggerConfig),
    Cron(CronTriggerConfig),
    Queue(QueueTriggerConfig),
    Subscribe(SubscribeTriggerConfig),
    State(StateTriggerConfig),
    Stream(StreamTriggerConfig),
    StreamJoin(StreamJoinLeaveTriggerConfig),
    StreamLeave(StreamJoinLeaveTriggerConfig),
    Log(LogTriggerConfig),
}

impl IIITrigger {
    fn trigger_type_id(&self) -> &'static str {
        match self {
            Self::Http(_) => "http",
            Self::Cron(_) => "cron",
            Self::Queue(_) => "queue",
            Self::Subscribe(_) => "subscribe",
            Self::State(_) => "state",
            Self::Stream(_) => "stream",
            Self::StreamJoin(_) => "stream:join",
            Self::StreamLeave(_) => "stream:leave",
            Self::Log(_) => "log",
        }
    }

    /// Create a [`RegisterTriggerInput`] binding this trigger to a function.
    pub fn for_function(self, function_id: impl Into<String>) -> RegisterTriggerInput {
        RegisterTriggerInput {
            trigger_type: self.trigger_type_id().to_string(),
            function_id: function_id.into(),
            config: serde_json::to_value(&self).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn http_trigger_config_serializes_typed_method_enum() {
        let config = HttpTriggerConfig::new("health").method(HttpMethod::Get);
        let value = serde_json::to_value(config).expect("http trigger config should serialize");

        assert_eq!(value["http_method"], "GET");
    }

    #[test]
    fn log_trigger_config_serializes_typed_level_enum() {
        let config = LogTriggerConfig::new().level(LogLevel::Error);
        let value = serde_json::to_value(config).expect("log trigger config should serialize");

        assert_eq!(value["level"], "error");
    }

    #[test]
    fn state_call_request_deserializes_typed_event_type() {
        let request: StateCallRequest = serde_json::from_value(json!({
            "type": "state",
            "event_type": "state:updated",
            "scope": "users",
            "key": "123",
            "old_value": { "name": "old" },
            "new_value": { "name": "new" }
        }))
        .expect("state call request should deserialize");

        assert!(matches!(request.event_type, StateEventType::Updated));
    }

    #[test]
    fn queue_config_returns_error_instead_of_panicking() {
        struct FailingSerialize;

        impl Serialize for FailingSerialize {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("boom"))
            }
        }

        let result = QueueTriggerConfig::new("emails").queue_config(FailingSerialize);

        assert!(result.is_err(), "serialization failures should be returned");
    }

    #[test]
    fn stream_join_uses_dedicated_join_leave_config_shape() {
        let trigger =
            IIITrigger::StreamJoin(StreamJoinLeaveTriggerConfig::new().stream_name("chat"))
                .for_function("example::on_join");

        assert_eq!(trigger.config, json!({ "stream_name": "chat" }));
    }
}
