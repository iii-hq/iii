// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Capacity and resource benchmarks for the iii engine. See README.md.
//!
//! Linux only: microVMs need KVM, and the engine is measured through `/proc`
//! and pinned with CPU affinity.

#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("iii-loadtest runs on Linux only (KVM, /proc, CPU affinity)");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod export;
#[cfg(target_os = "linux")]
mod monitor;
#[cfg(target_os = "linux")]
mod report;
#[cfg(target_os = "linux")]
mod rootfs;
#[cfg(target_os = "linux")]
mod runner;
#[cfg(target_os = "linux")]
mod sample;
#[cfg(target_os = "linux")]
mod scenarios;
#[cfg(target_os = "linux")]
mod stats;
#[cfg(target_os = "linux")]
mod suite;
#[cfg(target_os = "linux")]
mod vm;

#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use clap::{Parser, Subcommand};

#[cfg(target_os = "linux")]
#[derive(Parser)]
#[command(
    name = "iii-loadtest",
    about = "Capacity and resource benchmarks for the iii engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[cfg(target_os = "linux")]
#[derive(Subcommand)]
enum Cmd {
    /// Run the scenarios on each machine profile and write a report.
    Suite(Box<suite::SuiteArgs>),
    /// List the scenarios.
    Scenarios,
    /// Turn a results directory into the summary the benchmarks page reads.
    Export(export::ExportArgs),
    /// File a summary into a checkout of the gh-pages branch (runs + index).
    Publish(export::PublishArgs),
    /// Render report.md again from a results directory.
    Report {
        /// The directory `suite` wrote (holds results.json).
        dir: PathBuf,
    },
    /// Run one scenario against a fresh engine on this machine (what each VM
    /// of the suite runs).
    Run(Box<runner::RunArgs>),
    /// Boot a libkrun microVM with fixed vCPUs and RAM and run a command in it.
    Vm(vm::VmArgs),
    /// Build the guest root filesystem.
    Rootfs(rootfs::RootfsArgs),
}

#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        // No runtime here: libkrun takes the process over.
        Cmd::Vm(args) => match vm::boot(args)? {},
        Cmd::Rootfs(args) => rootfs::build(&args),
        Cmd::Suite(args) => suite::run(*args),
        Cmd::Scenarios => {
            for name in scenarios::ALL {
                println!("{name:<12} {}", scenarios::describe(name));
            }
            Ok(())
        }
        Cmd::Report { dir } => {
            let path = dir.join("results.json");
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            let results: suite::Results = serde_json::from_str(&text)?;
            std::fs::write(dir.join("report.md"), report::render(&results))?;
            println!("{}", dir.join("report.md").display());
            Ok(())
        }
        Cmd::Run(args) => runner::main(*args),
        Cmd::Export(args) => {
            println!("{}", export::export(&args)?.display());
            Ok(())
        }
        Cmd::Publish(args) => export::publish(&args),
    }
}
