/// Pattern: Observability
/// Comparable to: Datadog, Grafana, Honeycomb, OpenTelemetry SDK
///
/// iii has built-in OpenTelemetry support for traces, metrics, and logs.
/// This file shows how to configure the telemetry pipeline, create custom
/// spans and metrics, propagate trace context across function calls, listen
/// for log events, and cleanly shut down the exporter.
///
/// Requires the `otel` feature: iii-sdk = { version = "...", features = ["otel"] }

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

#[cfg(feature = "otel")]
use iii_sdk::{
    with_span, get_tracer, get_meter, shutdown_otel, init_otel,
    current_trace_id, inject_traceparent, inject_baggage,
    OtelConfig, SpanKind,
};

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct OrderInput {
    order_id: String,
    items: Option<Vec<OrderItem>>,
    region: Option<String>,
    user_id: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
struct OrderItem {
    price: f64,
    qty: i64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ExternalCallInput {
    user_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LogDemoInput {
    id: String,
    query: Option<String>,
    status: Option<String>,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());

    // ---
    // 1. SDK initialization with OpenTelemetry config
    // ---
    let iii = register_worker(
        &url,
        InitOptions {
            #[cfg(feature = "otel")]
            otel: Some(OtelConfig {
                enabled: Some(true),
                service_name: Some("my-service".into()),
                service_version: Some("1.2.0".into()),
                metrics_enabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    // ---
    // 2. Custom spans - wrap an operation in a named span for tracing
    // with_span(name, traceparent, kind, callback) creates a child span under
    // the current trace context. The span is automatically closed when the
    // callback completes or throws.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("orders::process", move |data: OrderInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let items = data.items.unwrap_or_default();

                #[cfg(feature = "otel")]
                let validation = with_span("validate-order", None, Some(SpanKind::Internal), || async {
                    logger.info("Validating order inside span", &json!({ "orderId": data.order_id }));

                    if items.is_empty() {
                        return Err("Empty cart".into());
                    }

                    Ok(json!({ "valid": true, "itemCount": items.len() }))
                })
                .await
                .map_err(|e| e.to_string())?;

                #[cfg(not(feature = "otel"))]
                let validation = {
                    if items.is_empty() {
                        return Err("Empty cart".into());
                    }
                    json!({ "valid": true, "itemCount": items.len() })
                };

                let total: f64 = items.iter().map(|i| i.price * i.qty as f64).sum();

                #[cfg(feature = "otel")]
                with_span("persist-order", None, None, || {
                    let iii = iii.clone();
                    async move {
                        iii.trigger(TriggerRequest {
                            function_id: "state::set".into(),
                            payload: json!({
                                "scope": "orders",
                                "key": data.order_id,
                                "value": { "_key": data.order_id, "total": total, "status": "confirmed" },
                            }),
                            action: None,
                            timeout_ms: None,
                        })
                        .await
                        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;
                        Ok(())
                    }
                })
                .await
                .map_err(|e| e.to_string())?;

                #[cfg(not(feature = "otel"))]
                iii.trigger(TriggerRequest {
                    function_id: "state::set".into(),
                    payload: json!({
                        "scope": "orders",
                        "key": data.order_id,
                        "value": { "_key": data.order_id, "total": total, "status": "confirmed" },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({
                    "order_id": data.order_id,
                    "total": total,
                    "validated": validation["valid"],
                }))
            }
        })
        .description("Process an order with tracing spans"),
    );

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/orders/process").method(HttpMethod::Post))
            .for_function("orders::process"),
    )
    .expect("failed");

    // ---
    // 3. Custom metrics - counters and histograms via get_meter()
    // ---
    #[cfg(feature = "otel")]
    let order_counter = {
        let meter = get_meter();
        meter
            .u64_counter("orders.processed")
            .with_description("Total number of orders processed")
            .build()
    };

    #[cfg(feature = "otel")]
    let latency_histogram = {
        let meter = get_meter();
        meter
            .f64_histogram("orders.latency_ms")
            .with_description("Order processing latency in milliseconds")
            .with_unit("ms")
            .build()
    };

    iii.register_function(
        RegisterFunction::new("orders::with-metrics", {
            #[cfg(feature = "otel")]
            let order_counter = order_counter.clone();
            #[cfg(feature = "otel")]
            let latency_histogram = latency_histogram.clone();
            move |data: OrderInput| -> Result<serde_json::Value, String> {
                let start = std::time::Instant::now();

                let result = json!({ "order_id": data.order_id, "status": "complete" });

                #[cfg(feature = "otel")]
                {
                    use opentelemetry::KeyValue;
                    let region = data.region.unwrap_or("us-east-1".into());
                    order_counter.add(1, &[
                        KeyValue::new("status", "success"),
                        KeyValue::new("region", region),
                    ]);
                    latency_histogram.record(start.elapsed().as_millis() as f64, &[
                        KeyValue::new("endpoint", "/orders"),
                    ]);
                }

                Ok(result)
            }
        })
        .description("Process order with custom metrics"),
    );

    // ---
    // 4. Trace context propagation
    // Access the current trace ID, inject traceparent headers for outbound HTTP
    // calls, and attach baggage for cross-service context.
    // ---
    iii.register_function(
        RegisterFunction::new("orders::call-external", move |data: ExternalCallInput| -> Result<serde_json::Value, String> {
            let logger = Logger::new();

            #[cfg(feature = "otel")]
            {
                let trace_id = current_trace_id();
                logger.info("Current trace", &json!({ "traceId": trace_id }));

                let mut headers = std::collections::HashMap::new();
                if let Some(tp) = inject_traceparent() {
                    headers.insert("traceparent".to_string(), tp);
                }
                if let Some(bg) = inject_baggage() {
                    headers.insert("baggage".to_string(), bg);
                }

                Ok(json!({ "traceId": trace_id, "propagated": true }))
            }

            #[cfg(not(feature = "otel"))]
            {
                logger.info("Trace propagation requires otel feature", &json!({}));
                Ok(json!({ "traceId": null, "propagated": false }))
            }
        })
        .description("Demonstrate trace context propagation"),
    );

    // ---
    // 5. Structured logging with trace correlation
    // Logger automatically attaches trace/span IDs when otel is enabled.
    // ---
    iii.register_function(
        RegisterFunction::new("debug::log-demo", |data: LogDemoInput| -> Result<serde_json::Value, String> {
            let logger = Logger::new();

            logger.info("Processing request", &json!({ "requestId": data.id }));
            logger.warn("Slow query detected", &json!({ "query": data.query, "duration_ms": 1200 }));
            logger.error("Unexpected state", &json!({ "expected": "active", "actual": data.status }));

            Ok(json!({ "logged": true }))
        })
        .description("Demonstrate structured logging"),
    );

    // ---
    // 6. Clean shutdown - flush pending spans and metrics on process exit
    // ---
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();

        #[cfg(feature = "otel")]
        shutdown_otel().await;
    });
    iii.shutdown();
}
