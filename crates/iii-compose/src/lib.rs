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
//! One daemon serves an engine and holds any number of projects, each named by
//! an id its operator chose. It registers `compose::*` in `default`, validates
//! a project before taking it on, starts and stops each graph, installs
//! `package://` workers from the registry, keeps watching its children after
//! they are ready, and adopts whatever survived a restart.

pub mod build;
pub mod cli;
pub mod config;
pub mod configuration;
pub mod daemon;
pub mod dag;
pub mod edit;
pub mod engine;
pub mod error;
pub mod hooks;
pub mod interpolate;
pub mod lifecycle;
pub mod logs;
mod managed_engine;
pub mod manifest;
pub mod namespace;
pub mod operation;
mod parallelism;
pub mod process;
pub mod project;
pub mod registry;
pub mod remote;
pub mod report;
mod shutdown;
pub mod spawn;
pub mod state;

pub use cli::{BuildCli, ComposeCli, ComposeCommand, ComposeLogsCli, ComposeSubcommand};
pub use config::{ComposeFile, Container, EngineSpec, RestartPolicy, WorkerSource};
pub use error::{ComposeError, Result};
pub use manifest::{StartSpec, ValidationReport, VmSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMode {
    Managed { url: String },
    External { url: String },
}

/// Resolves the engine URL and ownership after the compose file is parsed.
///
/// An explicit CLI URL always selects an external engine. Without one, an
/// `engine:` section is managed only when `--up` starts that file; a bare
/// daemon connects to its URL without taking ownership. File configuration
/// wins over the process environment, and the local engine address is the
/// final fallback.
pub fn resolve_engine_mode(
    file: Option<&ComposeFile>,
    start: bool,
    explicit_engine_url: Option<&str>,
    environment_engine_url: Option<&str>,
) -> EngineMode {
    if let Some(url) = explicit_engine_url {
        return EngineMode::External {
            url: url.to_string(),
        };
    }

    if start && let Some(engine) = file.and_then(|file| file.engine.as_ref()) {
        return EngineMode::Managed {
            url: engine.url.clone(),
        };
    }

    let url = file
        .and_then(|file| file.engine.as_ref())
        .map(|engine| engine.url.as_str())
        .or(environment_engine_url)
        .unwrap_or(config::DEFAULT_ENGINE_URL);
    EngineMode::External {
        url: url.to_string(),
    }
}

/// Validates a compose project offline: schema, dependency graph, worker
/// directories, manifests and start commands. Also reports the namespace the
/// project would register under.
pub fn validate_project(
    file: &std::path::Path,
    namespace: Option<&str>,
) -> Result<ValidationReport> {
    let compose = ComposeFile::load(file)?;
    let namespace = namespace::project_namespace(namespace, compose.namespace.as_deref());
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
        ComposeCommand::Build { file } => match build::build(&file).await {
            Ok(_) => 0,
            Err(err) => report_error(&err),
        },
        ComposeCommand::Serve {
            explicit_engine_url,
            explicit_daemon_namespace,
            file,
            start,
        } => match serve(explicit_engine_url, explicit_daemon_namespace, file, start).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
        ComposeCommand::Logs {
            explicit_engine_url,
            explicit_daemon_namespace,
            file,
            container,
            tail,
            follow,
            stream,
        } => match follow_worker_logs(
            explicit_engine_url,
            explicit_daemon_namespace,
            file,
            container,
            tail,
            follow,
            stream,
        )
        .await
        {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
    }
}

async fn follow_worker_logs(
    explicit_engine_url: Option<String>,
    explicit_daemon_namespace: Option<String>,
    file: Option<std::path::PathBuf>,
    container: Option<String>,
    tail: usize,
    follow: bool,
    stream: Option<logs::LogStream>,
) -> Result<()> {
    use iii_sdk::{InitOptions, protocol::TriggerRequest, register_worker};

    let local_config_path = file
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new(cli::DEFAULT_COMPOSE_FILE));
    let local_file = load_invocation_file(local_config_path, false)?;
    let daemon_namespace = resolve_daemon_namespace(explicit_daemon_namespace, local_file.as_ref());
    let environment_engine_url = std::env::var("III_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    let engine_url = match resolve_engine_mode(
        local_file.as_ref(),
        false,
        explicit_engine_url.as_deref(),
        environment_engine_url.as_deref(),
    ) {
        EngineMode::Managed { url } | EngineMode::External { url } => url,
    };

    let client = register_worker(
        &engine_url,
        InitOptions {
            metadata: Some(iii_sdk::iii::WorkerMetadata {
                name: format!(
                    "compose-logs:{}:{}",
                    std::process::id(),
                    uuid::Uuid::new_v4()
                ),
                description: Some("Read retained Compose worker output".to_string()),
                ..Default::default()
            }),
            // Environment namespace belongs to managed workers, not to this
            // short-lived operator client. Every call below routes explicitly.
            namespace: Some(namespace::DEFAULT_NAMESPACE.to_string()),
            ..Default::default()
        },
    );
    let mut cursors = std::collections::BTreeMap::new();
    let mut first = true;

    loop {
        let mut payload = serde_json::Map::new();
        if let Some(file) = &file {
            payload.insert(
                "file".to_string(),
                serde_json::Value::String(file.to_string_lossy().into_owned()),
            );
        }
        if let Some(container) = &container {
            payload.insert(
                "container".to_string(),
                serde_json::Value::String(container.clone()),
            );
        }
        if let Some(stream) = stream {
            payload.insert(
                "stream".to_string(),
                serde_json::Value::String(match stream {
                    logs::LogStream::Stdout => "stdout".to_string(),
                    logs::LogStream::Stderr => "stderr".to_string(),
                }),
            );
        }
        payload.insert("tail".to_string(), serde_json::json!(tail));
        if !cursors.is_empty() {
            let values = cursors
                .iter()
                .map(|(container, cursor): (&String, &logs::LogCursor)| {
                    (
                        container.clone(),
                        serde_json::json!({
                            "generation": cursor.generation,
                            "offset": cursor.offset,
                        }),
                    )
                })
                .collect();
            payload.insert("cursors".to_string(), serde_json::Value::Object(values));
        }
        if follow {
            payload.insert("wait_ms".to_string(), serde_json::json!(logs::MAX_WAIT_MS));
        }

        let request = TriggerRequest {
            function_id: "compose::logs".to_string(),
            payload: serde_json::Value::Object(payload),
            action: None,
            timeout_ms: Some(10_000),
        }
        .namespace(&daemon_namespace);

        let result = tokio::select! {
            result = client.trigger(request) => Some(result),
            _ = tokio::signal::ctrl_c(), if follow => None,
        };
        let Some(result) = result else {
            client.shutdown_async().await;
            return Ok(());
        };
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                client.shutdown_async().await;
                return Err(ComposeError::EngineCallFailed {
                    function: "compose::logs".to_string(),
                    message: error.to_string(),
                });
            }
        };
        let outcome: logs::LogsOutcome = match serde_json::from_value(value) {
            Ok(outcome) => outcome,
            Err(error) => {
                client.shutdown_async().await;
                return Err(ComposeError::EngineCallFailed {
                    function: "compose::logs".to_string(),
                    message: format!("daemon returned an invalid log response: {error}"),
                });
            }
        };

        if let Err(source) = print_worker_logs(outcome, &mut cursors) {
            client.shutdown_async().await;
            if source.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(ComposeError::Io {
                path: std::path::PathBuf::from("<stdout>"),
                source,
            });
        }

        if !follow {
            client.shutdown_async().await;
            return Ok(());
        }
        if first && cursors.is_empty() {
            // A project can be declared before any worker has produced output.
            // Avoid a hot loop until the daemon has a file it can long-poll.
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        first = false;
    }
}

fn print_worker_logs(
    outcome: logs::LogsOutcome,
    cursors: &mut std::collections::BTreeMap<String, logs::LogCursor>,
) -> std::io::Result<()> {
    use colored::Colorize;
    use std::io::Write;

    let mut stdout = std::io::stdout().lock();
    for batch in outcome.containers {
        let color = report::container_color(&batch.container);
        for entry in batch.entries {
            let tag = match entry.stream {
                logs::LogStream::Stdout => format!("[{}]", batch.container).color(color),
                logs::LogStream::Stderr => format!("[{}]", batch.container).color(color).bold(),
            };
            writeln!(stdout, "{tag} {}", entry.message)?;
        }
        if batch.truncated {
            eprintln!(
                "{}",
                format!(
                    "[{}] older output is no longer retained; showing the most recent retained lines",
                    batch.container
                )
                .yellow()
            );
        }
        if let Some(cursor) = batch.cursor {
            cursors.insert(batch.container, cursor);
        }
    }
    stdout.flush()
}

/// Serves `compose::*` until asked to stop.
///
/// `file` configures the daemon when it exists. `start` controls only whether
/// that file is also brought up before the first remote call.
async fn serve(
    explicit_engine_url: Option<String>,
    explicit_daemon_namespace: Option<String>,
    file: std::path::PathBuf,
    start: bool,
) -> Result<()> {
    use colored::Colorize;

    // Parse before any child is started. A bare daemon can run outside a
    // project, but an existing default file still supplies its URL and
    // namespace.
    let initial_file = load_invocation_file(&file, start)?;
    let daemon_namespace =
        resolve_daemon_namespace(explicit_daemon_namespace.clone(), initial_file.as_ref());
    let environment_engine_url = std::env::var("III_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    let engine_mode = resolve_engine_mode(
        initial_file.as_ref(),
        start,
        explicit_engine_url.as_deref(),
        environment_engine_url.as_deref(),
    );
    let engine_url = match &engine_mode {
        EngineMode::Managed { url } | EngineMode::External { url } => url.clone(),
    };

    let engine_policy = match &engine_mode {
        EngineMode::Managed { .. } => {
            let Some(policy) = initial_file
                .as_ref()
                .and_then(daemon::EnginePolicy::managed)
            else {
                unreachable!("managed mode is selected only from an engine section");
            };
            policy
        }
        EngineMode::External { .. } => {
            match initial_file.as_ref().filter(|file| file.engine.is_some()) {
                Some(file) if explicit_engine_url.is_some() => {
                    daemon::EnginePolicy::external_overriding(file)
                }
                Some(file) => daemon::EnginePolicy::external_from_file(file),
                None => daemon::EnginePolicy::External,
            }
        }
    };

    // Install this before the first owned process starts. Signal delivery uses
    // separate process groups, so compose must stay alive long enough to stop
    // each child itself.
    let shutdown = shutdown::ShutdownSignal::install()?;

    let managed_engine = match engine_mode {
        EngineMode::Managed { .. } => {
            let Some(owner) = initial_file.as_ref() else {
                unreachable!("managed mode is selected only from a compose file");
            };
            let Some(spec) = owner.engine.as_ref() else {
                unreachable!("managed mode is selected only from an engine section");
            };
            let engine = managed_engine::ManagedEngine::start(spec, &daemon_namespace).await?;
            println!("engine {}", "started".green());
            println!("  {} {}", "pid:".dimmed(), engine.pid());
            println!("  {} {}", "owner:".dimmed(), owner.path.display());
            println!(
                "  {} {}",
                "config:".dimmed(),
                engine.config_path().display()
            );
            println!("  {} {}", "logs:".dimmed(), engine.log_path().display());
            println!("  {} {}", "follow logs:".dimmed(), engine.follow_command());
            println!();
            Some(engine)
        }
        EngineMode::External { .. } => None,
    };

    let result = if shutdown.requested() {
        Ok(())
    } else {
        let start_file = start.then_some(file);
        serve_daemon(
            engine_url,
            daemon_namespace,
            explicit_daemon_namespace,
            start_file,
            managed_engine.as_ref(),
            engine_policy,
            shutdown,
        )
        .await
    };

    if let Some(engine) = &managed_engine {
        println!("{}", "stopping engine...".dimmed());
        engine.stop_with_default_grace().await;
    }

    result
}

fn resolve_daemon_namespace(
    explicit_daemon_namespace: Option<String>,
    initial_file: Option<&ComposeFile>,
) -> String {
    namespace::project_namespace(
        explicit_daemon_namespace.as_deref(),
        initial_file.and_then(|file| file.namespace.as_deref()),
    )
}

fn load_invocation_file(file: &std::path::Path, required: bool) -> Result<Option<ComposeFile>> {
    match ComposeFile::load(file) {
        Ok(file) => Ok(Some(file)),
        Err(ComposeError::Io { source, .. })
            if !required && source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

async fn serve_daemon(
    engine_url: String,
    daemon_namespace: String,
    project_namespace_override: Option<String>,
    start: Option<std::path::PathBuf>,
    managed_engine: Option<&managed_engine::ManagedEngine>,
    engine_policy: daemon::EnginePolicy,
    shutdown: shutdown::ShutdownSignal,
) -> Result<()> {
    use colored::Colorize;

    let daemon = daemon::Daemon::start(
        engine_url,
        daemon_namespace,
        project_namespace_override,
        engine_policy,
    );

    // Announce only once the engine has accepted this daemon. A rejection
    // arrives within a round trip, and printing "serving" before hearing it
    // would put a success line above the error that contradicts it.
    //
    // Bounded, because not being connected yet is not a failure: a daemon
    // started before its engine waits and reconnects, and should say it is
    // there rather than sit silent until the engine appears.
    const EXTERNAL_ACCEPTED_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);
    const MANAGED_READY_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);
    let accepted_within = if managed_engine.is_some() {
        MANAGED_READY_WITHIN
    } else {
        EXTERNAL_ACCEPTED_WITHIN
    };
    let deadline = tokio::time::Instant::now() + accepted_within;
    while tokio::time::Instant::now() < deadline {
        if let Some(error) = daemon.fatal_error() {
            daemon.abandon().await;
            return Err(rejected(&daemon, &error));
        }
        if daemon.engine().is_connected() {
            break;
        }
        if let Some(engine) = managed_engine
            && let process::Outcome::Exited(status) = engine.poll()
        {
            daemon.shutdown().await;
            return Err(engine_exited(engine, status).await);
        }
        let mut interrupted = shutdown.clone();
        tokio::select! {
            _ = interrupted.wait() => {
                daemon.shutdown().await;
                return Ok(());
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    if let Some(engine) = managed_engine
        && !daemon.engine().is_connected()
    {
        daemon.shutdown().await;
        return Err(ComposeError::EngineReadinessTimeout {
            engine_url: daemon.engine_url.clone(),
            seconds: accepted_within.as_secs(),
            tail: engine.log_tail(),
        });
    }

    println!("compose {}", "serving".green());
    println!("  {} {}", "engine:".dimmed(), daemon.engine_url);
    println!("  {} {}", "namespace:".dimmed(), daemon.daemon_namespace);
    // Printed with this daemon's own address already in it. Several daemons
    // can share an engine, and which one a call reaches is a flag an operator
    // should not have to work out.
    //
    // Not printed when a project was named: the operator already started one,
    // and the line would be telling them to do what they just did.
    if start.is_none() {
        println!(
            "  {} iii trigger compose::up --namespace {} file=./worker-compose.yaml",
            "start a project:".dimmed(),
            daemon.daemon_namespace
        );
    }

    let startup_operation = if start.is_some() {
        Some(daemon.operations.create(1).await)
    } else {
        None
    };
    // Read-only operations, cancellation, and stop remain available while the
    // foreground renderer owns the initial startup tree.
    remote::register_controls(&daemon);

    // A failed initial project still ends the command. Cancellation rolls its
    // partial startup back but leaves the daemon available for later calls.
    if let (Some(file), Some(operation)) = (&start, startup_operation) {
        println!();
        let operation_id = operation.id().to_string();
        let startup_shutdown = shutdown.clone().or(shutdown::ShutdownSignal::from_receiver(
            operation.cancellation(),
        ));
        let result = daemon
            .up_until_shutdown(Some(file), None, operation_id, startup_shutdown)
            .await;
        match result {
            Ok(None) => {
                operation
                    .finish(
                        operation::OperationStatus::Cancelled,
                        "initial project startup cancelled",
                    )
                    .await;
                if shutdown.requested() || daemon.stop_requested() {
                    println!(
                        "{}",
                        "startup interrupted; stopping every project...".dimmed()
                    );
                    daemon.shutdown().await;
                    return Ok(());
                }
                println!("{}", "startup cancelled; daemon remains available".dimmed());
            }
            Ok(Some(result)) if result.status == lifecycle::OpStatus::Failed => {
                let error = ComposeError::ProjectDidNotStart { path: file.clone() };
                operation
                    .finish(operation::OperationStatus::Failed, error.to_string())
                    .await;
                daemon.shutdown().await;
                return Err(error);
            }
            Ok(Some(_)) => {
                operation
                    .finish(
                        operation::OperationStatus::Succeeded,
                        "initial project is ready",
                    )
                    .await;
            }
            Err(err) => {
                operation
                    .finish(operation::OperationStatus::Failed, err.to_string())
                    .await;
                daemon.shutdown().await;
                return Err(err);
            }
        }
    }
    // Publish project mutations only after the foreground startup tree is complete.
    // The renderer intentionally owns one global in-place block; admitting a
    // remote mutation during initial --up would replace that block and interleave
    // two operations' cursor movement.
    remote::register_mutations(&daemon);

    // Serve until asked to stop, or until the engine refuses this identity.
    //
    loop {
        let mut interrupted = shutdown.clone();
        let stop = tokio::select! {
            _ = interrupted.wait() => true,
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
        };

        // `compose::stop` answered its caller a moment ago; leaving now is
        // what makes that answer true.
        if stop || daemon.stop_requested() {
            break;
        }

        if let Some(engine) = managed_engine
            && let process::Outcome::Exited(status) = engine.poll()
        {
            println!(
                "{}",
                "managed engine exited; stopping every project...".dimmed()
            );
            daemon.shutdown().await;
            return Err(engine_exited(engine, status).await);
        }

        if let Some(error) = daemon.fatal_error() {
            // Not `shutdown`: children recorded under these ids may belong to
            // the daemon that already holds them.
            daemon.abandon().await;
            return Err(rejected(&daemon, &error));
        }
    }

    println!("{}", "stopping every project...".dimmed());
    daemon.shutdown().await;
    Ok(())
}

async fn engine_exited(
    engine: &managed_engine::ManagedEngine,
    status: std::process::ExitStatus,
) -> ComposeError {
    engine.finish_logging().await;
    ComposeError::EngineExited {
        code: status.code().unwrap_or(-1),
        tail: engine.log_tail(),
    }
}

/// A registration rejection, as the thing it always is in practice.
fn rejected(daemon: &daemon::Daemon, error: &iii_sdk::Error) -> ComposeError {
    ComposeError::DaemonAlreadyServing {
        engine_url: daemon.engine_url.clone(),
        detail: error.to_string(),
    }
}

fn report_error(err: &ComposeError) -> i32 {
    use colored::Colorize;
    eprintln!("{} {err}", format!("error[{}]:", err.code()).red().bold());
    1
}

#[cfg(test)]
mod tests {
    use super::{ComposeFile, load_invocation_file, resolve_daemon_namespace};

    fn compose_with_namespace() -> ComposeFile {
        ComposeFile::parse(
            "namespace: orders\ncontainers:\n  api:\n    worker: path://./api\n",
            "/srv/app/worker-compose.yaml",
        )
        .unwrap()
    }

    #[test]
    fn initial_compose_namespace_is_inherited() {
        let compose = compose_with_namespace();
        assert_eq!(resolve_daemon_namespace(None, Some(&compose)), "orders");
    }

    #[test]
    fn explicit_daemon_namespace_overrides_compose_file() {
        let compose = compose_with_namespace();
        assert_eq!(
            resolve_daemon_namespace(Some("development".to_string()), Some(&compose)),
            "development"
        );
    }

    #[test]
    fn daemon_without_a_namespace_uses_default() {
        assert_eq!(
            resolve_daemon_namespace(None, None),
            crate::namespace::DEFAULT_NAMESPACE
        );
    }

    #[test]
    fn initial_compose_without_a_namespace_uses_default() {
        let compose = ComposeFile::parse(
            "containers:\n  api:\n    worker: path://./api\n",
            "/srv/app/worker-compose.yaml",
        )
        .unwrap();

        assert_eq!(
            resolve_daemon_namespace(None, Some(&compose)),
            crate::namespace::DEFAULT_NAMESPACE
        );
    }

    #[test]
    fn bare_compose_loads_an_existing_default_file_for_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            "namespace: orders\nengine: { workers: {} }\ncontainers: {}\n",
        )
        .unwrap();

        let loaded = load_invocation_file(&path, false).unwrap();

        assert_eq!(
            loaded.and_then(|file| file.namespace).as_deref(),
            Some("orders")
        );
    }

    #[test]
    fn bare_compose_tolerates_a_missing_default_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");

        let loaded = load_invocation_file(&path, false).unwrap();

        assert!(loaded.is_none());
    }
}
