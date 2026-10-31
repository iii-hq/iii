//! The observe wire contract: every golden file in
//! `sdk/fixtures/protocol/observe/` decodes into the SDK's `Message` and
//! encodes back to the same JSON value, and what the SDK builds for the
//! worker → engine messages is that JSON.
//!
//! No engine needed.

use std::path::PathBuf;

use serde_json::{Value, json};

use iii_sdk::Message;
use iii_sdk::protocol::{
    Capture, InvocationStatus, ObserverState, On, RegisterFunctionMessage, SDK_FEATURES,
};

/// The two files that carry the 0.25 `observe` object, which no v2 SDK sends
/// and v2 SDK types do not have (fixtures README).
const ENGINE_ONLY: [&str; 2] = [
    "registerfunction-v1-owner.json",
    "registerfunction-v1-not-observable.json",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/protocol/observe")
}

fn fixture(name: &str) -> Value {
    let path = fixtures_dir().join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn all_fixtures() -> Vec<(String, Value)> {
    let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures directory")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.ends_with(".json").then_some(name)
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no fixtures found");
    names
        .into_iter()
        .map(|n| (n.clone(), fixture(&n)))
        .collect()
}

fn decode(value: &Value) -> Message {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("decode {value}: {e}"))
}

#[test]
fn every_fixture_round_trips_without_loss() {
    for (name, value) in all_fixtures() {
        if ENGINE_ONLY.contains(&name.as_str()) {
            continue;
        }
        let message = decode(&value);
        assert!(
            !matches!(message, Message::Unknown),
            "{name}: decoded as an unknown message type"
        );
        let encoded = serde_json::to_value(&message).unwrap();
        assert_eq!(encoded, value, "{name}: re-encoding changed the JSON");
    }
}

/// Optional fields may arrive as `null` and read as absent.
#[test]
fn optional_fields_accept_null() {
    let required: &[(&str, &[&str])] = &[
        ("workerregistered", &["type", "worker_id"]),
        ("observeresult", &["type", "id", "state"]),
        ("observerstatechanged", &["type", "id", "state"]),
        ("functionregistered", &["type", "id", "revision"]),
        (
            "observeradded",
            &["type", "function_id", "revision", "observer"],
        ),
        (
            "observerremoved",
            &["type", "function_id", "revision", "id"],
        ),
        ("invokefunction", &["type", "function_id", "data"]),
        (
            "invocationresult",
            &["type", "invocation_id", "function_id"],
        ),
        ("registerfunction", &["type", "id"]),
    ];
    for (name, value) in all_fixtures() {
        if ENGINE_ONLY.contains(&name.as_str()) {
            continue;
        }
        let tag = value["type"].as_str().unwrap();
        let Some((_, keep)) = required.iter().find(|(t, _)| *t == tag) else {
            continue;
        };
        let keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        for key in keys.iter().filter(|k| !keep.contains(&k.as_str())) {
            let mut nulled = value.clone();
            nulled[key.as_str()] = Value::Null;
            serde_json::from_value::<Message>(nulled)
                .unwrap_or_else(|e| panic!("{name}: `{key}: null` failed to decode: {e}"));
        }
        if value.get("observer").is_some() {
            for key in ["args", "filters", "capture", "on"] {
                let mut nulled = value.clone();
                nulled["observer"][key] = Value::Null;
                let Message::ObserverAdded { observer: view, .. } = decode(&nulled) else {
                    panic!("{name}: not observeradded");
                };
                match key {
                    "args" => assert_eq!(view.args, json!({}), "{name}"),
                    "filters" => assert_eq!(view.filters, json!({}), "{name}"),
                    "capture" => assert_eq!(
                        view.capture,
                        vec![Capture::Input, Capture::Output, Capture::Caller]
                    ),
                    _ => assert_eq!(view.on, On::Success),
                }
            }
        }
    }
}

#[test]
fn unknown_message_types_and_enum_values_decode() {
    let unknown: Message =
        serde_json::from_value(json!({ "type": "fromthefuture", "x": 1 })).unwrap();
    assert!(matches!(unknown, Message::Unknown));

    let changed: Message = serde_json::from_value(json!({
        "type": "observerstatechanged", "id": "o", "state": "hibernating", "fires": 2
    }))
    .unwrap();
    let Message::ObserverStateChanged { state, .. } = &changed else {
        panic!("expected observerstatechanged");
    };
    assert_eq!(*state, ObserverState::Unknown("hibernating".to_string()));
    assert_eq!(
        serde_json::to_value(&changed).unwrap()["state"],
        "hibernating"
    );

    let added: Message = serde_json::from_value(json!({
        "type": "observeradded", "function_id": "f", "revision": 1,
        "observer": { "id": "o", "target": "t", "capture": ["input", "telemetry"], "on": "start" }
    }))
    .unwrap();
    let Message::ObserverAdded {
        namespace,
        observer,
        ..
    } = added
    else {
        panic!("expected observeradded");
    };
    assert_eq!(namespace, "default", "absent namespace is the default one");
    assert_eq!(
        observer.capture,
        vec![Capture::Input, Capture::Unknown("telemetry".into())]
    );
    assert_eq!(observer.on, On::Unknown("start".into()));
    assert_eq!(observer.args, json!({}));
    assert_eq!(observer.filters, json!({}));
}

/// `null` for a field with a default reads as that default: `private` and
/// `observable` on `registerfunction`, `observed` on `invocationresult`,
/// `output_is_input` on report items.
#[test]
fn defaults_sent_as_null_read_as_the_default() {
    let Message::RegisterFunction {
        private,
        observable,
        args_format,
        ..
    } = decode(&json!({
        "type": "registerfunction", "id": "f", "private": null, "observable": null,
        "args_format": null
    }))
    else {
        panic!()
    };
    assert!(!private && observable && args_format.is_none());

    let Message::InvocationResult { observed, .. } = decode(&json!({
        "type": "invocationresult", "invocation_id": "1f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        "function_id": "f", "result": 1, "observed": null
    })) else {
        panic!()
    };
    assert!(observed.is_empty());

    let mut source = fixture("invocationsobserved-event-source.json");
    source["items"][0]["output_is_input"] = Value::Null;
    let Message::InvocationsObserved { items } = decode(&source) else {
        panic!()
    };
    assert!(!items[0].output_is_input);

    // and the defaults sent explicitly decode too
    let Message::RegisterFunction {
        private,
        observable,
        ..
    } = decode(
        &json!({ "type": "registerfunction", "id": "f", "private": false, "observable": true }),
    )
    else {
        panic!()
    };
    assert!(!private && observable);
}

/// The engine-only files carry the 0.25 `observe` object: the SDK decodes
/// them (the object is an unknown field) and never builds it.
#[test]
fn the_engine_only_observe_object_is_never_built() {
    for name in ENGINE_ONLY {
        let value = fixture(name);
        assert!(value.get("observe").is_some(), "{name}");
        let message = decode(&value);
        assert!(
            serde_json::to_value(&message)
                .unwrap()
                .get("observe")
                .is_none(),
            "{name}: re-encoded with an observe object"
        );
    }
}

#[test]
fn engine_to_worker_fixtures_carry_their_meaning() {
    let Message::WorkerRegistered { features, .. } = decode(&fixture("workerregistered.json"))
    else {
        panic!()
    };
    assert_eq!(
        features,
        vec!["observe.v1", "observe.v2", "local-dispatch.v1"]
    );
    let Message::WorkerRegistered { features, .. } = decode(&fixture("workerregistered-v1.json"))
    else {
        panic!()
    };
    assert_eq!(features, vec!["observe.v1", "local-dispatch.v1"]);
    let Message::WorkerRegistered { features, .. } =
        decode(&fixture("workerregistered-legacy.json"))
    else {
        panic!()
    };
    assert!(features.is_empty());

    let Message::FunctionRegistered {
        local_dispatch,
        observers,
        revision,
        ..
    } = decode(&fixture("functionregistered.json"))
    else {
        panic!()
    };
    assert!(local_dispatch);
    assert_eq!(revision, 12);
    assert_eq!(observers.len(), 2);
    assert_eq!(observers[0].capture, vec![Capture::Input]);
    assert_eq!(observers[1].filters, json!({}));

    let Message::ObserverAdded { observer, .. } = decode(&fixture("observeradded.json")) else {
        panic!()
    };
    assert_eq!(observer.args["expression"], "0 0 9 * * Mon-Fri");
    assert_eq!(observer.target, "billing::send-reminders");

    let Message::ObserveResult {
        state,
        error,
        warnings,
        ..
    } = decode(&fixture("observeresult-error.json"))
    else {
        panic!()
    };
    assert_eq!(state, ObserverState::Error);
    assert_eq!(error.unwrap().code, "INVALID_FILTER_PATH");
    assert_eq!(warnings.len(), 1);

    let Message::InvokeFunction {
        caller,
        observe_depth,
        activation,
        ..
    } = decode(&fixture("invokefunction-activation.json"))
    else {
        panic!()
    };
    assert_eq!(caller.unwrap().worker_name.as_deref(), Some("iii-engine"));
    assert_eq!(observe_depth, Some(1));
    assert!(activation.unwrap().activation_id.is_some());

    let Message::InvokeFunction { caller, .. } = decode(&fixture("invokefunction-caller.json"))
    else {
        panic!()
    };
    let caller = caller.unwrap();
    assert_eq!(caller.worker_name.as_deref(), Some("billing"));
    assert_eq!(
        caller.parent.unwrap().function_id.as_deref(),
        Some("checkout::confirm")
    );

    let Message::InvocationsObserved { items } = decode(&fixture("invocationsobserved.json"))
    else {
        panic!()
    };
    assert_eq!(items[0].status, InvocationStatus::Error);
    assert_eq!(items[1].output, Some(Value::Null), "null output is kept");
    assert!(items[0].output.is_none(), "absent output stays absent");
    assert!(items[1].caller.is_none(), "a call made outside any handler");
    assert!(
        items
            .iter()
            .filter_map(|item| item.caller.as_ref())
            .all(|caller| caller.worker_id.is_none() && caller.worker_name.is_none()),
        "the engine stamps the worker"
    );

    let Message::InvocationsObserved { items } =
        decode(&fixture("invocationsobserved-event-source.json"))
    else {
        panic!()
    };
    assert!(items[0].output_is_input);
    assert!(items[0].output.is_none(), "the value travels once");

    let Message::InvocationResult {
        result, observed, ..
    } = decode(&fixture("invocationresult-observed.json"))
    else {
        panic!()
    };
    assert!(result.is_some());
    assert_eq!(observed.len(), 2);
    let Message::InvocationResult {
        result,
        error,
        observed,
        ..
    } = decode(&fixture("invocationresult-observed-error.json"))
    else {
        panic!()
    };
    assert!(result.is_none());
    assert_eq!(error.unwrap().code, "CARD_DECLINED");
    assert_eq!(observed, vec!["c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60"]);

    let Message::InvokeFunction { activation, .. } =
        decode(&fixture("invokefunction-observer-call-forwarded.json"))
    else {
        panic!()
    };
    let activation = activation.unwrap();
    assert_eq!(
        activation.observer_id,
        "b6c4e2a0-9f1d-4b3a-8e7c-6d5f4e3a2b1c"
    );
    assert!(activation.activation_id.is_some());
}

/// What this SDK declares on `engine::workers::register`.
#[test]
fn the_sdk_declares_the_native_feature_list() {
    let register = fixture("invokefunction-workers-register.json");
    assert_eq!(json!(SDK_FEATURES), register["data"]["features"]);
}

#[test]
fn registerfunction_messages_build_the_fixtures() {
    let plain: RegisterFunctionMessage = RegisterFunctionMessage {
        id: "orders::archive".into(),
        description: Some("Archives a closed order.".into()),
        request_format: Some(json!({
            "type": "object", "required": ["order_id"],
            "properties": { "order_id": { "type": "string" } }
        })),
        response_format: Some(json!({ "type": "null" })),
        metadata: None,
        invocation: None,
        args_format: None,
        private: false,
        observable: true,
    };
    assert_eq!(
        serde_json::to_value(plain.to_message()).unwrap(),
        fixture("registerfunction.json")
    );

    let private = fixture("registerfunction-private.json");
    let mut message = RegisterFunctionMessage::with_id("cron::on-tick".into()).with_description(
        "One call per tick of a cron expression. Observe it with args { expression }.".into(),
    );
    message.request_format = Some(private["request_format"].clone());
    message.response_format = Some(private["response_format"].clone());
    message.args_format = Some(private["args_format"].clone());
    message.private = true;
    assert_eq!(serde_json::to_value(message.to_message()).unwrap(), private);

    let closed = fixture("registerfunction-not-observable.json");
    let mut message = RegisterFunctionMessage::with_id("vault::read-secret".into())
        .with_description(closed["description"].as_str().unwrap().into());
    message.request_format = Some(closed["request_format"].clone());
    message.observable = false;
    assert_eq!(serde_json::to_value(message.to_message()).unwrap(), closed);
}
