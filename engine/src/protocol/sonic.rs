// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Sonic's serialization view of the protocol. `RawJson` keeps its Serde JSON
//! implementation; this view uses Sonic's public borrowed raw-value API instead
//! of depending on either library's private Serde identifiers.

use std::sync::OnceLock;

use serde::{Serialize, Serializer, ser::Error as _};

use super::{Caller, InvocationStatus, Message, ObservedItem, ObservedItems, RawJson};

pub(super) fn supports_serde_json_numbers() -> bool {
    // Cargo can unify arbitrary_precision from another crate. Test the actual
    // numeric wire representation instead of assuming the dependency's features.
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        sonic_rs::to_string(&serde_json::Number::from(0)).is_ok_and(|encoded| encoded == "0")
    })
}

pub(super) fn to_string(message: &Message) -> sonic_rs::Result<String> {
    match message {
        Message::InvocationsObserved { items } => sonic_rs::to_string(&Report {
            kind: "invocationsobserved",
            items: Items(items),
        }),
        _ => sonic_rs::to_string(message),
    }
}

#[derive(Serialize)]
struct Report<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    items: Items<'a>,
}

struct Items<'a>(&'a ObservedItems);

impl Serialize for Items<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.items.iter().map(Item))
    }
}

struct Item<'a>(&'a ObservedItem);

impl Serialize for Item<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ItemFields::serialize(self.0, serializer)
    }
}

// A remote derive borrows the original fields without copying payloads or
// changing ObservedItem's public type or its Serde JSON implementation.
#[derive(Serialize)]
#[serde(remote = "ObservedItem")]
struct ItemFields {
    function_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    invocation_id: String,
    matched: Vec<String>,
    status: InvocationStatus,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "raw")]
    input: Option<RawJson>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "raw")]
    output: Option<RawJson>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "raw")]
    error: Option<RawJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    caller: Option<Caller>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "raw")]
    metadata: Option<RawJson>,
    started_at_ms: u64,
    duration_ms: f64,
    observe_depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    traceparent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baggage: Option<String>,
    #[serde(skip_serializing_if = "super::is_false")]
    output_is_input: bool,
}

fn raw<S: Serializer>(value: &Option<RawJson>, serializer: S) -> Result<S::Ok, S::Error> {
    value
        .as_ref()
        .map(|raw| sonic_rs::from_str::<sonic_rs::LazyValue<'_>>(raw.get()))
        .transpose()
        .map_err(S::Error::custom)?
        .serialize(serializer)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::{Message, RawJson};

    fn report(input: &str) -> Message {
        Message::from_frame(&format!(
            r#"{{"type":"invocationsobserved","items":[{{"function_id":"orders::archive","invocation_id":"i-1","status":"success","input":{input},"started_at_ms":1,"duration_ms":0.5}}]}}"#
        ))
        .expect("valid raw report")
    }

    #[test]
    fn raw_text_and_every_report_field_match_the_existing_encoder() {
        let frame = r#"{"type":"invocationsobserved","items":[{"function_id":"orders::archive","namespace":"billing","invocation_id":"i-1","matched":["o-1","o-2"],"status":"error","input":{"z": 1, "z":2, "a":[1.5000,"\u00e9"]},"output":null,"error":{"code": "E", "message":"erro 🚀"},"caller":{"worker_id":"w-1","worker_name":"billing","function_id":"orders::create","invocation_id":"i-0","parent":{"function_id":"http::on-request"}},"metadata":{"number":9007199254740993,"escaped":"\u0061"},"started_at_ms":42,"duration_ms":0.125,"observe_depth":3,"traceparent":"trace-context","baggage":"owner=billing","output_is_input":true}]}"#;
        let message = Message::from_frame(frame).unwrap();
        let baseline = serde_json::to_string(&message).unwrap();
        let encoded = super::to_string(&message).expect("the Sonic view accepts raw fields");
        assert_eq!(encoded, baseline);

        let Message::InvocationsObserved { items: before } = message else {
            panic!("expected report");
        };
        let Message::InvocationsObserved { items: after } = Message::from_frame(&encoded).unwrap()
        else {
            panic!("expected report");
        };
        assert_eq!(after, before);
    }

    #[test]
    fn present_null_is_distinct_from_absent_raw_fields() {
        let encoded = report("null").to_frame().unwrap();
        let value: Value = serde_json::from_str(&encoded).unwrap();
        let item = &value["items"][0];
        assert_eq!(item.get("input"), Some(&Value::Null));
        for absent in ["output", "error", "metadata", "output_is_input"] {
            assert!(item.get(absent).is_none(), "{absent} must remain absent");
        }
    }

    #[test]
    fn raw_numbers_escapes_and_duplicate_keys_are_not_materialized() {
        for raw in [
            "1844674407370955161600000000000000000000000000000",
            "1e10000",
            "-0.0000000000000000000000000000000000000010",
            r#""\ud800""#,
            r#""\udc00""#,
            r#"{"same":1,"same":2,"$serde_json::private::RawValue":"literal","$sonic_rs::LazyValue":"literal"}"#,
            r#"["\u00e9", "🚀", "line\nquote\"", null, true]"#,
        ] {
            let message = report(raw);
            let encoded = message.to_frame().unwrap();
            assert_eq!(encoded, serde_json::to_string(&message).unwrap());
            let Message::InvocationsObserved { items } = Message::from_frame(&encoded).unwrap()
            else {
                panic!("expected report");
            };
            assert_eq!(items.items[0].input.as_ref().map(RawJson::get), Some(raw));
        }
    }

    #[test]
    fn deeply_nested_raw_payloads_keep_the_existing_reader_contract() {
        let raw = format!("{}0{}", "[".repeat(512), "]".repeat(512));
        let message = report(&raw);
        assert_eq!(
            message.to_frame().unwrap(),
            serde_json::to_string(&message).unwrap()
        );
    }

    #[test]
    fn rejected_items_are_not_added_to_the_output() {
        let frame = r#"{"type":"invocationsobserved","items":[{"function_id":"orders::archive","invocation_id":"good","status":"paused","started_at_ms":1,"duration_ms":0.5},{"function_id":"orders::archive"},false]}"#;
        let message = Message::from_frame(frame).unwrap();
        let Message::InvocationsObserved { items } = &message else {
            panic!("expected report");
        };
        assert_eq!(items.rejected.len(), 2);
        let encoded = message.to_frame().unwrap();
        assert_eq!(encoded, serde_json::to_string(&message).unwrap());
        let value: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["items"].as_array().unwrap().len(), 1);
        assert_eq!(value["items"][0]["status"], json!("paused"));
    }

    #[test]
    fn ordinary_frames_keep_their_wire_values() {
        for frame in [
            r#"{"type":"ping"}"#,
            r#"{"type":"invokefunction","function_id":"orders::archive","data":{"text":"linha\n🚀","number":9007199254740993,"nested":[null,true]}}"#,
            r#"{"type":"invocationresult","invocation_id":"00000000-0000-0000-0000-00000000002a","function_id":"orders::archive","result":null}"#,
            r#"{"type":"invocationresult","invocation_id":"00000000-0000-0000-0000-00000000002a","function_id":"orders::archive"}"#,
        ] {
            let message = Message::from_frame(frame).unwrap();
            let baseline = serde_json::to_value(&message).unwrap();
            let encoded = message.to_frame().unwrap();
            assert_eq!(encoded, serde_json::to_string(&message).unwrap());
            assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), baseline);
        }
    }
}
