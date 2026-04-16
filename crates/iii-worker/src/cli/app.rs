// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use clap::{Args, Parser, Subcommand};

/// Default engine WebSocket port (must match engine's DEFAULT_PORT).
pub const DEFAULT_PORT: u16 = 49134;

/// Shared arguments for `add` and `reinstall` commands.
#[derive(Args, Debug)]
pub struct AddArgs {
    /// Worker names or OCI image references (e.g., "pdfkit", "pdfkit@1.0.0", "ghcr.io/org/worker:tag")
    #[arg(value_name = "WORKER[@VERSION]", required = true, num_args = 1..)]
    pub worker_names: Vec<String>,

    /// Reset config: also remove config.yaml entry before re-adding (requires --force on add)
    #[arg(long)]
    pub reset_config: bool,
}

#[derive(Parser, Debug)]
#[command(
    name = "iii worker",
    bin_name = "iii worker",
    version,
    about = "iii managed worker runtime"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add one or more workers from the registry or by OCI image reference
    Add {
        #[command(flatten)]
        args: AddArgs,

        /// Force re-download: delete existing artifacts before adding
        #[arg(long, short = 'f')]
        force: bool,

        /// Don't block waiting for the engine to finish booting the sandbox.
        /// By default `add` waits up to 120s for the worker to report ready.
        #[arg(long)]
        no_wait: bool,
    },

    /// Remove one or more workers from config.yaml. The engine's file watcher
    /// tears down any running sandbox. Artifacts under ~/.iii/managed/{name}/
    /// remain; use `iii worker clear {name}` to delete them.
    Remove {
        /// Worker names to remove (e.g., "pdfkit")
        #[arg(value_name = "WORKER", required = true, num_args = 1..)]
        worker_names: Vec<String>,

        /// Skip the confirmation prompt when a worker is currently running
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Re-download a worker (equivalent to `add --force`; pass `--reset-config` to also clear config.yaml)
    Reinstall {
        #[command(flatten)]
        args: AddArgs,
    },

    /// Clear downloaded worker artifacts from ~/.iii/ (local-only, no engine connection needed)
    Clear {
        /// Worker name to clear (omit to clear all)
        #[arg(value_name = "WORKER")]
        worker_name: Option<String>,

        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Start a previously stopped managed worker container.
    /// By default waits up to 120s for the worker to report ready.
    Start {
        /// Worker name to start
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Don't block waiting for the worker to report ready.
        #[arg(long)]
        no_wait: bool,

        /// Engine WebSocket port the spawned worker connects back to. Defaults
        /// to DEFAULT_PORT; the engine passes its configured
        /// iii-worker-manager port when auto-spawning external workers so
        /// non-default manager ports don't silently break connectivity.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Stop a managed worker container
    Stop {
        /// Worker name to stop
        #[arg(value_name = "WORKER")]
        worker_name: String,
    },

    /// Restart a managed worker: stop if running, then start. Idempotent --
    /// running workers get a clean cycle, stopped workers just start.
    /// By default waits up to 120s for the worker to report ready.
    Restart {
        /// Worker name to restart
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Don't block waiting for the worker to report ready.
        #[arg(long)]
        no_wait: bool,

        /// Engine WebSocket port the spawned worker connects back to. Same
        /// semantics as `start --port`.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// List all workers and their status
    List,

    /// Show detailed status of one worker (config, sandbox, process, logs).
    /// By default refreshes live in place until the worker reaches a terminal
    /// phase (ready/missing). Pass --no-watch for a one-shot snapshot.
    Status {
        /// Worker name
        #[arg(value_name = "WORKER")]
        worker_name: String,

        /// Print a one-shot snapshot and exit immediately (no live refresh)
        #[arg(long)]
        no_watch: bool,
    },

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

    /// Internal: host-side source watcher sidecar for local-path workers
    #[command(name = "__watch-source", hide = true)]
    WatchSource(WatchSourceArgs),
}

/// Arguments for the hidden `__watch-source` subcommand.
#[derive(Args, Debug)]
pub struct WatchSourceArgs {
    /// Worker name to restart when source files change
    #[arg(long, value_name = "NAME")]
    pub worker: String,

    /// Absolute project directory to watch recursively
    #[arg(long, value_name = "PATH")]
    pub project: String,
}
