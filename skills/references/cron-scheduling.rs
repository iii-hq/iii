/// Pattern: Cron Scheduling
/// Comparable to: node-cron, APScheduler, crontab
///
/// Schedules recurring tasks using 7-field cron expressions:
///   second  minute  hour  day  month  weekday  year
///
/// Cron handlers should be fast - enqueue heavy work to a queue.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CleanupInput {
    #[serde(rename = "sessionId")]
    session_id: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // Hourly cleanup - runs at the top of every hour
    // Cron: 0 0 * * * * *  (second=0, minute=0, every hour)
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("cron::hourly-cleanup", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                logger.info("Hourly cleanup started", &json!({}));

                let expired_items = iii
                    .trigger(TriggerRequest {
                        function_id: "state::list".into(),
                        payload: json!({ "scope": "sessions" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let now = chrono::Utc::now().timestamp_millis();
                let mut cleaned = 0u64;

                if let Some(sessions) = expired_items.as_array() {
                    for session in sessions {
                        let last_active = session["last_active"]
                            .as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.timestamp_millis())
                            .unwrap_or(0);

                        let age = now - last_active;
                        if age > 3_600_000 {
                            let session_id = session["id"].as_str().unwrap_or("").to_string();
                            iii.trigger(TriggerRequest {
                                function_id: "cleanup::process-expired".into(),
                                payload: json!({ "sessionId": session_id }),
                                action: Some(TriggerAction::Enqueue { queue: "cleanup".into() }),
                                timeout_ms: None,
                            })
                            .await
                            .ok();
                            cleaned += 1;
                        }
                    }
                }

                logger.info("Hourly cleanup enqueued", &json!({ "cleaned": cleaned }));
                Ok(json!({ "cleaned": cleaned }))
            }
        })
        .description("Hourly cleanup of expired sessions"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 0 * * * * *"))
            .for_function("cron::hourly-cleanup"),
    )
    .expect("failed");

    // ---
    // Daily report - runs at midnight every day
    // Cron: 0 0 0 * * * *  (second=0, minute=0, hour=0, every day)
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("cron::daily-report", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                logger.info("Daily report generation started", &json!({}));

                let metrics = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "daily-metrics", "key": "today" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let metrics_val = if metrics.is_null() {
                    json!({ "signups": 0, "orders": 0, "revenue": 0 })
                } else {
                    metrics
                };

                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "reports::generate".into(),
                        payload: json!({
                            "type": "daily-summary",
                            "date": today,
                            "metrics": metrics_val,
                        }),
                        action: Some(TriggerAction::Enqueue { queue: "reports".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Daily report enqueued", &json!({ "messageReceiptId": result["messageReceiptId"] }));

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "daily-metrics",
                        "key": "today",
                        "value": {
                            "signups": 0,
                            "orders": 0,
                            "revenue": 0,
                            "reset_at": chrono::Utc::now().to_rfc3339(),
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "status": "enqueued" }))
            }
        })
        .description("Generate daily report at midnight"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 0 0 * * * *"))
            .for_function("cron::daily-report"),
    )
    .expect("failed");

    // ---
    // Health check - runs every 5 minutes
    // Cron: 0 */5 * * * * *  (second=0, every 5th minute)
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("cron::health-check", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let timestamp = chrono::Utc::now().to_rfc3339();

                let status = iii
                    .trigger(TriggerRequest {
                        function_id: "state::get".into(),
                        payload: json!({ "scope": "system", "key": "health" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let healthy = !status.is_null();

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "system",
                        "key": "health",
                        "value": { "healthy": healthy, "checked_at": timestamp },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                if !healthy {
                    logger.warn("Health check failed", &json!({ "timestamp": timestamp }));

                    iii.trigger(TriggerRequest {
                        function_id: "alerts::send".into(),
                        payload: json!({ "type": "health-check-failed", "timestamp": timestamp }),
                        action: Some(TriggerAction::Enqueue { queue: "alerts".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .ok();
                }

                Ok(json!({ "healthy": healthy, "checked_at": timestamp }))
            }
        })
        .description("Health check every 5 minutes"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 */5 * * * * *"))
            .for_function("cron::health-check"),
    )
    .expect("failed");

    // ---
    // Worker for enqueued cleanup tasks
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("cleanup::process-expired", move |data: CleanupInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                iii.trigger(TriggerRequest {
                    function_id: "state::delete".into(),
                    payload: json!({ "scope": "sessions", "key": data.session_id }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                logger.info("Expired session cleaned up", &json!({ "sessionId": data.session_id }));
                Ok(json!({ "deleted": data.session_id }))
            }
        })
        .description("Clean up an expired session"),
    );

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
