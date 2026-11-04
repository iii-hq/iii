//! The observe handshake over a real WebSocket connection: the engine
//! advertises its features in `workerregistered`, records the features a
//! worker declares on `engine::workers::register` (and sizes what it sends the
//! worker by them), and drops the 0.25 trigger messages without closing the
//! connection.

mod observe_support;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::{Engine, EngineTrait};
use iii::protocol::{DEFAULT_NAMESPACE, ENGINE_FEATURES};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{Message as WsMessage, protocol::WebSocketConfig};
use uuid::Uuid;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Boots the engine-functions module (which owns `engine::workers::register`)
/// and the `WorkerManager` router on a free port.
async fn spawn_engine() -> (u16, Arc<Engine>) {
    let t = observe_support::spawn_engine(Default::default()).await;
    (t.port, t.engine)
}

async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

async fn connect(port: u16) -> (Ws, Value) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("connect");
    let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("workerregistered arrives")
        .expect("stream yields")
        .expect("frame decodes");
    let registered: Value =
        serde_json::from_str(frame.to_text().expect("text frame")).expect("json");
    assert_eq!(registered["type"], "workerregistered");
    (ws, registered)
}

async fn send(ws: &mut Ws, frame: Value) {
    ws.send(WsMessage::Text(frame.to_string().into()))
        .await
        .expect("send");
}

fn workers_register(features: Option<Value>) -> Value {
    let mut data = json!({ "runtime": "node", "version": "0.25.0", "name": "observe-e2e" });
    if let Some(features) = features {
        data["features"] = features;
    }
    json!({
        "type": "invokefunction",
        "function_id": "engine::workers::register",
        "data": data,
        "action": { "type": "void" },
    })
}

#[tokio::test]
async fn the_engine_advertises_its_features_and_records_the_workers() {
    let (port, engine) = spawn_engine().await;
    let (mut ws, registered) = connect(port).await;

    assert_eq!(
        registered["features"],
        json!(ENGINE_FEATURES),
        "workerregistered lists what the engine supports"
    );
    assert_eq!(
        registered["features"],
        json!(["observe.v1", "observe.v2", "local-dispatch.v1"])
    );
    let worker_id: Uuid = registered["worker_id"]
        .as_str()
        .expect("worker_id")
        .parse()
        .expect("uuid");

    send(
        &mut ws,
        workers_register(Some(json!(["observe.v1", "local-dispatch.v1"]))),
    )
    .await;

    assert!(
        eventually(|| {
            engine
                .worker_registry
                .get_worker(&worker_id)
                .is_some_and(|w| w.features.observe_v1() && w.features.local_dispatch_v1())
        })
        .await,
        "the declared features are recorded on the connection"
    );
    assert!(
        !engine
            .worker_registry
            .get_worker(&worker_id)
            .unwrap()
            .features
            .observe_v2(),
        "observe.v1 alone: the engine evaluates this host's routed calls"
    );
}

/// A v2 SDK declares `observe.v1` and `observe.v2`: it delivers the calls of
/// the functions it hosts.
#[tokio::test]
async fn a_worker_declaring_observe_v2_delivers_its_own_calls() {
    let (port, engine) = spawn_engine().await;
    let (mut ws, registered) = connect(port).await;
    let worker_id: Uuid = registered["worker_id"].as_str().unwrap().parse().unwrap();

    send(
        &mut ws,
        workers_register(Some(json!([
            "observe.v1",
            "observe.v2",
            "local-dispatch.v1",
            "large-messages.v1"
        ]))),
    )
    .await;

    assert!(
        eventually(|| {
            engine
                .worker_registry
                .get_worker(&worker_id)
                .is_some_and(|w| w.features.observe_v1() && w.features.observe_v2())
        })
        .await
    );
}

#[tokio::test]
async fn a_worker_without_features_registers_as_before() {
    let (port, engine) = spawn_engine().await;
    let (mut ws, registered) = connect(port).await;
    let worker_id: Uuid = registered["worker_id"].as_str().unwrap().parse().unwrap();

    send(&mut ws, workers_register(None)).await;

    assert!(
        eventually(|| {
            engine
                .worker_registry
                .get_worker(&worker_id)
                .is_some_and(|w| w.name.as_deref() == Some("observe-e2e"))
        })
        .await,
        "the registration itself succeeds"
    );
    let worker = engine.worker_registry.get_worker(&worker_id).unwrap();
    assert!(!worker.features.observe_v1() && !worker.features.local_dispatch_v1());
}

/// A malformed `features` value must not cost the worker its registration:
/// the namespace rides on the same call.
#[tokio::test]
async fn malformed_features_do_not_break_the_registration() {
    let (port, engine) = spawn_engine().await;
    let (mut ws, registered) = connect(port).await;
    let worker_id: Uuid = registered["worker_id"].as_str().unwrap().parse().unwrap();

    send(&mut ws, workers_register(Some(json!("observe.v1")))).await;

    assert!(
        eventually(|| {
            engine
                .worker_registry
                .get_worker(&worker_id)
                .is_some_and(|w| w.name.as_deref() == Some("observe-e2e"))
        })
        .await
    );
    assert!(
        !engine
            .worker_registry
            .get_worker(&worker_id)
            .unwrap()
            .features
            .observe_v1()
    );
}

/// The trigger messages a 0.25 SDK sends are dropped, unanswered, and the
/// connection keeps being served.
#[tokio::test]
async fn removed_messages_are_dropped_and_the_connection_stays_open() {
    let (port, engine) = spawn_engine().await;
    let (mut ws, _) = connect(port).await;
    send(&mut ws, workers_register(None)).await;

    for frame in [
        json!({ "type": "registertriggertype", "id": "webhook", "description": "e2e" }),
        json!({ "type": "registertrigger", "id": "t-1", "trigger_type": "webhook",
                "function_id": "observe::probe", "config": {} }),
        json!({ "type": "registertrigger", "id": "t-2", "trigger_type": "webhook",
                "function_id": "observe::probe", "config": {} }),
        json!({ "type": "unregistertrigger", "id": "t-1" }),
        json!({ "type": "unregistertriggertype", "id": "webhook" }),
    ] {
        send(&mut ws, frame).await;
    }
    send(
        &mut ws,
        json!({ "type": "registerfunction", "id": "observe::probe" }),
    )
    .await;
    assert!(
        eventually(|| engine
            .functions
            .get(DEFAULT_NAMESPACE, "observe::probe")
            .is_some())
        .await,
        "the connection is served normally after the dropped frames"
    );

    send(&mut ws, json!({ "type": "ping" })).await;
    let reply = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("an answer")
        .expect("stream yields")
        .expect("frame decodes");
    let reply: Value = serde_json::from_str(reply.to_text().expect("text frame")).expect("json");
    assert_eq!(
        reply["type"], "pong",
        "nothing answered the dropped frames: the next frame is the pong"
    );
}

/// Over one 16 MiB WebSocket frame, within the 64 MiB that SDK 0.24.5 and
/// later receive.
const OVER_ONE_FRAME: usize = 20 * 1024 * 1024;
const SDK_RECEIVE_LIMIT: usize = 64 * 1024 * 1024;

/// A worker announced with `data` (its `engine::workers::register` payload)
/// that serves `function_id` by answering with the length of the string it
/// receives. Its socket accepts what SDK 0.24.5 and later accept, so only the
/// engine's receive limit for it decides whether a large call arrives.
async fn size_worker(port: u16, function_id: &str, data: Value) -> tokio::task::JoinHandle<()> {
    let config = WebSocketConfig::default()
        .max_frame_size(Some(SDK_RECEIVE_LIMIT))
        .max_message_size(Some(SDK_RECEIVE_LIMIT));
    let (mut ws, _) = tokio_tungstenite::connect_async_with_config(
        format!("ws://127.0.0.1:{port}/"),
        Some(config),
        false,
    )
    .await
    .expect("connect");
    let registered = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("workerregistered arrives")
        .expect("stream yields")
        .expect("frame decodes");
    assert!(registered.to_text().unwrap().contains("workerregistered"));
    send(
        &mut ws,
        json!({
            "type": "invokefunction",
            "function_id": "engine::workers::register",
            "data": data,
            "action": { "type": "void" },
        }),
    )
    .await;
    send(
        &mut ws,
        json!({ "type": "registerfunction", "id": function_id }),
    )
    .await;
    let function_id = function_id.to_string();
    tokio::spawn(async move {
        while let Some(Ok(frame)) = ws.next().await {
            let Ok(text) = frame.to_text() else { continue };
            let Ok(message) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if message["type"] != "invokefunction" || message["function_id"] != function_id.as_str()
            {
                continue;
            }
            let size = message["data"].as_str().map_or(0, str::len);
            let result = json!({
                "type": "invocationresult",
                "invocation_id": message["invocation_id"],
                "function_id": function_id,
                "result": size,
            });
            send(&mut ws, result).await;
        }
    })
}

/// The engine sends a worker more than one 16 MiB frame only when the worker
/// can receive it: it declared `large-messages.v1`, or (SDK 0.24.5 to
/// 0.25.0-rc.5, which do not declare it) reports an SDK version of 0.24.5 or
/// later. A Rust worker reports its own crate version
/// (`WorkerMetadata.version`) and a Python pre-release its PyPI version
/// (`0.25.0rc5`), so the version alone does not identify the SDK.
#[tokio::test]
async fn the_receive_limit_follows_the_declared_sdk_not_the_reported_version() {
    let (port, engine) = spawn_engine().await;
    let sdk_0_25 = json!(["observe.v1", "local-dispatch.v1", "large-messages.v1"]);
    let rc = json!(["observe.v1", "local-dispatch.v1"]);
    let cases = [
        // Declares `large-messages.v1`, whatever version it reports.
        ("rust", Some("0.3.0"), Some(sdk_0_25.clone()), true),
        ("node", None, Some(json!(["large-messages.v1"])), true),
        // Other features do not count (SDK 0.25.0-rc.1 to rc.5 declare only
        // these): the version decides.
        ("rust", Some("0.3.0"), Some(rc.clone()), false),
        ("rust", Some("0.25.0-rc.5"), Some(rc), true),
        // No features (SDK 0.24.5, 0.24.6): the version must be the SDK's,
        // semver or PEP 440.
        ("python", Some("0.25.0rc5"), None, true),
        ("python", Some("0.24.5.post1"), None, true),
        ("rust", Some("0.24.6"), None, true),
        ("rust", Some("0.3.0"), None, false),
        ("python", Some("0.24.5rc1"), None, false),
        ("node", None, None, false),
        // A browser receives one 16 MiB frame at most.
        ("browser", Some("0.25.0"), Some(sdk_0_25), false),
    ];
    let payload = "x".repeat(OVER_ONE_FRAME);

    for (index, (runtime, version, features, receives)) in cases.into_iter().enumerate() {
        let case = format!("runtime={runtime} version={version:?} features={features:?}");
        let function_id = format!("limit::size-{index}");
        let mut data = json!({ "runtime": runtime, "name": format!("limit-{index}") });
        if let Some(version) = version {
            data["version"] = json!(version);
        }
        if let Some(features) = features {
            data["features"] = features;
        }
        let worker = size_worker(port, &function_id, data).await;
        assert!(
            eventually(|| engine
                .functions
                .get(DEFAULT_NAMESPACE, &function_id)
                .is_some())
            .await,
            "{case}: the worker registered its function"
        );

        let result = tokio::time::timeout(
            Duration::from_secs(30),
            engine.call(&function_id, payload.as_str()),
        )
        .await
        .unwrap_or_else(|_| panic!("{case}: the call completes"));
        if receives {
            match result {
                Ok(value) => assert_eq!(value, Some(json!(OVER_ONE_FRAME)), "{case}"),
                Err(error) => panic!("{case}: {}: {}", error.code, error.message),
            }
        } else {
            let error = result.expect_err(&case);
            assert_eq!(error.code, "payload_too_large", "{case}");
            assert!(
                error.message.contains("accepts up to 16777216 bytes"),
                "{case}: {}",
                error.message
            );
        }
        worker.abort();
    }
}
