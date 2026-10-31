// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `configuration` worker exercising the
//! register / set / get / list / schema surface, the observers of
//! `configuration::on-updated` (with `${VAR:default}` expansion), and the
//! file-watcher surfacing external edits as `configuration:updated` events.
//!
//! Modeled on `engine/tests/state_stream_update_e2e.rs` — drives the
//! worker through its public function surface against a real `FsAdapter`
//! pointed at a `tempfile::tempdir()`. No engine boot, no WebSocket, no
//! subprocess. Anything that needs the real engine routing is covered
//! by the unit tests inside `configuration.rs`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;

use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::observer::{EngineObserver, events::CONFIGURATION_ON_UPDATED};
use iii::protocol::{DEFAULT_NAMESPACE, Deliver};
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::{
    ConfigurationEnsureInput, ConfigurationGetInput, ConfigurationListInput,
    ConfigurationRegisterInput, ConfigurationSetInput,
};
use iii::workers::traits::Worker;

/// Build a `ConfigurationWorker` backed by a real `FsAdapter` rooted at `dir`
/// for direct, in-process end-to-end testing (no engine boot, no WebSocket).
async fn build_worker(dir: &std::path::Path) -> (Arc<Engine>, ConfigurationWorker) {
    iii::workers::observability::metrics::ensure_default_meter();
    let adapter = Arc::new(
        FsAdapter::new(Some(json!({ "directory": dir.to_str().unwrap() })))
            .await
            .expect("fs adapter"),
    ) as Arc<dyn ConfigurationAdapter>;
    let engine = Arc::new(Engine::new());
    let worker = ConfigurationWorker::for_test(engine.clone(), adapter);
    iii::observer::events::register_configuration_event_source(&engine);
    (engine, worker)
}

/// An observer of `configuration::on-updated` delivering each event of the
/// entry `configuration_id` (every entry for `None`) to `target`.
fn observe_updates(engine: &Arc<Engine>, target: (&str, &str), configuration_id: Option<&str>) {
    engine
        .observers
        .observe_as_engine(EngineObserver {
            id: format!("test-observer:{}:{}", target.0, target.1),
            function: (
                DEFAULT_NAMESPACE.to_string(),
                CONFIGURATION_ON_UPDATED.to_string(),
            ),
            args: json!({}),
            filters: configuration_id.map(|id| json!({ "input.id": { "$eq": id } })),
            target: (target.0.to_string(), target.1.to_string()),
            deliver: Deliver::Input,
            metadata: None,
        })
        .expect("observe configuration::on-updated");
}

/// Subscribe a fresh handler that forwards every received event payload
/// through an mpsc channel. Returns the receiver and the function id.
fn install_event_capture(
    engine: &Arc<Engine>,
    function_id: &'static str,
) -> mpsc::UnboundedReceiver<Value> {
    let (tx, rx) = mpsc::unbounded_channel::<Value>();
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |input: Value| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(input);
                FunctionResult::Success(None)
            }
        }),
    );
    rx
}

#[tokio::test]
async fn register_set_get_round_trip_with_env_var_expansion() {
    let dir = tempfile::tempdir().unwrap();
    let (_engine, worker) = build_worker(dir.path()).await;

    unsafe {
        std::env::set_var("CFG_E2E_HOST", "expanded.local");
    }

    let registered = worker
        .register_fn(ConfigurationRegisterInput {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "Connection settings".into(),
            schema: json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string" },
                    "port": { "type": "integer" }
                },
                "required": ["host"]
            }),
            initial_value: Some(json!({
                "host": "${CFG_E2E_HOST:fallback}",
                "port": 3112
            })),
            metadata: None,
        })
        .await;
    match registered {
        FunctionResult::Success(entry) => {
            assert_eq!(entry.value["host"], "${CFG_E2E_HOST:fallback}");
        }
        _ => panic!("expected register success"),
    }

    let read = worker
        .get_fn(ConfigurationGetInput {
            id: "iii-stream".into(),
            raw: false,
        })
        .await;
    match read {
        FunctionResult::Success(out) => {
            assert_eq!(out.value["host"], "expanded.local");
            assert_eq!(out.value["port"], 3112);
        }
        _ => panic!("expected get success"),
    }

    let set = worker
        .set_fn(ConfigurationSetInput {
            flush: true,
            id: "iii-stream".into(),
            value: json!({ "host": "${CFG_E2E_HOST:fallback}", "port": 4242 }),
        })
        .await;
    assert!(matches!(set, FunctionResult::Success(_)));

    let listed = worker.list_fn(ConfigurationListInput {}).await;
    match listed {
        FunctionResult::Success(out) => {
            assert_eq!(out.configurations.len(), 1);
            assert_eq!(out.configurations[0].id, "iii-stream");
        }
        _ => panic!("expected list success"),
    }
}

/// Registers a handler under `function_id` in `namespace` that records `tag`
/// into `recorder` when invoked.
fn register_recording_handler(
    engine: &Arc<Engine>,
    namespace: &str,
    function_id: &str,
    tag: &'static str,
    recorder: Arc<std::sync::Mutex<Vec<String>>>,
) {
    engine.register_function_handler_ns(
        namespace,
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |_input: Value| {
            let recorder = recorder.clone();
            async move {
                recorder.lock().unwrap().push(tag.to_string());
                FunctionResult::Success(None)
            }
        }),
    );
}

/// An observer's target runs in the namespace the observer names:
/// `cfg::react` exists in both `orders` ("from-orders") and `default`
/// ("from-default"), and an observer targeting the `orders` one runs only
/// that one. Drives the real mechanism: `worker.register_fn` emits
/// `configuration:registered`.
#[tokio::test]
async fn an_observer_target_runs_in_the_namespace_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path()).await;

    let fired = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    register_recording_handler(
        &engine,
        "orders",
        "cfg::react",
        "from-orders",
        fired.clone(),
    );
    register_recording_handler(
        &engine,
        "default",
        "cfg::react",
        "from-default",
        fired.clone(),
    );
    observe_updates(&engine, ("orders", "cfg::react"), Some("iii-stream"));

    worker
        .register_fn(ConfigurationRegisterInput {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "...".into(),
            schema: json!({ "type": "object", "properties": { "host": { "type": "string" } } }),
            initial_value: Some(json!({ "host": "h" })),
            metadata: None,
        })
        .await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    let fired = fired.lock().unwrap().clone();
    assert_eq!(
        fired,
        vec!["from-orders".to_string()],
        "the observer must run the orders target; got: {fired:?}"
    );
}

#[tokio::test]
async fn observers_get_the_expanded_event_payload() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path()).await;

    unsafe {
        std::env::set_var("CFG_E2E_OBSERVED_HOST", "observed.local");
    }

    let mut events = install_event_capture(&engine, "test::on_configuration_change");

    observe_updates(
        &engine,
        (DEFAULT_NAMESPACE, "test::on_configuration_change"),
        Some("iii-stream"),
    );

    worker
        .register_fn(ConfigurationRegisterInput {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "...".into(),
            schema: json!({
                "type": "object",
                "properties": { "host": { "type": "string" } }
            }),
            initial_value: Some(json!({ "host": "${CFG_E2E_OBSERVED_HOST:fallback}" })),
            metadata: None,
        })
        .await;

    let payload = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("the observer should run")
        .expect("channel open");
    assert_eq!(payload["type"], "configuration");
    assert_eq!(payload["event_type"], "configuration:registered");
    assert_eq!(payload["id"], "iii-stream");
    assert_eq!(payload["new_value"]["host"], "observed.local");
    assert!(payload["old_value"].is_null());

    worker
        .set_fn(ConfigurationSetInput {
            flush: true,
            id: "iii-stream".into(),
            value: json!({ "host": "set.local" }),
        })
        .await;
    let payload = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("set should reach the observer")
        .expect("channel open");
    assert_eq!(payload["event_type"], "configuration:updated");
    assert_eq!(payload["new_value"]["host"], "set.local");
}

#[tokio::test]
async fn fs_watcher_surfaces_external_file_edits_as_updates() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path()).await;

    let mut events = install_event_capture(&engine, "test::on_external_change");
    observe_updates(
        &engine,
        (DEFAULT_NAMESPACE, "test::on_external_change"),
        Some("iii-bridge"),
    );

    // Boot the worker watcher so external file edits are picked up.
    worker.initialize().await.unwrap();

    let entry = iii::workers::configuration::structs::ConfigurationEntry {
        id: "iii-bridge".into(),
        name: "Bridge".into(),
        description: "Test fixture".into(),
        schema: json!({ "type": "object" }),
        value: json!({ "url": "ws://primary" }),
        metadata: None,
    };
    let yaml = serde_yaml::to_string(&entry).unwrap();
    tokio::fs::write(dir.path().join("iii-bridge.yaml"), yaml)
        .await
        .unwrap();

    let payload = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("file watcher should reach the observer")
        .expect("channel open");
    assert_eq!(payload["event_type"], "configuration:registered");
    assert_eq!(payload["id"], "iii-bridge");
    assert_eq!(payload["new_value"]["url"], "ws://primary");

    worker.destroy().await.expect("destroy");
}

#[tokio::test]
/// Verify the routed ensure contract, first-registration event and preservation of later values.
async fn ensure_seeds_once_then_preserves_and_fires_registered_event() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path()).await;

    let mut events = install_event_capture(&engine, "test::on_ensure");
    observe_updates(
        &engine,
        (DEFAULT_NAMESPACE, "test::on_ensure"),
        Some("iii-stream"),
    );

    let schema = json!({
        "type": "object",
        "required": ["port"],
        "properties": { "port": { "type": "integer" } }
    });

    // First ensure: no stored value -> seeds and fires configuration:registered.
    let seeded = worker
        .ensure_fn(ConfigurationEnsureInput {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "first".into(),
            schema: schema.clone(),
            initial_value: Some(json!({ "port": 3112 })),
            metadata: None,
        })
        .await;
    match seeded {
        FunctionResult::Success(out) => assert_eq!(out.entry.value, json!({ "port": 3112 })),
        _ => panic!("expected ensure seed success"),
    }

    let evt = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("ensure should reach the observer")
        .expect("channel open");
    assert_eq!(evt["event_type"], "configuration:registered");
    assert_eq!(evt["new_value"]["port"], 3112);

    // Second ensure with a DIFFERENT seed: the stored value must be preserved.
    worker
        .ensure_fn(ConfigurationEnsureInput {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "second".into(),
            schema: schema.clone(),
            initial_value: Some(json!({ "port": 9999 })),
            metadata: None,
        })
        .await;

    let read = worker
        .get_fn(ConfigurationGetInput {
            id: "iii-stream".into(),
            raw: false,
        })
        .await;
    match read {
        FunctionResult::Success(out) => assert_eq!(out.value["port"], 3112),
        _ => panic!("expected get success"),
    }

    // An explicit set still overrides after seeding.
    let set = worker
        .set_fn(ConfigurationSetInput {
            flush: true,
            id: "iii-stream".into(),
            value: json!({ "port": 4242 }),
        })
        .await;
    assert!(matches!(set, FunctionResult::Success(_)));
    let read2 = worker
        .get_fn(ConfigurationGetInput {
            id: "iii-stream".into(),
            raw: false,
        })
        .await;
    match read2 {
        FunctionResult::Success(out) => assert_eq!(out.value["port"], 4242),
        _ => panic!("expected get success after set"),
    }
}

/// Migration changes ids, not values; subscribers and disk reload observe the move.
#[tokio::test]
async fn migration_events_and_restart_preserve_the_entry() {
    use iii::workers::configuration::structs::{ConfigurationMigrateInput, MigrateAction};
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path()).await;
    worker.initialize().await.unwrap();
    let original = json!({ "token": "${TOKEN}", "enabled": false, "count": 0, "empty": null });
    assert!(matches!(
        worker
            .register_fn(ConfigurationRegisterInput {
                id: "default-harness-a14f3656efb8d5ea".into(),
                name: "Manual name".into(),
                description: "Manual description".into(),
                schema: json!({}),
                initial_value: Some(original.clone()),
                metadata: Some(json!({"manual": true})),
            })
            .await,
        FunctionResult::Success(_)
    ));
    let active = json!({"token": "runtime-only", "enabled": true});
    for (id, value) in [
        ("default-harness-a14f3656efb8d5ea", active.clone()),
        ("default-harness", json!({"stale_destination": true})),
    ] {
        assert!(matches!(
            worker
                .set_fn(ConfigurationSetInput {
                    id: id.into(),
                    value,
                    flush: false,
                })
                .await,
            FunctionResult::Success(_)
        ));
    }
    let mut events = install_event_capture(&engine, "test::migration_events");
    observe_updates(&engine, (DEFAULT_NAMESPACE, "test::migration_events"), None);
    let input = ConfigurationMigrateInput {
        from_id: "default-harness-a14f3656efb8d5ea".into(),
        to_id: "default-harness".into(),
    };
    let FunctionResult::Success(out) = worker.migrate_fn(input.clone()).await else {
        panic!("migration failed")
    };
    assert_eq!(out.action, MigrateAction::Migrated);
    let entry = out.entry.unwrap();
    assert_eq!(entry.value, original);
    assert_eq!(entry.metadata, Some(json!({"manual": true})));
    match worker
        .get_fn(ConfigurationGetInput {
            id: input.from_id.clone(),
            raw: true,
        })
        .await
    {
        FunctionResult::Failure(error) => assert_eq!(error.code, "NOT_FOUND"),
        _ => panic!("the retired id must not remain readable through runtime memory"),
    }
    let FunctionResult::Success(current) = worker
        .get_fn(ConfigurationGetInput {
            id: input.to_id.clone(),
            raw: true,
        })
        .await
    else {
        panic!("the active value must follow the destination")
    };
    assert_eq!(current.value, active);
    let mut observed = Vec::new();
    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(3), events.recv())
            .await
            .unwrap()
            .unwrap();
        if event["event_type"] == "configuration:registered" {
            assert_eq!(event["new_value"], active, "event must agree with GET");
        }
        observed.push((
            event["id"].as_str().unwrap().to_string(),
            event["event_type"].as_str().unwrap().to_string(),
        ));
    }
    observed.sort();
    assert_eq!(
        observed,
        vec![
            ("default-harness".into(), "configuration:registered".into()),
            (input.from_id.clone(), "configuration:deleted".into())
        ]
    );
    let FunctionResult::Success(out) = worker.migrate_fn(input).await else {
        panic!("repeat failed")
    };
    assert_eq!(out.action, MigrateAction::Preserved);
    assert!(
        tokio::time::timeout(Duration::from_millis(1100), events.recv())
            .await
            .is_err()
    );
    worker.destroy().await.unwrap();
    let (_, restarted) = build_worker(dir.path()).await;
    restarted.initialize().await.unwrap();
    let FunctionResult::Success(raw) = restarted
        .get_fn(ConfigurationGetInput {
            id: "default-harness".into(),
            raw: true,
        })
        .await
    else {
        panic!("reload failed")
    };
    assert_eq!(raw.value, original);
    restarted.destroy().await.unwrap();
}
