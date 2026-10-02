//! real engine routing and main WebSocket transport, Rust, Node, Python and Go SDKs.
//! Opt-in cross-language integration; run from the repository root:
//! ```sh
//! pnpm install --frozen-lockfile --ignore-scripts
//! pnpm --dir sdk/packages/node/helpers build
//! uv sync --project sdk/packages/python/iii --extra dev
//! # Go >= 1.24 must be on PATH (or set JSON_SIZE_TEST_GO to its verified binary).
//! cargo test -p iii --test json_size_sdk_e2e --locked -- --ignored --nocapture
//! ```
//! The default Rust/engine-coverage gate does not provision these prerequisites.
//! Explicit execution requires them and fails on missing dependencies; no silent skip.
use futures_util::{SinkExt, StreamExt};
use iii::{EngineBuilder, engine::EngineTrait};
use iii_helpers::observability::OtelConfig;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Participant(Child);
impl Drop for Participant {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Outbound JSON envelope: 16 MiB (16,777,216 bytes).
const JSON_FRAME_LIMIT_BYTES: usize = 16 * 1024 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires pnpm dependencies, built Node helpers uv Python dev .venv and Go >= 1.24; see module setup/run commands"]
async fn oversized_json_preserves_connections_across_sdks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let engine_log = dir.path().join("engine.log");
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::sync::Arc::new(
            std::fs::File::create(&engine_log).unwrap(),
        ))
        .try_init()
        .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let builder = EngineBuilder::new()
        .add_worker(
            "configuration",
            Some(json!({"adapter": {"name": "fs", "config": {"directory": dir.path()}}})),
        )
        .add_worker("iii-stream", Some(json!({"port": 0})))
        .add_worker(
            "iii-worker-manager",
            Some(json!({"host": "127.0.0.1", "port": port})),
        )
        .build()
        .await
        .unwrap();
    for worker in builder.running() {
        worker
            .worker
            .start_background_tasks(worker.shutdown_tx.subscribe(), worker.shutdown_tx.clone())
            .await
            .unwrap();
    }
    let url = format!("ws://127.0.0.1:{port}");
    let sdk = register_worker(
        &url,
        InitOptions {
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    sdk.register_function(
        "size::rust",
        RegisterFunction::new_async(|input: Value| async move {
            Ok(Value::String(
                "x".repeat(input["size"].as_u64().unwrap() as usize),
            ))
        }),
    );
    sdk.register_function(
        "size::rust-length",
        RegisterFunction::new_async(|input: Value| async move {
            Ok(json!(input.as_str().unwrap().len()))
        }),
    );
    let node_log = dir.path().join("node.log");
    let py_log = dir.path().join("python.log");
    let mut node = Participant(
        Command::new("pnpm")
            .args([
                "exec",
                "tsx",
                "sdk/packages/node/iii/tests/json-size-engine-participant.ts",
            ])
            .current_dir(root)
            .env("JSON_SIZE_TEST_URL", &url)
            .stdout(Stdio::from(std::fs::File::create(&node_log).unwrap()))
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut python = Participant(
        Command::new("sdk/packages/python/iii/.venv/bin/python")
            .arg("sdk/packages/python/iii/tests/json_size_engine_participant.py")
            .current_dir(root)
            .env("JSON_SIZE_TEST_URL", &url)
            .stdout(Stdio::from(std::fs::File::create(&py_log).unwrap()))
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let go_log = dir.path().join("go.log");
    let mut go = Participant(
        Command::new(std::env::var("JSON_SIZE_TEST_GO").unwrap_or_else(|_| "go".into()))
            .args(["run", "./tests/json-size-engine-participant"])
            .current_dir(root.join("sdk/packages/go/iii"))
            .env("JSON_SIZE_TEST_URL", &url)
            .stdout(Stdio::from(std::fs::File::create(&go_log).unwrap()))
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    for target in ["size::node", "size::python", "size::rust", "size::go"] {
        let mut ready = false;
        for _ in 0..50 {
            if sdk
                .trigger(TriggerRequest {
                    function_id: target.into(),
                    payload: json!({"size": 1}),
                    timeout_ms: Some(1000),
                    action: None,
                })
                .await
                .is_ok()
            {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(ready, "{target} never registered");
        let workers_before = builder
            .engine()
            .call("engine::workers::list", json!({}))
            .await
            .unwrap();
        let error = sdk
            .trigger(TriggerRequest {
                function_id: target.into(),
                payload: json!({"size": JSON_FRAME_LIMIT_BYTES}),
                timeout_ms: Some(10000),
                action: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
        assert_eq!(
            sdk.trigger(TriggerRequest {
                function_id: target.into(),
                payload: json!({"size": 2}),
                timeout_ms: Some(10000),
                action: None
            })
            .await
            .unwrap(),
            json!("xx")
        );
        let workers_after = builder
            .engine()
            .call("engine::workers::list", json!({}))
            .await
            .unwrap();
        let identities = |workers: Option<Value>| {
            let value = workers.unwrap();
            let mut ids: Vec<String> = value["workers"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|worker| worker["id"].as_str().map(str::to_owned))
                .collect();
            ids.sort();
            ids
        };
        let before = identities(workers_before);
        assert!(!before.is_empty());
        assert_eq!(
            before,
            identities(workers_after),
            "worker identity changed after rejection"
        );
    }
    // Bypass the fixed SDK deliberately on a separate raw connection to exercise
    // the engine's capacity-error diagnostic, never the operator's live engine.
    let (mut raw, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let _ = raw.next().await;
    let rejected = format!(
        "{{\"secret\":\"JSON_SIZE_PRIVATE_MARKER{}\"}}",
        "x".repeat(JSON_FRAME_LIMIT_BYTES)
    );
    let _ = raw
        .send(tokio_tungstenite::tungstenite::Message::Text(
            rejected.into(),
        ))
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let diagnostic = std::fs::read_to_string(&engine_log).unwrap();
    assert!(
        diagnostic.contains("Worker WebSocket read failed"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("worker_id="));
    assert!(diagnostic.contains("16777216"));
    assert!(diagnostic.contains("Message too long"));
    assert!(!diagnostic.contains("JSON_SIZE_PRIVATE_MARKER"));
    for _ in 0..120 {
        if node.0.try_wait().unwrap().is_some()
            && python.0.try_wait().unwrap().is_some()
            && go.0.try_wait().unwrap().is_some()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let node_output = std::fs::read_to_string(node_log).unwrap();
    let py_output = std::fs::read_to_string(py_log).unwrap();
    println!("{node_output}\n{py_output}");
    assert!(node_output.contains("JSON_SIZE_NODE_CROSS_SDK_OK"));
    assert!(py_output.contains("JSON_SIZE_PYTHON_CROSS_SDK_OK"));
    assert!(node.0.try_wait().unwrap().unwrap().success());
    assert!(python.0.try_wait().unwrap().unwrap().success());
    let go_output = std::fs::read_to_string(go_log).unwrap();
    println!("{go_output}");
    assert!(go_output.contains("JSON_SIZE_GO_CROSS_SDK_OK"));
    assert!(go.0.try_wait().unwrap().unwrap().success());
    sdk.shutdown_async().await;
    builder.destroy().await.unwrap();
}
