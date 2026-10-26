// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Real ingest → hot cache → SQLite coverage. Separate process because the
//! tracer provider and dynamic sampler are process-global OnceLocks.
use iii::workers::observability::{
    config::{
        ObservabilityWorkerConfig, OtelExporterType, RateLimitConfig, SamplingConfig, SamplingRule,
        TraceStorageConfig,
    },
    otel::{self, trace_storage_test_support as archive},
};
use opentelemetry::trace::{
    Span as _, SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState,
    Tracer as _, TracerProvider as _,
};
use serde_json::json;

fn worker_request(spans: serde_json::Value) -> String {
    json!({"resourceSpans":[{
        "resource":{"attributes":[{"key":"service.name","value":{"stringValue":"worker-fixture"}}]},
        "scopeSpans":[{"spans":spans}]
    }]})
    .to_string()
}

/// A worker span; `flags: Some(1)` is the W3C sampled flag SDKs export.
fn worker_span(
    trace: &str,
    span: &str,
    parent: Option<&str>,
    name: &str,
    end: &str,
    flags: Option<u32>,
) -> serde_json::Value {
    let mut value = json!({"traceId":trace,"spanId":span,"name":name,"kind":1,
        "startTimeUnixNano":"1700000000000000000","endTimeUnixNano":end});
    if let Some(parent) = parent {
        value["parentSpanId"] = json!(parent);
    }
    if let Some(flags) = flags {
        value["flags"] = json!(flags);
    }
    value
}

fn stored_names(trace: &str) -> Vec<String> {
    let mut names: Vec<String> = otel::get_span_storage()
        .unwrap()
        .get_spans()
        .iter()
        .filter(|span| span.trace_id == trace)
        .map(|span| {
            format!(
                "{}{}",
                span.name,
                if span.pending { "(pending)" } else { "" }
            )
        })
        .collect();
    names.sort();
    names
}

const END: &str = "1700000000200000000";
const CHILD_END: &str = "1700000000100000000";

#[tokio::test]
async fn sdk_and_engine_spans_obey_live_sampling_in_memory_and_archive() {
    let directory = tempfile::tempdir().unwrap();
    let config = ObservabilityWorkerConfig {
        enabled: Some(true),
        exporter: Some(OtelExporterType::Memory),
        sampling_ratio: Some(0.0),
        live_spans: Some(true),
        memory_max_spans: Some(20_000),
        trace_storage: Some(TraceStorageConfig {
            directory: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        }),
        ..Default::default()
    };
    otel::set_otel_config(config.clone());
    let _layer = otel::init_otel::<tracing_subscriber::Registry>(&otel::OtelConfig::default());
    let hot = otel::get_span_storage().unwrap();
    let tracer = opentelemetry::global::tracer_provider().tracer("sampling-regression");

    for (ratio, expected) in [(0.0, 0), (0.1, 100), (1.0, 1000)] {
        otel::update_otel_config(ObservabilityWorkerConfig {
            sampling_ratio: Some(ratio),
            ..config.clone()
        });
        otel::refresh_sampler();
        hot.clear();
        otel::get_trace_disk_storage().unwrap().clear().unwrap();

        // Spread the lower 63 trace-id bits evenly: exactly 10% survive
        // TraceIdRatioBased(0.1), rather than a statistical assertion.
        let spans: Vec<_> = (0..1000_u128)
            .map(|i| {
                let id = ((2 * i + 1) * (1_u128 << 64)) / 2000;
                json!({
                    "traceId": format!("{id:032x}"), "spanId": "0000000000000001",
                    "name": "sdk-load", "kind": 1, "flags": 1,
                    "startTimeUnixNano": "1700000000000000000", "endTimeUnixNano": "0",
                    "attributes": [{"key":"iii.payload.json", "value":{"stringValue":"x".repeat(1024)}}]
                })
            })
            .collect();
        let mut request = json!({"resourceSpans":[{
            "resource":{"attributes":[{"key":"service.name","value":{"stringValue":"sdk-fixture"}}]},
            "scopeSpans":[{"spans":spans}]
        }]});
        otel::ingest_otlp_json(&request.to_string()).await.unwrap();
        assert_eq!(hot.len(), expected, "SDK start snapshots at ratio {ratio}");
        archive::flush().unwrap();
        assert!(
            archive::read_spans().unwrap().is_empty(),
            "pending is memory-only"
        );
        for span in request["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array_mut()
            .unwrap()
        {
            span["endTimeUnixNano"] = json!("1700000000100000000");
        }
        otel::ingest_otlp_json(&request.to_string()).await.unwrap();
        assert_eq!(hot.len(), expected, "SDK finals at ratio {ratio}");
        assert!(hot.get_spans().iter().all(|span| !span.pending));
        archive::flush().unwrap();
        assert_eq!(
            archive::read_spans().unwrap().len(),
            expected,
            "SQLite at ratio {ratio}"
        );

        // Engine spans use the provider's same sampler, not the ingest path.
        let mut span = tracer.start("engine-local");
        span.end();
        archive::flush().unwrap();
        let engine_count = hot
            .get_spans()
            .iter()
            .filter(|s| s.name == "engine-local")
            .count();
        if ratio == 0.0 || ratio == 1.0 {
            assert_eq!(engine_count, usize::from(ratio == 1.0));
        }
        eprintln!(
            "SAMPLING_EVIDENCE ratio={ratio} sdk_sent=1000 sdk_stored={expected} engine_stored={engine_count}"
        );
    }
    // A final dropped after a runtime sampler swap must remove its live
    // snapshot rather than leaving a permanently pending span.
    hot.clear();
    let mut request = json!({"resourceSpans":[{"scopeSpans":[{"spans":[{
        "traceId":"abcdef1234567890abcdef1234567890", "spanId":"1234567890abcdef",
        "name":"swapped", "startTimeUnixNano":"1", "endTimeUnixNano":"0"
    }]}]}]});
    otel::ingest_otlp_json(&request.to_string()).await.unwrap();
    assert_eq!(hot.len(), 1);
    otel::update_otel_config(ObservabilityWorkerConfig {
        sampling_ratio: Some(0.0),
        ..config.clone()
    });
    otel::refresh_sampler();
    request["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["endTimeUnixNano"] = json!("2");
    otel::ingest_otlp_json(&request.to_string()).await.unwrap();
    assert!(hot.is_empty(), "dropped final removes its pending snapshot");

    // Advanced sampling decides once per trace, where the trace is rooted in
    // the worker: a local root gets the configured policy (rules, one
    // rate-limit token, default) whatever its W3C sampled bit, a real remote
    // parent (OTLP flags 0x300 plus a parent id) carries the caller's
    // decision, and every other span follows its trace.
    let apply = |sampling: SamplingConfig| {
        otel::update_otel_config(ObservabilityWorkerConfig {
            sampling: Some(sampling),
            ..config.clone()
        });
        otel::refresh_sampler();
    };
    let rate_limit_1 = || SamplingConfig {
        default: Some(1.0),
        rules: vec![],
        parent_based: None,
        rate_limit: Some(RateLimitConfig {
            max_traces_per_second: 1,
        }),
    };
    let root_rule = |default: f64| SamplingConfig {
        default: Some(default),
        rules: vec![SamplingRule {
            operation: Some("root-op".into()),
            service: None,
            rate: 1.0,
        }],
        parent_based: None,
        rate_limit: None,
    };
    let parent_based_0 = || SamplingConfig {
        default: Some(0.0),
        rules: vec![],
        parent_based: Some(true),
        rate_limit: None,
    };

    // A: one token for the trace, children follow the root (Node flags).
    apply(rate_limit_1());
    let a = "0af7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(
            a,
            "00000000000000a2",
            Some("00000000000000a1"),
            "child-op",
            CHILD_END,
            Some(0x101)
        ),
        worker_span(
            a,
            "00000000000000a3",
            Some("00000000000000a1"),
            "child-op",
            CHILD_END,
            Some(0x101)
        ),
        worker_span(a, "00000000000000a1", None, "root-op", END, Some(0x101)),
    ])))
    .await
    .unwrap();
    assert_eq!(
        stored_names(a),
        ["child-op", "child-op", "root-op"],
        "rate limit A"
    );

    // A2: after a refill the start snapshot takes the token; its final
    // follows and spends nothing.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let b = "1bf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        b,
        "00000000000000b1",
        None,
        "root-op",
        "0",
        Some(0x101)
    )])))
    .await
    .unwrap();
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        b,
        "00000000000000b1",
        None,
        "root-op",
        END,
        Some(0x101)
    )])))
    .await
    .unwrap();
    assert_eq!(stored_names(b), ["root-op"], "snapshot then final A2");

    // B2: 50 Node-flagged local roots get exactly the policy unflagged roots
    // get (rate limit 1/s, rules with default 0, parent_based with default 0).
    let count_roots = |prefix: &str| {
        otel::get_span_storage()
            .unwrap()
            .get_spans()
            .iter()
            .filter(|span| span.trace_id.starts_with(prefix))
            .count()
    };
    let mut policy_counts = Vec::new();
    for (policy, sampling, expected) in [
        ("rate_limit", rate_limit_1(), 1),
        ("rules", root_rule(0.0), 0),
        ("parent_based", parent_based_0(), 0),
    ] {
        for (tag, flags) in [("0101", Some(0x101)), ("0000", None)] {
            apply(sampling.clone());
            let prefix = format!("{tag}{:02x}", policy.len());
            let spans: Vec<_> = (0..50_u32)
                .map(|index| {
                    worker_span(
                        &format!("{prefix}{index:026x}"),
                        "00000000000000a1",
                        None,
                        "other-op",
                        END,
                        flags,
                    )
                })
                .collect();
            otel::ingest_otlp_json(&worker_request(json!(spans)))
                .await
                .unwrap();
            let kept = count_roots(&prefix);
            policy_counts.push(format!("{policy}/{tag}={kept}"));
            assert_eq!(kept, expected, "{policy} with flags {flags:?}");
        }
    }

    // Unflagged: the first trace takes the refilled token for all its spans,
    // the next one within the same second is dropped whole.
    apply(rate_limit_1());
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let u1 = "4af7651916cd43dd8448eb211c80319c";
    let u2 = "5bf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(u1, "00000000000000e1", None, "root-op", "0", None),
        worker_span(u2, "00000000000000f1", None, "root-op", "0", None),
    ])))
    .await
    .unwrap();
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(
            u1,
            "00000000000000e2",
            Some("00000000000000e1"),
            "child-op",
            CHILD_END,
            None
        ),
        worker_span(u1, "00000000000000e1", None, "root-op", END, None),
        worker_span(
            u2,
            "00000000000000f2",
            Some("00000000000000f1"),
            "child-op",
            CHILD_END,
            None
        ),
        worker_span(u2, "00000000000000f1", None, "root-op", END, None),
    ])))
    .await
    .unwrap();
    assert_eq!(
        stored_names(u1),
        ["child-op", "root-op"],
        "one token per trace"
    );
    assert!(
        stored_names(u2).is_empty(),
        "a rate-limited trace leaves no fragment"
    );

    // N8: a kept trace forgotten by the bounded verdict cache (17,000 newer
    // traces) is still recognised from the hot cache, so its late child is
    // not re-decided (and dropped) by the rate limit.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let r = "6cf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        r,
        "00000000000000d1",
        None,
        "root-op",
        END,
        None
    )])))
    .await
    .unwrap();
    // N12: likewise a long root whose start snapshot (and a child) was
    // kept: its final is recognised by the live snapshot, not re-decided.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let long = "8df7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(long, "00000000000000c1", None, "root-op", "0", None),
        worker_span(
            long,
            "00000000000000c2",
            Some("00000000000000c1"),
            "child-op",
            CHILD_END,
            None
        ),
    ])))
    .await
    .unwrap();
    assert_eq!(stored_names(long), ["child-op", "root-op(pending)"]);
    for chunk in 0..17_u64 {
        let spans: Vec<_> = (0..1000_u64)
            .map(|index| {
                worker_span(
                    &format!("77{:030x}", chunk * 1000 + index),
                    "00000000000000e1",
                    None,
                    "root-op",
                    END,
                    None,
                )
            })
            .collect();
        otel::ingest_otlp_json(&worker_request(json!(spans)))
            .await
            .unwrap();
    }
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        r,
        "00000000000000d2",
        Some("00000000000000d1"),
        "child-op",
        CHILD_END,
        None
    )])))
    .await
    .unwrap();
    assert_eq!(
        stored_names(r),
        ["child-op", "root-op"],
        "cache pressure N8"
    );
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        long,
        "00000000000000c1",
        None,
        "root-op",
        END,
        None
    )])))
    .await
    .unwrap();
    assert_eq!(
        stored_names(long),
        ["child-op", "root-op"],
        "cache pressure N12: the root final keeps its snapshot's verdict"
    );

    // Rules: worker children follow the root like engine children do, also
    // when the child is exported before the root (same request or an earlier
    // one, N7).
    apply(root_rule(0.0));
    let c = "2cf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(
            c,
            "00000000000000c2",
            Some("00000000000000c1"),
            "child-op",
            CHILD_END,
            Some(0x101)
        ),
        worker_span(c, "00000000000000c1", None, "root-op", END, Some(0x101)),
    ])))
    .await
    .unwrap();
    let n7 = "8cf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        n7,
        "00000000000000c2",
        Some("00000000000000c1"),
        "child-op",
        CHILD_END,
        None
    )])))
    .await
    .unwrap();
    assert!(
        stored_names(n7).is_empty(),
        "the early child waits for its root"
    );
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        n7,
        "00000000000000c1",
        None,
        "root-op",
        END,
        None
    )])))
    .await
    .unwrap();
    let root = tracer.start("root-op");
    let engine_trace = root.span_context().trace_id().to_string();
    let cx = opentelemetry::Context::current_with_span(root);
    let mut child = tracer.start_with_context("child-op", &cx);
    child.end();
    cx.span().end();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(stored_names(c), ["child-op", "root-op"], "worker rules B");
    assert_eq!(
        stored_names(n7),
        ["child-op", "root-op"],
        "child before root N7"
    );
    assert_eq!(
        stored_names(&engine_trace),
        ["child-op", "root-op"],
        "engine rules B"
    );

    // An early child whose root never comes is released after the bounded
    // wait and decided by its own span (here: no rule matches, default 1).
    apply(root_rule(1.0));
    let orphan = "9cf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([worker_span(
        orphan,
        "00000000000000c2",
        Some("00000000000000c1"),
        "child-op",
        CHILD_END,
        None
    )])))
    .await
    .unwrap();
    assert!(stored_names(orphan).is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
    assert_eq!(
        stored_names(orphan),
        ["child-op"],
        "released after the wait"
    );

    // parent_based, default 0: a worker span under a real sampled remote
    // parent is kept with the engine span under that same parent; an
    // unsampled remote parent drops it even with default 1.
    apply(parent_based_0());
    let d = "3df7651916cd43dd8448eb211c80319c";
    let remote = SpanContext::new(
        TraceId::from_hex(d).unwrap(),
        SpanId::from_hex("00000000000000d0").unwrap(),
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    );
    let remote_cx = opentelemetry::Context::new().with_remote_span_context(remote);
    let mut engine_child = tracer.start_with_context("engine-child", &remote_cx);
    engine_child.end();
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(
            d,
            "00000000000000d2",
            Some("00000000000000d1"),
            "worker-child",
            CHILD_END,
            Some(0x101)
        ),
        worker_span(
            d,
            "00000000000000d1",
            Some("00000000000000d0"),
            "worker-root",
            END,
            Some(0x301)
        ),
    ])))
    .await
    .unwrap();
    apply(SamplingConfig {
        default: Some(1.0),
        ..parent_based_0()
    });
    let e = "aaf7651916cd43dd8448eb211c80319c";
    otel::ingest_otlp_json(&worker_request(json!([
        worker_span(
            e,
            "00000000000000d2",
            Some("00000000000000d1"),
            "worker-child",
            CHILD_END,
            Some(0x101)
        ),
        worker_span(
            e,
            "00000000000000d1",
            Some("00000000000000d0"),
            "worker-root",
            END,
            Some(0x300)
        ),
    ])))
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(
        stored_names(d),
        ["engine-child", "worker-child", "worker-root"],
        "parent-based C keeps the trace under a sampled remote parent"
    );
    assert!(stored_names(e).is_empty(), "unsampled remote parent drops");
    eprintln!(
        "ADVANCED_SAMPLING_EVIDENCE A={:?} A2={:?} policies={policy_counts:?} U1={:?} U2={:?} N8={:?} B_worker={:?} N7={:?} B_engine={:?} orphan={:?} C={:?} C_unsampled={:?}",
        stored_names(a),
        stored_names(b),
        stored_names(u1),
        stored_names(u2),
        stored_names(r),
        stored_names(c),
        stored_names(n7),
        stored_names(&engine_trace),
        stored_names(orphan),
        stored_names(d),
        stored_names(e)
    );
    archive::reset();
}
