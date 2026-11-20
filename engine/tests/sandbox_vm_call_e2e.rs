//! `iii call sandbox::build` and `iii call sandbox::start` end to end: the `iii` binary, the
//! engine's sandbox router, and a fake adapter on the Rust SDK that serves the contract 1.4
//! `worker::*` calls (tech-specs/2026-11-19-iii-call-sandbox, section 4).
//!
//! The request's `state_key` picks how the fake adapter behaves.

use std::{
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use iii::EngineBuilder;
use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    Error, IIIClient, InitOptions, OutputStream, RegisterFunction, WorkerIdentityMode,
    register_worker,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const WAIT: Duration = Duration::from_secs(15);
const ADAPTER: &str = "fake-adapter";

/// What the fake adapter saw.
#[derive(Default)]
struct Seen {
    started: Mutex<Option<Value>>,
    stopped: AtomicBool,
    build_aborted: AtomicBool,
    /// The VM exited ("exit-*" modes) or was stopped: `worker::status` says so.
    ended: Mutex<Option<(String, i32)>>,
}

struct TestEngine {
    _builder: EngineBuilder,
    _adapter: IIIClient,
    _dir: tempfile::TempDir,
    url: String,
    seen: Arc<Seen>,
}

async fn ready(function: &FunctionRef) {
    let deadline = Instant::now() + WAIT;
    while !function.local_dispatch_active() {
        assert!(Instant::now() < deadline, "the function never registered");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn record(stream: &str, line: &str, cursor: u64) -> String {
    json!({ "stream": stream, "line": line, "ts_ms": 1, "cursor": cursor }).to_string()
}

fn status(seen: &Seen) -> Value {
    let ended = seen.ended.lock().unwrap().clone();
    match ended {
        None => json!({ "vm_id": "vm-1", "instance": 1, "state": "running", "exit_code": null,
                        "signal": null, "started_at_ms": 0, "ended_at_ms": null }),
        Some((state, code)) => json!({ "vm_id": "vm-1", "instance": 1, "state": state,
                        "exit_code": code, "signal": null, "started_at_ms": 0, "ended_at_ms": 1 }),
    }
}

async fn start() -> TestEngine {
    let dir = tempfile::tempdir().unwrap();
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
    let adapter = register_worker(
        &url,
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: ADAPTER.to_string(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..Default::default()
        },
    );
    adapter
        .wait_until_registered(WAIT)
        .await
        .expect("engine reachable");

    let seen = Arc::new(Seen::default());
    let id = |suffix: &str| format!("{ADAPTER}::{suffix}");
    let mut functions = vec![adapter.register_function(
        id("adapter::info"),
        RegisterFunction::new_async(|_: Value| async move {
            Ok::<_, Error>(json!({
                "adapter": ADAPTER, "adapter_version": "0.5.0",
                "contract": { "major": 1, "minor": 4 },
                "features": ["worker.scoped_auth", "worker.lease", "worker.logs_from_end",
                             "worker.build", "worker.binary", "worker.prune"]
            }))
        }),
    )];

    let build_seen = seen.clone();
    functions.push(adapter.register_function(
        id("worker::build"),
        RegisterFunction::new_async(move |req: Value| {
            let seen = build_seen.clone();
            async move {
                let logs: OutputStream = serde_json::from_value(req["logs"].clone())?;
                logs.send_text(&record("stdout", "installing dependencies", 1))
                    .await?;
                logs.send_text(&record("stderr", "a warning", 2)).await?;
                if req["state_key"] == "build-hang" {
                    // A build that lasts as long as its output is read.
                    loop {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        if logs.write(&[0]).await.is_err() {
                            seen.build_aborted.store(true, Ordering::SeqCst);
                            return Err(Error::Runtime("the build was aborted".into()));
                        }
                    }
                }
                logs.close().await?;
                Ok::<_, Error>(json!({
                    "build_id": "sha256:abc",
                    "base_image": { "reference": "docker.io/iiidev/python", "digest": null },
                    "built_at_ms": 7
                }))
            }
        }),
    ));

    let start_seen = seen.clone();
    functions.push(adapter.register_function(
        id("worker::start"),
        RegisterFunction::new_async(move |req: Value| {
            let seen = start_seen.clone();
            async move {
                let mode = req["state_key"].as_str().unwrap_or_default().to_string();
                *seen.started.lock().unwrap() = Some(req);
                *seen.ended.lock().unwrap() = match mode.as_str() {
                    "exit-0" => Some(("exited".into(), 0)),
                    "exit-3" => Some(("exited".into(), 3)),
                    _ => None,
                };
                Ok::<_, Error>(json!({ "vm_id": "vm-1", "instance": 1 }))
            }
        }),
    ));

    let logs_seen = seen.clone();
    functions.push(adapter.register_function(
        id("worker::logs"),
        RegisterFunction::new_async(move |req: Value| {
            let seen = logs_seen.clone();
            async move {
                let channel: OutputStream = serde_json::from_value(req["channel"].clone())?;
                channel.keep_open();
                let from_cursor = req["cursor"].as_u64().unwrap_or(0);
                tokio::spawn(async move {
                    if from_cursor < 1 {
                        let _ = channel
                            .send_text(&record("stdout", "hello from the guest", 1))
                            .await;
                        let _ = channel
                            .send_text(&record("stderr", "guest warning", 2))
                            .await;
                    }
                    // The stream stays open while the VM runs, and closes once it ended.
                    while seen.ended.lock().unwrap().is_none() {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        if channel.write(&[0]).await.is_err() {
                            return;
                        }
                    }
                    let _ = channel.close().await;
                });
                Ok::<_, Error>(json!({ "cursor": null }))
            }
        }),
    ));

    let status_seen = seen.clone();
    functions.push(adapter.register_function(
        id("worker::status"),
        RegisterFunction::new_async(move |_: Value| {
            let seen = status_seen.clone();
            async move { Ok::<_, Error>(status(&seen)) }
        }),
    ));

    let stop_seen = seen.clone();
    functions.push(adapter.register_function(
        id("worker::stop"),
        RegisterFunction::new_async(move |_: Value| {
            let seen = stop_seen.clone();
            async move {
                seen.stopped.store(true, Ordering::SeqCst);
                *seen.ended.lock().unwrap() = Some(("stopped".into(), 143));
                Ok::<_, Error>(status(&seen))
            }
        }),
    ));
    for function in &functions {
        ready(function).await;
    }
    TestEngine {
        _builder: builder,
        _adapter: adapter,
        _dir: dir,
        url,
        seen,
    }
}

/// The defaults Compose would hand a container's scripts, with `mode` as the state key.
fn defaults(mode: &str) -> String {
    let common = json!({
        "worker": "api", "adapter": ADAPTER, "namespace": "default",
        "auth": "scoped-token", "state_key": mode,
        "source": { "kind": "local", "host_path": "/srv/project/api" },
        "env": { "KEY": "secret" },
        "build_dir": "/srv/project/.iii/build/api"
    });
    let mut start = common.clone();
    start["build_id"] = json!("sha256:abc");
    start["engine_url"] = json!("ws://127.0.0.1:1");
    start["stop_grace_ms"] = json!(2000);
    let mut build = common;
    build["timeout_ms"] = json!(60_000);
    json!({ "sandbox::start": start, "sandbox::build": build }).to_string()
}

fn iii_call(engine: &TestEngine, mode: &str, args: &[&str]) -> (Command, tempfile::TempDir) {
    let cwd = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_iii"));
    command
        .arg("call")
        .args(args)
        .args(["--engine", &engine.url])
        .current_dir(cwd.path())
        .env_remove("III_URL")
        .env("III_CALL_DEFAULTS", defaults(mode))
        .env("III_TELEMETRY_ENABLED", "false")
        .kill_on_drop(true);
    (command, cwd)
}

async fn output(mut command: Command) -> std::process::Output {
    tokio::time::timeout(WAIT, command.output())
        .await
        .expect("iii call ended")
        .expect("iii call ran")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vm_that_exits_with_0_prints_its_output_and_succeeds() {
    let engine = start().await;
    let (command, _cwd) = iii_call(
        &engine,
        "exit-0",
        &["sandbox::start", "worker=api", "command=python main.py"],
    );
    let out = output(command).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "hello from the guest\n");
    assert_eq!(text(&out.stderr), "guest warning\n");

    let started = engine.seen.started.lock().unwrap().clone().unwrap();
    assert_eq!(started["worker_name"], "api");
    assert_eq!(started["auth"], "scoped-token");
    assert_eq!(started["env"]["KEY"], "secret");
    assert_eq!(started["source"]["run_override"], "python main.py");
    assert_eq!(started["build"]["build_id"], "sha256:abc");
    assert_eq!(started["build"]["build_dir"], "/srv/project/.iii/build/api");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vm_that_fails_makes_the_call_fail_with_its_code_in_the_message() {
    let engine = start().await;
    let (command, _cwd) = iii_call(&engine, "exit-3", &["sandbox::start", "worker=api"]);
    let out = output(command).await;
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("WORKER_EXITED"), "{stderr}");
    assert!(stderr.contains("exited with code 3"), "{stderr}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn sigterm_stops_the_vm_and_the_call_exits_with_the_stop() {
    let engine = start().await;
    let (mut command, _cwd) = iii_call(
        &engine,
        "run",
        &["sandbox::start", "worker=api", "--print-result"],
    );
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = tokio::time::timeout(WAIT, lines.next_line())
        .await
        .expect("the guest wrote")
        .unwrap();
    assert_eq!(first.as_deref(), Some("hello from the guest"));

    let pid = child.id().unwrap().to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );
    let mut rest = Vec::new();
    while let Ok(Some(line)) = tokio::time::timeout(WAIT, lines.next_line())
        .await
        .expect("iii call ended its output")
    {
        rest.push(line);
    }
    let status = tokio::time::timeout(WAIT, child.wait())
        .await
        .expect("iii call exited")
        .unwrap();
    assert!(status.success(), "exited with {status}");
    assert!(
        engine.seen.stopped.load(Ordering::SeqCst),
        "worker::stop was not called"
    );
    let rest = rest.join("\n");
    assert!(rest.contains("\"stopped\": true"), "{rest}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_build_prints_its_output_and_answers_with_the_image() {
    let engine = start().await;
    let (command, _cwd) = iii_call(
        &engine,
        "build-ok",
        &["sandbox::build", "worker=api", "--print-result"],
    );
    let out = output(command).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("installing dependencies\n"), "{stdout}");
    assert!(stdout.contains("\"build_id\": \"sha256:abc\""), "{stdout}");
    assert!(
        stdout.contains("\"adapter_version\": \"0.5.0\""),
        "{stdout}"
    );
    assert_eq!(text(&out.stderr), "a warning\n");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn sigterm_cancels_a_build_and_the_adapter_aborts_it() {
    let engine = start().await;
    let (mut command, _cwd) = iii_call(&engine, "build-hang", &["sandbox::build", "worker=api"]);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = tokio::time::timeout(WAIT, lines.next_line())
        .await
        .expect("the build wrote")
        .unwrap();
    assert_eq!(first.as_deref(), Some("installing dependencies"));

    let pid = child.id().unwrap().to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );
    let out = tokio::time::timeout(WAIT, child.wait_with_output())
        .await
        .expect("iii call exited")
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("OPERATION_CANCELLED"), "{stderr}");
    let deadline = Instant::now() + WAIT;
    while !engine.seen.build_aborted.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "the adapter never saw the build's output close"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_on_a_package_is_refused() {
    let engine = start().await;
    let mut defaults: Value = serde_json::from_str(&defaults("exit-0")).unwrap();
    defaults["sandbox::start"]["source"] =
        json!({ "kind": "bundle", "package": "api", "host_path": "/srv/project/.iii/workers/api" });
    let (mut command, _cwd) = iii_call(
        &engine,
        "exit-0",
        &["sandbox::start", "worker=api", "command=sh"],
    );
    command.env("III_CALL_DEFAULTS", defaults.to_string());
    let out = output(command).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("SANDBOX_INVALID_REQUEST"));
    assert!(engine.seen.started.lock().unwrap().is_none());
}
