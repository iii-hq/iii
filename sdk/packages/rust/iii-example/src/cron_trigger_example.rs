use iii_sdk::builtin_triggers::*;
use iii_sdk::{III, IIITrigger, RegisterFunction};
use serde_json::json;

/// Examples using built-in trigger types with the typed `IIITrigger` enum.
pub fn setup(iii: &III) {
    // ── Cron trigger ────────────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::scheduled_cleanup", scheduled_cleanup)
            .description("Runs periodic cleanup every minute"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 * * * * *"))
            .for_function("example::scheduled_cleanup"),
    )
    .expect("failed to register cron trigger");

    // ── State trigger ───────────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::on_user_updated", on_user_updated)
            .description("Reacts when a user record is updated in state"),
    );

    iii.register_trigger(
        IIITrigger::State(StateTriggerConfig::new().scope("users"))
            .for_function("example::on_user_updated"),
    )
    .expect("failed to register state trigger");

    // ── HTTP trigger (GET) ──────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::health_check", health_check)
            .description("Simple health check endpoint"),
    );

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("health").method(HttpMethod::Get))
            .for_function("example::health_check"),
    )
    .expect("failed to register http trigger");

    // ── Subscribe trigger ───────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::on_order_created", on_order_created)
            .description("Processes new order events"),
    );

    iii.register_trigger(
        IIITrigger::Subscribe(SubscribeTriggerConfig::new("orders.created"))
            .for_function("example::on_order_created"),
    )
    .expect("failed to register subscribe trigger");

    // ── Queue trigger ───────────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::process_email", process_email)
            .description("Processes emails from the queue"),
    );

    iii.register_trigger(
        IIITrigger::Queue(QueueTriggerConfig::new("emails")).for_function("example::process_email"),
    )
    .expect("failed to register queue trigger");

    // ── Log trigger ─────────────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::on_error_log", on_error_log)
            .description("Alerts on error logs"),
    );

    iii.register_trigger(
        IIITrigger::Log(LogTriggerConfig::new().level(LogLevel::Error))
            .for_function("example::on_error_log"),
    )
    .expect("failed to register log trigger");

    // ── Stream trigger ──────────────────────────────────────────────
    iii.register_function(
        RegisterFunction::new("example::on_chat_message", on_chat_message)
            .description("Handles chat stream events"),
    );

    iii.register_trigger(
        IIITrigger::Stream(StreamTriggerConfig::new().stream_name("chat"))
            .for_function("example::on_chat_message"),
    )
    .expect("failed to register stream trigger");
}

// ── Handlers ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CronEvent {
    trigger: String,
    job_id: String,
}

fn scheduled_cleanup(input: CronEvent) -> Result<serde_json::Value, String> {
    Ok(json!({ "cleaned": true, "trigger": input.trigger, "job_id": input.job_id }))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StateEvent {
    event_type: String,
    scope: String,
    key: String,
    new_value: serde_json::Value,
}

fn on_user_updated(input: StateEvent) -> Result<serde_json::Value, String> {
    Ok(json!({ "event": input.event_type, "scope": input.scope, "key": input.key }))
}

fn health_check(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({ "status": "ok" }))
}

fn on_order_created(input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({ "processed": true, "order": input }))
}

fn process_email(input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({ "sent": true, "email": input }))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LogEvent {
    severity_text: String,
    body: String,
}

fn on_error_log(input: LogEvent) -> Result<serde_json::Value, String> {
    Ok(json!({ "alerted": true, "severity": input.severity_text, "message": input.body }))
}

fn on_chat_message(input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({ "received": true, "event": input }))
}
