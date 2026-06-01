// Regression test for iii-hq/iii#1618 review (ytallo's gherkin §"One bad
// span does not poison the rest of the forwarded batch").
//
// The earlier `otlp_span_to_proto` was infallible — it called
// `hex_decode_or_empty` on malformed `trace_id` / `span_id` strings and
// emitted spans with empty-byte IDs on the wire. Most collectors reject
// any batch containing such spans wholesale, so a single bad SDK emitter
// (instrumentation bug, truncated context propagation, custom client)
// could blackhole otherwise-valid telemetry from the same payload.
//
// The contract pinned here: `otlp_span_to_proto` returns `None` for
// spans whose `trace_id` or `span_id` fails to hex-decode OR decodes to
// a non-spec byte length (16 bytes for trace_id, 8 for span_id);
// `otlp_scope_spans_to_proto` `filter_map`s them out. Valid spans in
// the same batch reach the collector.
//
// Owns its own process so the global `SDK_SPAN_FORWARDER` `OnceLock`
// is fresh.

mod common;

use std::time::Duration;

use common::forwarder_mock::spawn_body_capturing_collector;

// Three-span payload covering all three OTLP ID-shape failure modes:
// (1) non-hex characters — `hex::decode` returns Err.
// (2) hex-decodable but wrong length — surfaced by adversarial review
//     DISS-001 as a gap in the original filter. `traceId: "01"`
//     decodes successfully to `[1]` (1 byte instead of the spec's 16);
//     without the length check it would land on the wire and the
//     collector would reject the batch. Locks the spec-length contract.
// (3) valid 32-hex-char trace_id + 16-hex-char span_id. Survives.
//
// Both bad spans share the same scope as the valid one so we exercise
// `otlp_scope_spans_to_proto`'s `filter_map` on a heterogeneous list.
const MIXED_BATCH_PAYLOAD: &str = r#"{
    "resourceSpans": [{
        "resource": {
            "attributes": [{"key": "service.name", "value": {"stringValue": "mixed-batch-svc"}}]
        },
        "scopeSpans": [{
            "scope": {"name": "iii-1618-partial-batch-regression"},
            "spans": [
                {
                    "traceId": "not-hex",
                    "spanId": "also-not-hex",
                    "name": "bad-non-hex",
                    "startTimeUnixNano": "1700000000000000000",
                    "endTimeUnixNano": "1700000000001000000"
                },
                {
                    "traceId": "01",
                    "spanId": "01",
                    "name": "bad-wrong-length",
                    "startTimeUnixNano": "1700000000000500000",
                    "endTimeUnixNano": "1700000000001500000"
                },
                {
                    "traceId": "0af7651916cd43dd8448eb211c80319c",
                    "spanId": "b7ad6b7169203331",
                    "name": "valid-span",
                    "startTimeUnixNano": "1700000000002000000",
                    "endTimeUnixNano": "1700000000003000000"
                }
            ]
        }]
    }]
}"#;

#[tokio::test]
async fn forwarder_drops_malformed_spans_and_relays_the_rest_of_the_batch() {
    let (endpoint, received) = spawn_body_capturing_collector().await;

    iii::workers::observability::otel::init_sdk_span_forwarder(&endpoint);

    iii::workers::observability::otel::ingest_otlp_json(MIXED_BATCH_PAYLOAD)
        .await
        .expect("ingest_otlp_json should accept a well-formed (mixed-validity) payload");

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

    // Walk every span the collector actually saw. The bad-span MUST NOT
    // be present (no empty-byte IDs on the wire), and the valid-span
    // MUST be — exactly once, with its original hex-decoded IDs intact.
    let all_spans: Vec<_> = req
        .resource_spans
        .iter()
        .flat_map(|rs| rs.scope_spans.iter())
        .flat_map(|ss| ss.spans.iter())
        .collect();

    // OTLP spec lengths: trace_id MUST be 16 bytes, span_id MUST be 8.
    // No span on the wire may violate these — empty IDs (bad hex case)
    // OR wrong-length IDs (good hex but bad length, e.g. "01" → [1]).
    for span in &all_spans {
        assert_eq!(
            span.trace_id.len(),
            16,
            "Forwarder emitted span with non-spec trace_id length {} (expected 16): name = {:?}",
            span.trace_id.len(),
            span.name
        );
        assert_eq!(
            span.span_id.len(),
            8,
            "Forwarder emitted span with non-spec span_id length {} (expected 8): name = {:?}",
            span.span_id.len(),
            span.name
        );
    }

    assert_eq!(
        all_spans.len(),
        1,
        "Expected exactly one span on the wire (the valid one). Got {}: names = {:?}",
        all_spans.len(),
        all_spans.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    let valid = all_spans[0];
    assert_eq!(
        valid.name, "valid-span",
        "The surviving span should be the valid one, not the malformed one"
    );
    assert_eq!(
        valid.trace_id,
        hex::decode("0af7651916cd43dd8448eb211c80319c").unwrap(),
        "Valid span's trace_id was rewritten by the forwarder"
    );
    assert_eq!(
        valid.span_id,
        hex::decode("b7ad6b7169203331").unwrap(),
        "Valid span's span_id was rewritten by the forwarder"
    );
}
