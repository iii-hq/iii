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
    Capture, Delivery, InvocationStatus, ObserveDescriptor, ObserverState, On,
    RegisterFunctionMessage, UnregisterTriggerTypeMessage,
};

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
    ];
    for (name, value) in all_fixtures() {
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
            for key in ["args", "filters", "capture", "on", "legacy"] {
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
                    "on" => assert_eq!(view.on, On::Success),
                    _ => assert!(view.legacy.is_none()),
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

#[test]
fn engine_to_worker_fixtures_carry_their_meaning() {
    let Message::WorkerRegistered { features, .. } = decode(&fixture("workerregistered.json"))
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

    let Message::ObserverAdded { observer, .. } = decode(&fixture("observeradded-legacy.json"))
    else {
        panic!()
    };
    let legacy = observer.legacy.expect("legacy trigger");
    assert_eq!(legacy.trigger_type, "http");
    assert_eq!(legacy.config["api_path"], "/users/:id");

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
    assert_eq!(items[2].matched.len(), 2);
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
        observe: None,
    };
    assert_eq!(
        serde_json::to_value(plain.to_message()).unwrap(),
        fixture("registerfunction.json")
    );

    let owner = fixture("registerfunction-owner.json");
    let mut message = RegisterFunctionMessage::with_id("cron::on-tick".into())
        .with_description("Fires on a cron schedule. Observe it with args { expression }.".into());
    message.request_format = Some(owner["request_format"].clone());
    message.observe = Some(ObserveDescriptor {
        delivery: Some(Delivery::Owner),
        args_format: Some(owner["observe"]["args_format"].clone()),
        legacy_trigger_type: Some("cron".into()),
        allow: Some(true),
    });
    assert_eq!(serde_json::to_value(message.to_message()).unwrap(), owner);

    let closed = fixture("registerfunction-not-observable.json");
    let mut message = RegisterFunctionMessage::with_id("vault::read-secret".into())
        .with_description(closed["description"].as_str().unwrap().into());
    message.request_format = Some(closed["request_format"].clone());
    message.observe = Some(ObserveDescriptor {
        delivery: Some(Delivery::Auto),
        allow: Some(false),
        ..ObserveDescriptor::default()
    });
    assert_eq!(serde_json::to_value(message.to_message()).unwrap(), closed);

    let unregister = UnregisterTriggerTypeMessage {
        id: "webhook".into(),
    };
    assert_eq!(
        serde_json::to_value(unregister.to_message()).unwrap(),
        fixture("unregistertriggertype.json")
    );
}
