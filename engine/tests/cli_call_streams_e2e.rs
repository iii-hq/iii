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
    for function in &functions {
        ready(function).await;
    }
    TestEngine {
        _builder: builder,
        _sdk: sdk,
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

    // `--no-bind` sends no stream, and the function refuses the payload.
    let out = output(iii_call(
        &engine.url,
        cwd.path(),
        &["cli-streams::lines", "count=1", "--no-bind"],
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
    let defaults = r#"{"cli-streams::lines": {"count": 2}, "other::fn": {"count": 9}}"#;

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
        NOWHERE,
        &["cli-streams::lines", "--json", "[1]", "--stdout", "out"],
        "a stream can only be bound in an object payload",
    ), (
        NOWHERE,
        &["cli-streams::echo", "--stdout", "io", "--stdin", "io"],
        "--stdout and --stdin both bind the field \"io\"",
    ), (
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
