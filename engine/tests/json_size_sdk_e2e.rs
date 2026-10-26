//! Opt-in integration against a real engine for JSON fragmentation and target compatibility.
//! Setup from the repository root (the Python sync below is frozen and cannot rewrite uv.lock):
//! ```sh
//! pnpm install --frozen-lockfile --ignore-scripts
//! pnpm --dir sdk/packages/node/helpers build
//! uv sync --frozen --project sdk/packages/python/iii --extra dev
//! # Go >= 1.24 on PATH, or set JSON_SIZE_TEST_GO to the verified Go binary.
//! cargo test -p iii --test json_size_sdk_e2e --locked -- --ignored --nocapture
//! ```
//! Optional modes: `core`, `guard`, `guard-target`, and `guard-result`; omit for the full suite.
use futures_util::{SinkExt, StreamExt};
use iii::{EngineBuilder, engine::EngineTrait};
use iii_helpers::observability::OtelConfig;
use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{Dispatch, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Bytes, Message as WsMessage,
        protocol::frame::{
            Frame,
            coding::{Data, OpCode},
        },
    },
};
use uuid::Uuid;

/// An SDK participant. `pnpm exec` and `go run` run the real worker as their own
/// child, so each participant leads a process group and teardown signals the
/// whole group: killing only the direct child left the worker holding the test's
/// stderr open, which hung piped runs.
struct Participant(Child);
impl Participant {
    fn spawn(command: &mut Command) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            interrupt::install();
        }
        let participant = Self(command.spawn().unwrap());
        #[cfg(unix)]
        interrupt::track(participant.0.id());
        participant
    }
}
impl Drop for Participant {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, killpg};
            let group = nix::unistd::Pid::from_raw(self.0.id() as i32);
            // The group is gone once every member has exited and the leader (our
            // child) has been reaped.
            let mut wait_gone = |grace: Duration| {
                let deadline = Instant::now() + grace;
                loop {
                    let _ = self.0.try_wait();
                    if killpg(group, None).is_err() {
                        return true;
                    }
                    if Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            };
            let _ = killpg(group, Signal::SIGTERM);
            if !wait_gone(Duration::from_secs(3)) {
                let _ = killpg(group, Signal::SIGKILL);
                wait_gone(Duration::from_secs(3));
            }
            interrupt::untrack(self.0.id());
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Participants no longer share the terminal's process group, so Ctrl-C (or a
/// SIGTERM/SIGHUP sent to the test) would skip them. Kill every live group, then
/// let the signal end the test with its default action.
#[cfg(unix)]
mod interrupt {
    use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
    use std::sync::atomic::{AtomicI32, Ordering};

    static GROUPS: [AtomicI32; 3] = [const { AtomicI32::new(0) }; 3];

    extern "C" fn forward(signal: libc::c_int) {
        for group in &GROUPS {
            let group = group.load(Ordering::SeqCst);
            if group > 0 {
                // SAFETY: kill(2) is async-signal-safe.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
        }
        // SAFETY: signal(2) and raise(3) are async-signal-safe. The signal stays
        // blocked until this handler returns, then terminates the process.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }

    pub fn install() {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            let action = SigAction::new(
                SigHandler::Handler(forward),
                SaFlags::empty(),
                SigSet::empty(),
            );
            for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
                // SAFETY: `forward` only makes async-signal-safe calls.
                unsafe { sigaction(signal, &action) }.unwrap();
            }
        });
    }

    pub fn track(pid: u32) {
        let tracked = GROUPS.iter().any(|slot| {
            slot.compare_exchange(0, pid as i32, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        });
        assert!(tracked, "too many participants");
    }

    pub fn untrack(pid: u32) {
        for slot in &GROUPS {
            let _ = slot.compare_exchange(pid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
}

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
const FRAME_LIMIT: usize = 16 * 1024 * 1024;
const MESSAGE_LIMIT: usize = 64 * 1024 * 1024;
const LARGE: usize = 40 * 1024 * 1024;
const LINK: &str = "https://iii.dev/docs/creating-workers/channels#payload-size";
const PERIOD: usize = 97;
const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const TEST_BLOCK: usize = 1024 * 1024;

struct TestEngine {
    builder: EngineBuilder,
    sdk: iii_sdk::IIIClient,
    url: String,
    log: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn start_engine() -> TestEngine {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("engine.log");
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::sync::Arc::new(std::fs::File::create(&log).unwrap()))
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
            Some(json!({"adapter":{"name":"fs","config":{"directory":dir.path()}}})),
        )
        .add_worker("iii-stream", Some(json!({"port":0})))
        .add_worker(
            "iii-worker-manager",
            Some(json!({"host":"127.0.0.1","port":port})),
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
            let result = match input {
                Value::String(value) => value,
                Value::Object(object) => {
                    if let Some(text) = object.get("payload").and_then(Value::as_str) {
                        text.to_owned()
                    } else {
                        let size = object.get("size").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let marker = object
                            .get("marker")
                            .and_then(Value::as_str)
                            .and_then(|text| text.chars().next())
                            .unwrap_or('R');
                        pattern(size, marker as u8)
                    }
                }
                _ => String::new(),
            };
            Ok(Value::String(result))
        }),
    );
    TestEngine {
        builder,
        sdk,
        url,
        log,
        _dir: dir,
    }
}

// Must be byte-for-byte identical in Rust, Node, Python and Go. The 97-byte period
// does not divide the RFC 6455 1 MiB WebSocket fragment size.
fn expected_byte(index: usize, marker: u8) -> u8 {
    ALPHABET[(index % PERIOD + usize::from(marker)) % ALPHABET.len()]
}
fn pattern(size: usize, marker: u8) -> String {
    String::from_utf8(
        (0..size)
            .map(|index| expected_byte(index, marker))
            .collect(),
    )
    .unwrap()
}
fn matches(value: &str, size: usize, marker: u8) -> bool {
    value.len() == size
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| byte == expected_byte(index, marker))
}
fn payload(size: usize, marker: char) -> Value {
    Value::String(pattern(size, marker as u8))
}
fn assert_content(value: &Value, size: usize, marker: char) {
    assert!(
        matches(value.as_str().unwrap(), size, marker as u8),
        "fragmented content/order mismatch"
    );
}

#[test]
fn content_comparator_detects_swapped_megabyte_fragments() {
    assert_ne!(TEST_BLOCK % PERIOD, 0);
    let expected = pattern(2 * TEST_BLOCK, b'Q');
    let mut corrupted = expected.into_bytes();
    let first_block = corrupted[..TEST_BLOCK].to_vec();
    corrupted.copy_within(TEST_BLOCK.., 0);
    corrupted[TEST_BLOCK..].copy_from_slice(&first_block);
    let corrupted = String::from_utf8(corrupted).unwrap();
    assert!(!matches(&corrupted, 2 * TEST_BLOCK, b'Q'));
}

fn envelope_size(message: &str) -> usize {
    message
        .strip_prefix("Serialized JSON envelope is ")
        .and_then(|rest| rest.split_once(" bytes")?.0.parse().ok())
        .unwrap()
}
fn assert_sdk_error(message: &str) {
    let size = envelope_size(message);
    assert!(size > MESSAGE_LIMIT);
    assert_eq!(
        message,
        format!(
            "Serialized JSON envelope is {size} bytes; limit is {MESSAGE_LIMIT} bytes. Use channels for larger payloads: {LINK}"
        )
    );
}
fn assert_engine_error(message: &str) {
    let size = envelope_size(message);
    assert!(size > FRAME_LIMIT);
    assert_eq!(
        message,
        format!(
            "Serialized JSON envelope is {size} bytes; target worker accepts up to {FRAME_LIMIT} bytes. It did not declare large-messages.v1 (SDK 0.25.0 or later), and the version it reports is older than 0.24.5 or unknown. Upgrade the target worker SDK to 0.25.0 or later, or use channels: {LINK}"
        )
    );
}
fn engine_error(message: &str) {
    let size = envelope_size(message);
    assert!(size > FRAME_LIMIT);
    assert_eq!(
        message,
        format!(
            "Serialized JSON envelope is {size} bytes; target worker accepts up to {FRAME_LIMIT} bytes. It did not declare large-messages.v1 (SDK 0.25.0 or later), and the version it reports is older than 0.24.5 or unknown. Upgrade the target worker SDK to 0.25.0 or later, or use channels: {LINK}"
        )
    );
}

async fn invoke(
    sdk: &iii_sdk::IIIClient,
    id: &str,
    payload: Value,
    ms: u64,
) -> Result<Value, iii_sdk::Error> {
    invoke_remote(sdk, id, payload, ms).await
}

async fn invoke_remote(
    sdk: &iii_sdk::IIIClient,
    id: &str,
    payload: Value,
    ms: u64,
) -> Result<Value, iii_sdk::Error> {
    sdk.trigger(
        TriggerRequest {
            function_id: id.to_owned(),
            payload,
            timeout_ms: Some(ms),
            action: None,
        }
        .dispatch(Dispatch::Remote),
    )
    .await
}

async fn raw_connect(url: &str, name: &str, version: Option<&str>) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let greeting = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(greeting.to_text().unwrap()).unwrap()["type"],
        "workerregistered"
    );
    let mut metadata = json!({"runtime":"node","name":name});
    if let Some(version) = version {
        metadata["version"] = json!(version);
    }
    ws.send(WsMessage::Text(json!({"type":"invokefunction","invocation_id":null,"function_id":"engine::workers::register","data":metadata,"action":{"type":"void"}}).to_string().into())).await.unwrap();
    ws
}

async fn raw_target(
    engine: &TestEngine,
    suffix: &str,
    version: Option<&str>,
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<Value>,
    tokio::task::JoinHandle<()>,
) {
    let function_id = format!("size::old-{suffix}");
    let mut ws = raw_connect(&engine.url, &format!("old-{suffix}"), version).await;
    ws.send(WsMessage::Text(json!({"type":"registerfunction","id":function_id,"request_format":null,"response_format":null}).to_string().into())).await.unwrap();
    let (mut sink, mut stream) = ws.split();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let registered_id = function_id.clone();
    let task = tokio::spawn(async move {
        while let Some(Ok(frame)) = stream.next().await {
            let Ok(text) = frame.to_text() else { continue };
            let Ok(message) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if message["type"] != "invokefunction" || message["function_id"] != registered_id {
                continue;
            }
            if tx.send(message.clone()).is_err() {
                return;
            }
            let response = json!({"type":"invocationresult","invocation_id":message["invocation_id"],"function_id":registered_id,"result":message["data"]});
            if sink
                .send(WsMessage::Text(response.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
    });
    (function_id, rx, task)
}

async fn worker_ids(engine: &TestEngine) -> Vec<String> {
    let response = engine
        .builder
        .engine()
        .call("engine::workers::list", json!({}))
        .await
        .unwrap()
        .unwrap();
    let mut ids: Vec<String> = response["workers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|worker| worker["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids
}

async fn guard_target(engine: &TestEngine, version: Option<&str>, suffix: &str) {
    let (function_id, mut events, task) = raw_target(engine, suffix, version).await;
    let mut ready = false;
    for _ in 0..60 {
        if invoke(&engine.sdk, &function_id, json!("ready"), 1_000)
            .await
            .is_ok()
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ready, "legacy target did not register");
    assert!(events.recv().await.is_some());
    let identities_before = worker_ids(engine).await;

    let mut caller =
        raw_connect(&engine.url, &format!("new-caller-{suffix}"), Some("0.24.5")).await;
    let ready_id = Uuid::new_v4();
    caller.send(WsMessage::Text(json!({"type":"invokefunction","invocation_id":ready_id,"function_id":"engine::workers::list","data":{}}).to_string().into())).await.unwrap();
    let ready_frame = tokio::time::timeout(Duration::from_secs(5), caller.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(ready_frame.to_text().unwrap()).unwrap()["invocation_id"],
        ready_id.to_string()
    );

    let invocation_id = Uuid::new_v4();
    let mut envelope = format!(
        "{{\"type\":\"invokefunction\",\"invocation_id\":\"{invocation_id}\",\"function_id\":\"{function_id}\",\"data\":\""
    );
    envelope.push_str(&pattern(LARGE, b'L'));
    envelope.push_str("\"}");
    let bytes = Bytes::from(envelope.into_bytes());
    for start in (0..bytes.len()).step_by(1024 * 1024) {
        let end = (start + 1024 * 1024).min(bytes.len());
        let opcode = if start == 0 {
            Data::Text
        } else {
            Data::Continue
        };
        caller
            .send(WsMessage::Frame(Frame::message(
                bytes.slice(start..end),
                OpCode::Data(opcode),
                end == bytes.len(),
            )))
            .await
            .unwrap();
    }
    let started = Instant::now();
    let response = tokio::time::timeout(Duration::from_secs(5), caller.next())
        .await
        .expect("guard timeout")
        .unwrap()
        .unwrap();
    match events.try_recv() {
        Ok(message) => panic!("legacy target received oversized invocation: {message}"),
        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
            panic!("legacy target disconnected")
        }
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    let response: Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
    assert_eq!(response["type"], "invocationresult");
    assert_eq!(response["invocation_id"], invocation_id.to_string());
    assert_eq!(response["error"]["code"], "payload_too_large");
    assert_engine_error(response["error"]["message"].as_str().unwrap());
    match tokio::time::timeout(Duration::from_millis(250), events.recv()).await {
        Err(_) => {}
        Ok(Some(message)) => panic!("legacy target received oversized data: {message}"),
        Ok(None) => panic!("legacy target stream closed"),
    }
    assert_eq!(
        invoke(&engine.sdk, &function_id, json!("small-after-guard"), 5_000)
            .await
            .unwrap(),
        json!("small-after-guard")
    );
    assert!(events.recv().await.is_some());
    assert_eq!(worker_ids(engine).await, identities_before);
    task.abort();
}

async fn guard_result_caller(engine: &TestEngine, version: Option<&str>, suffix: &str) {
    let mut ws = raw_connect(&engine.url, &format!("old-result-{suffix}"), version).await;
    // The old caller sends a small request. The new Rust target uses its `size`
    // argument to build the large result, isolating the engine's outbound guard.
    let invocation = Uuid::new_v4();
    let request = json!({"size": LARGE, "marker": "R"});
    assert!(request.to_string().len() < FRAME_LIMIT);
    ws.send(WsMessage::Text(json!({"type":"invokefunction","invocation_id":invocation,"function_id":"size::rust","data":request}).to_string().into())).await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("large result guard timed out")
        .unwrap()
        .unwrap();
    let response: Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
    assert_eq!(response["type"], "invocationresult");
    assert_eq!(response["invocation_id"], invocation.to_string());
    assert_eq!(response["function_id"], "size::rust");
    assert!(response.get("result").is_none());
    assert_eq!(response["error"]["code"], "payload_too_large");
    engine_error(response["error"]["message"].as_str().unwrap());

    let next_id = Uuid::new_v4();
    ws.send(WsMessage::Text(json!({"type":"invokefunction","invocation_id":next_id,"function_id":"size::rust","data":"still-open"}).to_string().into())).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let next: Value = serde_json::from_str(next.to_text().unwrap()).unwrap();
    assert_eq!(next["invocation_id"], next_id.to_string());
    assert_eq!(next["result"], "still-open");
}
async fn wait_for(engine: &TestEngine, id: &str) {
    for _ in 0..100 {
        if invoke(&engine.sdk, id, json!("ready"), 1_000).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{id} did not register")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires pnpm Node dependencies/helpers, frozen-lock Python venv and Go >=1.24"]
async fn fragmented_json_roundtrips_and_engine_compatibility_guard() {
    let started = Instant::now();
    let engine = start_engine().await;
    let mode = std::env::var("JSON_SIZE_TEST_MODE").unwrap_or_else(|_| "all".into());
    if mode == "core" {
        let large = payload(LARGE, 'Q');
        assert_eq!(
            invoke(&engine.sdk, "size::rust", large.clone(), 120_000)
                .await
                .unwrap(),
            large
        );
        let error = invoke(
            &engine.sdk,
            "size::rust",
            payload(MESSAGE_LIMIT, 'Z'),
            120_000,
        )
        .await
        .unwrap_err()
        .invocation_error()
        .unwrap();
        assert_eq!(error.code, "payload_too_large");
        assert_sdk_error(&error.message);
        assert_eq!(
            invoke(&engine.sdk, "size::rust", json!("small"), 5_000)
                .await
                .unwrap(),
            json!("small")
        );
        engine.sdk.shutdown_async().await;
        engine.builder.destroy().await.unwrap();
        return;
    }
    if mode == "guard" || mode == "guard-target" {
        guard_target(&engine, Some("0.24.4"), "legacy").await;
        guard_target(&engine, None, "unknown").await;
    }
    if mode == "guard" || mode == "guard-result" {
        guard_result_caller(&engine, Some("0.24.4"), "legacy").await;
        guard_result_caller(&engine, None, "unknown").await;
    }
    if !["all", "core", "guard", "guard-target", "guard-result"].contains(&mode.as_str()) {
        panic!("unknown mode {mode}")
    };
    if mode != "all" {
        engine.sdk.shutdown_async().await;
        engine.builder.destroy().await.unwrap();
        return;
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let node_log = engine._dir.path().join("node.log");
    let python_log = engine._dir.path().join("python.log");
    let go_log = engine._dir.path().join("go.log");
    let _node = Participant::spawn(
        Command::new("pnpm")
            .args([
                "exec",
                "tsx",
                "sdk/packages/node/iii/tests/json-size-engine-participant.ts",
            ])
            .current_dir(root)
            .env("JSON_SIZE_TEST_URL", &engine.url)
            .stdout(Stdio::from(std::fs::File::create(&node_log).unwrap()))
            .stderr(Stdio::inherit()),
    );
    let _python = Participant::spawn(
        Command::new("sdk/packages/python/iii/.venv/bin/python")
            .arg("sdk/packages/python/iii/tests/json_size_engine_participant.py")
            .current_dir(root)
            .env("JSON_SIZE_TEST_URL", &engine.url)
            .stdout(Stdio::from(std::fs::File::create(&python_log).unwrap()))
            .stderr(Stdio::inherit()),
    );
    let _go = Participant::spawn(
        Command::new(std::env::var("JSON_SIZE_TEST_GO").unwrap_or_else(|_| "go".into()))
            .args(["run", "./tests/json-size-engine-participant"])
            .current_dir(root.join("sdk/packages/go/iii"))
            .env("JSON_SIZE_TEST_URL", &engine.url)
            // A signalled `go run` skips removing its work directory; keep it in
            // the test's temp dir so it is removed with it.
            .env("GOTMPDIR", engine._dir.path())
            .stdout(Stdio::from(std::fs::File::create(&go_log).unwrap()))
            .stderr(Stdio::inherit()),
    );
    for id in ["size::node-ready", "size::python-ready", "size::go-ready"] {
        wait_for(&engine, id).await;
    }
    for (target, marker) in [
        ("size::node", 'R'),
        ("size::python", 'P'),
        ("size::go", 'G'),
    ] {
        let result = invoke(&engine.sdk, target, payload(LARGE, marker), 120_000)
            .await
            .unwrap();
        assert_content(&result, LARGE, marker);
    }
    let rust_target = invoke(
        &engine.sdk,
        "size::rust",
        json!({"payload":pattern(LARGE,b'S')}),
        120_000,
    )
    .await
    .unwrap();
    assert_content(&rust_target, LARGE, 'S');
    // Most Rust workers report their own crate version in
    // `WorkerMetadata.version`: the SDK's `large-messages.v1` identifies it.
    let crate_version = register_worker(
        &engine.url,
        InitOptions {
            metadata: Some(WorkerMetadata {
                version: "0.3.0".into(),
                name: "json-size-rust-crate-version".into(),
                ..Default::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    crate_version.register_function(
        "size::rust-crate-version",
        RegisterFunction::new_async(|input: Value| async move { Ok(input) }),
    );
    wait_for(&engine, "size::rust-crate-version").await;
    let crate_version_target = invoke(
        &engine.sdk,
        "size::rust-crate-version",
        payload(LARGE, 'V'),
        120_000,
    )
    .await
    .unwrap();
    assert_content(&crate_version_target, LARGE, 'V');
    crate_version.shutdown_async().await;
    for (caller, target, marker) in [
        ("size::node-roundtrip", "size::python", 'N'),
        ("size::python-roundtrip", "size::go", 'P'),
        ("size::go-roundtrip", "size::python", 'G'),
    ] {
        assert_eq!(
            invoke(
                &engine.sdk,
                caller,
                json!({"target":target,"size":LARGE,"marker":marker.to_string()}),
                120_000
            )
            .await
            .unwrap(),
            json!(true)
        );
    }
    let identities_before_sdk_limits = worker_ids(&engine).await;
    for caller in [
        "size::node-local-limit",
        "size::python-local-limit",
        "size::go-local-limit",
    ] {
        assert_eq!(
            invoke(&engine.sdk, caller, Value::Null, 120_000)
                .await
                .unwrap(),
            json!(true)
        );
    }
    assert_eq!(
        worker_ids(&engine).await,
        identities_before_sdk_limits,
        "SDK local limit changed worker identity"
    );
    let identities_before_rust_limit = worker_ids(&engine).await;
    let rust_error = invoke_remote(
        &engine.sdk,
        "size::rust",
        payload(MESSAGE_LIMIT, 'Z'),
        120_000,
    )
    .await
    .unwrap_err()
    .invocation_error()
    .unwrap();
    assert_eq!(rust_error.code, "payload_too_large");
    assert_sdk_error(&rust_error.message);
    assert_eq!(
        invoke(&engine.sdk, "size::rust", json!("small-after-rust"), 5_000)
            .await
            .unwrap(),
        json!("small-after-rust")
    );
    assert_eq!(
        worker_ids(&engine).await,
        identities_before_rust_limit,
        "Rust local limit changed worker identity"
    );
    for (version, suffix) in [(Some("0.24.4"), "legacy"), (None, "unknown")] {
        guard_target(&engine, version, suffix).await;
        guard_result_caller(&engine, version, suffix).await;
    }
    // A raw frame over the message limit is dropped: the warning names the
    // limit and the size but not the content, and the connection stays open.
    let (mut raw, _) = tokio_tungstenite::connect_async(&engine.url).await.unwrap();
    let _ = raw.next().await;
    let frame = format!(
        "{{\"secret\":\"JSON_SIZE_PRIVATE_MARKER{}\"}}",
        "x".repeat(MESSAGE_LIMIT)
    );
    let frame_len = frame.len();
    raw.send(WsMessage::Text(frame.into())).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let diagnostic = loop {
        let text = std::fs::read_to_string(&engine.log).unwrap();
        if text.contains("Worker message exceeds the inbound limit") {
            break text;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for diagnostic: {text}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        diagnostic.contains("worker_id=")
            && diagnostic.contains(&MESSAGE_LIMIT.to_string())
            && diagnostic.contains(&frame_len.to_string())
    );
    assert!(!diagnostic.contains("JSON_SIZE_PRIVATE_MARKER"));
    raw.send(WsMessage::Ping(b"open".to_vec().into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = raw.next().await {
            if matches!(frame.unwrap(), WsMessage::Pong(data) if data.as_ref() == b"open") {
                return;
            }
        }
        panic!("connection closed after an oversized frame");
    })
    .await
    .expect("pong after an oversized frame");
    assert!(
        std::fs::read_to_string(node_log)
            .unwrap()
            .contains("JSON_SIZE_NODE_CROSS_SDK_OK")
    );
    assert!(
        std::fs::read_to_string(python_log)
            .unwrap()
            .contains("JSON_SIZE_PYTHON_CROSS_SDK_OK")
    );
    assert!(
        std::fs::read_to_string(go_log)
            .unwrap()
            .contains("JSON_SIZE_GO_CROSS_SDK_OK")
    );
    println!("JSON_SIZE_E2E_ELAPSED_MS {}", started.elapsed().as_millis());
    engine.sdk.shutdown_async().await;
    engine.builder.destroy().await.unwrap();
}
