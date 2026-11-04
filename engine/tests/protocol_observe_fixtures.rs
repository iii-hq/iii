//! The observe golden fixtures (`sdk/fixtures/protocol/observe/*.json`) are the
//! wire contract every SDK decodes and encodes. This suite keeps them
//! canonical for the engine: each one must decode into `Message` and encode
//! back to exactly the same JSON, so a fixture can never describe a shape the
//! engine would not produce or accept.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use iii::protocol::{
    Capture, FEATURE_LARGE_MESSAGES_V1, FEATURE_LOCAL_DISPATCH_V1, FEATURE_OBSERVE_V1,
    FEATURE_OBSERVE_V2, InvocationStatus, Message, ObserverState, engine_features,
};
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

/// The message types that existed before 0.25 and still do. Every other type
/// is new with observe and must have a fixture; every one of these has a
/// pre-0.25 frame below.
const PRE_025: &[&str] = &[
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
            "args_format",
            "private",
            "observable",
            "observed",
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
    (
        "invocationsobserved-event-source",
        "/items/0/output_is_input",
    ),
    ("registerfunction", "/description"),
    ("registerfunction", "/request_format"),
    ("registerfunction", "/response_format"),
    ("registerfunction-private", "/args_format"),
    ("registerfunction-private", "/private"),
    ("registerfunction-not-observable", "/observable"),
    ("registerfunction-v1-owner", "/observe"),
    ("registerfunction-v1-owner", "/observe/delivery"),
    ("registerfunction-v1-not-observable", "/observe/allow"),
    ("invokefunction-activation", "/caller"),
    ("invokefunction-activation", "/observe_depth"),
    ("invokefunction-activation", "/activation"),
    ("invokefunction-activation", "/activation/activation_id"),
    ("invokefunction-activation", "/traceparent"),
    ("invokefunction-activation", "/metadata"),
    ("invokefunction-caller", "/caller/parent"),
    ("invokefunction-observer-call-namespace", "/namespace"),
    ("invokefunction-observer-call", "/caller"),
    ("invokefunction-workers-register", "/data/features"),
    ("invocationresult-routed", "/result"),
    ("invocationresult-matched", "/matched"),
    ("invocationresult-matched", "/result"),
    ("invocationresult-matched-error", "/error"),
    ("invocationresult-matched-none", "/matched"),
    ("invocationresult-delivered", "/delivered"),
    ("workerregistered", "/reattach_token"),
    ("workerregistered", "/features"),
    ("observeresult-error", "/error"),
    ("observeresult-error", "/warnings"),
    ("observerstatechanged-reason", "/reason"),
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

    let Message::InvocationsObserved { items } = message("invocationsobserved") else {
        panic!("invocationsobserved");
    };
    assert!(items.rejected.is_empty());
    let items = items.items;
    assert!(
        items.iter().any(|i| i.status == InvocationStatus::Error),
        "a call that failed"
    );
    assert!(
        items.iter().any(|i| i.output == Some(Value::Null)),
        "a call that returned null"
    );
    assert!(
        items.iter().all(|i| !i.output_is_input),
        "output_is_input is left out at its default"
    );

    let Message::InvocationsObserved { items } = message("invocationsobserved-event-source") else {
        panic!("invocationsobserved-event-source");
    };
    let event_source = &items.items[0];
    assert!(event_source.output_is_input && event_source.output.is_none());
    assert!(event_source.matched.len() > 1, "one event, two observers");
    let event = event_source.input.as_ref().expect("the row change");
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
        assert!(event.get(key).is_some(), "row change field `{key}`");
    }

    let Message::RegisterFunction {
        args_format,
        private,
        observable,
        observe,
        ..
    } = message("registerfunction-private")
    else {
        panic!("registerfunction-private");
    };
    assert!(args_format.is_some() && private && observable && observe.is_none());
    let Message::RegisterFunction {
        args_format,
        private,
        observable,
        observe,
        ..
    } = message("registerfunction")
    else {
        panic!("registerfunction");
    };
    assert!(args_format.is_none() && !private && observable && observe.is_none());
    let Message::RegisterFunction { observable, .. } = message("registerfunction-not-observable")
    else {
        panic!("registerfunction-not-observable");
    };
    assert!(!observable);
    let Message::RegisterFunction { observe, .. } = message("registerfunction-v1-owner") else {
        panic!("registerfunction-v1-owner");
    };
    assert!(!observe.expect("the 0.25 object").is_supported());
    let Message::RegisterFunction { observe, .. } = message("registerfunction-v1-not-observable")
    else {
        panic!("registerfunction-v1-not-observable");
    };
    let observe = observe.expect("the 0.25 object");
    assert!(observe.is_supported() && !observe.allow());

    for (name, code) in [
        ("registrationrejected-unsupported", "UNSUPPORTED"),
        (
            "registrationrejected-invalid-args-format",
            "INVALID_ARGS_FORMAT",
        ),
    ] {
        let Message::RegistrationRejected {
            code: got,
            owner_worker_id,
            ..
        } = message(name)
        else {
            panic!("{name}");
        };
        assert_eq!(got, code);
        assert!(owner_worker_id.is_empty(), "{name}");
    }

    // How each answer says its routed call is delivered (decision 55):
    // `None` not narrowed, `Some(n)` narrowed to n observers.
    for (name, matched, delivered) in [
        ("invocationresult-routed", None, false),
        ("invocationresult-matched", Some(2), false),
        ("invocationresult-matched-error", Some(1), false),
        ("invocationresult-matched-none", Some(0), false),
        ("invocationresult-delivered", None, true),
        ("invocationresult", None, false),
        ("invocationresult-not-function-owner", None, false),
    ] {
        let Message::InvocationResult {
            matched: ids,
            delivered: by_host,
            error,
            ..
        } = message(name)
        else {
            panic!("{name}");
        };
        assert_eq!(ids.as_ref().map(Vec::len), matched, "{name}");
        assert_eq!(by_host, delivered, "{name}");
        assert_eq!(
            error.is_some(),
            name.ends_with("error") || name.ends_with("owner"),
            "{name}"
        );
    }

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
    } = message("invokefunction-observer-call-namespace")
    else {
        panic!("invokefunction-observer-call-namespace");
    };
    assert!(
        namespace.is_some() && activation.is_some(),
        "D13: a namespace the engine ignores"
    );

    let Message::InvokeFunction {
        observe_depth,
        activation,
        ..
    } = message("invokefunction-observer-call")
    else {
        panic!("invokefunction-observer-call");
    };
    assert_eq!(observe_depth, None, "depth 0 is omitted");
    assert!(activation.expect("activation").activation_id.is_none());

    let Message::InvokeFunction {
        invocation_id,
        action,
        activation,
        ..
    } = message("invokefunction-observer-call-void")
    else {
        panic!("invokefunction-observer-call-void");
    };
    assert!(invocation_id.is_none() && action.is_some() && activation.is_some());

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
    assert_eq!(
        features,
        [
            FEATURE_OBSERVE_V1,
            FEATURE_OBSERVE_V2,
            FEATURE_LOCAL_DISPATCH_V1
        ]
    );
    assert_eq!(features, engine_features(), "what this engine advertises");
    let Message::WorkerRegistered { features, .. } = message("workerregistered-v1") else {
        panic!("workerregistered-v1");
    };
    assert_eq!(
        features,
        [FEATURE_OBSERVE_V1, FEATURE_LOCAL_DISPATCH_V1],
        "a 0.25 engine: no observe.v2"
    );
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
            _ => {}
        }
    }
}

/// SDKs declare their features in the `engine::workers::register` payload:
/// `observe.v1`, `observe.v2`, `local-dispatch.v1`, plus `large-messages.v1`,
/// which only workers declare. The engine reads them from what the fixture
/// sends (plus the `_caller_worker_id` it injects into every forwarded
/// payload).
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
    assert_eq!(
        input.features,
        [
            FEATURE_OBSERVE_V1,
            FEATURE_OBSERVE_V2,
            FEATURE_LOCAL_DISPATCH_V1,
            FEATURE_LARGE_MESSAGES_V1
        ]
    );
}
