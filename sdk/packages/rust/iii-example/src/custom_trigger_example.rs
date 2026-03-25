use iii_sdk::{III, IIIError, RegisterTriggerType, TriggerConfig, TriggerHandler};
use serde_json::json;

// ── Example 1: Typed trigger with full config ───────────────────────────

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduleTriggerConfig {
    /// ISO 8601 datetime for when to fire (e.g. "2099-01-01T10:00:00Z")
    pub at: String,
    /// Optional timezone (defaults to UTC)
    pub timezone: Option<String>,
    /// Whether to repeat at the same time daily
    pub repeat_daily: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduleCallRequest {
    /// The scheduled datetime that triggered this
    pub scheduled_at: String,
    /// Actual firing time
    pub fired_at: String,
    /// How many times this schedule has fired
    pub fire_count: u64,
}

struct ScheduleHandler;

#[async_trait::async_trait]
impl TriggerHandler for ScheduleHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        println!("[schedule] Registered: {}", config.config);
        Ok(())
    }
    async fn unregister_trigger(&self, _config: TriggerConfig) -> Result<(), IIIError> {
        Ok(())
    }
}

// ── Example 2: Typed trigger with minimal config ────────────────────────

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct FileWatchConfig {
    /// Glob pattern to watch (e.g. "/data/*.csv")
    pub pattern: String,
    /// Watch for these events
    pub events: Vec<FileEvent>,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub enum FileEvent {
    #[serde(rename = "created")]
    Created,
    #[serde(rename = "modified")]
    Modified,
    #[serde(rename = "deleted")]
    Deleted,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct FileWatchCallRequest {
    pub event: String,
    pub path: String,
    pub size_bytes: Option<u64>,
}

struct FileWatchHandler;

#[async_trait::async_trait]
impl TriggerHandler for FileWatchHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        println!("[file-watch] Watching: {}", config.config);
        Ok(())
    }
    async fn unregister_trigger(&self, _config: TriggerConfig) -> Result<(), IIIError> {
        Ok(())
    }
}

// ── Example 3: Untyped trigger (no trigger_request_format) ──────────────

struct NoopHandler;

#[async_trait::async_trait]
impl TriggerHandler for NoopHandler {
    async fn register_trigger(&self, _config: TriggerConfig) -> Result<(), IIIError> {
        Ok(())
    }
    async fn unregister_trigger(&self, _config: TriggerConfig) -> Result<(), IIIError> {
        Ok(())
    }
}

// ── Setup ───────────────────────────────────────────────────────────────

pub fn setup(iii: &III) {
    // ── Example 1: Schedule trigger (fully typed) ───────────────────
    let schedule = iii.register_trigger_type(
        RegisterTriggerType::new(
            "schedule",
            "One-time or daily scheduled trigger",
            ScheduleHandler,
        )
        .trigger_request_format::<ScheduleTriggerConfig>()
        .call_request_format::<ScheduleCallRequest>(),
    );

    // register_function on the handle: enforces Fn(ScheduleCallRequest) -> ...
    schedule.register_function("example::send_report", |_input: ScheduleCallRequest| {
        Ok::<_, String>(json!({ "sent": true }))
    });

    // register_trigger on the handle: enforces ScheduleTriggerConfig
    schedule
        .register_trigger(
            "example::send_report",
            ScheduleTriggerConfig {
                at: "2099-01-01T09:00:00Z".into(),
                timezone: Some("America/Sao_Paulo".into()),
                repeat_daily: Some(true),
            },
        )
        .expect("failed to register schedule trigger");

    // ── Example 2: File watch trigger (typed with enum) ─────────────
    let file_watch = iii.register_trigger_type(
        RegisterTriggerType::new(
            "file-watch",
            "Watch filesystem for changes",
            FileWatchHandler,
        )
        .trigger_request_format::<FileWatchConfig>()
        .call_request_format::<FileWatchCallRequest>(),
    );

    // Compile-time safe: function input must be FileWatchCallRequest
    file_watch.register_function("example::process_csv", |input: FileWatchCallRequest| {
        Ok::<_, String>(json!({ "processed": true, "path": input.path }))
    });

    // Compile-time safe: config must be FileWatchConfig
    file_watch
        .register_trigger(
            "example::process_csv",
            FileWatchConfig {
                pattern: "/data/incoming/*.csv".into(),
                events: vec![FileEvent::Created, FileEvent::Modified],
            },
        )
        .expect("failed to register file-watch trigger");

    // ── Example 3: Untyped trigger (Value fallback) ─────────────────
    // When no formats are set, TriggerTypeRef<Value, Value> accepts json!() and Value functions
    let custom = iii.register_trigger_type(RegisterTriggerType::new(
        "custom-event",
        "Generic custom event trigger",
        NoopHandler,
    ));

    custom.register_function("example::on_custom_event", |input: serde_json::Value| {
        Ok::<_, String>(json!({ "received": input }))
    });

    custom
        .register_trigger(
            "example::on_custom_event",
            json!({ "channel": "notifications", "priority": "high" }),
        )
        .expect("failed to register custom-event trigger");
}
