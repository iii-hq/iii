//! `iii call` against a real engine: the streams a call carries, the first signal as a stop, and
//! `III_CALL_DEFAULTS` (tech-specs/2026-11-19-iii-call-sandbox, section 3).

use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use iii::EngineBuilder;
use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    Error, IIIClient, InitOptions, InputStream, OutputStream, RegisterFunction, WorkerIdentityMode,
    register_worker,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const WAIT: Duration = Duration::from_secs(10);

struct TestEngine {
    _builder: EngineBuilder,
    _sdk: IIIClient,
    /// A worker in the `orders` namespace, with a function id `default` has too.
    _orders: IIIClient,
    _dir: tempfile::TempDir,
    url: String,
    /// Set by `cli-streams::ticks` once a write failed: its caller left.
    stopped: Arc<AtomicBool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Lines {
    count: u32,
    stdout: OutputStream,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Both {
    stdout: OutputStream,
    stderr: OutputStream,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Ticks {
    stdout: OutputStream,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Echo {
    stdin: InputStream,
    stdout: OutputStream,
}

/// Binds stdin and never reads it.
#[derive(Deserialize, schemars::JsonSchema)]
struct Ignore {
    #[serde(rename = "stdin")]
    _stdin: InputStream,
    stdout: OutputStream,
}

/// What `cli-streams::endless` answers once it was stopped: a stream that never ends.
#[derive(Serialize, schemars::JsonSchema)]
struct Endless {
    stopped: bool,
    stdout: InputStream,
}

#[derive(Serialize, schemars::JsonSchema)]
struct Exported {
    rows: u32,
    stdout: InputStream,
}

async fn ready(function: &FunctionRef) {
    let deadline = Instant::now() + WAIT;
    while !function.local_dispatch_active() {
        assert!(Instant::now() < deadline, "the function never registered");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Builds the engine and starts its workers, the worker manager on `port`. On a failure, the
/// workers already started are shut down.
async fn listen(dir: &std::path::Path, port: u16) -> anyhow::Result<EngineBuilder> {
    let builder = EngineBuilder::new()
        .add_worker(
            "configuration",
            Some(json!({"adapter":{"name":"fs","config":{"directory":dir}}})),
        )
        .add_worker(
            "iii-worker-manager",
            Some(json!({"host":"127.0.0.1","port":port})),
        )
        .build()
        .await?;
    for worker in builder.running() {
        if let Err(error) = worker
            .worker
            .start_background_tasks(worker.shutdown_tx.subscribe(), worker.shutdown_tx.clone())
            .await
        {
            for worker in builder.running() {
                let _ = worker.shutdown_tx.send(true);
            }
            return Err(error);
        }
    }
    Ok(builder)
}

async fn start() -> TestEngine {
    // The port is free when it is picked, not reserved: another test, or another process, can
    // bind it before the engine does. Losing that race costs a retry on another port, a bounded
    // number of times, never the test.
    let mut attempt = 1;
    let (builder, dir, port) = loop {
        let dir = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        match listen(dir.path(), port).await {
            Ok(builder) => break (builder, dir, port),
            Err(error) if attempt < 5 => {
                eprintln!("the engine did not start on port {port}, retrying: {error:#}");
                attempt += 1;
            }
            Err(error) => panic!("the engine did not start: {error:#}"),
        }
    };
    let url = format!("ws://127.0.0.1:{port}");
    let sdk = register_worker(
        &url,
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: "cli-streams".to_string(),
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
    sdk.wait_until_registered(WAIT)
        .await
        .expect("engine reachable");

    let stopped = Arc::new(AtomicBool::new(false));
    let mut functions = vec![
        sdk.register_function(
            "cli-streams::lines",
            RegisterFunction::new_async(|req: Lines| async move {
                for n in 1..=req.count {
                    req.stdout.write(format!("line {n}\n").as_bytes()).await?;
                }
                Ok::<_, Error>(json!({ "written": req.count }))
            }),
        ),
        sdk.register_function(
            "cli-streams::both",
            RegisterFunction::new_async(|req: Both| async move {
                req.stdout.write(b"to stdout\n").await?;
                req.stderr.write(b"to stderr\n").await?;
                Ok::<_, Error>(json!({}))
            }),
        ),
        sdk.register_function(
            "cli-streams::echo",
            RegisterFunction::new_async(|req: Echo| async move {
                let text = req.stdin.text().await?;
                req.stdout.write(text.to_uppercase().as_bytes()).await?;
                Ok::<_, Error>(json!({}))
            }),
        ),
        // Reads its stdin and answers success even when the stream was aborted.
        sdk.register_function(
            "cli-streams::tolerant",
            RegisterFunction::new_async(|req: Echo| async move {
                let text = req.stdin.text().await.unwrap_or_default();
                req.stdout.write(text.as_bytes()).await?;
                Ok::<_, Error>(json!({}))
            }),
        ),
        sdk.register_function(
            "cli-streams::ignore",
            RegisterFunction::new_async(|req: Ignore| async move {
                req.stdout.write(b"done\n").await?;
                Ok::<_, Error>(json!({}))
            }),
        ),
        sdk.register_function(
            "cli-streams::export",
            RegisterFunction::new_async(|_: Value| async move {
                Ok::<_, Error>(Exported {
                    rows: 2,
                    stdout: InputStream::from_bytes("a,1\nb,2\n"),
                })
            }),
        ),
        // Answers with the payload it got and the namespace it runs in.
        sdk.register_function(
            "cli-streams::payload",
            RegisterFunction::new_async(|req: Value| async move {
                Ok::<_, Error>(json!({ "namespace": "default", "payload": req }))
            }),
        ),
        // Answers with a result stream far bigger than a pipe holds.
        sdk.register_function(
            "cli-streams::flood",
            RegisterFunction::new_async(|_: Value| async move {
                Ok::<_, Error>(Exported {
                    rows: 0,
                    stdout: InputStream::from_bytes(vec![b'x'; 8 << 20]),
                })
            }),
        ),
        sdk.register_function(
            "cli-streams::fail",
            RegisterFunction::new_async(|_: Value| async move {
                Err::<Value, _>(Error::Runtime("it failed".into()))
            }),
        ),
    ];
    let seen = stopped.clone();
    functions.push(sdk.register_function(
        "cli-streams::ticks",
        RegisterFunction::new_async(move |req: Ticks| {
            let seen = seen.clone();
            async move {
                req.stdout.write(b"ready\n").await?;
                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if req.stdout.write(b"tick\n").await.is_err() {
                        seen.store(true, Ordering::SeqCst);
                        return Ok::<_, Error>(json!({ "stopped": true }));
                    }
                }
            }
        }),
    ));
    // Like `ticks`, but once stopped it answers with a result stream it keeps open for good.
    let client = sdk.clone();
    let held = Arc::new(std::sync::Mutex::new(Vec::new()));
    functions.push(sdk.register_function(
        "cli-streams::endless",
        RegisterFunction::new_async(move |req: Ticks| {
            let client = client.clone();
            let held = held.clone();
            async move {
                req.stdout.write(b"ready\n").await?;
                while req.stdout.write(b"tick\n").await.is_ok() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                let pipe = client.pipe().await?;
                held.lock().unwrap().push(pipe.output);
                Ok::<_, Error>(Endless {
                    stopped: true,
                    stdout: pipe.input,
                })
            }
        }),
    ));
    let orders = register_worker(
        &url,
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: "cli-streams-orders".to_string(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            namespace: Some("orders".to_string()),
            identity: WorkerIdentityMode::Explicit,
            ..Default::default()
        },
    );
    orders
        .wait_until_registered(WAIT)
        .await
        .expect("engine reachable");
    functions.push(orders.register_function(
        "cli-streams::payload",
        RegisterFunction::new_async(|req: Value| async move {
            Ok::<_, Error>(json!({ "namespace": "orders", "payload": req }))
        }),
    ));
    for function in &functions {
        ready(function).await;
    }
    TestEngine {
        _builder: builder,
        _sdk: sdk,
        _orders: orders,
        _dir: dir,
        url,
        stopped,
    }
}

/// `iii call` with `args`, against `url`, from an empty directory and without `III_URL`.
fn iii_call(url: &str, cwd: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_iii"));
    command
        .arg("call")
        .args(args)
        .args(["--engine", url])
        .current_dir(cwd)
        .env_remove("III_URL")
        .env_remove("III_CALL_DEFAULTS")
        .env_remove("III_NAMESPACE")
        .env("III_TELEMETRY_ENABLED", "false")
        .kill_on_drop(true);
    command
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
async fn bound_streams_carry_the_output_and_the_input() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();

    // The schema binds `stdout`; the result is not printed beside the streamed output.
    let out = output(iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::lines", "count=3"],
    ))
    .await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "line 1\nline 2\nline 3\n");

    // `--print-result` prints it after the streams ended.
    let out = output(iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::lines", "count=1", "--print-result"],
    ))
    .await;
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("line 1\n"), "{stdout}");
    assert!(stdout.contains("\"written\": 1"), "{stdout}");

    // stdout and stderr go each to their own descriptor.
    let out = output(iii_call(&engine.url, cwd.path(), &["cli-streams::both"])).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "to stdout\n");
    assert_eq!(text(&out.stderr), "to stderr\n");

    // stdin is bound too.
    let mut echo = iii_call(&engine.url, cwd.path(), &["cli-streams::echo"]);
    echo.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = echo.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut stdin, b"hello")
        .await
        .unwrap();
    drop(stdin);
    let out = tokio::time::timeout(WAIT, child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "HELLO");

    // `--no-follow` sends no stream, and the function, which needs one, refuses the payload.
    let out = output(iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::lines", "count=1", "--no-follow"],
    ))
    .await;
    assert_eq!(out.status.code(), Some(1));
}

/// A function that answers without reading its stdin ends the call, though the parent keeps the
/// stdin pipe open: no read of it holds up the exit.
#[tokio::test(flavor = "multi_thread")]
async fn an_unread_stdin_does_not_hold_up_the_exit() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::ignore"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let _open = child.stdin.take().unwrap();
    let out = tokio::time::timeout(WAIT, child.wait_with_output())
        .await
        .expect("iii call exited while its stdin stayed open")
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "done\n");
}

/// Input this process could not read is lost: the call fails, though the function took the
/// aborted stream and answered success.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_stdin_fails_the_call() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::tolerant"]);
    // Reading a directory fails (EISDIR).
    command.stdin(std::fs::File::open(cwd.path()).unwrap());
    let out = output(command).await;
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the stdin stream failed"), "{stderr}");
}

/// A script Compose starts carries its container's `III_WORKER_NAME` and `III_NAMESPACE`. The
/// call must not take that name: here a live worker holds it, and the call still runs.
#[tokio::test(flavor = "multi_thread")]
async fn the_call_does_not_take_the_worker_name_of_its_process() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();

    // The name and namespace the fixture worker holds.
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::lines", "count=1"]);
    command
        .env("III_WORKER_NAME", "cli-streams")
        .env("III_NAMESPACE", "default");
    let out = output(command).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "line 1\n");

    // `--help` reads the schema through its own connection, which takes no name either.
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::lines", "--help"]);
    command
        .env("III_WORKER_NAME", "cli-streams")
        .env("III_NAMESPACE", "default");
    let out = output(command).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("count"), "{}", text(&out.stdout));
}

/// Without `-n` a call resolves in `default`, as `--help` says, also from a process whose
/// `III_NAMESPACE` names another namespace, as every script Compose starts does.
#[tokio::test(flavor = "multi_thread")]
async fn iii_namespace_does_not_route_a_call_without_n() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();

    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::lines", "count=2"]);
    command.env("III_NAMESPACE", "elsewhere");
    let out = output(command).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "line 1\nline 2\n");

    // `-n` still routes where it says.
    let out = output(iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::lines", "count=2", "-n", "elsewhere"],
    ))
    .await;
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("elsewhere"),
        "{}",
        text(&out.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn result_streams_are_followed_unless_asked_not_to() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();

    let out = output(iii_call(&engine.url, cwd.path(), &["cli-streams::export"])).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "a,1\nb,2\n");

    let out = output(iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::export", "--no-follow"],
    ))
    .await;
    let result: Value = serde_json::from_slice(&out.stdout).expect("the result as JSON");
    assert_eq!(result["rows"], 2);
    assert_eq!(result["stdout"]["direction"], "read");
}

#[tokio::test(flavor = "multi_thread")]
async fn defaults_come_from_the_environment_for_their_function_only() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let defaults =
        r#"{"default": {"cli-streams::lines": {"count": 2}, "other::fn": {"count": 9}}}"#;

    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::lines"]);
    command.env("III_CALL_DEFAULTS", defaults);
    let out = output(command).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "line 1\nline 2\n");

    // A key=value pair overrides the default.
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::lines", "count=1"]);
    command.env("III_CALL_DEFAULTS", defaults);
    let out = output(command).await;
    assert_eq!(text(&out.stdout), "line 1\n");

    // A value that is not a JSON object is a usage error, which quotes nothing of it.
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::lines"]);
    command.env("III_CALL_DEFAULTS", "secret-token");
    let out = output(command).await;
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("III_CALL_DEFAULTS is not valid JSON"),
        "{stderr}"
    );
    assert!(!stderr.contains("secret-token"), "{stderr}");
}

/// An entry of `III_CALL_DEFAULTS` is for its function id in the namespace it is written under:
/// the same function id in another namespace, another handler, receives nothing of it.
#[tokio::test(flavor = "multi_thread")]
async fn defaults_reach_only_the_namespace_they_are_written_under() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let defaults = r#"{"orders": {"cli-streams::payload": {"token": "orders-token"}}}"#;
    let call = |args: &[&str]| {
        let mut command = iii_call(&engine.url, cwd.path(), args);
        command.env("III_CALL_DEFAULTS", defaults);
        command
    };

    // Without `-n` the call resolves in `default`: the token written for `orders` stays out.
    let out = output(call(&["cli-streams::payload"])).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["namespace"], "default", "{result}");
    assert!(result["payload"].get("token").is_none(), "{result}");

    // In `orders` it is the entry's own.
    let out = output(call(&["cli-streams::payload", "-n", "orders"])).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["namespace"], "orders", "{result}");
    assert_eq!(result["payload"]["token"], "orders-token", "{result}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_call_prints_its_error_on_one_line() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let out = output(iii_call(&engine.url, cwd.path(), &["cli-streams::fail"])).await;
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    let line = stderr
        .lines()
        .find(|line| line.starts_with("Error: "))
        .unwrap_or_else(|| panic!("no error line: {stderr}"));
    let error: Value = serde_json::from_str(&line["Error: ".len()..]).expect("one JSON line");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("it failed")),
        "{error}"
    );
}

/// Arguments that cannot make a call exit with 2 before anything is sent: no engine listens at
/// `NOWHERE`, so a case that got past its check would fail to connect instead.
#[tokio::test(flavor = "multi_thread")]
async fn usage_errors_exit_with_2_before_any_call() {
    const NOWHERE: &str = "ws://127.0.0.1:9";
    let cwd = tempfile::tempdir().unwrap();
    let cases: &[(&str, &[&str], &str)] = &[(
        "http://localhost",
        &["cli-streams::lines"],
        "must start with ws://",
    )];
    for (engine, args, expected) in cases {
        let out = output(iii_call(engine, cwd.path(), args)).await;
        let stderr = text(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains(expected), "{args:?}: {stderr}");
    }

    // Defaults that are not UTF-8 are refused, not read as unset.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut command = iii_call(NOWHERE, cwd.path(), &["cli-streams::lines"]);
        command.env(
            "III_CALL_DEFAULTS",
            std::ffi::OsStr::from_bytes(b"{\"cli-streams::lines\": {\"count\": 2}}\xff"),
        );
        let out = output(command).await;
        let stderr = text(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{stderr}");
        assert!(
            stderr.contains("III_CALL_DEFAULTS is not valid UTF-8"),
            "{stderr}"
        );
    }
}

/// The first SIGTERM closes the streams, the function sees its writes fail and answers, and
/// `iii call` exits with that answer.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_first_signal_stops_the_function_and_waits_for_its_answer() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let mut command = iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::ticks", "--print-result"],
    );
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = tokio::time::timeout(WAIT, lines.next_line())
        .await
        .expect("the function wrote")
        .unwrap();
    assert_eq!(first.as_deref(), Some("ready"));

    let pid = child.id().unwrap().to_string();
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .unwrap();
    assert!(killed.success());

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
        engine.stopped.load(Ordering::SeqCst),
        "the function was not stopped"
    );
    let rest = rest.join("\n");
    assert!(rest.contains("\"stopped\": true"), "{rest}");
}

/// The first signal after the answer ends the call with 130 though its output is stuck: a write
/// into a pipe whose reader stopped reading is not waited for.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_signal_after_the_answer_does_not_wait_for_a_stuck_write() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let mut command = iii_call(&engine.url, cwd.path(), &["cli-streams::flood"]);
    // The stdout pipe stays open and is never read: it fills, and the copy blocks in its write.
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    // One byte read says the answer came and its stream is followed; the rest stays unread.
    let mut unread = child.stdout.take().unwrap();
    let mut first = [0u8; 1];
    tokio::time::timeout(
        WAIT,
        tokio::io::AsyncReadExt::read_exact(&mut unread, &mut first),
    )
    .await
    .expect("the result stream was followed")
    .unwrap();
    assert_eq!(&first, b"x");

    let pid = child.id().unwrap().to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-INT", &pid])
            .status()
            .unwrap()
            .success()
    );
    let status = tokio::time::timeout(WAIT, child.wait())
        .await
        .expect("iii call waited for the stuck write")
        .unwrap();
    assert_eq!(status.code(), Some(130));
}

/// A result stream that arrives after the first signal is cancelled too: the function answered a
/// stop with a stream it never closes, and `iii call` still exits with that answer.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn result_streams_after_a_stop_are_cancelled_too() {
    let engine = start().await;
    let cwd = tempfile::tempdir().unwrap();
    let mut command = iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::endless", "--print-result"],
    );
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = tokio::time::timeout(WAIT, lines.next_line())
        .await
        .expect("the function wrote")
        .unwrap();
    assert_eq!(first.as_deref(), Some("ready"));

    let pid = child.id().unwrap().to_string();
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .unwrap();
    assert!(killed.success());

    let mut rest = Vec::new();
    while let Some(line) = tokio::time::timeout(WAIT, lines.next_line())
        .await
        .expect("iii call kept following a result stream after the stop")
        .unwrap()
    {
        rest.push(line);
    }
    let status = tokio::time::timeout(WAIT, child.wait())
        .await
        .expect("iii call exited")
        .unwrap();
    assert!(status.success(), "exited with {status}");
    let rest = rest.join("\n");
    assert!(rest.contains("\"stopped\": true"), "{rest}");
}
