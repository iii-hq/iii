//! The observe golden fixtures (`sdk/fixtures/protocol/observe/*.json`) are the
//! wire contract every SDK decodes and encodes. This suite keeps them
//! canonical for the engine: each one must decode into `Message` and encode
//! back to exactly the same JSON, so a fixture can never describe a shape the
//! engine would not produce or accept.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use iii::protocol::{Capture, Delivery, InvocationStatus, Message, ObserverState, engine_features};
use iii::workers::engine_fn::RegisterWorkerInput;
use serde_json::{Value, json};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../sdk/fixtures/protocol/observe")
}

/// Every `*.json` fixture, by file stem.
fn fixtures() -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures directory exists")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).expect("readable fixture");
            let value: Value = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("{stem}.json is not JSON: {err}"));
            (stem, value)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no fixtures found in {:?}", fixtures_dir());
    out
}

fn fixture(name: &str) -> Value {
    let path = fixtures_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{name}.json exists"));
    serde_json::from_str(&text).expect("fixture is JSON")
}

fn message(name: &str) -> Message {
    serde_json::from_value(fixture(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
}

#[test]
fn every_observe_fixture_round_trips_through_message() {
    for (name, value) in fixtures() {
        let msg: Message = serde_json::from_value(value.clone())
            .unwrap_or_else(|err| panic!("{name}.json does not decode as Message: {err}"));
        let encoded = serde_json::to_value(&msg).expect("encodes");
        assert_eq!(
            encoded, value,
            "{name}.json is not canonical: the engine re-encodes it differently"
        );
    }
}

/// The file name starts with the message's `type` tag, so an SDK can pick the
/// decoder from the name and a reader can find a message's fixtures.
#[test]
fn every_fixture_is_named_after_its_type() {
    for (name, value) in fixtures() {
        let tag = value["type"].as_str().expect("a type tag");
        let prefix = name.split('-').next().unwrap();
        assert_eq!(prefix, tag, "{name}.json carries type `{tag}`");
    }
}

/// Every message type `Message` decodes, read from the enum itself: serde
/// lists the variants it expects when it meets an unknown one. A variant added
/// later shows up here without anyone remembering to list it.
fn message_tags() -> BTreeSet<String> {
    let err = serde_json::from_value::<Message>(json!({ "type": "__no_such_message__" }))
        .expect_err("an unknown type does not decode")
        .to_string();
    let expected = err
        .split("expected one of")
        .nth(1)
        .unwrap_or_else(|| panic!("unexpected serde error: {err}"));
    expected
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The message types that existed before 0.25. Every other type is new with
/// observe and must have a fixture; every one of these has a pre-0.25 frame
/// below.
const PRE_025: &[&str] = &[
    "registertriggertype",
    "registertrigger",
    "triggerregistrationresult",
    "unregistertrigger",
    "registerfunction",
    "unregisterfunction",
    "invokefunction",
    "invocationresult",
    "registerservice",
    "ping",
    "pong",
    "reattach",
    "workerregistered",
    "registrationrejected",
];

/// One frame per pre-0.25 message type, as an SDK or engine of that era sends
/// it: none of the fields observe adds.
fn pre_025_frames() -> Vec<Value> {
    vec![
        json!({ "type": "registertriggertype", "id": "webhook", "description": "Webhooks",
                "trigger_request_format": { "type": "object" } }),
        json!({ "type": "registertrigger", "id": "t-1", "trigger_type": "cron",
                "function_id": "reports::daily", "config": { "expression": "0 0 9 * * *" } }),
        json!({ "type": "triggerregistrationresult", "id": "t-1", "trigger_type": "cron",
                "function_id": "reports::daily" }),
        json!({ "type": "unregistertrigger", "id": "t-1", "trigger_type": "cron" }),
        json!({ "type": "registerfunction", "id": "orders::create" }),
        json!({ "type": "unregisterfunction", "id": "orders::create" }),
        json!({ "type": "invokefunction", "invocation_id": "3c5e7a9b-1d3f-4b5d-9f1a-3c5e7a9b1d3f",
                "function_id": "orders::create", "data": { "sku": "A-1" } }),
        json!({ "type": "invocationresult", "invocation_id": "3c5e7a9b-1d3f-4b5d-9f1a-3c5e7a9b1d3f",
                "function_id": "orders::create", "result": { "id": 1 } }),
        json!({ "type": "registerservice", "id": "orders", "name": "orders" }),
        json!({ "type": "ping" }),
        json!({ "type": "pong" }),
        json!({ "type": "reattach", "previous_worker_id": "7c1e0b52-93d4-4f0e-8a3b-6d2f1c9e4a70",
                "reattach_token": "0f9e8d7c-6b5a-4c3d-9e2f-1a0b9c8d7e6f" }),
        json!({ "type": "workerregistered", "worker_id": "7c1e0b52-93d4-4f0e-8a3b-6d2f1c9e4a70",
                "reattach_token": "0f9e8d7c-6b5a-4c3d-9e2f-1a0b9c8d7e6f" }),
        json!({ "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
                "namespace": "default", "function_id": "orders::create", "owner_worker_id": "w-2" }),
    ]
}

#[test]
fn every_message_type_has_a_fixture_or_a_pre_025_frame() {
    let tags = message_tags();
    assert!(
        tags.len() > PRE_025.len(),
        "serde listed the variants: {tags:?}"
    );
    let with_fixture: BTreeSet<String> = fixtures()
        .into_iter()
        .map(|(_, value)| value["type"].as_str().unwrap().to_string())
        .collect();
    for tag in &tags {
        if !PRE_025.contains(&tag.as_str()) {
            assert!(
                with_fixture.contains(tag),
                "no fixture for the new message `{tag}`"
            );
        }
    }
    for tag in PRE_025 {
        assert!(tags.contains(*tag), "`{tag}` is no longer a message type");
    }
    // The existing messages observe extends have fixtures too.
    for extended in ["registerfunction", "invokefunction", "workerregistered"] {
        assert!(
            with_fixture.contains(extended),
            "no fixture for `{extended}`"
        );
    }
}

#[test]
fn every_pre_025_frame_still_decodes_and_re_encodes_without_new_fields() {
    let frames = pre_025_frames();
    let covered: BTreeSet<&str> = frames.iter().map(|f| f["type"].as_str().unwrap()).collect();
    for tag in PRE_025 {
        assert!(covered.contains(tag), "no pre-0.25 frame for `{tag}`");
    }
    for frame in frames {
        let msg: Message = serde_json::from_value(frame.clone())
            .unwrap_or_else(|err| panic!("{frame} no longer decodes: {err}"));
        let encoded = serde_json::to_value(&msg).unwrap();
        for key in [
            "caller",
            "observe_depth",
            "activation",
            "observe",
            "features",
        ] {
            assert!(encoded.get(key).is_none(), "`{key}` appeared in {encoded}");
        }
    }
}

/// The optional fields the README lists, by fixture and JSON pointer. Each
/// must accept `null` as well as absence.
const OPTIONAL: &[(&str, &str)] = &[
    ("observe", "/source/namespace"),
    ("observe", "/source/args"),
    ("observe", "/source/filters"),
    ("observe", "/source/capture"),
    ("observe", "/source/on"),
    ("observe", "/source/deliver"),
    ("observe", "/target/namespace"),
    ("observe", "/metadata"),
    ("observe", "/max_fires"),
    ("observe", "/expires_in_ms"),
    ("observe", "/max_in_flight"),
    ("observerack", "/namespace"),
    ("observerack-error", "/error"),
    ("invocationsobserved", "/items/0/namespace"),
    ("invocationsobserved", "/items/0/input"),
    ("invocationsobserved", "/items/0/output"),
    ("invocationsobserved", "/items/0/error"),
    ("invocationsobserved", "/items/0/caller"),
    ("invocationsobserved", "/items/0/metadata"),
    ("invocationsobserved", "/items/0/traceparent"),
    ("invocationsobserved", "/items/0/baggage"),
    ("localinvocationstats", "/functions/0/errors"),
    ("localinvocationstats", "/functions/0/duration_ms_sum"),
    ("localinvocationstats", "/functions"),
    ("localinvocationstats", "/dropped_reports"),
    ("registerfunction", "/description"),
    ("registerfunction", "/request_format"),
    ("registerfunction", "/response_format"),
    ("registerfunction-owner", "/observe"),
    ("registerfunction-owner", "/observe/delivery"),
    ("registerfunction-owner", "/observe/args_format"),
    ("registerfunction-owner", "/observe/legacy_trigger_type"),
    ("registerfunction-owner", "/observe/allow"),
    ("invokefunction-activation", "/caller"),
    ("invokefunction-activation", "/observe_depth"),
    ("invokefunction-activation", "/activation"),
    ("invokefunction-activation", "/activation/activation_id"),
    ("invokefunction-activation", "/traceparent"),
    ("invokefunction-activation", "/metadata"),
    ("invokefunction-caller", "/caller/parent"),
    ("invokefunction-owner-call-namespace", "/namespace"),
    ("invokefunction-workers-register", "/data/features"),
    ("unregistertriggertype", "/namespace"),
    ("workerregistered", "/reattach_token"),
    ("workerregistered", "/features"),
    ("observeresult-error", "/error"),
    ("observeresult-error", "/warnings"),
    ("observerstatechanged-reason", "/reason"),
    ("observeradded-legacy", "/observer/legacy"),
];

#[test]
fn null_is_accepted_for_every_optional_field() {
    for (name, pointer) in OPTIONAL {
        let mut value = fixture(name);
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let target = if parent.is_empty() {
            &mut value
        } else {
            value
                .pointer_mut(parent)
                .unwrap_or_else(|| panic!("{name}: no {parent}"))
        };
        target[key] = Value::Null;

        let msg: Message = serde_json::from_value(value)
            .unwrap_or_else(|err| panic!("{name} with {pointer}: null does not decode: {err}"));
        if *name == "invokefunction-workers-register" {
            let Message::InvokeFunction { mut data, .. } = msg else {
                panic!("{name}");
            };
            data["_caller_worker_id"] = json!("550e8400-e29b-41d4-a716-446655440000");
            let input: RegisterWorkerInput = serde_json::from_value(data)
                .unwrap_or_else(|err| panic!("{name} with {pointer}: null: {err}"));
            assert!(input.features.is_empty());
        }
    }
}

#[test]
fn readme_describes_every_fixture() {
    let readme = std::fs::read_to_string(fixtures_dir().join("README.md")).expect("README.md");
    for (name, _) in fixtures() {
        assert!(
            readme.contains(&format!("`{name}.json`")),
            "README.md does not describe {name}.json"
        );
    }
}

/// The variants the fixtures exist to pin down carry what their names say.
#[test]
fn fixtures_carry_the_documented_variants() {
    let Message::Observe {
        source,
        metadata,
        max_fires,
        expires_in_ms,
        max_in_flight,
        ..
    } = message("observe")
    else {
        panic!("observe");
    };
    assert!(source.namespace.is_some() && source.args.is_some() && source.filters.is_some());
    assert!(source.capture.is_some() && source.on.is_some() && source.deliver.is_some());
    assert!(
        metadata.is_some()
            && max_fires.is_some()
            && expires_in_ms.is_some()
            && max_in_flight.is_some()
    );

    let Message::Observe {
        source, metadata, ..
    } = message("observe-minimal")
    else {
        panic!("observe-minimal");
    };
    assert!(source.args.is_none() && source.filters.is_none() && source.capture.is_none());
    assert!(metadata.is_none());

    let Message::ObserveResult {
        state,
        error,
        warnings,
        ..
    } = message("observeresult-error")
    else {
        panic!("observeresult-error");
    };
    assert_eq!(state, ObserverState::Error);
    assert_eq!(error.expect("error").code, "INVALID_FILTER_PATH");
    assert!(!warnings.is_empty());

    let Message::ObserverStateChanged { reason, .. } = message("observerstatechanged-reason")
    else {
        panic!("observerstatechanged-reason");
    };
    assert!(reason.is_some());

    let Message::ObserverAck { error, .. } = message("observerack-error") else {
        panic!("observerack-error");
    };
    assert!(error.is_some());
    let Message::ObserverAck { error, .. } = message("observerack") else {
        panic!("observerack");
    };
    assert!(error.is_none());

    let Message::FunctionRegistered {
        observers,
        local_dispatch,
        ..
    } = message("functionregistered")
    else {
        panic!("functionregistered");
    };
    assert!(local_dispatch && !observers.is_empty());
    let Message::FunctionRegistered { observers, .. } = message("functionregistered-empty") else {
        panic!("functionregistered-empty");
    };
    assert!(observers.is_empty());

    let Message::ObserverAdded { observer, .. } = message("observeradded-legacy") else {
        panic!("observeradded-legacy");
    };
    assert_eq!(observer.legacy.expect("legacy").trigger_type, "http");

    let Message::InvocationsObserved { items } = message("invocationsobserved") else {
        panic!("invocationsobserved");
    };
    assert!(items.rejected.is_empty());
    let items = items.items;
    assert!(
        items.iter().any(|i| i.status == InvocationStatus::Error),
        "an auto item that failed"
    );
    assert!(
        items.iter().any(|i| i.output == Some(Value::Null)),
        "an auto item that returned null"
    );
    let owner_item = items
        .iter()
        .find(|i| i.matched.len() > 1 && i.caller.as_ref().is_some_and(|c| c.parent.is_some()))
        .expect("an owner emit coalesced for two observers, with a parent caller");
    let event = owner_item.input.as_ref().expect("the row-changed event");
    for key in [
        "db",
        "schema",
        "table",
        "op",
        "affected_rows",
        "returning",
        "truncated",
        "at",
    ] {
        assert!(
            event.get(key).is_some(),
            "database.md §2.2 event field `{key}`"
        );
    }

    let Message::RegisterFunction { observe, .. } = message("registerfunction-owner") else {
        panic!("registerfunction-owner");
    };
    let observe = observe.expect("descriptor");
    assert_eq!(observe.delivery(), Delivery::Owner);
    assert!(observe.args_format.is_some() && observe.legacy_trigger_type.is_some());
    let Message::RegisterFunction { observe, .. } = message("registerfunction") else {
        panic!("registerfunction");
    };
    assert!(observe.is_none());
    let Message::RegisterFunction { observe, .. } = message("registerfunction-not-observable")
    else {
        panic!("registerfunction-not-observable");
    };
    assert!(!observe.expect("descriptor").allow());

    let Message::InvokeFunction {
        invocation_id,
        action,
        caller,
        observe_depth,
        activation,
        ..
    } = message("invokefunction-activation")
    else {
        panic!("invokefunction-activation");
    };
    assert!(caller.is_some() && observe_depth == Some(1));
    assert!(activation.expect("activation").activation_id.is_some());
    // An auto activation is fire-and-forget for the observed side (D16), and
    // reaches the target exactly as the engine forwards any call: with an
    // invocation id and no `action`; the target's SDK answers it as usual.
    assert!(invocation_id.is_some() && action.is_none());

    let Message::InvokeFunction {
        namespace,
        activation,
        ..
    } = message("invokefunction-owner-call-namespace")
    else {
        panic!("invokefunction-owner-call-namespace");
    };
    assert!(
        namespace.is_some() && activation.is_some(),
        "D13: a namespace the engine ignores"
    );

    let Message::InvokeFunction {
        observe_depth,
        activation,
        ..
    } = message("invokefunction-owner-call")
    else {
        panic!("invokefunction-owner-call");
    };
    assert_eq!(observe_depth, None, "depth 0 is omitted");
    assert!(activation.expect("activation").activation_id.is_none());

    let Message::InvokeFunction {
        caller,
        observe_depth,
        activation,
        ..
    } = message("invokefunction-legacy")
    else {
        panic!("invokefunction-legacy");
    };
    assert!(caller.is_none() && observe_depth.is_none() && activation.is_none());

    let Message::WorkerRegistered { features, .. } = message("workerregistered") else {
        panic!("workerregistered");
    };
    assert_eq!(features, engine_features(), "what this engine advertises");
    let Message::WorkerRegistered { features, .. } = message("workerregistered-legacy") else {
        panic!("workerregistered-legacy");
    };
    assert!(features.is_empty());
}

/// Only known enum values appear in the fixtures: an `Unknown` would mean a
/// typo in the contract, not a newer peer.
#[test]
fn fixtures_use_only_known_enum_values() {
    for (name, _) in fixtures() {
        match message(&name) {
            Message::Observe { source, .. } => {
                for capture in source.capture.iter().flatten() {
                    assert!(capture.is_known(), "{name}: {capture}");
                }
                assert!(
                    source.on().is_known() && source.deliver().is_known(),
                    "{name}"
                );
            }
            Message::ObserveResult { state, .. } | Message::ObserverStateChanged { state, .. } => {
                assert!(state.is_known(), "{name}: {state}");
            }
            Message::FunctionRegistered { observers, .. } => {
                for observer in observers {
                    assert!(observer.capture.iter().all(Capture::is_known), "{name}");
                    assert!(observer.on.is_known(), "{name}");
                }
            }
            Message::ObserverAdded { observer, .. } => {
                assert!(observer.capture.iter().all(Capture::is_known), "{name}");
                assert!(observer.on.is_known(), "{name}");
            }
            Message::InvocationsObserved { items } => {
                assert!(items.items.iter().all(|i| i.status.is_known()), "{name}");
            }
            Message::RegisterFunction {
                observe: Some(observe),
                ..
            } => assert!(observe.delivery().is_known(), "{name}"),
            _ => {}
        }
    }
}

/// SDKs declare their features in the `engine::workers::register` payload;
/// the engine reads them from what the fixture sends (plus the
/// `_caller_worker_id` it injects into every forwarded payload).
#[test]
fn workers_register_fixture_declares_features() {
    let Message::InvokeFunction {
        function_id,
        mut data,
        ..
    } = message("invokefunction-workers-register")
    else {
        panic!("invokefunction-workers-register");
    };
    assert_eq!(function_id, "engine::workers::register");
    data["_caller_worker_id"] = Value::String("550e8400-e29b-41d4-a716-446655440000".into());
    let input: RegisterWorkerInput = serde_json::from_value(data).expect("decodes");
    assert_eq!(input.features, engine_features());
}
