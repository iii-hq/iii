// Regression test for iii-hq/iii#1618 review (ytallo's gherkin §"SDK span
// forwarder honors OTEL_EXPORTER_OTLP_HEADERS").
//
// The pre-fix `opentelemetry_otlp::SpanExporter::builder().with_tonic()`
// parsed `OTEL_EXPORTER_OTLP_HEADERS` (and the signal-specific
// `OTEL_EXPORTER_OTLP_TRACES_HEADERS` override) automatically per the OTel
// SDK spec, injecting each `key=value` pair as a tonic `MetadataMap` entry
// on every export. The raw-`tonic::Channel` replacement in #1618 lost that
// behaviour silently — API-keyed collectors (Grafana Cloud, Honeycomb,
// SigNoz Cloud) would reject every forwarded span at auth with no signal
// surfaced engine-side. This test pins the env-var → metadata contract.
//
// Owns its own process so the env var and the process-global
// `SDK_SPAN_FORWARDER` `OnceLock` are both isolated from any other
// integration-test binary.

mod common;

use std::time::Duration;

use common::forwarder_mock::spawn_metadata_capturing_collector;

const MINIMAL_PAYLOAD: &str = r#"{
    "resourceSpans": [{
        "resource": {"attributes": [{"key": "service.name", "value": {"stringValue": "test-svc"}}]},
        "scopeSpans": [{
            "scope": {"name": "iii-1618-headers-regression"},
            "spans": [{
                "traceId": "00112233445566778899aabbccddeeff",
                "spanId": "0011223344556677",
                "name": "headers-test-span",
                "kind": 1,
                "startTimeUnixNano": "1700000000000000000",
                "endTimeUnixNano": "1700000000001000000"
            }]
        }]
    }]
}"#;

#[tokio::test]
async fn forwarder_injects_otel_exporter_otlp_headers_into_export_metadata() {
    // Order is load-bearing: the env var MUST be set BEFORE
    // init_sdk_span_forwarder is called, because the parsing happens at
    // forwarder-init time (matching the upstream `opentelemetry-otlp`
    // builder semantics). Setting it post-init would not retroactively
    // populate `SdkForwarderState.headers`.
    //
    // # Safety: this integration test binary is single-threaded with
    // respect to env-var mutation — only one test exists in the file,
    // and `SDK_SPAN_FORWARDER`'s `OnceLock` ensures we only init once.
    unsafe {
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_HEADERS",
            "authorization=Bearer-test-token-12345,x-tenant=acme",
        );
    }

    let (endpoint, received_metadata) = spawn_metadata_capturing_collector().await;

    iii::workers::observability::otel::init_sdk_span_forwarder(&endpoint);

    iii::workers::observability::otel::ingest_otlp_json(MINIMAL_PAYLOAD)
        .await
        .expect("ingest_otlp_json should accept a well-formed payload");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !received_metadata.lock().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let captured = received_metadata.lock().await;
    assert_eq!(
        captured.len(),
        1,
        "Mock collector should have received exactly one ExportTraceServiceRequest"
    );

    let metadata = &captured[0];

    // The authorization header from OTEL_EXPORTER_OTLP_HEADERS must
    // land on the wire as a tonic Ascii metadata entry. Without this
    // the upstream-spec env var is silently ignored and every API-keyed
    // collector rejects forwarded spans at auth.
    let auth = metadata
        .get("authorization")
        .expect("authorization metadata MUST be set from OTEL_EXPORTER_OTLP_HEADERS");
    assert_eq!(
        auth.to_str().unwrap(),
        "Bearer-test-token-12345",
        "authorization metadata value did not match OTEL_EXPORTER_OTLP_HEADERS"
    );

    // Second pair confirms the comma-split parser handles multiple
    // entries (single-pair could pass with a naive `=`-only split).
    let tenant = metadata
        .get("x-tenant")
        .expect("x-tenant metadata MUST be set; comma-split parser regression?");
    assert_eq!(
        tenant.to_str().unwrap(),
        "acme",
        "x-tenant metadata value did not match OTEL_EXPORTER_OTLP_HEADERS"
    );

    // Cleanup so a future test binary inheriting this process's env (if
    // any) doesn't leak the test credential. # Safety: same single-test
    // file rationale as the set_var above.
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
    }
}
