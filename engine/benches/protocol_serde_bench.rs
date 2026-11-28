mod common;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use iii::protocol::{ErrorBody, Message};
use serde_json::json;
use uuid::Uuid;

fn build_messages() -> Vec<(&'static str, Message)> {
    let invocation_id = Uuid::new_v4();

    vec![
        ("Ping", Message::Ping),
        ("Pong", Message::Pong),
        (
            "RegisterFunction",
            Message::RegisterFunction {
                id: "bench.echo".to_string(),
                description: Some("benchmark echo handler".to_string()),
                request_format: None,
                response_format: None,
                metadata: Some(json!({"version": "1.0"})),
                invocation: None,
                args_format: None,
                private: false,
                observable: true,
                observe: None,
            },
        ),
        (
            "InvokeFunction",
            Message::InvokeFunction {
                invocation_id: Some(invocation_id),
                function_id: "bench.echo".to_string(),
                data: common::benchmark_payload(),
                traceparent: Some(
                    "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
                ),
                baggage: Some("userId=alice".to_string()),
                action: None,
                metadata: None,
                namespace: None,
                caller: None,
                observe_depth: None,
                activation: None,
            },
        ),
        (
            "InvocationResult",
            Message::InvocationResult {
                invocation_id,
                function_id: "bench.echo".to_string(),
                result: Some(json!({"status_code": 200, "body": common::benchmark_payload()})),
                error: None,
                traceparent: Some(
                    "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
                ),
                baggage: None,
                matched: None,
                delivered: false,
            },
        ),
        (
            "InvocationResult_Error",
            Message::InvocationResult {
                invocation_id,
                function_id: "bench.echo".to_string(),
                result: None,
                error: Some(ErrorBody {
                    code: "timeout".to_string(),
                    message: "Function execution timed out after 30s".to_string(),
                    stacktrace: None,
                }),
                traceparent: None,
                baggage: None,
                matched: None,
                delivered: false,
            },
        ),
        (
            "WorkerRegistered",
            Message::WorkerRegistered {
                worker_id: Uuid::new_v4().to_string(),
                reattach_token: Some(Uuid::new_v4().to_string()),
                features: Vec::new(),
            },
        ),
    ]
}

fn protocol_serialize_benchmark(c: &mut Criterion) {
    let messages = build_messages();
    let mut group = c.benchmark_group("protocol_serialize");

    for (label, message) in &messages {
        let json = serde_json::to_string(message).expect("serialize");
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), message, |b, msg| {
            b.iter(|| serde_json::to_string(msg).expect("serialize"));
        });
    }

    group.finish();
}

fn protocol_deserialize_benchmark(c: &mut Criterion) {
    let messages = build_messages();
    let mut group = c.benchmark_group("protocol_deserialize");

    for (label, message) in &messages {
        let json = serde_json::to_string(message).expect("serialize");
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &json, |b, json| {
            b.iter(|| serde_json::from_str::<Message>(json).expect("deserialize"));
        });
    }

    group.finish();
}

fn protocol_roundtrip_benchmark(c: &mut Criterion) {
    let messages = build_messages();
    let mut group = c.benchmark_group("protocol_roundtrip");

    for (label, message) in &messages {
        let json = serde_json::to_string(message).expect("serialize");
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), message, |b, msg| {
            b.iter(|| {
                let json = serde_json::to_string(msg).expect("serialize");
                let _: Message = serde_json::from_str(&json).expect("deserialize");
            });
        });
    }

    group.finish();
}

fn protocol_frame_serialize_benchmark(c: &mut Criterion) {
    let mut messages = build_messages();
    for (label, size) in [("RawReport1kb", 1024), ("RawReport100kb", 100 * 1024)] {
        let payload = serde_json::to_string(&common::sized_payload(size)).expect("encode payload");
        let frame = format!(
            r#"{{"type":"invocationsobserved","items":[{{"function_id":"bench::echo","invocation_id":"i-1","matched":["o-1"],"status":"success","input":{payload},"output":null,"metadata":{{"n":9007199254740993}},"started_at_ms":1,"duration_ms":0.5}}]}}"#
        );
        messages.push((
            label,
            Message::from_frame(&frame).expect("decode raw report"),
        ));
    }

    let structured = json!({
        "items": (0..500).map(|index| json!({
            "id": index, "name": "example order", "total": 123.45,
            "active": true, "labels": ["alpha", "beta"],
        })).collect::<Vec<_>>()
    });
    let frame = format!(
        r#"{{"type":"invocationsobserved","items":[{{"function_id":"bench::echo","invocation_id":"i-1","status":"success","input":{structured},"started_at_ms":1,"duration_ms":0.5}}]}}"#
    );
    messages.push((
        "RawReportStructured",
        Message::from_frame(&frame).expect("decode raw report"),
    ));

    let mut group = c.benchmark_group("protocol_frame_serialize");
    for (label, message) in messages {
        let baseline = serde_json::to_string(&message).expect("serialize baseline");
        let encoded = message.to_frame().expect("serialize frame");
        assert_eq!(
            baseline, encoded,
            "the {label} frame must keep its wire text"
        );
        group.throughput(Throughput::Bytes(encoded.len() as u64));
        group.bench_with_input(BenchmarkId::new("serde_json", label), &message, |b, msg| {
            b.iter(|| serde_json::to_string(msg).expect("serialize baseline"));
        });
        group.bench_with_input(BenchmarkId::new("to_frame", label), &message, |b, msg| {
            b.iter(|| msg.to_frame().expect("serialize frame"));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    protocol_serialize_benchmark,
    protocol_deserialize_benchmark,
    protocol_roundtrip_benchmark,
    protocol_frame_serialize_benchmark,
);
criterion_main!(benches);
