#![allow(dead_code)]

use std::io::Write;

use serde_json::Value;
use tempfile::NamedTempFile;

pub fn write_minimal_config_file() -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temp config file");

    // Empty modules list keeps startup deterministic and avoids optional adapters.
    let yaml = "port: 49134\nmodules: []\n";
    file.write_all(yaml.as_bytes())
        .expect("write benchmark config");
    file.flush().expect("flush benchmark config");

    file
}

pub fn benchmark_payload() -> Value {
    serde_json::json!({
        "source": "criterion",
        "value": 42,
        "labels": ["startup", "core-runtime"]
    })
}

pub fn payload_sizes() -> [(&'static str, usize); 4] {
    [
        ("1kb", 1024),
        ("10kb", 10 * 1024),
        ("100kb", 100 * 1024),
        ("1mb", 1024 * 1024),
    ]
}

pub fn sized_payload(target_bytes: usize) -> Value {
    let payload = "x".repeat(target_bytes);

    serde_json::json!({
        "payload": payload,
        "size_bytes": target_bytes,
    })
}

pub fn concurrency_levels() -> [usize; 4] {
    [1, 8, 32, 128]
}

pub fn fanout_levels() -> [usize; 4] {
    [1, 8, 32, 128]
}

pub fn churn_batch_sizes() -> [usize; 3] {
    [100, 1_000, 5_000]
}

pub fn trigger_payload() -> Value {
    serde_json::json!({
        "source": "criterion",
        "event": "fanout",
        "sequence": 1,
    })
}

pub fn route_counts() -> [usize; 4] {
    [1, 10, 100, 1_000]
}

pub fn http_concurrency_levels() -> [usize; 4] {
    [1, 8, 32, 128]
}

pub fn http_api_path(index: usize) -> String {
    format!("bench/{index}")
}

pub fn http_function_id(index: usize) -> String {
    format!("bench.http.{index}")
}

pub fn http_request_body() -> Value {
    serde_json::json!({
        "source": "criterion",
        "kind": "http",
        "value": 42,
    })
}

pub fn kv_contention_levels() -> [usize; 4] {
    [1, 4, 16, 64]
}

pub fn kv_value() -> Value {
    serde_json::json!({
        "name": "bench-item",
        "counter": 0,
        "tags": ["alpha", "beta"],
    })
}

pub fn pubsub_subscriber_counts() -> [usize; 4] {
    [1, 8, 32, 128]
}

pub fn queue_producer_counts() -> [usize; 4] {
    [1, 4, 16, 64]
}

pub fn state_value() -> Value {
    serde_json::json!({
        "status": "active",
        "counter": 0,
        "metadata": {"region": "us-east-1"},
    })
}

pub fn invocation_tracking_levels() -> [usize; 4] {
    [1, 8, 32, 128]
}
