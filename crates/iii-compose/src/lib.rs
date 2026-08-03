// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Compose daemon for iii.
//!
//! A compose project declares several workers, their dependencies and how each
//! one starts; the daemon validates the declaration, resolves configuration,
//! starts the graph in order and supervises the children it created. It is a
//! greenfield crate: it shares no runtime code with `crates/iii-worker`, whose
//! lifecycle system it is meant to replace rather than extend.
//!
//! Status: `iii compose validate` is complete offline, and daemon mode
//! connects, serves `compose::*` in its own namespace, starts and stops the
//! graph, installs `package://` workers from the registry, keeps watching its
//! children after they are ready, and adopts whatever survived a restart.

pub mod cli;
pub mod config;
pub mod configuration;
pub mod daemon;
pub mod dag;
pub mod engine;
pub mod error;
pub mod hooks;
pub mod lifecycle;
pub mod logs;
pub mod manifest;
pub mod namespace;
pub mod process;
pub mod registry;
pub mod remote;
pub mod report;
pub mod spawn;
pub mod state;

pub use cli::{ComposeAction, ComposeCli, ComposeCommand};
pub use config::{ComposeFile, Container, WorkerSource};
pub use error::{ComposeError, Result};
pub use manifest::{StartSpec, ValidationReport};

/// Validates a compose project offline: schema, dependency graph, worker
/// directories, manifests and start commands. Also reports the namespace the
/// project would register under.
pub fn validate_project(
    file: &std::path::Path,
    namespace: Option<&str>,
) -> Result<ValidationReport> {
    let compose = ComposeFile::load(file)?;
    let namespace = namespace::project_namespace(namespace, compose.name.as_deref());
    manifest::validate_offline(&compose, &namespace)
}

/// Entry point behind `iii compose`. Returns the process exit code, matching
/// the other `iii` subcommands.
pub async fn run(cli: ComposeCli) -> i32 {
    let command = match cli.plan() {
        Ok(command) => command,
        Err(err) => return report_error(&err),
    };

    match command {
        ComposeCommand::Validate { file, namespace } => {
            match validate_project(&file, namespace.as_deref()) {
                Ok(report) => {
                    print_report(&report);
                    0
                }
                Err(err) => report_error(&err),
            }
        }
        ComposeCommand::Logs {
            namespace,
            engine_url,
            container,
            tail,
            follow,
        } => match show_logs(&namespace, &engine_url, container.as_deref(), tail, follow).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
        ComposeCommand::Stop {
            namespace,
            engine_url,
        } => match stop_daemon(&namespace, &engine_url).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
        ComposeCommand::Daemon {
            id,
            file,
            engine_url,
            up_on_start,
            detach,
        } => {
            if detach {
                return match detach_daemon(id.as_deref(), &file, &engine_url).await {
                    Ok(()) => 0,
                    Err(err) => report_error(&err),
                };
            }
            match run_daemon(id, &file, engine_url, up_on_start).await {
                Ok(()) => 0,
                Err(err) => report_error(&err),
            }
        }
    }
}

/// Daemon mode: bind to one compose file, serve `compose::*`, and stop the
/// children on the way out.
///
/// Starting the daemon does not implicitly `up` anything — that is a separate
/// decision the operator makes.
async fn run_daemon(
    id: Option<String>,
    file: &std::path::Path,
    engine_url: String,
    up_on_start: bool,
) -> Result<()> {
    let compose = ComposeFile::load(file)?;
    let namespace = namespace::project_namespace(id.as_deref(), compose.name.as_deref());
    // Validate before announcing: a daemon that is serving `compose::up` for a
    // project that cannot start is worse than one that never came up.
    manifest::validate_offline(&compose, &namespace)?;

    let daemon = daemon::Daemon::start(id, compose, engine_url).await?;
    remote::register(&daemon);

    use colored::Colorize;
    println!(
        "compose daemon {} bound to {}",
        daemon.id.bold(),
        daemon.file.path.display().to_string().dimmed()
    );
    println!(
        "  {} {}",
        "namespace:".dimmed(),
        daemon.project_namespace.cyan()
    );
    println!(
        "  {} iii compose logs --ns {}",
        "reach it with:".dimmed(),
        daemon.project_namespace
    );

    // `--up` makes the daemon a one-shot: bring the project up, and if that
    // fails there is nothing left to supervise, so stop with a non-zero code
    // instead of idling over a project that never started.
    if up_on_start {
        let result = daemon.up(None, "startup".to_string()).await;
        if result.status == lifecycle::OpStatus::Failed {
            daemon.shutdown().await;
            return Err(ComposeError::UpFailed {
                operation_id: result.operation_id,
            });
        }
    }

    // Serve until asked to stop, or until the engine refuses this identity.
    //
    // Both signals matter and for different reasons: an operator presses Ctrl-C
    // (SIGINT), while every supervisor — systemd, docker, a `kill` in a script
    // — sends SIGTERM. Handling only the first leaves the children orphaned on
    // the path that production actually takes.
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    #[cfg(unix)]
    let mut terminated = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|err| ComposeError::SpawnFailed {
            container: "<daemon>".to_string(),
            message: format!("could not listen for SIGTERM: {err}"),
        })?;

    loop {
        #[cfg(unix)]
        let stop = tokio::select! {
            _ = &mut interrupted => true,
            _ = terminated.recv() => true,
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
        };
        #[cfg(not(unix))]
        let stop = tokio::select! {
            _ = &mut interrupted => true,
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
        };

        // `compose::stop` answered its caller a moment ago; leaving now is
        // what makes that answer true.
        if stop || daemon.stop_requested() {
            break;
        }

        if let Some(error) = daemon.fatal_error() {
            eprintln!("error[REGISTRATION_REJECTED]: {error}");
            // Not `shutdown`: the children recorded under this id belong to the
            // daemon that already holds it.
            daemon.abandon().await;
            return Err(ComposeError::EngineCallFailed {
                function: "engine::workers::register".to_string(),
                message: error.to_string(),
            });
        }
    }

    println!(
        "{}",
        format!("stopping {} container(s)...", daemon.file.containers.len()).dimmed()
    );
    daemon.shutdown().await;
    Ok(())
}

/// How long `--detach` waits for the background daemon to start serving before
/// giving up on it.
const DETACH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Re-launches this command in the background and waits until it serves.
///
/// A re-exec rather than a fork: the async runtime is already up by the time
/// this runs, and only the calling thread survives a fork — the daemon would
/// come back in a runtime with no workers. The child gets the same argv minus
/// the detach flag, plus a guard in its environment so it can never fork again.
///
/// Returns only once `compose::status` answers in the daemon's namespace. A
/// detached daemon that dies immediately — a duplicate `--id`, an engine that
/// is not there — has to fail this command, not return a shell prompt over
/// nothing.
async fn detach_daemon(id: Option<&str>, file: &std::path::Path, engine_url: &str) -> Result<()> {
    use colored::Colorize;

    // Validate before forking anything: a project that cannot start should say
    // so on the terminal the operator is looking at.
    let compose = ComposeFile::load(file)?;
    let namespace = namespace::project_namespace(id, compose.name.as_deref());
    manifest::validate_offline(&compose, &namespace)?;

    // The state directory follows the daemon's worker name, resolved the same
    // way the child process will resolve it.
    let id = namespace::daemon_worker_name(id, compose.name.as_deref());
    let id = id.as_str();
    let store = state::StateStore::for_daemon(id)?;
    std::fs::create_dir_all(store.dir()).map_err(|source| ComposeError::Io {
        path: store.dir().to_path_buf(),
        source,
    })?;
    let log_path = store.dir().join("daemon.log");
    // Append, never truncate: a restart should extend the story, and a second
    // daemon started on a taken `--id` must not erase the log of the one that
    // is actually running.
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|source| ComposeError::Io {
            path: log_path.clone(),
            source,
        })?;

    let exe = std::env::current_exe().map_err(|source| ComposeError::Io {
        path: std::path::PathBuf::from("<current exe>"),
        source,
    })?;
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "-d" && arg != "--detach")
        .collect();

    let mut command = std::process::Command::new(exe);
    command
        .args(&args)
        .env(cli::DETACHED_GUARD, "1")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|source| ComposeError::Io {
            path: log_path.clone(),
            source,
        })?)
        .stderr(log);

    // Its own process group, so closing the terminal does not SIGHUP it and
    // Ctrl-C on the shell that started it does not reach the project.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);

    let mut child = command.spawn().map_err(|source| ComposeError::Io {
        path: std::path::PathBuf::from("<detached daemon>"),
        source,
    })?;
    let pid = child.id();

    let deadline = tokio::time::Instant::now() + DETACH_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(ComposeError::DetachFailed {
                id: id.to_string(),
                message: format!(
                    "the daemon exited with {} before it began serving. Its output is in {}",
                    status.code().unwrap_or(-1),
                    log_path.display()
                ),
            });
        }

        if daemon_is_serving(engine_url, &namespace, pid).await {
            println!(
                "compose daemon {} detached {}",
                id.bold(),
                format!("(pid {pid})").dimmed()
            );
            println!("  {} {}", "logs:".dimmed(), log_path.display());
            println!("  {} iii compose stop --ns {namespace}", "stop with:".dimmed());
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            // Left running on purpose: it may still be coming up, and killing
            // a daemon that is mid-start would be worse than saying so.
            return Err(ComposeError::DetachFailed {
                id: id.to_string(),
                message: format!(
                    "no answer from the daemon after {}s. It is still running as pid {pid}; its \
                     output is in {}",
                    DETACH_TIMEOUT.as_secs(),
                    log_path.display()
                ),
            });
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Whether *this* child is the daemon answering in `id`'s namespace.
///
/// The pid has to match: `compose::*` is addressed by namespace, so a daemon
/// already holding this `--id` would answer for the one we just launched, and
/// the launch would report success over a process about to be rejected.
async fn daemon_is_serving(engine_url: &str, id: &str, expected_pid: u32) -> bool {
    use iii_sdk::{InitOptions, protocol::TriggerRequest, register_worker};

    let client = register_worker(
        engine_url,
        InitOptions {
            metadata: Some(iii_sdk::iii::WorkerMetadata {
                name: format!("compose-detach-{}", std::process::id()),
                description: Some("iii compose --detach".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let answered = client
        .trigger(
            TriggerRequest {
                function_id: "compose::status".to_string(),
                payload: serde_json::json!({}),
                action: None,
                timeout_ms: Some(2_000),
            }
            .namespace(id),
        )
        .await
        .ok()
        .and_then(|response| response.get("daemon_pid").and_then(|pid| pid.as_u64()))
        .is_some_and(|pid| pid == u64::from(expected_pid));

    client.shutdown();
    answered
}

/// How long `stop` waits for the daemon to actually be gone.
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Stops a running daemon and waits until it is gone.
///
/// Waiting is the point. `compose::stop` answers before the teardown runs — it
/// has to, or the reply would race the socket closing — so returning on the
/// acknowledgement alone would hand back a prompt while the containers are
/// still being signalled. A script that stops a project and immediately starts
/// another would find the names still taken.
async fn stop_daemon(namespace: &str, engine_url: &str) -> Result<()> {
    use colored::Colorize;
    use iii_sdk::{InitOptions, protocol::TriggerRequest, register_worker};

    let client = register_worker(
        engine_url,
        InitOptions {
            metadata: Some(iii_sdk::iii::WorkerMetadata {
                name: format!("compose-stop-{}", std::process::id()),
                description: Some("iii compose stop".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let acknowledged = client
        .trigger(
            TriggerRequest {
                function_id: "compose::stop".to_string(),
                payload: serde_json::json!({}),
                action: None,
                timeout_ms: Some(15_000),
            }
            .namespace(namespace),
        )
        .await
        .map_err(|source| ComposeError::EngineCallFailed {
            function: "compose::stop".to_string(),
            message: source.to_string(),
        })?;

    let stopping: Vec<&str> = acknowledged
        .get("stopping")
        .and_then(|value| value.as_array())
        .map(|containers| containers.iter().filter_map(|c| c.as_str()).collect())
        .unwrap_or_default();

    if stopping.is_empty() {
        println!("{}", "no container was running".dimmed());
    } else {
        println!("{} {}", "stopping:".dimmed(), stopping.join(", "));
    }

    // Gone means it stopped answering. Polling `compose::status` is the same
    // question from the outside as "is the daemon still there".
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    while daemon_answers(&client, namespace).await {
        if tokio::time::Instant::now() >= deadline {
            client.shutdown();
            return Err(ComposeError::EngineCallFailed {
                function: "compose::stop".to_string(),
                message: format!(
                    "the daemon in '{namespace}' was still answering after {}s",
                    STOP_TIMEOUT.as_secs()
                ),
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    println!("compose daemon in {} {}", namespace.bold(), "stopped".green());
    client.shutdown();
    Ok(())
}

async fn daemon_answers(client: &iii_sdk::IIIClient, namespace: &str) -> bool {
    use iii_sdk::protocol::TriggerRequest;

    client
        .trigger(
            TriggerRequest {
                function_id: "compose::status".to_string(),
                payload: serde_json::json!({}),
                action: None,
                timeout_ms: Some(2_000),
            }
            .namespace(namespace),
        )
        .await
        .is_ok()
}

/// How often `--follow` asks for more.
const FOLLOW_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Lines requested per poll while following. Larger than what is printed: the
/// overlap is what lets the next poll find where the last one stopped.
const FOLLOW_TAIL: usize = 200;

/// Asks a running daemon what its containers printed, and renders it.
///
/// Connects as an ordinary client under its own name: `compose::*` lives in the
/// namespace of the daemon's `--id`, and registering under that name would
/// collide with the daemon itself on `(namespace, worker_name)`.
async fn show_logs(
    namespace: &str,
    engine_url: &str,
    container: Option<&str>,
    tail: usize,
    follow: bool,
) -> Result<()> {
    use iii_sdk::{InitOptions, register_worker};

    let client = register_worker(
        engine_url,
        InitOptions {
            metadata: Some(iii_sdk::iii::WorkerMetadata {
                name: format!("compose-logs-{}", std::process::id()),
                description: Some("iii compose logs".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let response = fetch_logs(&client, namespace, container, tail).await?;
    let mut seen = std::collections::BTreeMap::new();
    print_logs(&response, &mut seen, true);

    if follow {
        // Ctrl-C is the documented way out, so leave through it cleanly rather
        // than letting the process die on a half-open socket.
        let interrupted = tokio::signal::ctrl_c();
        tokio::pin!(interrupted);
        loop {
            tokio::select! {
                _ = &mut interrupted => break,
                _ = tokio::time::sleep(FOLLOW_INTERVAL) => {}
            }
            let response = fetch_logs(&client, namespace, container, FOLLOW_TAIL).await?;
            print_logs(&response, &mut seen, false);
        }
    }

    client.shutdown();
    Ok(())
}

async fn fetch_logs(
    client: &iii_sdk::IIIClient,
    namespace: &str,
    container: Option<&str>,
    tail: usize,
) -> Result<serde_json::Value> {
    use iii_sdk::protocol::TriggerRequest;

    let mut payload = serde_json::json!({ "tail": tail });
    if let Some(container) = container {
        payload["container"] = serde_json::Value::String(container.to_string());
    }

    client
        .trigger(
            TriggerRequest {
                function_id: "compose::logs".to_string(),
                payload,
                action: None,
                timeout_ms: Some(15_000),
            }
            .namespace(namespace),
        )
        .await
        .map_err(|source| ComposeError::EngineCallFailed {
            function: "compose::logs".to_string(),
            message: source.to_string(),
        })
}

/// Renders the `compose::logs` payload the way the daemon's own console does,
/// so the same container looks the same in both places.
///
/// `seen` carries the highest sequence number printed per container, which is
/// how a follow poll finds where the previous one stopped. Matching on content
/// would not do: a worker prints the same banner every time it restarts, so the
/// newest copy looks exactly like the line we stopped at.
fn print_logs(
    response: &serde_json::Value,
    seen: &mut std::collections::BTreeMap<String, u64>,
    headers: bool,
) {
    use colored::Colorize;

    let containers = response
        .get("containers")
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default();

    if headers && containers.is_empty() {
        println!("{}", "no container has printed anything yet".dimmed());
        return;
    }

    for (key, lines) in containers {
        let lines = lines.as_array().cloned().unwrap_or_default();
        let color = report::container_color(&key);

        if headers {
            println!(
                "{} {}",
                key.color(color).bold(),
                format!("({} line(s))", lines.len()).dimmed()
            );
            // A container asked for by name and silent is worth saying out
            // loud: otherwise an empty screen reads as a failed command.
            if lines.is_empty() {
                println!("  {}", "nothing captured".dimmed());
            }
        }

        // The daemon's buffer is bounded, so a follower that fell behind will
        // see the oldest line it is handed jump past where it stopped. Saying
        // so beats presenting the output as whole.
        if let (Some(&last), Some(first)) = (seen.get(&key), lines.first())
            && seq_of(first) > last + 1
        {
            println!(
                "{}",
                format!("[{key}] ...{} line(s) were dropped", seq_of(first) - last - 1).dimmed()
            );
        }

        for entry in &lines {
            if seen.get(&key).is_some_and(|last| seq_of(entry) <= *last) {
                continue;
            }
            let text = entry.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let tag = format!("[{key}]");
            // Same shape as the live console: the tag carries the container's
            // colour, and stderr is the bold one. A note compose wrote itself
            // is dimmed whole, so it never reads as something the worker said.
            match entry.get("stream").and_then(|s| s.as_str()) {
                Some("stderr") => println!("{} {text}", tag.color(color).bold()),
                Some("compose") => println!("{} {}", tag.dimmed(), text.dimmed()),
                _ => println!("{} {text}", tag.color(color)),
            }
        }

        if let Some(last) = lines.last() {
            seen.insert(key.clone(), seq_of(last));
        }
    }
}

/// Position of a line in its container's output. A line without one sorts as
/// the very first, which only happens against a daemon older than this field.
fn seq_of(entry: &serde_json::Value) -> u64 {
    entry.get("seq").and_then(|s| s.as_u64()).unwrap_or(0)
}

fn report_error(err: &ComposeError) -> i32 {
    use colored::Colorize;
    eprintln!("{} {err}", format!("error[{}]:", err.code()).red().bold());
    1
}

fn print_report(report: &ValidationReport) {
    use colored::Colorize;

    println!(
        "{} {}",
        report.project.bold(),
        format!("{} container(s) valid", report.start_order.len()).green()
    );
    println!("{} {}", "namespace:".dimmed(), report.namespace.cyan());
    println!(
        "{} {}",
        "start order:".dimmed(),
        report.start_order.join(" -> ")
    );

    for plan in &report.resolved {
        let command = match &plan.start {
            StartSpec::Shell(command) => command.clone(),
            StartSpec::Exec { program, args } => {
                format!("{} {}", program.display(), args.join(" "))
            }
        };
        println!("  {} {command}", format!("{}:", plan.key).bold());
        detail("dir", &plan.working_dir.display().to_string());
        detail("readiness", &format!("{}s", plan.startup_timeout.as_secs()));
        if let Some(config_name) = &plan.config_name {
            detail("config", config_name);
        }
        // Names only: an env_file's values are routinely secrets.
        if !plan.environment.is_empty() {
            detail("env", &plan.environment.join(", "));
        }
        for env_file in &plan.env_file {
            detail("env_file", &env_file.display().to_string());
        }
    }

    if !report.deferred_packages.is_empty() {
        println!(
            "{} {}",
            // Validate stays offline: a `package://` reference is checked against
            // the registry at start, not here.
            "resolved at start (package://, needs the registry):".yellow(),
            report.deferred_packages.join(", ")
        );
    }
}

/// One indented `label: value` line under a container.
fn detail(label: &str, value: &str) {
    use colored::Colorize;
    println!("    {} {value}", format!("{label}:").dimmed());
}
