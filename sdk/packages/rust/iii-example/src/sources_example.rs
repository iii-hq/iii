use iii_sdk::engine::EngineSources;
use iii_sdk::{Deliver, Error, IIIClient, RegisterFunction, Source};
use serde_json::json;

/// Observers of the event sources the cron, state, http, pubsub, queue,
/// stream and observability workers host: when the source runs, the target
/// runs. `args` tell the source's worker what to set up (a cron expression, a
/// route, a queue subscriber); `filters` pick the calls that reach the target.
pub fn setup(iii: &IIIClient) {
    // ── Cron: one call per tick of the expression ────────────────────
    iii.register_function(
        "example::scheduled_cleanup",
        RegisterFunction::new(scheduled_cleanup).description("Runs periodic cleanup every minute"),
    );
    iii.observe(
        Source::function("cron::on-tick")
            .args(json!({ "expression": "0 * * * * * *" }))
            .deliver(Deliver::Input),
        "example::scheduled_cleanup",
    );

    // ── State: changes of the `users` scope ──────────────────────────
    iii.register_function(
        "example::on_user_updated",
        RegisterFunction::new(on_user_updated)
            .description("Reacts when a user record is updated in state"),
    );
    iii.observe(
        Source::function("state::on-changed")
            .filters(json!({ "input.scope": { "$eq": "users" } }))
            .deliver(Deliver::Input),
        "example::on_user_updated",
    );

    // ── HTTP: a route of the http worker ─────────────────────────────
    iii.register_function(
        "example::health_check",
        RegisterFunction::new(health_check).description("Simple health check endpoint"),
    );
    iii.observe(
        Source::function("http::on-request").args(json!({ "method": "GET", "path": "/health" })),
        "example::health_check",
    );

    // ── Pubsub: messages published to a topic ────────────────────────
    iii.register_function(
        "example::on_order_created",
        RegisterFunction::new(on_order_created).description("Processes new order events"),
    );
    iii.observe(
        Source::function("publish")
            .filters(json!({ "input.topic": { "$eq": "orders.created" } }))
            .deliver(Deliver::Input),
        "example::on_order_created",
    );

    // ── Queue: a subscriber of a topic ───────────────────────────────
    iii.register_function(
        "example::process_email",
        RegisterFunction::new(process_email).description("Processes emails from the queue"),
    );
    iii.observe(
        Source::function("queue::on-message").args(json!({ "topic": "emails" })),
        "example::process_email",
    );

    // ── Logs: error records only ─────────────────────────────────────
    iii.register_function(
        "example::on_error_log",
        RegisterFunction::new(on_error_log).description("Alerts on error logs"),
    );
    iii.observe(
        Source::function(EngineSources::LOG)
            .filters(json!({ "input.severity_text": { "$eq": "ERROR" } }))
            .deliver(Deliver::Input),
        "example::on_error_log",
    );

    // ── Streams: changes of the `chat` stream ────────────────────────
    iii.register_function(
        "example::on_chat_message",
        RegisterFunction::new(on_chat_message).description("Handles chat stream events"),
    );
    iii.observe(
        Source::function(EngineSources::STREAM_CHANGED)
            .filters(json!({ "input.stream_name": { "$eq": "chat" } }))
            .deliver(Deliver::Input),
        "example::on_chat_message",
    );
}

// ── Handlers ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CronTick {
    expression: String,
    scheduled_time: String,
}

fn scheduled_cleanup(tick: CronTick) -> Result<serde_json::Value, Error> {
    Ok(json!({ "cleaned": true, "expression": tick.expression, "at": tick.scheduled_time }))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StateChange {
    event_type: String,
    scope: String,
    key: String,
    #[allow(dead_code)]
    new_value: serde_json::Value,
}

fn on_user_updated(change: StateChange) -> Result<serde_json::Value, Error> {
    Ok(json!({ "event": change.event_type, "scope": change.scope, "key": change.key }))
}

fn health_check(_request: serde_json::Value) -> Result<serde_json::Value, Error> {
    Ok(json!({ "status_code": 200, "body": { "status": "ok" } }))
}

fn on_order_created(message: serde_json::Value) -> Result<serde_json::Value, Error> {
    Ok(json!({ "processed": true, "order": message["data"] }))
}

fn process_email(message: serde_json::Value) -> Result<serde_json::Value, Error> {
    Ok(json!({ "sent": true, "email": message }))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LogRecord {
    severity_text: String,
    body: String,
}

fn on_error_log(record: LogRecord) -> Result<serde_json::Value, Error> {
    Ok(json!({ "alerted": true, "severity": record.severity_text, "message": record.body }))
}

fn on_chat_message(change: serde_json::Value) -> Result<serde_json::Value, Error> {
    Ok(json!({ "received": true, "event": change }))
}
