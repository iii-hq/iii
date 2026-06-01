// Regression test for iii-hq/iii#1617 — OTLP span forwarding drops resource
// attributes (`service.name` shows up as `<nil>` in downstream collectors).
//
// Owns its own process via being an integration test file, so the
// process-global `SDK_SPAN_FORWARDER` `OnceLock` is fresh on every run.
// The mock collector lives in `tests/common/forwarder_mock.rs` and is
// shared across the three `otel_forwarder_*` regression tests so the
// readiness/spawn contract stays in lockstep.
//
// The assertion contract: the captured `ExportTraceServiceRequest`
// carries the full inbound `resource_spans[].resource` block — not just
// `service.name`, but every resource attribute we put on the wire — to
// lock in full-resource preservation rather than a single-field band-aid.

mod common;

use std::time::Duration;

use opentelemetry_proto::tonic::common::v1::{KeyValue, any_value};

use common::forwarder_mock::spawn_body_capturing_collector;

fn payload_with_resource(service_name: &str, namespace: &str, version: &str) -> String {
    // Minimal but real OTLP/JSON ExportTraceServiceRequest.
    // `resourceSpans[0].resource.attributes` is the structure under audit.
    // The attribute mix exercises three AnyValue discriminator branches —
    // stringValue (service.name/namespace/version), intValue
    // (replica.count), and boolValue (telemetry.enabled) — so the proto
    // conversion's non-string code paths are also pinned by the regression
    // contract. Spans deliberately omit `flags` so the test also asserts
    // the SAMPLED default that mirrors the in-memory storage path.
    format!(
        r#"{{
            "resourceSpans": [{{
                "resource": {{
                    "attributes": [
                        {{"key": "service.name", "value": {{"stringValue": "{service_name}"}}}},
                        {{"key": "service.namespace", "value": {{"stringValue": "{namespace}"}}}},
                        {{"key": "service.version", "value": {{"stringValue": "{version}"}}}},
                        {{"key": "replica.count", "value": {{"intValue": "7"}}}},
                        {{"key": "telemetry.enabled", "value": {{"boolValue": true}}}}
                    ]
                }},
                "scopeSpans": [{{
                    "scope": {{"name": "iii-1617-regression"}},
                    "spans": [{{
                        "traceId": "00112233445566778899aabbccddeeff",
                        "spanId": "0011223344556677",
                        "name": "forwarder.preserves.resource",
                        "kind": 1,
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano": "1700000000001000000"
                    }}]
                }}]
            }}]
        }}"#
    )
}

fn find_string_attr<'a>(attrs: &'a [KeyValue], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .and_then(|v| v.value.as_ref())
        .and_then(|v| match v {
            any_value::Value::StringValue(s) => Some(s.as_str()),
            _ => None,
        })
}

fn find_int_attr(attrs: &[KeyValue], key: &str) -> Option<i64> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .and_then(|v| v.value.as_ref())
        .and_then(|v| match v {
            any_value::Value::IntValue(i) => Some(*i),
            _ => None,
        })
}

fn find_bool_attr(attrs: &[KeyValue], key: &str) -> Option<bool> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .and_then(|v| v.value.as_ref())
        .and_then(|v| match v {
            any_value::Value::BoolValue(b) => Some(*b),
            _ => None,
        })
}

#[tokio::test]
async fn forwarder_preserves_full_resource_attributes_through_to_collector() {
    let (endpoint, received) = spawn_body_capturing_collector().await;

    iii::workers::observability::otel::init_sdk_span_forwarder(&endpoint);

    let payload = payload_with_resource("my-service", "production", "1.4.2");
    iii::workers::observability::otel::ingest_otlp_json(&payload)
        .await
        .expect("ingest_otlp_json should accept a well-formed payload");

    // Wait briefly for the async export to complete. Pinning to a poll loop
    // rather than a fixed sleep so the test is fast in the happy case and
    // still bounded in the failure case.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !received.lock().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let captured = received.lock().await;
    assert_eq!(
        captured.len(),
        1,
        "Mock collector should have received exactly one ExportTraceServiceRequest"
    );

    let req = &captured[0];
    assert_eq!(
        req.resource_spans.len(),
        1,
        "Expected one ResourceSpans in the captured request"
    );

    let resource = req.resource_spans[0].resource.as_ref().expect(
        "resource_spans[0].resource MUST be present — the whole point of #1617 \
             is that the inbound resource block reaches the collector",
    );

    // The load-bearing assertions. Each one is a separate inbound resource
    // attribute; the test passes only if the forwarder relays the entire
    // resource block, not just `service.name` as a single-field workaround.
    assert_eq!(
        find_string_attr(&resource.attributes, "service.name"),
        Some("my-service"),
        "service.name was dropped on the forwarder hop (#1617 root symptom)"
    );
    assert_eq!(
        find_string_attr(&resource.attributes, "service.namespace"),
        Some("production"),
        "service.namespace was dropped on the forwarder hop — fix must preserve \
         every resource attribute, not just service.name"
    );
    assert_eq!(
        find_string_attr(&resource.attributes, "service.version"),
        Some("1.4.2"),
        "service.version was dropped on the forwarder hop — fix must preserve \
         every resource attribute, not just service.name"
    );

    // Non-string AnyValue branches. The proto-conversion helper probes
    // discriminator variants in spec order (`string → int → double → bool`
    // → composites); these assertions catch regressions in any branch
    // selection beyond the simple string case.
    assert_eq!(
        find_int_attr(&resource.attributes, "replica.count"),
        Some(7),
        "intValue resource attribute was lost or mistranslated by the forwarder"
    );
    assert_eq!(
        find_bool_attr(&resource.attributes, "telemetry.enabled"),
        Some(true),
        "boolValue resource attribute was lost or mistranslated by the forwarder"
    );

    // Pass-through fidelity. The forwarder must relay exactly what
    // arrived — no engine-injected hop-level attributes (e.g. a
    // `forwarder.id`), no span-name rewrites. A bug that ADDS attrs or
    // mutates the span would pass the preceding assertions but is
    // caught here. Five inbound attributes; expect five out.
    assert_eq!(
        resource.attributes.len(),
        5,
        "Forwarder injected or dropped resource attributes; expected exactly 5 (inbound), got {} (names: {:?})",
        resource.attributes.len(),
        resource
            .attributes
            .iter()
            .map(|kv| &kv.key)
            .collect::<Vec<_>>()
    );

    // Trace flags default. The inbound payload omits `flags`, and the
    // forwarder must default to SAMPLED (W3C trace-flags bit 0 = 1) to
    // match the in-memory storage path's behaviour. Defaulting to 0
    // (unsampled) would silently drop spans at downstream samplers /
    // collectors that respect the SAMPLED bit.
    let span = &req.resource_spans[0].scope_spans[0].spans[0];
    assert_eq!(
        span.flags & 0x01,
        0x01,
        "Forwarder must default missing trace flags to SAMPLED; got {:#x}",
        span.flags
    );
    assert_eq!(
        span.name, "forwarder.preserves.resource",
        "Forwarder rewrote span name; pass-through contract violated"
    );
}
