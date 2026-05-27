// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test for the `configuration` worker exercising the
//! register / set / get / list / schema surface, the `configuration`
//! trigger fan-out (with `${VAR:default}` expansion), and the file-watcher
//! surfacing external edits as `configuration:updated` events.
//!
//! Modeled on `engine/tests/state_stream_update_e2e.rs` — drives the
//! worker through its public function surface against a real `FsAdapter`
//! pointed at a `tempfile::tempdir()`. No engine boot, no WebSocket, no
//! subprocess. Anything that needs the real engine routing is covered
//! by the unit tests inside `configuration.rs` and `trigger.rs`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;

use iii::engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest};
use iii::function::FunctionResult;
use iii::trigger::{Trigger, TriggerRegistrator};
use iii::workers::configuration::ConfigurationWorker;
use iii::workers::configuration::adapters::ConfigurationAdapter;
use iii::workers::configuration::adapters::fs::FsAdapter;
use iii::workers::configuration::structs::{
    ConfigurationGetInput, ConfigurationListInput, ConfigurationRegisterInput,
    ConfigurationSetInput,
};
use iii::workers::traits::Worker;

async fn build_worker(
    dir: &std::path::Path,
    ttl_seconds: u64,
) -> (Arc<Engine>, ConfigurationWorker) {
    iii::workers::observability::metrics::ensure_default_meter();
    let adapter = Arc::new(
        FsAdapter::new(Some(json!({ "directory": dir.to_str().unwrap() })))
            .await
            .expect("fs adapter"),
    ) as Arc<dyn ConfigurationAdapter>;
    let engine = Arc::new(Engine::new());
    let worker = ConfigurationWorker::for_test(engine.clone(), adapter, ttl_seconds);
    (engine, worker)
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
    let (_engine, worker) = build_worker(dir.path(), 0).await;

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

#[tokio::test]
async fn trigger_fan_out_delivers_expanded_event_payload() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path(), 0).await;

    unsafe {
        std::env::set_var("CFG_E2E_TRIGGER_HOST", "trigger.local");
    }

    let mut events = install_event_capture(&engine, "test::on_configuration_change");

    let trigger = Trigger {
        id: "trig-1".into(),
        trigger_type: "configuration".into(),
        function_id: "test::on_configuration_change".into(),
        config: json!({ "configuration_id": "iii-stream" }),
        worker_id: None,
        metadata: None,
    };
    worker
        .register_trigger(trigger.clone())
        .await
        .expect("register configuration trigger");

    worker
        .register_fn(ConfigurationRegisterInput {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "...".into(),
            schema: json!({
                "type": "object",
                "properties": { "host": { "type": "string" } }
            }),
            initial_value: Some(json!({ "host": "${CFG_E2E_TRIGGER_HOST:fallback}" })),
            metadata: None,
        })
        .await;

    let payload = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("trigger should fire")
        .expect("channel open");
    assert_eq!(payload["type"], "configuration");
    assert_eq!(payload["event_type"], "configuration:registered");
    assert_eq!(payload["id"], "iii-stream");
    assert_eq!(payload["new_value"]["host"], "trigger.local");
    assert!(payload["old_value"].is_null());

    worker
        .set_fn(ConfigurationSetInput {
            id: "iii-stream".into(),
            value: json!({ "host": "set.local" }),
        })
        .await;
    let payload = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("set should fire trigger")
        .expect("channel open");
    assert_eq!(payload["event_type"], "configuration:updated");
    assert_eq!(payload["new_value"]["host"], "set.local");
}

#[tokio::test]
async fn fs_watcher_surfaces_external_file_edits_as_updates() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, worker) = build_worker(dir.path(), 0).await;

    let mut events = install_event_capture(&engine, "test::on_external_change");
    worker
        .register_trigger(Trigger {
            id: "trig-watch".into(),
            trigger_type: "configuration".into(),
            function_id: "test::on_external_change".into(),
            config: json!({ "configuration_id": "iii-bridge" }),
            worker_id: None,
            metadata: None,
        })
        .await
        .unwrap();

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
        .expect("file watcher should fire trigger")
        .expect("channel open");
    assert_eq!(payload["event_type"], "configuration:registered");
    assert_eq!(payload["id"], "iii-bridge");
    assert_eq!(payload["new_value"]["url"], "ws://primary");

    worker.destroy().await.expect("destroy");
}

#[tokio::test]
async fn ttl_cleanup_removes_configuration_after_last_trigger_unregistered() {
    let dir = tempfile::tempdir().unwrap();
    // 1-second TTL keeps the test fast while exercising the real
    // tokio::time::sleep cleanup path.
    let (engine, worker) = build_worker(dir.path(), 1).await;

    let mut events = install_event_capture(&engine, "test::on_ttl_change");

    let trigger = Trigger {
        id: "trig-ttl".into(),
        trigger_type: "configuration".into(),
        function_id: "test::on_ttl_change".into(),
        config: json!({ "configuration_id": "ephemeral" }),
        worker_id: None,
        metadata: None,
    };
    worker.register_trigger(trigger.clone()).await.unwrap();

    worker
        .register_fn(ConfigurationRegisterInput {
            id: "ephemeral".into(),
            name: "Ephemeral".into(),
            description: "Used by a worker that comes and goes.".into(),
            schema: json!({ "type": "object" }),
            initial_value: Some(json!({})),
            metadata: None,
        })
        .await;
    let _registered_evt = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("register fires trigger")
        .expect("channel open");

    worker.unregister_trigger(trigger).await.unwrap();

    // Poll the public function surface until the entry vanishes or the
    // deadline elapses. The cleanup task runs on tokio's real-time timer
    // because the worker spawns it via `tokio::spawn`.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let after = worker
            .get_fn(ConfigurationGetInput {
                id: "ephemeral".into(),
                raw: false,
            })
            .await;
        if matches!(after, FunctionResult::Failure(_)) {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("ephemeral configuration should have been TTL-deleted");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
