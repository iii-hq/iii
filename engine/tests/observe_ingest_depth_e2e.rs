//! Telemetry a worker sends while it runs an invocation in a chain of
//! observers carries that depth (the deepest it is running): what
//! `observability::on-log` / `on-span` events take for their loop guard.

mod observe_support;

use iii::observer::ObserversConfig;
use iii::workers::observability::otel;
use observe_support::{Client, connect, registered, spawn_engine};
use serde_json::{Value, json};
use uuid::Uuid;

fn logs_frame(body: &str) -> Vec<u8> {
    let otlp = json!({
        "resourceLogs": [{
            "resource": { "attributes": [{
                "key": "service.name", "value": { "stringValue": "deep" }
            }] },
            "scopeLogs": [{ "logRecords": [{
                "timeUnixNano": "1704067200000000000",
                "observedTimeUnixNano": "1704067200000000000",
                "severityNumber": 9,
                "severityText": "INFO",
                "body": { "stringValue": body }
            }] }]
        }]
    });
    [b"LOGS".as_slice(), otlp.to_string().as_bytes()].concat()
}

fn spans_frame(trace_id: &str) -> Vec<u8> {
    let otlp = json!({
        "resourceSpans": [{
            "resource": { "attributes": [{
                "key": "service.name", "value": { "stringValue": "deep" }
            }] },
            "scopeSpans": [{ "spans": [{
                "traceId": trace_id,
                "spanId": "1234567890abcdef",
                "name": "execute deep::fn",
                "kind": 1,
                "startTimeUnixNano": "1704067200000000000",
                "endTimeUnixNano": "1704067200100000000",
                "status": { "code": 1 }
            }] }]
        }]
    });
    [b"OTLP".as_slice(), otlp.to_string().as_bytes()].concat()
}

fn log_depth(body: &str) -> Option<u32> {
    otel::get_log_storage()?
        .get_logs()
        .into_iter()
        .find(|log| log.body == body)
        .map(|log| log.observe_depth)
}

/// A call on the same connection: answered after every frame sent before.
async fn barrier(client: &mut Client) {
    let call = client
        .call("engine::observers::get", json!({ "id": "barrier" }))
        .await;
    client.result(call).await;
}

#[tokio::test]
async fn telemetry_from_a_worker_carries_the_depth_it_runs_at() {
    otel::init_log_storage(None);
    let _ = otel::InMemorySpanExporter::new(10_000, "test".to_string());
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut deep = connect(&t, "deep", true).await;
    deep.register_function("deep::fn").await;
    registered(&t.engine, "default", "deep::fn").await;
    let mut caller = connect(&t, "caller", true).await;
    let marker = Uuid::new_v4().simple().to_string();

    // Running an invocation at depth 2.
    let call = caller
        .call_with("deep::fn", json!({}), json!({ "observe_depth": 2 }))
        .await;
    let invoke = deep.invoked("deep::fn").await;
    assert_eq!(invoke["observe_depth"], 2);
    deep.send_binary(logs_frame(&format!("deep {marker}")))
        .await;
    deep.send_binary(spans_frame(&marker)).await;
    deep.reply(&invoke, Value::Null).await;
    // Its frames before the reply were ingested before the reply was handled.
    caller.result(call).await;
    assert_eq!(log_depth(&format!("deep {marker}")), Some(2));
    let spans = otel::get_span_storage()
        .expect("span storage")
        .get_spans_by_trace_id(&marker);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].observe_depth, 2);

    // Running nothing.
    deep.send_binary(logs_frame(&format!("idle {marker}")))
        .await;
    barrier(&mut deep).await;
    assert_eq!(log_depth(&format!("idle {marker}")), Some(0));
}
