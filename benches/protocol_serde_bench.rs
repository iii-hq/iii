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
            },
        ),
        (
            "RegisterTrigger",
            Message::RegisterTrigger {
                id: "bench-trigger-0".to_string(),
                trigger_type: "http".to_string(),
                function_id: "bench.echo".to_string(),
                config: json!({"api_path": "bench/0", "http_method": "POST"}),
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
                }),
                traceparent: None,
                baggage: None,
            },
        ),
        (
            "WorkerRegistered",
            Message::WorkerRegistered {
                worker_id: Uuid::new_v4().to_string(),
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

criterion_group!(
    benches,
    protocol_serialize_benchmark,
    protocol_deserialize_benchmark,
    protocol_roundtrip_benchmark,
);
criterion_main!(benches);
