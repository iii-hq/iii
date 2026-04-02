//! Custom OTLP JSON serializers that match the format expected by the III Engine.
//!
//! The engine parses OTLP JSON with `serde(rename_all = "camelCase")` and expects
//! integer attribute values as JSON numbers (not protobuf-style string-encoded int64).
//! This module replaces the `opentelemetry-proto` serde serialization with a
//! hand-built JSON format matching the Node.js/Python SDK output.

use opentelemetry::logs::AnyValue;
use opentelemetry::{Array, KeyValue, Value};
use serde_json::{Value as JsonValue, json};
use std::time::{SystemTime, UNIX_EPOCH};

/// Convert a SystemTime to nanoseconds-since-epoch as a string.
/// The engine accepts both string and number for timestamps (OtlpNumericString).
pub fn system_time_to_nanos_string(t: SystemTime) -> String {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// Convert a byte slice to a lowercase hex string.
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        hex.push_str(&format!("{:02x}", b));
    }
    hex
}

/// Convert an OTel attribute value to the OTLP JSON representation.
/// Integer values are emitted as JSON numbers (not strings).
pub fn attr_value_to_json(v: &Value) -> JsonValue {
    match v {
        Value::Bool(b) => json!({ "boolValue": b }),
        Value::I64(i) => json!({ "intValue": i }),
        Value::F64(f) => json!({ "doubleValue": f }),
        Value::String(s) => json!({ "stringValue": s.as_str() }),
        Value::Array(arr) => {
            let values: Vec<JsonValue> = match arr {
                Array::Bool(vs) => vs.iter().map(|v| json!({ "boolValue": v })).collect(),
                Array::I64(vs) => vs.iter().map(|v| json!({ "intValue": v })).collect(),
                Array::F64(vs) => vs.iter().map(|v| json!({ "doubleValue": v })).collect(),
                Array::String(vs) => vs
                    .iter()
                    .map(|v| json!({ "stringValue": v.as_str() }))
                    .collect(),
                _ => vec![],
            };
            json!({ "arrayValue": { "values": values } })
        }
        _ => json!({ "stringValue": format!("{:?}", v) }),
    }
}

/// Convert a slice of KeyValue pairs to the OTLP JSON attribute list.
pub fn attrs_to_json(attrs: &[KeyValue]) -> Vec<JsonValue> {
    attrs
        .iter()
        .map(|kv| {
            json!({
                "key": kv.key.as_str(),
                "value": attr_value_to_json(&kv.value)
            })
        })
        .collect()
}

/// Convert resource key-value iterator to OTLP JSON attribute list.
pub fn resource_attrs_to_json<'a>(
    iter: impl Iterator<Item = (&'a opentelemetry::Key, &'a Value)>,
) -> Vec<JsonValue> {
    iter.map(|(k, v)| {
        json!({
            "key": k.as_str(),
            "value": attr_value_to_json(v)
        })
    })
    .collect()
}

/// Convert an OTel log `AnyValue` to the OTLP JSON representation.
/// Used by the log exporter where attribute values are `AnyValue` instead of `Value`.
pub fn anyvalue_to_json(v: &AnyValue) -> JsonValue {
    match v {
        AnyValue::Boolean(b) => json!({ "boolValue": b }),
        AnyValue::Int(i) => json!({ "intValue": i }),
        AnyValue::Double(f) => json!({ "doubleValue": f }),
        AnyValue::String(s) => json!({ "stringValue": s.as_str() }),
        AnyValue::Bytes(bytes) => json!({ "bytesValue": bytes_to_hex_str(bytes) }),
        AnyValue::ListAny(list) => {
            let values: Vec<JsonValue> = list.iter().map(anyvalue_to_json).collect();
            json!({ "arrayValue": { "values": values } })
        }
        AnyValue::Map(map) => {
            let kvs: Vec<JsonValue> = map
                .iter()
                .map(|(k, v)| {
                    json!({
                        "key": k.as_str(),
                        "value": anyvalue_to_json(v)
                    })
                })
                .collect();
            json!({ "kvlistValue": { "values": kvs } })
        }
        _ => json!({ "stringValue": format!("{:?}", v) }),
    }
}

/// Hex-encode a byte slice (used for OTLP bytes values).
fn bytes_to_hex_str(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes.iter() {
        let _ = write!(s, "{:02x}", b);
    }
    s
}
