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

pub mod cli;
pub mod config;
pub mod configuration;
pub mod daemon;
pub mod dag;
pub mod engine;
pub mod error;
pub mod hooks;
pub mod lifecycle;
pub mod manifest;
pub mod namespace;
pub mod process;
pub mod project;
pub mod registry;
pub mod remote;
pub mod report;
pub mod spawn;
pub mod state;

pub use cli::{ComposeCli, ComposeCommand};
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
        ComposeCommand::Serve {
            engine_url,
            daemon_namespace,
        } => match serve(engine_url, daemon_namespace).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
        ComposeCommand::Detach {
            engine_url,
            daemon_namespace,
        } => match detach(&engine_url, &daemon_namespace).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
        ComposeCommand::Attach { daemon_namespace } => match attach(daemon_namespace).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
    }
}

/// Where a detached daemon writes what it says.
///
/// Under the daemon's own id, because several of them share this machine as
/// readily as they share an engine: one log per daemon, next to the state that
/// daemon owns.
fn daemon_log_path(daemon_namespace: &str) -> Result<std::path::PathBuf> {
    // The same root the state uses, so relocating one relocates both. Reading
    // the home directory here instead would put the log somewhere `--attach`
    // does not look the moment an operator moves the state.
    Ok(state::StateStore::root()?
        .join(daemon_namespace)
        .join("daemon.log"))
}

/// Serves `compose::*` until asked to stop.
///
/// No project is loaded here. A daemon that has just started knows nothing,
/// and the first `compose::up id=… file=…` is what teaches it — which is what
/// lets one daemon hold several projects without being restarted for each.
async fn serve(engine_url: String, daemon_namespace: String) -> Result<()> {
    use colored::Colorize;

    let daemon = daemon::Daemon::start(engine_url, daemon_namespace);
    remote::register(&daemon);

    // Announce only once the engine has accepted this daemon. A rejection
    // arrives within a round trip, and printing "serving" before hearing it
    // would put a success line above the error that contradicts it.
    //
    // Bounded, because not being connected yet is not a failure: a daemon
    // started before its engine waits and reconnects, and should say it is
    // there rather than sit silent until the engine appears.
    const ACCEPTED_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);
    let deadline = tokio::time::Instant::now() + ACCEPTED_WITHIN;
    while tokio::time::Instant::now() < deadline {
        if let Some(error) = daemon.fatal_error() {
            daemon.abandon().await;
            return Err(rejected(&daemon, &error));
        }
        if daemon.engine().is_connected() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    println!("compose {}", "serving".green());
    println!("  {} {}", "engine:".dimmed(), daemon.engine_url);
    println!("  {} {}", "namespace:".dimmed(), daemon.daemon_namespace);
    // Printed with this daemon's own address already in it. Several daemons
    // share an engine, and which one a call reaches is a flag an operator
    // should not have to work out — least of all when the id is a uuid nobody
    // is going to retype.
    println!(
        "  {} iii trigger compose::up --namespace {} file=./worker-compose.yaml",
        "start a project:".dimmed(),
        daemon.daemon_namespace
    );

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

/// How long `--detach` waits for the background daemon to start serving.
const DETACH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Re-launches this command in the background and waits until it serves.
///
/// A re-exec rather than a fork: the async runtime is already up by the time
/// this runs, and only the calling thread survives a fork — the daemon would
/// come back in a runtime with no workers. The child gets the same argv minus
/// the detach flag, plus a guard in its environment so it can never fork again.
async fn detach(engine_url: &str, daemon_namespace: &str) -> Result<()> {
    use colored::Colorize;

    let log_path = daemon_log_path(daemon_namespace)?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    // Append, never truncate: a restart extends the story rather than erasing
    // the run that explains why it was needed.
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
    // The child is told the id rather than left to resolve one: an unnamed
    // invocation generates a fresh uuid, so re-execing the same argv would
    // bring the daemon back under a name nobody has — including the caller who
    // is about to be handed this one. Any `--id` already there is dropped
    // first, so the flag appears exactly once whichever way we got here.
    let mut args: Vec<String> = Vec::new();
    let mut skip_next = false;
    for arg in std::env::args().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "-d" || arg == "--detach" {
            continue;
        }
        if arg == "--ns" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--ns=") {
            continue;
        }
        args.push(arg);
    }
    args.push("--ns".to_string());
    args.push(daemon_namespace.to_string());

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
    // Ctrl-C on the shell that started it does not reach the projects.
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
                id: pid.to_string(),
                message: format!(
                    "the daemon exited with {} before it began serving. Its output is in {}",
                    status.code().unwrap_or(-1),
                    log_path.display()
                ),
            });
        }

        if daemon_is_serving(engine_url, daemon_namespace, pid).await {
            println!(
                "compose {} {}",
                "detached".green(),
                format!("(pid {pid})").dimmed()
            );
            // Printed only once the daemon answered: handing back an id for a
            // process that died on start would be a name that addresses
            // nothing. Its own line, in a fixed shape, because a script's next
            // move is to capture it.
            println!("  {} {daemon_namespace}", "namespace:".dimmed());
            println!("  {} {}", "log:".dimmed(), log_path.display());
            println!(
                "  {} iii compose --attach --ns {daemon_namespace}",
                "follow it with:".dimmed()
            );
            println!(
                "  {} iii trigger compose::up --namespace {daemon_namespace} file=./worker-compose.yaml",
                "start a project:".dimmed()
            );
            println!(
                "  {} iii trigger compose::stop --namespace {daemon_namespace}",
                "stop it with:".dimmed()
            );
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            // Left running on purpose: it may still be coming up, and killing a
            // daemon mid-start would be worse than saying so.
            return Err(ComposeError::DetachFailed {
                id: pid.to_string(),
                message: format!(
                    "no answer after {}s. It is still running as pid {pid}; its output is in {}",
                    DETACH_TIMEOUT.as_secs(),
                    log_path.display()
                ),
            });
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Whether *this* child is the daemon answering `compose::list`.
///
/// The pid has to match: another daemon on the same engine would answer for
/// the one just launched, and the launch would report success over a process
/// about to be refused.
async fn daemon_is_serving(engine_url: &str, daemon_namespace: &str, expected_pid: u32) -> bool {
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

    // Named, not bare: another machine's daemon answering this would report a
    // pid that is not the child we just launched, and `--detach` would keep
    // waiting for a daemon that is already serving.
    let request = TriggerRequest {
        function_id: "compose::list".to_string(),
        payload: serde_json::json!({}),
        action: None,
        timeout_ms: Some(2_000),
    };
    let answered = client
        .trigger(request.namespace(daemon_namespace))
        .await
        .ok()
        .and_then(|response| response.get("daemon_pid").and_then(|pid| pid.as_u64()))
        .is_some_and(|pid| pid == u64::from(expected_pid));

    client.shutdown();
    answered
}

/// Follows a detached daemon's log until interrupted.
///
/// Attaching is a view, not a reparenting: the daemon keeps its own process
/// group and keeps running when this returns. What comes back is what it says
/// from here on, plus the tail of what it already said.
/// Every namespace on this machine that has a detached daemon's log.
///
/// The log is what `--attach` follows, so its presence is the question being
/// asked — not whether a process is alive, which is `compose::list`'s job.
fn attachable_namespaces() -> Vec<String> {
    let Ok(root) = state::StateStore::root() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut found: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().join("daemon.log").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    found.sort();
    found
}

/// Which daemon to follow.
///
/// Named, or — when only one has ever detached here — that one. A machine with
/// several is asked rather than guessed at: attaching to the wrong daemon is a
/// silent wrong answer, and the ids are uuids nobody remembers.
fn resolve_attach_target(requested: Option<String>) -> Result<String> {
    if let Some(namespace) = requested {
        return Ok(namespace);
    }

    let mut candidates = attachable_namespaces();
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(ComposeError::NoDaemonToAttach { candidates: None }),
        _ => Err(ComposeError::NoDaemonToAttach {
            candidates: Some(candidates.join(", ")),
        }),
    }
}

async fn attach(requested: Option<String>) -> Result<()> {
    let daemon_namespace = resolve_attach_target(requested)?;
    let daemon_namespace = daemon_namespace.as_str();
    use colored::Colorize;
    use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

    let log_path = daemon_log_path(daemon_namespace)?;
    let mut file = tokio::fs::File::open(&log_path)
        .await
        .map_err(|source| ComposeError::Io {
            path: log_path.clone(),
            source,
        })?;

    // Start near the end: an operator attaching wants what is happening, not
    // every line since the daemon was first started days ago.
    const TAIL_BYTES: i64 = 8 * 1024;
    let len = file
        .metadata()
        .await
        .map(|meta| meta.len() as i64)
        .unwrap_or(0);
    let _ = file
        .seek(std::io::SeekFrom::Start((len - TAIL_BYTES).max(0) as u64))
        .await;

    println!("{} {}", "attached to".dimmed(), log_path.display());
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);

    loop {
        line.clear();
        let read = tokio::select! {
            _ = &mut interrupted => return Ok(()),
            read = reader.read_line(&mut line) => read,
        };
        match read {
            Ok(0) => {
                // End of file is not the end of the daemon: it is simply quiet.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Ok(_) => print!("{line}"),
            Err(err) => {
                return Err(ComposeError::Io {
                    path: log_path,
                    source: err,
                });
            }
        }
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
