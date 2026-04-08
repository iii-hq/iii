// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use clap::{Parser, Subcommand};

/// Default engine WebSocket port (must match engine's DEFAULT_PORT).
pub const DEFAULT_PORT: u16 = 49134;

#[derive(Parser, Debug)]
#[command(name = "iii-worker", version, about = "iii managed worker runtime")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add a worker from the registry or by OCI image reference
    Add {
        /// Worker name or OCI image reference (e.g., "pdfkit", "pdfkit@1.0.0", "ghcr.io/org/worker:tag")
        #[arg(value_name = "WORKER[@VERSION]")]
        worker_name: String,

        /// Container runtime
        #[arg(long, default_value = "libkrun")]
        runtime: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Remove a worker (stops and removes the container)
    Remove {
        /// Worker name to remove (e.g., "pdfkit")
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Start a previously stopped managed worker container
    Start {
        /// Worker name to start
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Stop a managed worker container
    Stop {
        /// Worker name to stop
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Run a worker project in an isolated environment for development.
    ///
    /// Auto-detects the project type (package.json, Cargo.toml, pyproject.toml)
    /// and runs it inside a VM (libkrun) connected
    /// to the engine.
    Dev {
        /// Path to the worker project directory
        #[arg(value_name = "PATH")]
        path: String,

        /// Sandbox name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,

        /// Runtime to use (auto-detected if not set)
        #[arg(long, value_parser = ["libkrun"])]
        runtime: Option<String>,

        /// Force rebuild: re-run setup and install scripts (libkrun only)
        #[arg(long)]
        rebuild: bool,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// List all workers and their status
    List,

    /// Show logs from a managed worker container
    Logs {
        /// Worker name
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Follow log output
        #[arg(long, short)]
        follow: bool,

        /// Engine host address
        #[arg(long, default_value = "localhost")]
        address: String,

        /// Engine WebSocket port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Internal: boot a libkrun VM (crash-isolated subprocess)
    #[command(name = "__vm-boot", hide = true)]
    VmBoot(super::vm_boot::VmBootArgs),
}
