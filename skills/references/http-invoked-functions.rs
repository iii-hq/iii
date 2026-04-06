/// Pattern: HTTP-Invoked Functions
/// Comparable to: AWS Lambda URL invocations, Cloudflare Workers, webhook proxies
///
/// Registers external HTTP endpoints as iii functions so the engine
/// calls them when triggered - no client-side HTTP code needed.
/// Combines with cron, state, and queue triggers for reactive integrations.
///
/// Prerequisites:
///   - HttpFunctionsModule enabled in iii engine config
///   - Env vars: SLACK_WEBHOOK_TOKEN, STRIPE_API_KEY, ORDER_WEBHOOK_SECRET

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, RegisterFunctionMessage,
    TriggerRequest, TriggerAction, HttpInvocationConfig, HttpAuthConfig,
    builtin_triggers::*, IIITrigger, Logger,
    protocol::HttpMethod as ProtoHttpMethod,
};
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

use serde;
use schemars;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ProcessOrderInput {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: f64,
    #[serde(rename = "paymentToken")]
    payment_token: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct EnqueueChargeInput {
    amount: f64,
    #[serde(rename = "paymentToken")]
    payment_token: String,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // Data-driven registration for immutable legacy endpoints
    // ---
    let legacy_base_url = std::env::var("LEGACY_API_URL")
        .unwrap_or("https://legacy.internal.example.com".into());

    let legacy_endpoints = vec![
        ("/webhook", "legacy::webhook"),
        ("/orders", "legacy::orders"),
    ];

    for (path, id) in legacy_endpoints {
        let mut msg = RegisterFunctionMessage::with_id(id.into())
            .with_description(format!("Proxy legacy endpoint {path}"));

        iii.register_function_with(
            msg,
            HttpInvocationConfig {
                url: format!("{legacy_base_url}{path}"),
                method: ProtoHttpMethod::Post,
                timeout_ms: Some(8000),
                headers: HashMap::new(),
                auth: None,
            },
        );
    }

    // ---
    // HTTP-invoked function: Slack webhook (bearer auth)
    // ---
    iii.register_function_with(
        RegisterFunctionMessage::with_id("integrations::slack-notify".into())
            .with_description("POST notification to Slack webhook".into()),
        HttpInvocationConfig {
            url: "https://hooks.slack.example.com/services/incoming".into(),
            method: ProtoHttpMethod::Post,
            timeout_ms: Some(5000),
            headers: {
                let mut h = HashMap::new();
                h.insert("Content-Type".into(), "application/json".into());
                h
            },
            auth: Some(HttpAuthConfig::Bearer {
                token_key: "SLACK_WEBHOOK_TOKEN".into(),
            }),
        },
    );

    // ---
    // HTTP-invoked function: Stripe charges (api_key auth)
    // ---
    iii.register_function_with(
        RegisterFunctionMessage::with_id("integrations::stripe-charge".into())
            .with_description("Create a charge via Stripe API".into()),
        HttpInvocationConfig {
            url: "https://api.stripe.example.com/v1/charges".into(),
            method: ProtoHttpMethod::Post,
            timeout_ms: Some(10000),
            headers: {
                let mut h = HashMap::new();
                h.insert("Content-Type".into(), "application/x-www-form-urlencoded".into());
                h
            },
            auth: Some(HttpAuthConfig::ApiKey {
                header: "Authorization".into(),
                value_key: "STRIPE_API_KEY".into(),
            }),
        },
    );

    // ---
    // HTTP-invoked function: Analytics endpoint (no auth)
    // ---
    iii.register_function_with(
        RegisterFunctionMessage::with_id("integrations::analytics-track".into())
            .with_description("POST event to analytics service".into()),
        HttpInvocationConfig {
            url: "https://analytics.internal.example.com/events".into(),
            method: ProtoHttpMethod::Post,
            timeout_ms: Some(3000),
            headers: HashMap::new(),
            auth: None,
        },
    );

    // ---
    // HTTP-invoked function: Order status webhook (hmac auth)
    // ---
    iii.register_function_with(
        RegisterFunctionMessage::with_id("integrations::order-webhook".into())
            .with_description("POST order status change to fulfillment partner".into()),
        HttpInvocationConfig {
            url: "https://fulfillment.partner.example.com/webhooks/orders".into(),
            method: ProtoHttpMethod::Post,
            timeout_ms: Some(5000),
            headers: HashMap::new(),
            auth: Some(HttpAuthConfig::Hmac {
                secret_key: "ORDER_WEBHOOK_SECRET".into(),
            }),
        },
    );

    // ---
    // Handler-based function that triggers HTTP-invoked functions
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::process", move |data: ProcessOrderInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "orders",
                        "key": data.order_id,
                        "value": {
                            "orderId": data.order_id,
                            "amount": data.amount,
                            "status": "processing",
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                let charge_result = iii
                    .trigger(TriggerRequest {
                        function_id: "integrations::stripe-charge".into(),
                        payload: json!({
                            "amount": data.amount,
                            "currency": "usd",
                            "source": data.payment_token,
                        }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let charge_id = charge_result["id"].as_str().unwrap_or("unknown");
                logger.info("Payment charged", &json!({ "orderId": data.order_id, "chargeId": charge_id }));

                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "orders",
                        "key": data.order_id,
                        "value": {
                            "orderId": data.order_id,
                            "amount": data.amount,
                            "status": "charged",
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                iii.trigger(TriggerRequest {
                    function_id: "integrations::slack-notify".into(),
                    payload: json!({ "text": format!("Order {} charged ${}", data.order_id, data.amount) }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                iii.trigger(TriggerRequest {
                    function_id: "integrations::analytics-track".into(),
                    payload: json!({
                        "event": "order.charged",
                        "properties": { "orderId": data.order_id, "amount": data.amount },
                    }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                Ok(json!({
                    "orderId": data.order_id,
                    "chargeId": charge_id,
                    "status": "charged",
                }))
            }
        })
        .description("Process an order with payment and notifications"),
    );

    // ---
    // Trigger: state change -> notify fulfillment partner via HTTP-invoked function
    // ---
    iii.register_trigger(
        IIITrigger::State(StateTriggerConfig::new().scope("orders").key("status"))
            .for_function("integrations::order-webhook"),
    )
    .expect("failed");

    // ---
    // Trigger: scheduled analytics ping every hour
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("integrations::hourly-heartbeat", move |_: serde_json::Value| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let worker_count = iii
                    .trigger(TriggerRequest {
                        function_id: "engine::workers::list".into(),
                        payload: json!({}),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let count = worker_count.as_array().map(|a| a.len()).unwrap_or(0);

                iii.trigger(TriggerRequest {
                    function_id: "integrations::analytics-track".into(),
                    payload: json!({
                        "event": "system.heartbeat",
                        "properties": {
                            "workers": count,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                logger.info("Hourly heartbeat sent", &json!({}));
                Ok(json!(null))
            }
        })
        .description("Send hourly analytics heartbeat"),
    );

    iii.register_trigger(
        IIITrigger::Cron(CronTriggerConfig::new("0 0 * * * * *"))
            .for_function("integrations::hourly-heartbeat"),
    )
    .expect("failed");

    // ---
    // Trigger: enqueue Stripe charges for reliable delivery with retries
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::enqueue-charge", move |data: EnqueueChargeInput| {
            let iii = iii_clone.clone();
            async move {
                let result = iii
                    .trigger(TriggerRequest {
                        function_id: "integrations::stripe-charge".into(),
                        payload: json!({
                            "amount": data.amount,
                            "currency": "usd",
                            "source": data.payment_token,
                        }),
                        action: Some(TriggerAction::Enqueue { queue: "payments".into() }),
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                Ok(json!({ "messageReceiptId": result["messageReceiptId"] }))
            }
        })
        .description("Enqueue a Stripe charge for reliable processing"),
    );

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
