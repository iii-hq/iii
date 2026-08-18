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
pub mod edit;
pub mod engine;
pub mod error;
pub mod hooks;
pub mod interpolate;
pub mod lifecycle;
pub mod manifest;
pub mod name;
pub mod namespace;
pub mod process;
pub mod project;
pub mod registry;
pub mod remote;
pub mod report;
pub mod spawn;
pub mod state;

pub use cli::{ComposeCli, ComposeCommand, ComposeSub};
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
            start,
        } => match serve(engine_url, daemon_namespace, start).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
    }
}

/// Serves `compose::*` until asked to stop.
///
/// `start` is the project `iii compose up` named. Without it no project is
/// loaded: a daemon that has just started knows nothing, and the first
/// `compose::up file=…` is what teaches it — which is what lets one daemon hold
/// several projects without being restarted for each.
async fn serve(
    engine_url: String,
    daemon_namespace: String,
    start: Option<std::path::PathBuf>,
) -> Result<()> {
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
    // should not have to work out, and a generated name is meant to be read
    // once and typed from memory.
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

    // The project named on the command line, brought up before the first call
    // can arrive. A failure ends the command: rollback has already stopped
    // whatever started, so there is nothing left for this daemon to supervise
    // and staying would serve an empty project nobody asked for.
    if let Some(file) = &start {
        println!();
        let operation_id = uuid::Uuid::new_v4().to_string();
        let result = daemon.up(Some(file), None, operation_id).await;
        match result {
            Ok(result) if result.status == lifecycle::OpStatus::Failed => {
                daemon.shutdown().await;
                return Err(ComposeError::ProjectDidNotStart { path: file.clone() });
            }
            Ok(_) => {}
            Err(err) => {
                daemon.shutdown().await;
                return Err(err);
            }
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
