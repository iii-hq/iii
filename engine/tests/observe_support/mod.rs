//! A raw WebSocket worker for the observe end-to-end suites: it speaks the
//! wire protocol directly, so every assertion is about frames the engine
//! actually sends.
//!
//! A reader task owns the socket's receiving half. By default it answers the
//! owner-sync messages like an SDK whose hooks accept everything: every
//! observer in `functionregistered` and every `observeradded` gets an
//! `observerack`. Owner tests turn that off ([`Client::manual_acks`]) and ack
//! by hand.

#![allow(dead_code)]

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use iii::engine::Engine;
use iii::observer::ObserversConfig;
use iii::workers::engine_fn::EngineFunctionsWorker;
use iii::workers::traits::Worker;
use iii::workers::worker::WorkerManager;
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

/// How long any single expectation waits. Generous: CI machines stall, and a
/// passing test never waits this long.
pub const WAIT: Duration = Duration::from_secs(5);

/// What a 0.25 SDK declares on `engine::workers::register`: the engine
/// evaluates its routed calls.
pub const OBSERVE_V1: &[&str] = &["observe.v1", "local-dispatch.v1"];

/// What a v2 SDK declares: it delivers the calls of the functions it hosts,
/// listing the observers it matched on each routed answer.
pub const OBSERVE_V2: &[&str] = &["observe.v1", "observe.v2", "local-dispatch.v1"];

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Sink = Arc<tokio::sync::Mutex<SplitSink<Ws, WsMessage>>>;

pub struct TestEngine {
    pub port: u16,
    pub engine: Arc<Engine>,
}

/// How a test engine is set up.
#[derive(Default)]
pub struct Setup {
    pub observers: ObserversConfig,
    /// The worker manager's `rbac` section.
    pub rbac: Option<Value>,
    /// The worker manager's `middleware_function_id`.
    pub middleware_function_id: Option<String>,
    /// `registration_namespace_grace_ms`.
    pub namespace_grace_ms: Option<u64>,
}

/// Boots the engine-functions module (`engine::workers::register`, the
/// observer builtins) and the worker manager on a free port.
pub async fn spawn_engine(config: ObserversConfig) -> TestEngine {
    spawn(Setup {
        observers: config,
        ..Setup::default()
    })
    .await
}

/// As [`spawn_engine`], with the worker manager's `rbac` section.
pub async fn spawn_engine_with(config: ObserversConfig, rbac: Option<Value>) -> TestEngine {
    spawn(Setup {
        observers: config,
        rbac,
        ..Setup::default()
    })
    .await
}

pub async fn spawn(setup: Setup) -> TestEngine {
    iii::workers::observability::metrics::ensure_default_meter();
    // The probed port can be taken by a test running in parallel before the
    // worker manager binds it: start over on a fresh engine and port.
    let mut attempt = 1;
    loop {
        match try_spawn(&setup).await {
            Ok(t) => return t,
            Err(err) if attempt < 5 => {
                eprintln!("test engine did not start (attempt {attempt}): {err:#}");
                attempt += 1;
            }
            Err(err) => panic!("start WorkerManager: {err:#}"),
        }
    }
}

async fn try_spawn(setup: &Setup) -> anyhow::Result<TestEngine> {
    let probe = TcpListener::bind("127.0.0.1:0").await?;
    let port = probe.local_addr()?.port();
    drop(probe);

    let engine = Arc::new(Engine::new());
    engine.observers.set_config(setup.observers.clone());
    if let Some(ms) = setup.namespace_grace_ms {
        engine.set_registration_namespace_grace_ms(ms);
    }
    let engine_fn = EngineFunctionsWorker::create(engine.clone(), None).await?;
    engine_fn.initialize().await?;
    engine_fn.register_functions(engine.clone());

    let mut manager_config = json!({ "port": port, "host": "127.0.0.1" });
    if let Some(rbac) = &setup.rbac {
        manager_config["rbac"] = rbac.clone();
    }
    if let Some(middleware) = &setup.middleware_function_id {
        manager_config["middleware_function_id"] = json!(middleware);
    }
    let manager = WorkerManager::create(engine.clone(), Some(manager_config)).await?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    manager
        .start_background_tasks(shutdown_rx, shutdown_tx)
        .await?;

    Ok(TestEngine { port, engine })
}

/// Polls `f` until it holds or [`WAIT`] passes.
pub async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    tokio::time::timeout(WAIT, async {
        loop {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .is_ok()
}

pub struct Client {
    sink: Sink,
    frames: mpsc::UnboundedReceiver<Value>,
    reader: tokio::task::JoinHandle<()>,
    auto_ack: Arc<AtomicBool>,
    pub worker_id: Uuid,
    pub reattach_token: String,
    pub name: String,
    buffered: VecDeque<Value>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

async fn send_to(sink: &Sink, frame: &Value) {
    let _ = sink
        .lock()
        .await
        .send(WsMessage::Text(frame.to_string().into()))
        .await;
}

/// What an SDK whose hooks accept everything answers to an owner-sync frame.
fn acks_for(frame: &Value) -> Vec<Value> {
    let ack = |function_id: &Value, namespace: &Value, id: &Value, revision: &Value| {
        json!({
            "type": "observerack",
            "function_id": function_id,
            "namespace": namespace,
            "id": id,
            "revision": revision,
        })
    };
    match frame["type"].as_str() {
        Some("observeradded") => vec![ack(
            &frame["function_id"],
            &frame["namespace"],
            &frame["observer"]["id"],
            &frame["revision"],
        )],
        Some("functionregistered") => frame["observers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|observer| {
                ack(
                    &frame["id"],
                    &frame["namespace"],
                    &observer["id"],
                    &frame["revision"],
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl Client {
    /// Connects and announces itself as `name`, declaring `observe.v1` and
    /// `local-dispatch.v1` when `features` is set.
    pub async fn connect(port: u16, name: &str, features: bool) -> Client {
        Self::connect_with(port, name, features, None).await
    }

    /// Connects; with `reattach`, first presents the previous connection's id
    /// and token, as an SDK does on reconnect.
    pub async fn connect_with(
        port: u16,
        name: &str,
        features: bool,
        reattach: Option<(Uuid, String)>,
    ) -> Client {
        let mut client = Self::open(port, name).await;
        if let Some((previous, token)) = reattach {
            client
                .send(json!({
                    "type": "reattach",
                    "previous_worker_id": previous.to_string(),
                    "reattach_token": token,
                }))
                .await;
        }
        client.announce(features).await;
        client
    }

    /// Connects without announcing itself (`engine::workers::register`).
    pub async fn open(port: u16, name: &str) -> Client {
        Self::open_query(port, name, "").await
    }

    /// As [`Client::open`], with a query string the auth function sees.
    pub async fn open_query(port: u16, name: &str, query: &str) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/?{query}"))
            .await
            .expect("connect");
        let (sink, mut stream) = ws.split();
        let sink: Sink = Arc::new(tokio::sync::Mutex::new(sink));
        let (tx, frames) = mpsc::unbounded_channel();
        let auto_ack = Arc::new(AtomicBool::new(true));
        let reader = {
            let sink = sink.clone();
            let auto_ack = auto_ack.clone();
            tokio::spawn(async move {
                while let Some(Ok(message)) = stream.next().await {
                    let WsMessage::Text(text) = message else {
                        continue;
                    };
                    let frame: Value = serde_json::from_str(&text).expect("json frame");
                    if auto_ack.load(Ordering::SeqCst) {
                        for ack in acks_for(&frame) {
                            send_to(&sink, &ack).await;
                        }
                    }
                    if tx.send(frame).is_err() {
                        break;
                    }
                }
            })
        };
        let mut client = Client {
            sink,
            frames,
            reader,
            auto_ack,
            worker_id: Uuid::nil(),
            reattach_token: String::new(),
            name: name.to_string(),
            buffered: VecDeque::new(),
        };
        let registered = client
            .expect("workerregistered", |f| f["type"] == "workerregistered")
            .await;
        client.worker_id = registered["worker_id"].as_str().unwrap().parse().unwrap();
        client.reattach_token = registered["reattach_token"].as_str().unwrap().to_string();
        client
    }

    /// `engine::workers::register`, declaring `observe.v1` and
    /// `local-dispatch.v1` when `features` is set.
    pub async fn announce(&mut self, features: bool) {
        self.announce_with(if features { OBSERVE_V1 } else { &[] })
            .await;
    }

    /// `engine::workers::register`, declaring `features` (none when empty).
    pub async fn announce_with(&mut self, features: &[&str]) {
        let mut data = json!({ "runtime": "test", "version": "0.25.0", "name": self.name });
        if !features.is_empty() {
            data["features"] = json!(features);
        }
        self.send(json!({
            "type": "invokefunction",
            "function_id": "engine::workers::register",
            "data": data,
            "action": { "type": "void" },
        }))
        .await;
    }

    /// Stops answering owner-sync messages: the test acks by hand.
    pub fn manual_acks(&self) {
        self.auto_ack.store(false, Ordering::SeqCst);
    }

    pub async fn send(&mut self, frame: Value) {
        send_to(&self.sink, &frame).await;
    }

    /// Sends a binary frame (telemetry: `OTLP`, `LOGS`, `MTRC` + JSON).
    pub async fn send_binary(&mut self, bytes: Vec<u8>) {
        let _ = self
            .sink
            .lock()
            .await
            .send(WsMessage::Binary(bytes.into()))
            .await;
    }

    /// Closes the socket, as a crashed or disconnected worker does.
    pub async fn disconnect(self) {
        let _ = self.sink.lock().await.close().await;
    }

    /// The first frame (buffered or new) that `matches`; frames that do not
    /// match stay buffered for later expectations.
    pub async fn expect(&mut self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.buffered.iter().position(&matches) {
            return self.buffered.remove(index).unwrap();
        }
        let found = tokio::time::timeout(WAIT, async {
            loop {
                let frame = self.frames.recv().await?;
                if matches(&frame) {
                    return Some(frame);
                }
                self.buffered.push_back(frame);
            }
        })
        .await;
        match found {
            Ok(Some(frame)) => frame,
            _ => panic!(
                "{}: no frame `{what}`; received meanwhile: {:#?}",
                self.name, self.buffered
            ),
        }
    }

    /// Whether a frame matching `matches` has already arrived. Call it after
    /// a barrier that guarantees the frame would have been sent by now.
    pub fn has_seen(&mut self, matches: impl Fn(&Value) -> bool) -> bool {
        while let Ok(frame) = self.frames.try_recv() {
            self.buffered.push_back(frame);
        }
        self.buffered.iter().any(matches)
    }

    pub async fn register_function(&mut self, id: &str) {
        self.send(json!({ "type": "registerfunction", "id": id }))
            .await;
    }

    pub async fn register_function_with(&mut self, frame: Value) {
        self.send(frame).await;
    }

    /// Sends `observe` and returns its `observeresult`.
    pub async fn observe(&mut self, frame: Value) -> Value {
        let id = frame["id"].as_str().expect("observer id").to_string();
        let mut frame = frame;
        frame["type"] = json!("observe");
        self.send(frame).await;
        self.expect("observeresult", move |f| {
            f["type"] == "observeresult" && f["id"] == id.as_str()
        })
        .await
    }

    /// Sends `observe` and waits until the observer is `active` (its owner
    /// may have to ack it first). Returns the `observeresult`.
    pub async fn observe_active(&mut self, frame: Value) -> Value {
        let id = frame["id"].as_str().expect("observer id").to_string();
        let result = self.observe(frame).await;
        if result["state"] == "pending" {
            let changed = self.state_changed(&id).await;
            assert_eq!(changed["state"], "active", "{id}: {changed}");
        } else {
            assert_eq!(result["state"], "active", "{id}: {result}");
        }
        result
    }

    /// The next `observerstatechanged` for `id`.
    pub async fn state_changed(&mut self, id: &str) -> Value {
        let id = id.to_string();
        self.expect("observerstatechanged", move |f| {
            f["type"] == "observerstatechanged" && f["id"] == id.as_str()
        })
        .await
    }

    /// The next `observerstatechanged` for `id` into `state`.
    pub async fn state_becomes(&mut self, id: &str, state: &str) -> Value {
        let (id, state) = (id.to_string(), state.to_string());
        self.expect("observerstatechanged", move |f| {
            f["type"] == "observerstatechanged"
                && f["id"] == id.as_str()
                && f["state"] == state.as_str()
        })
        .await
    }

    /// Calls `function_id` and returns its invocation id.
    pub async fn call(&mut self, function_id: &str, data: Value) -> Uuid {
        self.call_with(function_id, data, json!({})).await
    }

    /// Calls `function_id` with extra top-level fields (`observe_depth`,
    /// `caller`, `namespace`, `activation`, ...).
    pub async fn call_with(&mut self, function_id: &str, data: Value, extra: Value) -> Uuid {
        let invocation_id = Uuid::new_v4();
        let mut frame = json!({
            "type": "invokefunction",
            "invocation_id": invocation_id.to_string(),
            "function_id": function_id,
            "data": data,
        });
        if let Value::Object(extra) = extra {
            for (key, value) in extra {
                frame[key] = value;
            }
        }
        self.send(frame).await;
        invocation_id
    }

    /// The `invocationresult` of `invocation_id`.
    pub async fn result(&mut self, invocation_id: Uuid) -> Value {
        let id = invocation_id.to_string();
        self.expect("invocationresult", move |f| {
            f["type"] == "invocationresult" && f["invocation_id"] == id.as_str()
        })
        .await
    }

    /// The next `invokefunction` the engine forwards for `function_id`.
    pub async fn invoked(&mut self, function_id: &str) -> Value {
        let function_id = function_id.to_string();
        self.expect("invokefunction", move |f| {
            f["type"] == "invokefunction" && f["function_id"] == function_id.as_str()
        })
        .await
    }

    /// Answers a forwarded `invokefunction` with `result`.
    pub async fn reply(&mut self, invoke: &Value, result: Value) {
        self.send(json!({
            "type": "invocationresult",
            "invocation_id": invoke["invocation_id"],
            "function_id": invoke["function_id"],
            "result": result,
        }))
        .await;
    }

    /// Answers a forwarded `invokefunction` with `result`, as a host that
    /// declared `observe.v2` does: listing the observers its delivery
    /// matched.
    pub async fn reply_observed(&mut self, invoke: &Value, result: Value, observed: &[&str]) {
        self.send(json!({
            "type": "invocationresult",
            "invocation_id": invoke["invocation_id"],
            "function_id": invoke["function_id"],
            "result": result,
            "observed": observed,
        }))
        .await;
    }

    /// Answers a forwarded `invokefunction` with an error.
    pub async fn reply_error(&mut self, invoke: &Value, code: &str, message: &str) {
        self.send(json!({
            "type": "invocationresult",
            "invocation_id": invoke["invocation_id"],
            "function_id": invoke["function_id"],
            "error": { "code": code, "message": message },
        }))
        .await;
    }

    /// Discards every frame received so far.
    pub fn discard_received(&mut self) {
        while self.frames.try_recv().is_ok() {}
        self.buffered.clear();
    }

    /// The next frame, buffered or new, with no time limit; `None` once the
    /// socket closed.
    pub async fn recv(&mut self) -> Option<Value> {
        match self.buffered.pop_front() {
            Some(frame) => Some(frame),
            None => self.frames.recv().await,
        }
    }

    /// Hosts `function_id` for one call: waits for it and answers `result`.
    pub async fn serve(&mut self, function_id: &str, result: Value) -> Value {
        let invoke = self.invoked(function_id).await;
        self.reply(&invoke, result).await;
        invoke
    }
}

/// Connects `name` and waits until the engine knows it by name, so the
/// `caller.worker_name` the engine stamps on its calls is already set.
pub async fn connect(t: &TestEngine, name: &str, features: bool) -> Client {
    let client = Client::connect(t.port, name, features).await;
    let id = client.worker_id;
    assert!(
        eventually(|| t.engine.worker_registry.get_worker_name(&id).as_deref() == Some(name)).await,
        "`{name}` never finished engine::workers::register"
    );
    client
}

/// As [`connect`], declaring `features`.
pub async fn connect_with(t: &TestEngine, name: &str, features: &[&str]) -> Client {
    let mut client = Client::open(t.port, name).await;
    client.announce_with(features).await;
    let id = client.worker_id;
    assert!(
        eventually(|| t.engine.worker_registry.get_worker_name(&id).as_deref() == Some(name)).await,
        "`{name}` never finished engine::workers::register"
    );
    client
}

/// As [`connect`], for a worker that declared `observe.v2`.
pub async fn connect_v2(t: &TestEngine, name: &str) -> Client {
    connect_with(t, name, OBSERVE_V2).await
}

/// As [`connect`], with a query string the auth function sees.
pub async fn connect_query(t: &TestEngine, name: &str, features: bool, query: &str) -> Client {
    let mut client = Client::open_query(t.port, name, query).await;
    client.announce(features).await;
    let id = client.worker_id;
    assert!(
        eventually(|| t.engine.worker_registry.get_worker_name(&id).as_deref() == Some(name)).await,
        "`{name}` never finished engine::workers::register"
    );
    client
}

/// Registers an in-process function on the engine (an auth function, an RBAC
/// hook, middleware) answering every call with `answer(input)`.
pub fn in_process(
    engine: &Engine,
    id: &str,
    answer: impl Fn(Value) -> Value + Send + Sync + 'static,
) {
    use iii::engine::{EngineTrait, Handler, RegisterFunctionRequest};
    use iii::function::FunctionResult;
    let answer = Arc::new(answer);
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |input: Value| {
            let answer = answer.clone();
            async move { FunctionResult::Success(Some(answer(input))) }
        }),
    );
}

/// Waits until `id` is registered in `namespace`.
pub async fn registered(engine: &Engine, namespace: &str, id: &str) {
    assert!(
        eventually(|| engine.functions.get(namespace, id).is_some()).await,
        "`{id}` was never registered in `{namespace}`"
    );
}

/// Waits until the routed hook evaluated `count` observed invocations in
/// total (evaluation runs after the caller has its result).
pub async fn evaluated(engine: &Engine, count: u64) {
    assert!(
        eventually(|| engine.observers.evaluated() >= count).await,
        "only {} observed invocations evaluated, expected {count}",
        engine.observers.evaluated()
    );
}
