/// Pattern: Queue Processing
/// Comparable to: BullMQ, Celery, SQS
///
/// Enqueue work for durable, retryable async processing.
/// Standard queues process concurrently; FIFO queues preserve order.
///
/// Retry / backoff is configured in iii-config.yaml under queue_configs:
///   queue_configs:
///     - name: payment
///       max_retries: 3
///       backoff_ms: 1000
///       backoff_multiplier: 2
///     - name: email
///       fifo: true
///       max_retries: 5
///       backoff_ms: 500

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SubmitPaymentInput {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: f64,
    currency: Option<String>,
    #[serde(rename = "paymentMethod")]
    payment_method: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ProcessPaymentInput {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: f64,
    currency: String,
    method: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct EnqueueEmailInput {
    to: String,
    subject: String,
    body: String,
    template: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SendEmailInput {
    to: String,
    subject: String,
    body: Option<String>,
    template: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PlaceOrderInput {
    #[serde(rename = "orderId")]
    order_id: String,
    total: f64,
    method: Option<String>,
    email: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // Enqueue work - standard queue (concurrent processing)
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("payments::submit", move |data: SubmitPaymentInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "payments::process".into(),
                        payload: json!({
                            "orderId": data.order_id,
                            "amount": data.amount,
                            "currency": data.currency.unwrap_or("usd".into()),
                            "method": data.payment_method,
                        }),
                        action: Some(TriggerAction::Enqueue { queue: "payment".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Payment enqueued", &json!({
                    "orderId": data.order_id,
                    "messageReceiptId": result["messageReceiptId"],
                }));

                Ok(json!({ "status": "queued", "messageReceiptId": result["messageReceiptId"] }))
            }
        })
        .description("Submit a payment for queued processing"),
    );

    // ---
    // Process payment - handler that runs from the queue
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("payments::process", move |data: ProcessPaymentInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                logger.info("Processing payment", &json!({ "orderId": data.order_id, "amount": data.amount }));

                let charge_id = format!("ch-{}", chrono::Utc::now().timestamp_millis());

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "payments",
                        "key": data.order_id,
                        "value": {
                            "orderId": data.order_id,
                            "chargeId": charge_id,
                            "amount": data.amount,
                            "currency": data.currency,
                            "status": "captured",
                            "processed_at": chrono::Utc::now().to_rfc3339(),
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                iii.trigger(TriggerRequest {
                    function_id: "notifications::send".into(),
                    payload: json!({ "type": "payment_captured", "orderId": data.order_id, "chargeId": charge_id }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                logger.info("Payment captured", &json!({ "orderId": data.order_id, "chargeId": charge_id }));
                Ok(json!({ "chargeId": charge_id, "status": "captured" }))
            }
        })
        .description("Process a payment from the queue"),
    );

    // ---
    // Enqueue work - FIFO queue (ordered processing)
    // FIFO queues guarantee messages are processed in the order they arrive.
    // Configure fifo: true in iii-config.yaml queue_configs.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("emails::enqueue", move |data: EnqueueEmailInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "emails::send".into(),
                        payload: json!({
                            "to": data.to,
                            "subject": data.subject,
                            "body": data.body,
                            "template": data.template,
                        }),
                        action: Some(TriggerAction::Enqueue { queue: "email".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Email enqueued (FIFO)", &json!({
                    "to": data.to,
                    "messageReceiptId": result["messageReceiptId"],
                }));

                Ok(json!({ "status": "queued", "messageReceiptId": result["messageReceiptId"] }))
            }
        })
        .description("Enqueue an email for FIFO delivery"),
    );

    // ---
    // Process email - FIFO handler preserves send order
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("emails::send", move |data: SendEmailInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                logger.info("Sending email", &json!({ "to": data.to, "subject": data.subject }));

                let message_id = format!("msg-{}", chrono::Utc::now().timestamp_millis());

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "email-log",
                        "key": message_id,
                        "value": {
                            "messageId": message_id,
                            "to": data.to,
                            "subject": data.subject,
                            "status": "sent",
                            "sent_at": chrono::Utc::now().to_rfc3339(),
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                logger.info("Email sent", &json!({ "messageId": message_id, "to": data.to }));
                Ok(json!({ "messageId": message_id, "status": "sent" }))
            }
        })
        .description("Send an email from the FIFO queue"),
    );

    // ---
    // Receipt capture - checking enqueue acknowledgement
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::place", move |data: PlaceOrderInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                let payment_receipt = iii
                    .trigger(TriggerRequest {
                        function_id: "payments::process".into(),
                        payload: json!({
                            "orderId": data.order_id,
                            "amount": data.total,
                            "currency": "usd",
                            "method": data.method,
                        }),
                        action: Some(TriggerAction::Enqueue { queue: "payment".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let email_receipt = iii
                    .trigger(TriggerRequest {
                        function_id: "emails::send".into(),
                        payload: json!({
                            "to": data.email,
                            "subject": "Order confirmed",
                            "body": format!("Order {}", data.order_id),
                        }),
                        action: Some(TriggerAction::Enqueue { queue: "email".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                logger.info("Order placed", &json!({
                    "orderId": data.order_id,
                    "paymentReceipt": payment_receipt["messageReceiptId"],
                    "emailReceipt": email_receipt["messageReceiptId"],
                }));

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "orders",
                        "key": data.order_id,
                        "value": {
                            "orderId": data.order_id,
                            "status": "pending",
                            "paymentReceiptId": payment_receipt["messageReceiptId"],
                            "emailReceiptId": email_receipt["messageReceiptId"],
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({
                    "orderId": data.order_id,
                    "paymentReceiptId": payment_receipt["messageReceiptId"],
                    "emailReceiptId": email_receipt["messageReceiptId"],
                }))
            }
        })
        .description("Place an order with queued payment and email"),
    );

    // ---
    // HTTP trigger to accept orders
    // ---
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/orders").method(HttpMethod::Post))
            .for_function("orders::place"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
