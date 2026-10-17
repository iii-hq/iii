//! The observe handshake over a real WebSocket connection: the engine
//! advertises its features in `workerregistered`, records the features a
//! worker declares on `engine::workers::register`, and decodes the
//! `unregistertriggertype` every SDK has always sent.

mod observe_support;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use iii::engine::Engine;
use iii::protocol::{DEFAULT_NAMESPACE, ENGINE_FEATURES};
use iii::trigger::type_key;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;
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

/// Before 0.25 this frame failed to decode on every engine and the type
/// lingered until the provider disconnected.
#[tokio::test]
async fn a_provider_retires_its_trigger_type_over_the_wire() {
    let (port, engine) = spawn_engine().await;
    let (mut ws, _) = connect(port).await;
    send(&mut ws, workers_register(None)).await;

    send(
        &mut ws,
        json!({ "type": "registertriggertype", "id": "webhook", "description": "e2e" }),
    )
    .await;
    let key = type_key(DEFAULT_NAMESPACE, "webhook");
    assert!(
        eventually(|| engine.trigger_registry.trigger_types.contains_key(&key)).await,
        "the provider registered its type"
    );

    // What the Node SDK sends: the whole trigger type, minus the tag.
    send(
        &mut ws,
        json!({ "type": "unregistertriggertype", "id": "webhook", "description": "e2e" }),
    )
    .await;
    assert!(
        eventually(|| !engine.trigger_registry.trigger_types.contains_key(&key)).await,
        "the provider retired its type"
    );
}

#[tokio::test]
async fn another_connection_cannot_retire_a_trigger_type() {
    let (port, engine) = spawn_engine().await;
    let (mut provider, _) = connect(port).await;
    send(&mut provider, workers_register(None)).await;
    send(
        &mut provider,
        json!({ "type": "registertriggertype", "id": "webhook", "description": "e2e" }),
    )
    .await;
    let key = type_key(DEFAULT_NAMESPACE, "webhook");
    assert!(eventually(|| engine.trigger_registry.trigger_types.contains_key(&key)).await);

    let (mut other, _) = connect(port).await;
    // A different name: a second live `observe-e2e` would be refused as a
    // worker-name conflict and closed.
    let mut register = workers_register(None);
    register["data"]["name"] = json!("observe-e2e-other");
    send(&mut other, register).await;
    send(
        &mut other,
        json!({ "type": "unregistertriggertype", "id": "webhook" }),
    )
    .await;
    // Registrations (the unregister included) are applied in arrival order,
    // possibly from the namespace drain rather than the read loop, so a
    // function registered after it is the marker that it has been applied.
    send(
        &mut other,
        json!({ "type": "registerfunction", "id": "observe::probe" }),
    )
    .await;
    assert!(
        eventually(|| engine
            .functions
            .get(DEFAULT_NAMESPACE, "observe::probe")
            .is_some())
        .await,
        "the other connection is served normally"
    );
    assert!(
        engine.trigger_registry.trigger_types.contains_key(&key),
        "only the provider may retire its type"
    );
}
