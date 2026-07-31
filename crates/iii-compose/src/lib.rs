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
//! connects, serves `compose::*` in its own namespace, and starts/stops the
//! graph. Registry (`package://`) resolution and per-container log capture are
//! the two declared gaps.

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
pub mod remote;
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
    let namespace = namespace::project_namespace(namespace, &compose.name, &compose.path);
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
        ComposeCommand::Daemon {
            id,
            file,
            engine_url,
            namespace,
            up_on_start,
        } => match run_daemon(id, &file, engine_url, namespace.as_deref(), up_on_start).await {
            Ok(()) => 0,
            Err(err) => report_error(&err),
        },
    }
}

/// Daemon mode: bind to one compose file, serve `compose::*`, and stop the
/// children on the way out.
///
/// Starting the daemon does not implicitly `up` anything — that is a separate
/// decision the operator makes.
async fn run_daemon(
    id: String,
    file: &std::path::Path,
    engine_url: String,
    namespace: Option<&str>,
    up_on_start: bool,
) -> Result<()> {
    let compose = ComposeFile::load(file)?;
    // Validate before announcing: a daemon that is serving `compose::up` for a
    // project that cannot start is worse than one that never came up.
    manifest::validate_offline(&compose, "pending")?;

    let daemon = daemon::Daemon::start(id, compose, engine_url, namespace).await?;
    remote::register(&daemon);

    use colored::Colorize;
    println!(
        "compose daemon {} bound to {}",
        daemon.id.bold(),
        daemon.file.path.display().to_string().dimmed()
    );
    println!(
        "  {} {}",
        "project namespace:".dimmed(),
        daemon.project_namespace.cyan()
    );
    println!(
        "  {} iii trigger compose::status --namespace {}",
        "reach it with:".dimmed(),
        daemon.id
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
        println!(
            "{}",
            format!("started {} container(s)", result.containers.len()).green()
        );
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

        if stop {
            break;
        }

        if let Some(error) = daemon.fatal_error() {
            eprintln!("error[REGISTRATION_REJECTED]: {error}");
            daemon.shutdown().await;
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
            "deferred (package:// resolution not implemented):".yellow(),
            report.deferred_packages.join(", ")
        );
    }
}

/// One indented `label: value` line under a container.
fn detail(label: &str, value: &str) {
    use colored::Colorize;
    println!("    {} {value}", format!("{label}:").dimmed());
}
