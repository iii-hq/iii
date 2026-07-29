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
//! Status: the offline path (`iii compose validate`) is complete. Daemon-mode
//! supervision is not implemented yet and reports `DAEMON_NOT_IMPLEMENTED`.

pub mod cli;
pub mod config;
pub mod configuration;
pub mod dag;
pub mod error;
pub mod hooks;
pub mod manifest;
pub mod namespace;
pub mod process;
pub mod spawn;

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
        ComposeCommand::Daemon { .. } => report_error(&ComposeError::DaemonNotImplemented),
    }
}

fn report_error(err: &ComposeError) -> i32 {
    eprintln!("error[{}]: {err}", err.code());
    1
}

fn print_report(report: &ValidationReport) {
    println!(
        "{}: {} container(s) valid",
        report.project,
        report.start_order.len()
    );
    println!("namespace: {}", report.namespace);
    println!("start order: {}", report.start_order.join(" -> "));
    for plan in &report.resolved {
        let command = match &plan.start {
            StartSpec::Shell(command) => command.clone(),
            StartSpec::Exec { program, args } => {
                format!("{} {}", program.display(), args.join(" "))
            }
        };
        println!("  {}: {command}", plan.key);
        println!("    dir: {}", plan.working_dir.display());
        if let Some(config_name) = &plan.config_name {
            println!("    config: {config_name}");
        }
    }
    if !report.deferred_packages.is_empty() {
        println!(
            "deferred (package:// resolution not implemented): {}",
            report.deferred_packages.join(", ")
        );
    }
}
