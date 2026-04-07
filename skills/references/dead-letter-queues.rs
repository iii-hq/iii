/// Pattern: Dead Letter Queues
/// Comparable to: SQS DLQ, RabbitMQ dead-letter exchanges, BullMQ failed jobs
///
/// When a queued function exhausts its retry budget (configured via
/// queue_configs.max_retries and backoff_ms in iii.config.yaml) the message
/// moves to the queue's dead-letter queue (DLQ). Messages in the DLQ can be
/// inspected and redriven back to the source queue via the SDK or CLI.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

// Queue configuration reference (iii.config.yaml)
//
//   queue_configs:
//     payment:
//       max_retries: 3        # after 3 failures the message goes to DLQ
//       backoff_ms: 1000      # exponential backoff base
//     email:
//       max_retries: 5
//       backoff_ms: 2000

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ChargeInput {
    order_id: String,
    amount: Option<f64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SubmitPaymentInput {
    order_id: String,
    amount: f64,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // 1. Function that processes payments - may fail and exhaust retries
    // After max_retries failures the message lands in the "payment" DLQ.
    // ---
    iii.register_function(
        RegisterFunction::new("payments::charge", |data: ChargeInput| -> Result<serde_json::Value, String> {
            let logger = Logger::new();
            logger.info("Attempting payment charge", &json!({ "orderId": data.order_id }));

            let gateway_up = rand::random::<f64>() > 0.7;
            if !gateway_up {
                return Err("Payment gateway timeout - will be retried".into());
            }

            logger.info("Payment succeeded", &json!({ "orderId": data.order_id }));
            Ok(json!({ "charged": true, "order_id": data.order_id }))
        })
        .description("Charge payment (may fail for DLQ demo)"),
    );

    iii.register_trigger(
        IIITrigger::Queue(QueueTriggerConfig::new("payment"))
            .for_function("payments::charge"),
    )
    .expect("failed");

    // ---
    // 2. Enqueue a payment to demonstrate the retry / DLQ flow
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::submit-payment", move |data: SubmitPaymentInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let receipt = iii
                    .trigger(TriggerRequest {
                        function_id: "payments::charge".into(),
                        payload: json!({ "order_id": data.order_id, "amount": data.amount }),
                        action: Some(TriggerAction::Enqueue { queue: "payment".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Payment enqueued", &json!({ "receiptId": receipt["messageReceiptId"] }));
                Ok(receipt)
            }
        })
        .description("Submit a payment to the queue"),
    );

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/orders/pay").method(HttpMethod::Post))
            .for_function("orders::submit-payment"),
    )
    .expect("failed");

    // ---
    // 3. Redrive DLQ messages back to the source queue via SDK
    // Calls the built-in iii::queue::redrive function. Returns the queue name
    // and the count of redriven messages.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("admin::redrive-payments", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "iii::queue::redrive".into(),
                        payload: json!({ "queue": "payment" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Redrive complete", &json!({ "queue": result["queue"], "redriven": result["redriven"] }));
                Ok(result)
            }
        })
        .description("Redrive payment DLQ messages"),
    );

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/admin/redrive/payments").method(HttpMethod::Post))
            .for_function("admin::redrive-payments"),
    )
    .expect("failed");

    // CLI alternative for redrive (run from terminal):
    //   iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}'
    //   iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}' --timeout-ms=60000

    // ---
    // 4. DLQ inspection pattern - check how many messages are stuck
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("admin::dlq-status", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let queues = vec!["payment", "email"];
                let mut statuses = Vec::new();

                for queue in queues {
                    let info = iii
                        .trigger(TriggerRequest {
                            function_id: "iii::queue::status".into(),
                            payload: json!({ "queue": queue }),
                            action: None,
                            timeout_ms: None,
                        })
                        .await
                        .map_err(|e| e.to_string())?;

                    logger.info("Queue status", &json!({
                        "queue": queue,
                        "dlq_count": info["dlq_count"],
                        "pending": info["pending"],
                    }));

                    statuses.push(json!({
                        "queue": queue,
                        "dlq_count": info["dlq_count"],
                        "pending": info["pending"],
                    }));
                }

                Ok(json!({ "queues": statuses }))
            }
        })
        .description("Inspect DLQ status for all queues"),
    );

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/admin/dlq/status").method(HttpMethod::Get))
            .for_function("admin::dlq-status"),
    )
    .expect("failed");

    // ---
    // 5. Targeted redrive - redrive a single queue from a cron schedule
    // Useful for automatically retrying failed messages every hour.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("admin::auto-redrive", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "iii::queue::redrive".into(),
                        payload: json!({ "queue": "payment" }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let redriven = result["redriven"].as_u64().unwrap_or(0);
                if redriven > 0 {
                    logger.info("Auto-redrive recovered messages", &json!({ "redriven": redriven }));
                }

                Ok(result)
            }
        })
        .description("Auto-redrive payment DLQ every hour"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 * * * *"))
            .for_function("admin::auto-redrive"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
