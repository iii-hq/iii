// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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

    /// Re-resolve locked workers and update iii.lock
    Update {
        /// Optional worker name to update. If omitted, updates every worker in iii.lock.
        #[arg(value_name = "WORKER")]
        worker_name: Option<String>,
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

        /// YAML config forwarded to the spawned worker binary as `--config <path>`.
        /// Binary workers only; OCI workers warn and ignore.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },

    /// Stop a managed worker container
    Stop {
        /// Worker name to stop
        #[arg(value_name = "WORKER")]
        worker_name: String,
        /// Skip the confirmation prompt. Required when stdin is non-interactive
        /// (scripts, CI, agents); without `-y` the call needs a tty.
        #[arg(short = 'y', long = "yes")]
        yes: bool,
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

        /// Same as `start --config`.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },

    /// List all workers and their status
    List,

    /// Install registry-managed workers exactly from iii.lock.
    /// Pass --frozen in CI to verify without mutating local files.
    Sync {
        /// Verify the lockfile without mutating local files.
        #[arg(long)]
        frozen: bool,
    },

    /// Verify config.yaml is represented in iii.lock without mutating files
    Verify {
        /// Also check dependency declarations against locked versions.
        #[arg(long)]
        strict: bool,
    },

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

    /// Run a command inside a running worker's VM. Pipes stdin/stdout/
    /// stderr through and returns the child's exit code. Pass `-t` for
    /// an interactive PTY.
    Exec(ExecArgs),

    /// Manage ephemeral sandboxes (create/exec/stop short-lived VMs).
    Sandbox {
        #[command(subcommand)]
        cmd: SandboxCmd,
    },

    /// Internal: sandbox RPC daemon. Started automatically by the iii
    /// engine when `iii-sandbox` appears in config.yaml.
    #[command(name = "sandbox-daemon", hide = true)]
    SandboxDaemon(SandboxDaemonArgs),

    /// Run the host-side worker-manager daemon. Connects to the engine,
    /// registers `worker::*` SDK triggers, and serves them until SIGINT.
    #[command(name = "worker-manager-daemon", hide = true)]
    WorkerManagerDaemon(WorkerManagerDaemonArgs),

    /// Internal: boot a libkrun VM (crash-isolated subprocess)
    #[command(name = "__vm-boot", hide = true)]
    VmBoot(super::vm_boot::VmBootArgs),

    /// Internal: host-side source watcher sidecar for local-path workers
    #[command(name = "__watch-source", hide = true)]
    WatchSource(WatchSourceArgs),
}

/// Arguments for `iii worker exec`. Mirrors `msb exec`'s shape so
/// users moving between microsandbox and iii have muscle memory.
#[derive(Args, Debug)]
pub struct ExecArgs {
    /// Worker name whose VM to run the command in.
    #[arg(value_name = "WORKER")]
    pub name: String,

    /// Set an environment variable inside the spawned process (repeatable).
    /// `KEY=VALUE` form; anything without `=` is silently skipped.
    #[arg(short, long)]
    pub env: Vec<String>,

    /// Working directory inside the guest. Defaults to the dispatcher's
    /// cwd (typically `/workspace`).
    #[arg(short, long)]
    pub workdir: Option<String>,

    /// Allocate a pseudo-terminal. Required for interactive shells
    /// and TUI programs; merges stdout/stderr through the PTY master
    /// and puts the host terminal in raw mode for the session. Auto-
    /// enabled when both stdin and stdout are TTYs (ssh-style); pass
    /// `--no-tty` to force pipe mode in that case.
    #[arg(short = 't', long)]
    pub tty: bool,

    /// Disable TTY auto-detection and force pipe mode. Use when you
    /// want byte-exact output in a terminal session (e.g. capturing
    /// structured output from an otherwise-interactive tool).
    #[arg(long, conflicts_with = "tty")]
    pub no_tty: bool,

    /// Kill the child after this long (e.g. `30s`, `5m`, `500ms`).
    /// Parsed by the standard `humantime` syntax. On expiry the client
    /// sends SIGKILL to the guest session and exits with code 124
    /// (matches coreutils `timeout(1)`), so shell scripts can
    /// distinguish a timeout from an ordinary nonzero exit.
    #[arg(long)]
    pub timeout: Option<String>,

    /// Program and arguments. Comes after `--`:
    /// `iii worker exec pdfkit -- /bin/ls -la /workspace`.
    /// First element is the executable; the rest are its argv.
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
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

/// Arguments for the `worker-manager-daemon` subcommand. Started by
/// the iii engine as a child process when iii-worker-manager is listed
/// in the project's worker config; rarely invoked directly.
#[derive(Args, Debug)]
pub struct WorkerManagerDaemonArgs {
    /// Engine WebSocket URL to connect back to.
    #[arg(long, default_value = "ws://127.0.0.1:49134")]
    pub engine: String,

    /// Project root the daemon mutates. Defaults to CWD at start.
    #[arg(long)]
    pub project_root: Option<std::path::PathBuf>,
}

/// Arguments for the `sandbox-daemon` subcommand. Started by the iii
/// engine as a child process; rarely invoked directly.
#[derive(Args, Debug)]
pub struct SandboxDaemonArgs {
    /// Path to the sandbox daemon's YAML config (flat `SandboxConfig` shape).
    #[arg(long, default_value = "./config.yaml")]
    pub config: String,

    /// Engine WebSocket URL to connect back to.
    #[arg(long, default_value = "ws://127.0.0.1:49134")]
    pub engine: String,
}

/// Subcommands for `iii sandbox`. Each one talks to the engine's
/// `sandbox::*` trigger handlers via the iii-sdk WebSocket client.
#[derive(Subcommand, Debug)]
pub enum SandboxCmd {
    /// Create a one-shot sandbox, run a command inside it, and stop it.
    /// For multi-step workflows (agent loops, REPLs) use `create` + `exec` +
    /// `stop` instead.
    Run {
        /// OCI image reference (must match the engine's sandbox allowlist).
        #[arg(value_name = "IMAGE")]
        image: String,

        /// vCPUs allocated to the sandbox VM.
        #[arg(long, default_value_t = 1)]
        cpus: u32,

        /// Memory in MiB allocated to the sandbox VM.
        #[arg(long, default_value_t = 512)]
        memory: u32,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,

        /// Program and arguments to exec inside the sandbox.
        #[arg(trailing_var_arg = true, value_name = "COMMAND")]
        cmd: Vec<String>,
    },

    /// Create a long-lived sandbox and print its id to stdout. Pair with
    /// `iii sandbox exec <id>` and `iii sandbox stop <id>`.
    ///
    /// Pipe-friendly: the sandbox id is the only thing on stdout, so you
    /// can do `SB=$(iii sandbox create python)` in a shell.
    Create {
        /// OCI image reference (must match the engine's sandbox allowlist).
        #[arg(value_name = "IMAGE")]
        image: String,

        /// vCPUs allocated to the sandbox VM.
        #[arg(long, default_value_t = 1)]
        cpus: u32,

        /// Memory in MiB allocated to the sandbox VM.
        #[arg(long, default_value_t = 512)]
        memory: u32,

        /// Auto-stop the sandbox after this many seconds of exec inactivity.
        /// Omit to use the engine's default.
        #[arg(long, value_name = "SECS")]
        idle_timeout: Option<u64>,

        /// Human-readable label for the sandbox (shown in `list`).
        #[arg(long, value_name = "NAME")]
        name: Option<String>,

        /// Enable guest network access. Default follows the engine's
        /// sandbox policy (typically disabled).
        #[arg(long)]
        network: bool,

        /// Set an environment variable inside the guest. Repeatable, `KEY=VALUE`
        /// form; entries without `=` are silently skipped.
        #[arg(short = 'e', long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Run a command inside an already-running sandbox.
    ///
    /// Pipe-mode only. Pair with `iii sandbox create` for the sandbox id.
    /// For interactive TTY sessions, use `iii worker exec` against a managed
    /// worker instead.
    Exec {
        /// Sandbox id from `iii sandbox create` / `iii sandbox list`.
        #[arg(value_name = "SANDBOX_ID")]
        id: String,

        /// Kill the child after this long (e.g. `30s`, `5m`, `500ms`).
        /// Parsed by the standard `humantime` syntax.
        #[arg(long)]
        timeout: Option<String>,

        /// Set an environment variable inside the spawned process.
        /// Repeatable, `KEY=VALUE` form; entries without `=` are silently skipped.
        #[arg(short = 'e', long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,

        /// Program and arguments to exec inside the sandbox. Comes after `--`:
        /// `iii sandbox exec <id> -- python3 -c 'print(2+2)'`.
        #[arg(trailing_var_arg = true, value_name = "COMMAND")]
        cmd: Vec<String>,
    },

    /// List every sandbox the daemon knows about.
    ///
    /// The daemon's list RPC is owner-scoped for multi-tenant SDK
    /// callers, but `iii sandbox` is a local admin tool with no
    /// authenticated identity, so the CLI always requests the unscoped
    /// view. The `--all` flag is a silent no-op, kept so scripts that
    /// pass it from earlier releases keep working.
    List {
        /// No-op. Kept for backward compat; the CLI always shows every
        /// sandbox regardless of this flag.
        #[arg(long, hide = true)]
        all: bool,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Stop a sandbox by id, waiting for the reaper to finish.
    Stop {
        /// Sandbox id returned by `sandbox create` / `sandbox list`.
        #[arg(value_name = "SANDBOX_ID")]
        id: String,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Copy a local file into a running sandbox.
    ///
    /// Streams bytes through an iii data channel — no JSON-envelope size cap.
    /// Reads `LOCAL_PATH` from disk (or stdin when `LOCAL_PATH` is `-`) and
    /// writes atomically (temp file + fsync + rename) to `REMOTE_PATH` inside
    /// the sandbox.
    ///
    /// Examples:
    ///   iii sandbox upload <SB> ./script.js /workspace/script.js
    ///   tar -cf - ./srcdir | iii sandbox upload <SB> - /workspace/src.tar
    Upload {
        /// Sandbox id from `iii sandbox create` / `iii sandbox list`.
        #[arg(value_name = "SANDBOX_ID")]
        id: String,

        /// Source path on the host. Use `-` to read from stdin.
        #[arg(value_name = "LOCAL_PATH")]
        local_path: String,

        /// Destination path inside the sandbox.
        #[arg(value_name = "REMOTE_PATH")]
        remote_path: String,

        /// File mode (octal) applied to the destination after the rename.
        #[arg(long, default_value = "0644", value_name = "MODE")]
        mode: String,

        /// Create parent directories of `REMOTE_PATH` if they're missing.
        #[arg(long)]
        parents: bool,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },

    /// Copy a file out of a running sandbox to a local path.
    ///
    /// Streams bytes through an iii data channel. Writes to `LOCAL_PATH` on
    /// disk (or stdout when `LOCAL_PATH` is `-`).
    ///
    /// Examples:
    ///   iii sandbox download <SB> /workspace/output.json ./output.json
    ///   iii sandbox download <SB> /workspace/build.tar - | tar -tf -
    Download {
        /// Sandbox id from `iii sandbox create` / `iii sandbox list`.
        #[arg(value_name = "SANDBOX_ID")]
        id: String,

        /// Source path inside the sandbox.
        #[arg(value_name = "REMOTE_PATH")]
        remote_path: String,

        /// Destination path on the host. Use `-` to write to stdout.
        #[arg(value_name = "LOCAL_PATH")]
        local_path: String,

        /// Engine WebSocket port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
}
