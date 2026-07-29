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
pub mod dag;
pub mod error;
pub mod manifest;
pub mod namespace;
pub mod spawn;

pub use cli::{ComposeAction, ComposeCli, ComposeCommand};
pub use config::{ComposeFile, Container, WorkerSource};
pub use error::{ComposeError, Result};
pub use manifest::{StartSpec, ValidationReport};

/// Validates a compose project offline: schema, dependency graph, worker
/// directories, manifests and start commands.
pub fn validate_project(file: &std::path::Path) -> Result<ValidationReport> {
    let compose = ComposeFile::load(file)?;
    manifest::validate_offline(&compose)
}

/// Entry point behind `iii compose`. Returns the process exit code, matching
/// the other `iii` subcommands.
pub async fn run(cli: ComposeCli) -> i32 {
    let command = match cli.plan() {
        Ok(command) => command,
        Err(err) => return report_error(&err),
    };

    match command {
        ComposeCommand::Validate { file } => match validate_project(&file) {
            Ok(report) => {
                print_report(&report);
                0
            }
            Err(err) => report_error(&err),
        },
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
    println!("start order: {}", report.start_order.join(" -> "));
    for (key, start) in &report.resolved {
        match start {
            StartSpec::Shell(command) => println!("  {key}: {command}"),
            StartSpec::Exec { program, args } => {
                println!("  {key}: {} {}", program.display(), args.join(" "))
            }
        }
    }
    if !report.deferred_packages.is_empty() {
        println!(
            "deferred (package:// resolution not implemented): {}",
            report.deferred_packages.join(", ")
        );
    }
}
