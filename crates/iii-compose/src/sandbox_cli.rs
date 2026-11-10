// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii sandbox`: one Compose container's VM, booted through its sandbox adapter.
//!
//! Compose starts `iii sandbox run` as the `scripts.run` of a VM container (a registry bundle, or
//! a `path://` worker whose manifest sets `runtime.base_image`), and hands it what it needs in the
//! environment ([`Context`]). The command boots the image of the container's last build, which
//! `build.json` in `III_BUILD_DIR` names, through the adapter's `worker::*` functions; nothing is
//! installed at boot. It prints the guest's output as its own, stops the VM when it is asked to
//! stop, and exits with the VM's code. Compose supervises it like any other process.
//!
//! `iii sandbox build`, the container's `scripts.build`, makes that image: it asks the adapter's
//! `worker::build` for it, prints the build's output as it comes, and records the image in
//! `build.json` once the adapter answered. The build lives as long as this command reads its
//! output, so a timeout or a signal stops it by closing the channel.

use std::{
    collections::BTreeMap,
    fmt,
    io::Write,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::{Arc, Mutex},
    time::Duration,
};

use iii_sandbox_contract::{
    self as contract, ADAPTER_INFO_SUFFIX, AdapterInfo, AdapterInfoRequest, CONTRACT_MAJOR,
    FEATURE_BUILD, FEATURE_LEASE, FEATURE_LOGS_FROM_END, FEATURE_SCOPED_AUTH, LogRecord, LogStream,
    SandboxErrorCode, WORKER_BUILD, WORKER_DESTROY, WORKER_LOGS, WORKER_START, WORKER_STOP,
    WorkerBuildRef, WorkerBuildRequest, WorkerBuildResponse, WorkerDestroyRequest,
    WorkerDestroyResponse, WorkerLogsRequest, WorkerLogsResponse, WorkerSource, WorkerStartRequest,
    WorkerStartResponse, WorkerStatus, WorkerStopRequest,
};
use iii_sdk::{IIIClient, InitOptions, WorkerIdentityMode, iii::WorkerMetadata, register_worker};
use tokio::{sync::watch, task::JoinHandle, time::Instant};

use crate::{
    build_record::{
        self, BuildAdapter, BuildBaseImage, BuildContract, BuildImage, BuildRecord, BuildSource,
    },
    error::ComposeError,
    process::{DEFAULT_STOP_GRACE, Outcome},
    sandbox,
    state::VmRecord,
};

/// The scoped `worker::*` token of the container's own VM.
pub const TOKEN_ENV: &str = "III_SANDBOX_TOKEN";
/// The VM's state key: the same key boots the same persistent state.
pub const STATE_KEY_ENV: &str = "III_SANDBOX_STATE_KEY";
/// The adapter's name, e.g. `sandbox-libkrun`.
pub const ADAPTER_ENV: &str = "III_SANDBOX_ADAPTER";
/// The namespace the adapter's functions are registered in.
pub const ADAPTER_NAMESPACE_ENV: &str = "III_SANDBOX_NAMESPACE";
/// A `path://` worker's directory, synced into the VM.
pub const WORKER_DIR_ENV: &str = "III_WORKER_DIR";
/// A registry bundle's install directory in the package cache.
pub const PACKAGE_DIR_ENV: &str = "III_PACKAGE_DIR";
/// A registry bundle's manifest name, which for an alias is the package it stands for.
pub const PACKAGE_NAME_ENV: &str = "III_PACKAGE_NAME";
// The three build variables are set by KAN-280 (spawn) and read by KAN-282 (`iii sandbox`).
/// The container's build directory, `<project>/.build/<container>`.
pub const BUILD_DIR_ENV: &str = "III_BUILD_DIR";
/// The compose file's canonical path, the same as `III_COMPOSE_FILE`, for `iii sandbox build`.
pub const BUILD_COMPOSE_FILE_ENV: &str = "III_BUILD_COMPOSE_FILE";
/// `scripts.build_timeout` in milliseconds, for `iii sandbox build`.
pub const BUILD_TIMEOUT_ENV: &str = "III_BUILD_TIMEOUT_MS";
/// Comma-separated names of the variables that belong to the worker and reach the guest.
pub const WORKER_ENV_KEYS_ENV: &str = "III_WORKER_ENV_KEYS";
/// How long Compose waits after its SIGTERM before it kills this command.
pub const STOP_TIMEOUT_ENV: &str = "III_STOP_TIMEOUT_MS";
/// Exit code of a failure of this command itself, apart from the codes the guest exits with;
/// `docker run` uses the same.
pub const RUNNER_FAILED: i32 = 125;
/// Exit code of a build that failed: the adapter refused or failed it, or it ran out of time.
pub const BUILD_FAILED_EXIT: i32 = 1;
/// Exit code of a build cancelled by SIGINT or SIGTERM, or by `compose::cancel` of its operation.
pub const CANCELLED_EXIT: i32 = 130;
/// How the last line of a failed `iii sandbox build` begins. The S-code or error code follows,
/// then `: ` and the message, all on that one line: Compose reads it for `BUILD_FAILED`.
pub const BUILD_FAILED_LINE: &str = "iii sandbox: build failed: ";

/// The adapter does not advertise `worker.build`, or another feature this command needs.
pub const SANDBOX_BUILD_UNSUPPORTED: &str = "SANDBOX_BUILD_UNSUPPORTED";
/// `iii sandbox run` or `build` on Windows, where no adapter boots a VM.
pub const SANDBOX_UNSUPPORTED_PLATFORM: &str = "SANDBOX_UNSUPPORTED_PLATFORM";
/// `iii sandbox run` without a `build.json` that records an image in `III_BUILD_DIR`.
pub const BUILD_MISSING: &str = "BUILD_MISSING";
/// `iii sandbox build` ran for longer than `III_BUILD_TIMEOUT_MS`.
pub const TIMED_OUT: &str = "TIMED_OUT";
/// `iii sandbox build` was stopped by a signal, or its `compose::build` operation was cancelled.
pub const BUILD_CANCELLED: &str = "BUILD_CANCELLED";
/// The engine, the adapter or the daemon was not reached, or did not answer as it should.
const ENGINE_CALL_FAILED: &str = "ENGINE_CALL_FAILED";
/// A variable Compose hands this command is missing or malformed.
const SANDBOX_ENV_INVALID: &str = "SANDBOX_ENV_INVALID";
/// The adapter implements another major version of the sandbox contract.
const SANDBOX_CONTRACT_UNSUPPORTED: &str = "SANDBOX_CONTRACT_UNSUPPORTED";
/// Arguments this command cannot follow where it runs.
const SANDBOX_INVALID_ARGUMENTS: &str = "SANDBOX_INVALID_ARGUMENTS";
/// Any other failure of `iii sandbox` itself.
const SANDBOX_FAILED: &str = "SANDBOX_FAILED";

const URL_ENV: &str = "III_URL";
const WORKER_NAME_ENV: &str = "III_WORKER_NAME";
/// The project token Compose gives the adapter, never a container.
const PROJECT_TOKEN_ENV: &str = "III_SANDBOX_WORKER_TOKEN";

/// Never forwarded into the guest, even when listed: this command's own inputs, the host's
/// engine URL (the adapter gives the guest its own) and the project token.
const NEVER_FORWARDED: [&str; 14] = [
    TOKEN_ENV,
    STATE_KEY_ENV,
    ADAPTER_ENV,
    ADAPTER_NAMESPACE_ENV,
    WORKER_DIR_ENV,
    PACKAGE_DIR_ENV,
    PACKAGE_NAME_ENV,
    BUILD_DIR_ENV,
    BUILD_COMPOSE_FILE_ENV,
    BUILD_TIMEOUT_ENV,
    WORKER_ENV_KEYS_ENV,
    STOP_TIMEOUT_ENV,
    URL_ENV,
    PROJECT_TOKEN_ENV,
];

/// What this command needs from the adapter beyond contract 1.1: its own VM's token, a VM that
/// stops once this command is gone and a reattach that does not replay the log (1.2), and the
/// image `worker::build` writes, which `worker::start` boots (1.3).
const REQUIRED_FEATURES: [&str; 4] = [
    FEATURE_SCOPED_AUTH,
    FEATURE_LEASE,
    FEATURE_LOGS_FROM_END,
    FEATURE_BUILD,
];

/// The connection's own name: never the container's, which belongs to the guest. A CLI command's
/// marked name, because the engine keeps one live worker per plain name and one per container runs.
const CLIENT_NAME: &str = "iii-cli:sandbox";
/// How long the adapter may take to register `adapter::info` after it connects.
const INFO_WAIT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 30 });
/// `worker::start` boots the VM; Compose's startup timeout bounds the guest's registration.
const START_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Every other call's budget.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// `worker::status` cadence.
const POLL_INTERVAL: Duration = Duration::from_millis(if cfg!(test) { 100 } else { 1000 });
/// `scripts.build_timeout`'s default, for an environment without `III_BUILD_TIMEOUT_MS`.
const DEFAULT_BUILD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How long a build's output may take to end once `worker::build` answered.
const DRAIN_WAIT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 10 });
/// The `scripts.build` of a VM container, as `build.json` records it.
const BUILD_COMMAND: &str = "iii sandbox build";
/// The image's file in the build directory.
const IMAGE_FILE: &str = "image.erofs";
/// How long `worker::status` may go unanswered before this command gives the VM up: the
/// adapter's reattach grace, after which its lease has stopped a VM no client follows.
const LOST_AFTER: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 45 });

/// `iii sandbox`: run a Compose container's VM through its sandbox adapter.
#[derive(Debug, Clone, clap::Args)]
pub struct SandboxCli {
    #[command(subcommand)]
    pub command: SandboxCommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum SandboxCommand {
    /// Build this container's image through its sandbox adapter and record it in the
    /// container's build.json. A running container keeps its image until it restarts. Exits 0
    /// once built, 1 when the build failed, 125 when `iii sandbox` itself failed and 130 when
    /// cancelled; on failure the last line is `iii sandbox: build failed: <code>: <message>`.
    Build(BuildArgs),
    /// Boot this container's last build in its VM, or reattach to the VM, and stay with it until
    /// it exits. Stopping this command stops the VM. Exits with the VM's code, 128+N when the VM
    /// ended on signal N, or 125 when `iii sandbox` itself failed (`BUILD_MISSING` without a
    /// build).
    Run(RunArgs),
    /// Remove this container's VM state: the disk its worker writes its data to.
    Destroy,
}

#[derive(Debug, Clone, clap::Args)]
pub struct RunArgs {
    /// Command to start in the guest instead of the manifest's `scripts.start`. Only for a
    /// `path://` worker: a registry bundle runs its own.
    #[arg(long, value_name = "COMMAND")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct BuildArgs {
    /// The container to build. Inside Compose, only the container whose `scripts.build` this
    /// is, which is also the default.
    #[arg(value_name = "CONTAINER")]
    pub container: Option<String>,
}

/// The VM `iii sandbox` acts on, and the adapter that runs it.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    engine_url: String,
    adapter: String,
    adapter_namespace: String,
    token: String,
    state_key: String,
}

impl Target {
    fn from_vars(vars: &BTreeMap<String, String>) -> Result<Self, String> {
        Ok(Self {
            engine_url: required(vars, URL_ENV)?,
            adapter: required(vars, ADAPTER_ENV)?,
            adapter_namespace: required(vars, ADAPTER_NAMESPACE_ENV)?,
            token: required(vars, TOKEN_ENV)?,
            state_key: required(vars, STATE_KEY_ENV)?,
        })
    }
}

/// Everything `iii sandbox run` needs, as Compose hands it over.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    target: Target,
    worker_name: String,
    source: WorkerSource,
    /// Only the worker's own variables: the host's environment never reaches the guest.
    env: BTreeMap<String, String>,
    /// Compose's stop timeout: its SIGKILL follows its SIGTERM after this long.
    stop_timeout: Duration,
}

impl Context {
    pub(crate) fn from_vars(
        vars: &BTreeMap<String, String>,
        command: Option<String>,
    ) -> Result<Self, String> {
        let target = Target::from_vars(vars)?;
        let source = match (
            present(vars, WORKER_DIR_ENV),
            present(vars, PACKAGE_DIR_ENV),
        ) {
            (Some(dir), _) => WorkerSource::Local {
                host_path: dir.to_string(),
                run_override: command,
            },
            (None, Some(dir)) => {
                if command.is_some() {
                    return Err(
                        "--command applies to a path:// worker only: a registry bundle runs the \
                         start command it was published with"
                            .to_string(),
                    );
                }
                WorkerSource::Bundle {
                    package: required(vars, PACKAGE_NAME_ENV)?,
                    host_path: dir.to_string(),
                }
            }
            (None, None) => {
                return Err(format!(
                    "neither {WORKER_DIR_ENV} nor {PACKAGE_DIR_ENV} is set, so there is no worker \
                     to boot; {STARTED_BY_COMPOSE}"
                ));
            }
        };
        let stop_timeout = match present(vars, STOP_TIMEOUT_ENV) {
            Some(ms) => Duration::from_millis(ms.parse().map_err(|_| {
                format!("{STOP_TIMEOUT_ENV} must be a number of milliseconds, not {ms:?}")
            })?),
            None => DEFAULT_STOP_GRACE,
        };
        Ok(Self {
            target,
            worker_name: required(vars, WORKER_NAME_ENV)?,
            source,
            env: worker_env(vars),
            stop_timeout,
        })
    }

    /// When everything a stop does must be over: nine tenths into Compose's stop timeout,
    /// leaving the rest for this command to exit before Compose kills it.
    fn stop_deadline(&self, signalled_at: Instant) -> Instant {
        let budget = self.stop_timeout * 9 / 10;
        signalled_at
            .checked_add(budget)
            .unwrap_or_else(|| signalled_at + Duration::from_secs(365 * 24 * 60 * 60))
    }
}

/// Everything `iii sandbox build` needs inside Compose, as Compose hands it over.
#[derive(Debug, Clone)]
pub(crate) struct BuildContext {
    container: Context,
    /// `III_BUILD_DIR`: where the adapter writes the image, and this command `build.json`.
    build_dir: String,
    /// `III_BUILD_COMPOSE_FILE`: the compose file that declares the container's source.
    compose_file: PathBuf,
    /// `III_BUILD_TIMEOUT_MS`: `scripts.build_timeout`.
    timeout: Duration,
}

impl BuildContext {
    pub(crate) fn from_vars(vars: &BTreeMap<String, String>) -> Result<Self, String> {
        let timeout = match present(vars, BUILD_TIMEOUT_ENV) {
            Some(ms) => Duration::from_millis(ms.parse().map_err(|_| {
                format!("{BUILD_TIMEOUT_ENV} must be a number of milliseconds, not {ms:?}")
            })?),
            None => DEFAULT_BUILD_TIMEOUT,
        };
        Ok(Self {
            container: Context::from_vars(vars, None)?,
            build_dir: required(vars, BUILD_DIR_ENV)?,
            compose_file: PathBuf::from(required(vars, BUILD_COMPOSE_FILE_ENV)?),
            timeout,
        })
    }

    /// The worker's directory, where its manifest and dependency files are.
    fn worker_dir(&self) -> &Path {
        match &self.container.source {
            WorkerSource::Local { host_path, .. } | WorkerSource::Bundle { host_path, .. } => {
                Path::new(host_path)
            }
        }
    }
}

const STARTED_BY_COMPOSE: &str = "`iii sandbox` runs as the `scripts.build` and `scripts.run` of a \
                                  Compose container in a VM: a registry bundle, or a `path://` \
                                  worker whose manifest sets `runtime.base_image`";

fn present<'a>(vars: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    vars.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn required(vars: &BTreeMap<String, String>, name: &str) -> Result<String, String> {
    present(vars, name)
        .map(str::to_string)
        .ok_or_else(|| format!("{name} is not set; {STARTED_BY_COMPOSE}"))
}

/// The variables `III_WORKER_ENV_KEYS` names, without [`NEVER_FORWARDED`].
fn worker_env(vars: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    present(vars, WORKER_ENV_KEYS_ENV)
        .into_iter()
        .flat_map(|keys| keys.split(','))
        .map(str::trim)
        .filter(|key| !key.is_empty() && !NEVER_FORWARDED.contains(key))
        .filter_map(|key| Some((key.to_string(), vars.get(key)?.clone())))
        .collect()
}

/// Why `iii sandbox` failed: a stable code, what happened, and the exit code the command ends
/// with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
    /// [`RUNNER_FAILED`], [`BUILD_FAILED_EXIT`] or [`CANCELLED_EXIT`].
    pub(crate) exit: i32,
}

impl Failure {
    /// A failure of `iii sandbox` itself, not of what it runs: exit 125.
    fn runner(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            exit: RUNNER_FAILED,
        }
    }

    /// Compose's own error, from `build.json` or the project's discovery: exit 125.
    fn compose(error: &ComposeError) -> Self {
        Self::runner(error.code(), error.to_string())
    }

    /// A build that failed: exit 1.
    fn build(code: &str, message: impl Into<String>) -> Self {
        Self {
            exit: BUILD_FAILED_EXIT,
            ..Self::runner(code, message)
        }
    }

    /// A build stopped by signal `number`: exit 130.
    fn cancelled_by(number: i32) -> Self {
        let signal = match number {
            2 => "SIGINT".to_string(),
            15 => "SIGTERM".to_string(),
            other => format!("signal {other}"),
        };
        Self {
            exit: CANCELLED_EXIT,
            ..Self::runner(
                BUILD_CANCELLED,
                format!(
                    "stopped by {signal}; the adapter abandons the build and keeps the previous image"
                ),
            )
        }
    }

    /// A build past `III_BUILD_TIMEOUT_MS`: exit 1.
    fn timed_out(timeout: Duration) -> Self {
        let millis = timeout.as_millis();
        let limit = if millis % 1000 == 0 {
            format!("{}s", millis / 1000)
        } else {
            format!("{millis}ms")
        };
        Self::build(
            TIMED_OUT,
            format!(
                "the build did not finish within {limit} ({BUILD_TIMEOUT_ENV}, scripts.build_timeout); \
                 the adapter abandons it and keeps the previous image"
            ),
        )
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// The last line of a failed `iii sandbox build`, on one line whatever the message holds.
pub fn build_failure_line(code: &str, message: &str) -> String {
    let message = message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    format!("{BUILD_FAILED_LINE}{code}: {message}")
}

/// The code and message of a [`build_failure_line`]; `None` for any other line.
pub fn parse_build_failure(line: &str) -> Option<(&str, &str)> {
    line.trim_end()
        .strip_prefix(BUILD_FAILED_LINE)?
        .split_once(": ")
}

fn env_failure(message: String) -> Failure {
    Failure::runner(SANDBOX_ENV_INVALID, message)
}

/// Refuses `run` and `build` where no adapter boots a VM: on Windows, where a bundle already
/// fails at install with `BUNDLE_NEEDS_A_VM`.
fn vm_platform(windows: bool) -> Result<(), Failure> {
    if windows {
        return Err(Failure::runner(
            SANDBOX_UNSUPPORTED_PLATFORM,
            "no sandbox adapter boots a VM on Windows; run this worker on the host by setting \
             its scripts.build and scripts.run to host commands, or run the project on Linux \
             or macOS",
        ));
    }
    Ok(())
}

/// `build.json` and the inputs it records, through [`build_record`] or a test's fake.
pub(crate) trait Records: Send + Sync {
    /// [`build_record::read`].
    fn read(&self, build_dir: &Path) -> crate::error::Result<Option<BuildRecord>>;
    /// [`build_record::write`].
    fn write(&self, build_dir: &Path, record: &BuildRecord) -> crate::error::Result<()>;
    /// [`build_record::source`].
    fn source(&self, compose_file: &Path, container: &str) -> crate::error::Result<BuildSource>;
    /// [`build_record::declared_base_image`].
    fn declared_base_image(&self, worker_dir: &Path) -> crate::error::Result<Option<String>>;
    /// [`build_record::dependency_files`].
    fn dependency_files(&self, worker_dir: &Path)
    -> crate::error::Result<BTreeMap<String, String>>;
}

/// The project's own `build.json` files.
pub(crate) struct BuildRecords;

impl Records for BuildRecords {
    fn read(&self, build_dir: &Path) -> crate::error::Result<Option<BuildRecord>> {
        build_record::read(build_dir)
    }

    fn write(&self, build_dir: &Path, record: &BuildRecord) -> crate::error::Result<()> {
        build_record::write(build_dir, record)
    }

    fn source(&self, compose_file: &Path, container: &str) -> crate::error::Result<BuildSource> {
        build_record::source(compose_file, container)
    }

    fn declared_base_image(&self, worker_dir: &Path) -> crate::error::Result<Option<String>> {
        build_record::declared_base_image(worker_dir)
    }

    fn dependency_files(
        &self,
        worker_dir: &Path,
    ) -> crate::error::Result<BTreeMap<String, String>> {
        build_record::dependency_files(worker_dir)
    }
}

/// Entry point behind `iii sandbox`. Returns the process exit code.
pub async fn run(cli: SandboxCli) -> i32 {
    let vars = process_vars();
    match cli.command {
        SandboxCommand::Build(args) => match build_command(&vars, args, &BuildRecords).await {
            Ok(()) => 0,
            Err(failure) => {
                // The last line, after all the build's output: Compose reads it for BUILD_FAILED.
                eprintln!("{}", build_failure_line(&failure.code, &failure.message));
                failure.exit
            }
        },
        SandboxCommand::Run(args) => report(run_command(&vars, args.command, &BuildRecords).await),
        SandboxCommand::Destroy => report(destroy_command(&vars).await),
    }
}

fn report(result: Result<i32, Failure>) -> i32 {
    result.unwrap_or_else(|failure| {
        eprintln!("iii sandbox: {failure}");
        failure.exit
    })
}

/// The process environment; a variable that is not UTF-8 cannot be the guest's anyway.
fn process_vars() -> BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

async fn run_command(
    vars: &BTreeMap<String, String>,
    command: Option<String>,
    records: &dyn Records,
) -> Result<i32, Failure> {
    vm_platform(cfg!(windows))?;
    let (ctx, build) = boot_plan(vars, command, records)?;
    // Before anything can boot.
    let shutdown = signal_handlers()?;
    let client = connect(&ctx.target.engine_url);
    let result = run_vm(&ctx, &build, &client, &shutdown, Printer::stdio()).await;
    client.shutdown_async().await;
    result.map(exit_code)
}

/// Installs the SIGTERM and SIGINT handlers: from here on neither signal ends this command by its
/// default action, whatever phase it comes in.
fn signal_handlers() -> Result<Shutdown, Failure> {
    Shutdown::signals().map_err(|error| {
        Failure::runner(
            SANDBOX_FAILED,
            format!("cannot handle SIGTERM and SIGINT: {error}"),
        )
    })
}

async fn build_command(
    vars: &BTreeMap<String, String>,
    args: BuildArgs,
    records: &dyn Records,
) -> Result<(), Failure> {
    vm_platform(cfg!(windows))?;
    let bctx = BuildContext::from_vars(vars).map_err(env_failure)?;
    own_container(&bctx, args.container.as_deref())?;
    let shutdown = signal_handlers()?;
    let client = connect(&bctx.container.target.engine_url);
    let result = build_vm(&bctx, records, &client, &shutdown, Printer::stdio()).await;
    client.shutdown_async().await;
    let record = result?;
    if let Some(image) = &record.image {
        eprintln!(
            "iii sandbox: built {} ({})",
            record.container, image.build_id
        );
    }
    Ok(())
}

/// Inside Compose the build is the container's own: CONTAINER may only name it.
fn own_container(bctx: &BuildContext, container: Option<&str>) -> Result<(), Failure> {
    let own = &bctx.container.worker_name;
    match container {
        Some(other) if other != own => Err(Failure::runner(
            SANDBOX_INVALID_ARGUMENTS,
            format!(
                "as a container's scripts.build, `iii sandbox build` builds that container, \
                 {own}, not {other}"
            ),
        )),
        _ => Ok(()),
    }
}

/// What `iii sandbox run` boots: the container Compose hands over, and the image of its last
/// build, as `build.json` in `III_BUILD_DIR` records it.
fn boot_plan(
    vars: &BTreeMap<String, String>,
    command: Option<String>,
    records: &dyn Records,
) -> Result<(Context, WorkerBuildRef), Failure> {
    let ctx = Context::from_vars(vars, command).map_err(env_failure)?;
    let build_dir = required(vars, BUILD_DIR_ENV).map_err(env_failure)?;
    let build_id = built_image(Path::new(&build_dir), &ctx.worker_name, records)?;
    Ok((
        ctx,
        WorkerBuildRef {
            build_id,
            build_dir,
        },
    ))
}

/// The `build_id` of the image the container's last build wrote, as its `build.json` records it.
fn built_image(
    build_dir: &Path,
    container: &str,
    records: &dyn Records,
) -> Result<String, Failure> {
    let missing = |what: String| {
        Failure::runner(
            BUILD_MISSING,
            format!("{what}; build it with `iii sandbox build {container}`"),
        )
    };
    match records.read(build_dir) {
        Err(error) => Err(Failure::compose(&error)),
        Ok(None) => Err(missing(format!(
            "{container} has no build: there is no build.json in {}",
            build_dir.display()
        ))),
        Ok(Some(BuildRecord { image: None, .. })) => Err(missing(format!(
            "{} records a host build of {container}, with no image to boot",
            build_dir.join("build.json").display()
        ))),
        Ok(Some(BuildRecord {
            image: Some(image), ..
        })) => Ok(image.build_id),
    }
}

async fn destroy_command(vars: &BTreeMap<String, String>) -> Result<i32, Failure> {
    let target = Target::from_vars(vars).map_err(env_failure)?;
    let client = connect(&target.engine_url);
    let request = WorkerDestroyRequest {
        auth: target.token.clone(),
        state_key: target.state_key.clone(),
    };
    let reply = sandbox::call::<WorkerDestroyResponse>(
        &client,
        &target.adapter,
        &target.adapter_namespace,
        WORKER_DESTROY,
        &request,
        Duration::from_secs(60),
    )
    .await;
    client.shutdown_async().await;
    match reply {
        Ok(WorkerDestroyResponse { destroyed: true }) => {
            println!("removed the VM state of {}", target.state_key);
            Ok(0)
        }
        Ok(WorkerDestroyResponse { destroyed: false }) => {
            println!("no VM state for {}", target.state_key);
            Ok(0)
        }
        Err(error) => Err(call_failure("worker::destroy", &error, &target)),
    }
}

fn connect(engine_url: &str) -> IIIClient {
    register_worker(
        engine_url,
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: CLIENT_NAME.to_string(),
                ..Default::default()
            }),
            // Compose hands this process the container's identity for the guest. Taking it
            // would register this client under the container's name, and the engine would
            // report the container ready before its guest has booted.
            identity: WorkerIdentityMode::Explicit,
            ..Default::default()
        },
    )
}

/// SIGTERM (Compose's stop) or SIGINT (an operator's Ctrl-C): the signal's number and when it
/// came, once one did.
#[derive(Clone)]
pub(crate) struct Shutdown(watch::Receiver<Option<(i32, Instant)>>);

impl Shutdown {
    /// Installs both handlers now, so a signal that comes while the adapter is still answering,
    /// preparing or booting is handled like any other, never by its default action. Later
    /// signals are absorbed: they cannot cut the stop the first one began.
    fn signals() -> std::io::Result<Self> {
        let (sender, requested) = watch::channel(None);
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate = signal(SignalKind::terminate())?;
            let mut interrupt = signal(SignalKind::interrupt())?;
            tokio::spawn(async move {
                let first = tokio::select! {
                    received = terminate.recv() => received.map(|()| 15),
                    received = interrupt.recv() => received.map(|()| 2),
                };
                let Some(number) = first else {
                    return;
                };
                sender.send_replace(Some((number, Instant::now())));
                loop {
                    let received = tokio::select! {
                        received = terminate.recv() => received,
                        received = interrupt.recv() => received,
                    };
                    if received.is_none() {
                        return;
                    }
                }
            });
        }
        #[cfg(windows)]
        {
            let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
            tokio::spawn(async move {
                if ctrl_c.recv().await.is_some() {
                    sender.send_replace(Some((2, Instant::now())));
                }
                while ctrl_c.recv().await.is_some() {}
            });
        }
        Ok(Self(requested))
    }

    #[cfg(test)]
    fn manual() -> (watch::Sender<Option<(i32, Instant)>>, Self) {
        let (sender, requested) = watch::channel(None);
        (sender, Self(requested))
    }

    /// Resolves once a signal came, to its number and when it came; never without one.
    async fn requested(&self) -> (i32, Instant) {
        let mut requested = self.0.clone();
        let signal = match requested.wait_for(Option::is_some).await {
            Ok(signal) => *signal,
            Err(_) => None,
        };
        match signal {
            Some(signal) => signal,
            None => std::future::pending().await,
        }
    }
}

/// Where the guest's records go: stdout and stderr, or a test's buffer.
#[derive(Clone)]
pub(crate) struct Printer(Arc<dyn Fn(&LogRecord) + Send + Sync>);

impl Printer {
    fn stdio() -> Self {
        Self(Arc::new(print_record))
    }
}

fn print_record(record: &LogRecord) {
    // Output nobody reads any more (a closed pipe) is not this command's failure.
    let _ = match record.stream {
        LogStream::Stdout => writeln!(std::io::stdout().lock(), "{}", record.line),
        LogStream::Stderr => writeln!(std::io::stderr().lock(), "{}", record.line),
    };
}

/// A VM's end as this command's exit code, as a shell reports a child's.
fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}

/// This command's end when a signal came before there was a VM to stop.
fn signalled(number: i32) -> ExitStatus {
    sandbox::vm_exit(None, Some(number))
}

/// Boots `build` in `ctx`'s VM, or reattaches to its live boot, and stays with it until it exits
/// or `shutdown` resolves, which stops it. Resolves to the VM's end as a process status.
pub(crate) async fn run_vm(
    ctx: &Context,
    build: &WorkerBuildRef,
    client: &IIIClient,
    shutdown: &Shutdown,
    printer: Printer,
) -> Result<ExitStatus, Failure> {
    let target = &ctx.target;
    let info = tokio::select! {
        biased;
        (number, _) = shutdown.requested() => return Ok(signalled(number)),
        info = adapter_info(target, client) => info?,
    };
    check_adapter(&info, &target.adapter)?;

    // The build is the guest's whole root: no `worker::prepare`, nothing installed at boot.
    let start = WorkerStartRequest {
        auth: target.token.clone(),
        state_key: target.state_key.clone(),
        worker_name: ctx.worker_name.clone(),
        source: ctx.source.clone(),
        env: ctx.env.clone(),
        engine_url: target.engine_url.clone(),
        build: Some(build.clone()),
    };
    // A signal does not abandon the start: its reply names the instance to stop. One that does
    // not come before the stop deadline leaves the VM to the adapter's lease.
    let started = tokio::select! {
        started = sandbox::call::<WorkerStartResponse>(
            client,
            &target.adapter,
            &target.adapter_namespace,
            WORKER_START,
            &start,
            START_TIMEOUT,
        ) => started,
        number = async {
            let (number, at) = shutdown.requested().await;
            tokio::time::sleep_until(ctx.stop_deadline(at)).await;
            number
        } => {
            eprintln!(
                "iii sandbox: worker::start did not answer before the stop timeout; {} stops \
                 the VM once no client follows it",
                target.adapter
            );
            return Ok(signalled(number));
        }
    };
    // S502 names a boot of this state that is still live: the VM outlived the command that
    // ran it (a SIGKILL, a daemon restart). Reattaching keeps the guest as it is.
    let (started, reattached) = match started {
        Ok(started) => (started, false),
        Err(error) => match (
            sandbox::sandbox_code(&error),
            sandbox::live_instance(&error),
        ) {
            (Some(SandboxErrorCode::S502), Some(live)) => (live, true),
            _ => {
                let mut failure = call_failure("worker::start", &error, target);
                if failure.code == SandboxErrorCode::S508.as_str() {
                    failure.message.push_str(&format!(
                        "; rebuild it with `iii sandbox build {}`",
                        ctx.worker_name
                    ));
                }
                return Err(failure);
            }
        },
    };
    let vm = VmRecord {
        adapter: target.adapter.clone(),
        namespace: target.adapter_namespace.clone(),
        vm_id: started.vm_id,
        instance: started.instance,
    };
    if reattached {
        eprintln!(
            "iii sandbox: reattached to VM {} instance {}",
            vm.vm_id, vm.instance
        );
    }
    let mut logs = Logs::new(reattached, printer);
    supervise(ctx, client, &vm, shutdown, &mut logs).await
}

/// `adapter::info`, asked again while the adapter has not registered it yet: the adapter starts
/// before this container (`start_after`), but registers its functions a moment after it connects.
async fn adapter_info(target: &Target, client: &IIIClient) -> Result<AdapterInfo, Failure> {
    let deadline = Instant::now() + INFO_WAIT;
    loop {
        let reply = sandbox::call::<AdapterInfo>(
            client,
            &target.adapter,
            &target.adapter_namespace,
            ADAPTER_INFO_SUFFIX,
            &AdapterInfoRequest {},
            CALL_TIMEOUT,
        )
        .await;
        match reply {
            Err(iii_sdk::Error::Remote { code, .. })
                if code == "function_not_found" && Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            reply => return reply.map_err(|error| call_failure("adapter::info", &error, target)),
        }
    }
}

/// Refuses an adapter this command cannot drive: another contract major, or one without the
/// contract 1.2 and 1.3 features a container's own VM depends on.
fn check_adapter(info: &AdapterInfo, adapter: &str) -> Result<(), Failure> {
    let version = &info.adapter_version;
    if info.contract.major != CONTRACT_MAJOR {
        return Err(Failure::runner(
            SANDBOX_CONTRACT_UNSUPPORTED,
            format!(
                "{adapter} {version} implements sandbox contract {}.{}, but this iii needs \
                 {CONTRACT_MAJOR}.x",
                info.contract.major, info.contract.minor
            ),
        ));
    }
    let missing: Vec<&str> = REQUIRED_FEATURES
        .into_iter()
        .filter(|feature| !info.supports(feature))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    // spec 6.1: an adapter offers a build only when it can boot one.
    let legacy = if missing.contains(&FEATURE_BUILD) {
        " An adapter offers worker.build only when it can boot a build: sandbox-libkrun does not \
         in legacy mode (III_ROOTFS_MODE=legacy, or a build without an embedded iii-init)."
    } else {
        ""
    };
    Err(Failure::runner(
        SANDBOX_BUILD_UNSUPPORTED,
        format!(
            "{adapter} {version} does not support {}, which this iii needs from sandbox contract \
             1.3.{legacy} A package adapter moves to its newest release with: iii call \
             compose::update worker=<its container key>@latest; if its version does not change, \
             no release has them yet",
            missing.join(", ")
        ),
    ))
}

/// A failed call to the adapter: its S-code and own words when it refused, else
/// `ENGINE_CALL_FAILED`, naming where the engine was looked for when it was not reached at all.
fn call_failure(function: &str, error: &iii_sdk::Error, target: &Target) -> Failure {
    if let Some((code, words)) = refusal(error) {
        return Failure::runner(&code, format!("{function}: {words}"));
    }
    let message = format!("{function}: {error}");
    Failure::runner(
        ENGINE_CALL_FAILED,
        match error {
            iii_sdk::Error::Timeout | iii_sdk::Error::NotConnected => format!(
                "{message}; is the engine at {} ({URL_ENV}) running, with {} connected?",
                target.engine_url, target.adapter
            ),
            _ => message,
        },
    )
}

/// An adapter's S-code refusal as its code and its own words, with the docs link it names;
/// `None` for anything else on the bus. The SDK carries it as `handler error: {json}`.
fn refusal(error: &iii_sdk::Error) -> Option<(String, String)> {
    let iii_sdk::Error::Remote { message, .. } = error else {
        return None;
    };
    let code = contract::code_of(message)?;
    let payload = message
        .find('{')
        .and_then(|at| serde_json::from_str::<serde_json::Value>(&message[at..]).ok());
    let words = payload
        .as_ref()
        .and_then(|payload| {
            let words = payload["message"].as_str()?;
            Some(match payload["docs_url"].as_str() {
                Some(docs) => format!("{words} ({docs})"),
                None => words.to_string(),
            })
        })
        .unwrap_or_else(|| message.clone());
    Some((code.as_str().to_string(), words))
}

enum Tick {
    Stop(Instant),
    Polled(Outcome),
}

/// Polls `vm` until it exits, printing its output, or stops it once `shutdown` resolves. The
/// whole tick races the signal, so a slow or restarting engine never delays a stop.
async fn supervise(
    ctx: &Context,
    client: &IIIClient,
    vm: &VmRecord,
    shutdown: &Shutdown,
    logs: &mut Logs,
) -> Result<ExitStatus, Failure> {
    let token = &ctx.target.token;
    let mut unanswered_since: Option<Instant> = None;
    loop {
        let tick = tokio::select! {
            biased;
            (_, at) = shutdown.requested() => Tick::Stop(at),
            outcome = poll(client, vm, token, logs) => Tick::Polled(outcome),
        };
        match tick {
            Tick::Stop(at) => {
                return Ok(stop(client, vm, token, ctx.stop_deadline(at), logs).await);
            }
            Tick::Polled(Outcome::Exited(status)) => {
                // The VM is gone: its last lines are read, unless a stop request cuts that short.
                tokio::select! {
                    biased;
                    _ = shutdown.requested() => {}
                    () = logs.finish(client, vm, token) => {}
                }
                return Ok(status);
            }
            Tick::Polled(Outcome::Running) => unanswered_since = None,
            Tick::Polled(Outcome::Unknown) => {
                let since = *unanswered_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= LOST_AFTER {
                    return Err(Failure::runner(
                        ENGINE_CALL_FAILED,
                        format!(
                            "{} has not answered about VM {} instance {} for {}s; giving it up, \
                             as the adapter's lease does",
                            vm.adapter,
                            vm.vm_id,
                            vm.instance,
                            LOST_AFTER.as_secs()
                        ),
                    ));
                }
            }
        }
    }
}

/// One tick: keeps the log stream open, waits a poll interval, then asks how the VM is.
async fn poll(client: &IIIClient, vm: &VmRecord, token: &str, logs: &mut Logs) -> Outcome {
    logs.follow(client, vm, token).await;
    tokio::time::sleep(POLL_INTERVAL).await;
    sandbox::outcome_of(sandbox::worker_status(client, vm, token).await)
}

/// `worker::stop`, asked again until the adapter confirms the VM ended or `deadline` passes;
/// every call and its grace fit in what is left. A stop never confirmed leaves the VM to the
/// adapter's lease, which stops it once this command is gone.
async fn stop(
    client: &IIIClient,
    vm: &VmRecord,
    token: &str,
    deadline: Instant,
    logs: &mut Logs,
) -> ExitStatus {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            eprintln!(
                "iii sandbox: VM {} instance {} did not confirm its stop in time; {} stops it \
                 once this command is gone",
                vm.vm_id, vm.instance, vm.adapter
            );
            return sandbox::vm_exit(None, None);
        }
        let request = WorkerStopRequest {
            auth: token.to_string(),
            vm_id: vm.vm_id.clone(),
            instance: vm.instance,
            // The rest of what is left is for the reply.
            grace_ms: (left * 8 / 9).as_millis() as u64,
        };
        let reply = sandbox::call::<WorkerStatus>(
            client,
            &vm.adapter,
            &vm.namespace,
            WORKER_STOP,
            &request,
            left,
        )
        .await;
        if let Outcome::Exited(status) = sandbox::outcome_of(reply) {
            let left = deadline.saturating_duration_since(Instant::now());
            let _ = tokio::time::timeout(left, logs.finish(client, vm, token)).await;
            return status;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(POLL_INTERVAL.min(left)).await;
    }
}

/// The guest's output, printed as this command's own: its stdout to stdout, its stderr to
/// stderr, where Compose captures them.
struct Logs {
    /// Reattached and no stream opened yet: the first starts at the end, after what the command
    /// that ran the VM before already printed.
    from_end: bool,
    /// Where a reopened stream continues: the last record printed, or where a `from_end`
    /// stream started.
    cursor: Option<u64>,
    capture: Option<JoinHandle<Option<u64>>>,
    printer: Printer,
}

impl Logs {
    fn new(from_end: bool, printer: Printer) -> Self {
        Self {
            from_end,
            cursor: None,
            capture: None,
            printer,
        }
    }

    /// Keeps one `worker::logs` stream open. One that closed while the VM runs (an engine
    /// restart, a dropped bus) is asked for again from [`Logs::cursor`].
    async fn follow(&mut self, client: &IIIClient, vm: &VmRecord, token: &str) {
        match self.capture.take() {
            Some(live) if !live.is_finished() => {
                self.capture = Some(live);
                return;
            }
            Some(done) => {
                if let Ok(Some(last)) = done.await {
                    self.cursor = Some(last);
                }
            }
            None => {}
        }
        let created =
            tokio::time::timeout(CALL_TIMEOUT, iii_sdk::helpers::create_channel(client, None))
                .await;
        let Ok(Ok(channel)) = created else {
            return;
        };
        let request = WorkerLogsRequest {
            auth: token.to_string(),
            vm_id: vm.vm_id.clone(),
            cursor: self.cursor,
            follow: true,
            channel: contract::StreamChannelRef {
                channel_id: channel.writer_ref.channel_id,
                access_key: channel.writer_ref.access_key,
                direction: contract::ChannelDirection::Write,
            },
            from_end: self.from_end,
        };
        let reply = sandbox::call::<WorkerLogsResponse>(
            client,
            &vm.adapter,
            &vm.namespace,
            WORKER_LOGS,
            &request,
            CALL_TIMEOUT,
        )
        .await;
        if let Ok(started) = reply {
            if self.from_end {
                // Later streams resume where this one started, not at a later end.
                self.from_end = false;
                self.cursor = started.cursor;
            }
            self.capture = Some(print_records(
                Arc::new(channel.reader),
                self.printer.clone(),
            ));
        }
    }

    /// After the VM exited: reads what its stream still holds, so its last lines are printed.
    async fn finish(&mut self, client: &IIIClient, vm: &VmRecord, token: &str) {
        self.follow(client, vm, token).await;
        if let Some(draining) = self.capture.take() {
            let _ = tokio::time::timeout(CALL_TIMEOUT, draining).await;
        }
    }
}

/// Prints every `LogRecord` text frame on `reader` until the channel closes, then resolves to the
/// cursor of the last one. Binary frames are keepalives, never records.
fn print_records(
    reader: Arc<iii_sdk::channel::ChannelReader>,
    printer: Printer,
) -> JoinHandle<Option<u64>> {
    tokio::spawn(async move {
        let last = Arc::new(Mutex::new(None));
        let seen = last.clone();
        reader
            .on_message(move |frame| {
                let Ok(record) = serde_json::from_str::<LogRecord>(&frame) else {
                    return;
                };
                (printer.0)(&record);
                if let Ok(mut seen) = seen.lock() {
                    *seen = Some(record.cursor);
                }
            })
            .await;
        while let Ok(Some(_)) = reader.next_binary().await {}
        last.lock().ok().and_then(|last| *last)
    })
}

/// What a build is made from, read before it starts: `build.json` records it with the image.
struct BuildInputs {
    source: BuildSource,
    declared_base_image: Option<String>,
    dependency_files: BTreeMap<String, String>,
}

/// Builds `bctx`'s container through the adapter's `worker::build`, printing the build's output
/// on stdout as it comes, and records the image in `build.json` once the adapter answered, never
/// before. A signal (130) or the build timeout (`TIMED_OUT`) closes the output channel, which
/// stops the build.
pub(crate) async fn build_vm(
    bctx: &BuildContext,
    records: &dyn Records,
    client: &IIIClient,
    shutdown: &Shutdown,
    printer: Printer,
) -> Result<BuildRecord, Failure> {
    let begun = Instant::now();
    let deadline = begun
        .checked_add(bctx.timeout)
        .unwrap_or_else(|| begun + Duration::from_secs(365 * 24 * 60 * 60));
    let ctx = &bctx.container;
    let target = &ctx.target;
    let info = tokio::select! {
        biased;
        (number, _) = shutdown.requested() => return Err(Failure::cancelled_by(number)),
        () = tokio::time::sleep_until(deadline) => return Err(Failure::timed_out(bctx.timeout)),
        info = adapter_info(target, client) => info?,
    };
    check_adapter(&info, &target.adapter)?;
    let inputs = build_inputs(bctx, records)?;

    let channel =
        tokio::time::timeout(CALL_TIMEOUT, iii_sdk::helpers::create_channel(client, None))
            .await
            .unwrap_or(Err(iii_sdk::Error::Timeout))
            .map_err(|error| call_failure("engine::channels::create", &error, target))?;
    let request = WorkerBuildRequest {
        auth: target.token.clone(),
        state_key: target.state_key.clone(),
        worker_name: ctx.worker_name.clone(),
        source: ctx.source.clone(),
        output_dir: bctx.build_dir.clone(),
        env: ctx.env.clone(),
        logs: contract::StreamChannelRef {
            channel_id: channel.writer_ref.channel_id,
            access_key: channel.writer_ref.access_key,
            direction: contract::ChannelDirection::Write,
        },
    };
    // Read from before the call: the build lives as long as its output is read.
    let reader = Arc::new(channel.reader);
    let output = print_records(reader.clone(), to_stdout(printer));
    let budget = deadline.saturating_duration_since(Instant::now()) + CALL_TIMEOUT;
    let reply = tokio::select! {
        // An answer that is there wins: its image is already in place.
        biased;
        reply = sandbox::call::<WorkerBuildResponse>(
            client,
            &target.adapter,
            &target.adapter_namespace,
            WORKER_BUILD,
            &request,
            budget,
        ) => reply,
        (number, _) = shutdown.requested() => {
            let _ = reader.close().await;
            return Err(Failure::cancelled_by(number));
        }
        () = tokio::time::sleep_until(deadline) => {
            let _ = reader.close().await;
            return Err(Failure::timed_out(bctx.timeout));
        }
    };
    // The build's last lines, until the adapter closes the channel.
    tokio::select! {
        biased;
        _ = shutdown.requested() => {}
        _ = tokio::time::timeout(DRAIN_WAIT, output) => {}
    }
    let _ = reader.close().await;

    let built = reply.map_err(|error| build_call_failure(&error, target))?;
    let record = build_record_of(bctx, &info, inputs, built);
    records
        .write(Path::new(&bctx.build_dir), &record)
        .map_err(|error| Failure::compose(&error))?;
    Ok(record)
}

/// The build's inputs, read before it starts so a project that cannot record one builds nothing.
fn build_inputs(bctx: &BuildContext, records: &dyn Records) -> Result<BuildInputs, Failure> {
    let worker_dir = bctx.worker_dir();
    let failed = |error: ComposeError| Failure::compose(&error);
    Ok(BuildInputs {
        source: records
            .source(&bctx.compose_file, &bctx.container.worker_name)
            .map_err(failed)?,
        declared_base_image: records.declared_base_image(worker_dir).map_err(failed)?,
        dependency_files: records.dependency_files(worker_dir).map_err(failed)?,
    })
}

/// `build.json` for the image `built` names (spec 5.4).
fn build_record_of(
    bctx: &BuildContext,
    info: &AdapterInfo,
    inputs: BuildInputs,
    built: WorkerBuildResponse,
) -> BuildRecord {
    BuildRecord {
        schema: build_record::SCHEMA,
        container: bctx.container.worker_name.clone(),
        source: inputs.source,
        command: BUILD_COMMAND.to_string(),
        dependency_files: inputs.dependency_files,
        image: Some(BuildImage {
            file: IMAGE_FILE.to_string(),
            build_id: built.build_id,
            base_image: BuildBaseImage {
                declared: inputs.declared_base_image,
                reference: built.base_image.reference,
                digest: built.base_image.digest,
            },
            adapter: BuildAdapter {
                name: info.adapter.clone(),
                version: info.adapter_version.clone(),
            },
            contract: BuildContract {
                major: info.contract.major,
                minor: info.contract.minor,
            },
        }),
        built_at: rfc3339(built.built_at_ms),
    }
}

/// Milliseconds since the epoch as RFC 3339 UTC to the second; now, when out of range.
fn rfc3339(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A `worker::build` that did not answer with an image. The adapter's answer is the build's
/// failure (exit 1), with its S-code, or else its error code; not reaching the adapter is this
/// command's own (125).
fn build_call_failure(error: &iii_sdk::Error, target: &Target) -> Failure {
    let failure = call_failure("worker::build", error, target);
    match error {
        iii_sdk::Error::Remote { code, .. } if code != "function_not_found" => Failure {
            code: if failure.code == ENGINE_CALL_FAILED {
                code.clone()
            } else {
                failure.code
            },
            exit: BUILD_FAILED_EXIT,
            ..failure
        },
        _ => failure,
    }
}

/// `printer`, with every record on stdout: a build's output is this command's stdout.
fn to_stdout(printer: Printer) -> Printer {
    Printer(Arc::new(move |record: &LogRecord| {
        let record = LogRecord {
            stream: LogStream::Stdout,
            ..record.clone()
        };
        (printer.0)(&record)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::tests::{answered, fake_engine, fake_engine_url, refused};
    use iii_sandbox_contract::{ContractVersion, State};
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn compose_vars() -> BTreeMap<String, String> {
        [
            (URL_ENV, "ws://127.0.0.1:49134"),
            (ADAPTER_ENV, "sandbox-libkrun"),
            (ADAPTER_NAMESPACE_ENV, "orders"),
            (TOKEN_ENV, "scoped"),
            (STATE_KEY_ENV, "9f2c1e"),
            (WORKER_NAME_ENV, "api"),
            (WORKER_DIR_ENV, "/work/api"),
            (BUILD_DIR_ENV, "/work/.build/api"),
            (BUILD_COMPOSE_FILE_ENV, "/work/worker-compose.yaml"),
            (BUILD_TIMEOUT_ENV, "60000"),
            (
                WORKER_ENV_KEYS_ENV,
                "III_WORKER_NAME, API_KEY,III_SANDBOX_TOKEN,III_URL,III_SANDBOX_WORKER_TOKEN,UNSET,\
                 III_BUILD_DIR,III_BUILD_COMPOSE_FILE,III_BUILD_TIMEOUT_MS",
            ),
            ("API_KEY", "k"),
            (PROJECT_TOKEN_ENV, "project"),
            ("HOME", "/home/operator"),
            (STOP_TIMEOUT_ENV, "10000"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
    }

    fn context() -> Context {
        Context::from_vars(&compose_vars(), None).unwrap()
    }

    fn context_stopping_within(ms: &str) -> Context {
        let mut vars = compose_vars();
        vars.insert(STOP_TIMEOUT_ENV.into(), ms.into());
        Context::from_vars(&vars, None).unwrap()
    }

    /// The build `compose_vars` names: what `iii sandbox run` boots.
    fn booted() -> WorkerBuildRef {
        WorkerBuildRef {
            build_id: "sha256:7d41".into(),
            build_dir: "/work/.build/api".into(),
        }
    }

    /// A `build.json`, with an image when `build_id` is given.
    fn record(build_id: Option<&str>) -> BuildRecord {
        BuildRecord {
            schema: build_record::SCHEMA,
            container: "api".into(),
            source: BuildSource::Path {
                dir: "./api".into(),
            },
            command: "iii sandbox build".into(),
            dependency_files: BTreeMap::new(),
            image: build_id.map(|build_id| BuildImage {
                file: "image.erofs".into(),
                build_id: build_id.into(),
                base_image: BuildBaseImage {
                    declared: None,
                    reference: "docker.io/iiidev/node:latest".into(),
                    digest: None,
                },
                adapter: BuildAdapter {
                    name: "sandbox-libkrun".into(),
                    version: "0.4.0".into(),
                },
                contract: BuildContract { major: 1, minor: 3 },
            }),
            built_at: "2026-11-10T14:03:22Z".into(),
        }
    }

    /// `build.json` in memory: what `read` returns and every directory it was read from, the
    /// inputs a build records, and every write.
    #[derive(Default)]
    struct FakeRecords {
        stored: Option<BuildRecord>,
        read_from: Mutex<Vec<PathBuf>>,
        written: Mutex<Vec<(PathBuf, BuildRecord)>>,
        /// What happened, in order: each input read and each write; a test adds its own steps.
        events: Arc<Mutex<Vec<String>>>,
    }

    impl FakeRecords {
        fn holding(record: BuildRecord) -> Self {
            Self {
                stored: Some(record),
                ..Self::default()
            }
        }
    }

    impl FakeRecords {
        fn happened(&self, event: String) {
            self.events.lock().unwrap().push(event);
        }
    }

    impl Records for FakeRecords {
        fn read(&self, build_dir: &Path) -> crate::error::Result<Option<BuildRecord>> {
            self.read_from.lock().unwrap().push(build_dir.to_path_buf());
            Ok(self.stored.clone())
        }

        fn write(&self, build_dir: &Path, record: &BuildRecord) -> crate::error::Result<()> {
            self.happened("write".into());
            self.written
                .lock()
                .unwrap()
                .push((build_dir.to_path_buf(), record.clone()));
            Ok(())
        }

        fn source(
            &self,
            compose_file: &Path,
            container: &str,
        ) -> crate::error::Result<BuildSource> {
            self.happened(format!("source {} {container}", compose_file.display()));
            Ok(BuildSource::Package {
                name: "onboarding".into(),
                version: "0.2.0".into(),
                sha256: "sha256:8c34".into(),
            })
        }

        fn declared_base_image(&self, worker_dir: &Path) -> crate::error::Result<Option<String>> {
            self.happened(format!("declared_base_image {}", worker_dir.display()));
            Ok(Some("docker.io/iiidev/python".into()))
        }

        fn dependency_files(
            &self,
            worker_dir: &Path,
        ) -> crate::error::Result<BTreeMap<String, String>> {
            self.happened(format!("dependency_files {}", worker_dir.display()));
            Ok([("package.json".to_string(), "sha256:2f1c".to_string())].into())
        }
    }

    fn build_context() -> BuildContext {
        BuildContext::from_vars(&compose_vars()).unwrap()
    }

    fn build_context_within(ms: &str) -> BuildContext {
        let mut vars = compose_vars();
        vars.insert(BUILD_TIMEOUT_ENV.into(), ms.into());
        BuildContext::from_vars(&vars).unwrap()
    }

    fn log_line(stream: &str, line: &str, cursor: u64) -> String {
        json!({"stream": stream, "line": line, "ts_ms": 1, "cursor": cursor}).to_string()
    }

    /// `worker::build`'s answer: image `sha256:7d41`, built at 2023-11-14T22:13:20Z.
    fn built() -> Value {
        answered(json!({
            "build_id": "sha256:7d41",
            "base_image": {"reference": "docker.io/iiidev/python:latest", "digest": "sha256:b1e2"},
            "built_at_ms": 1_700_000_000_000_u64,
        }))
    }

    /// An S-code refusal carrying `message`, as the SDK delivers an adapter handler's error.
    fn refused_with(code: SandboxErrorCode, message: &str) -> Value {
        let payload = contract::error_payload(code, message, None, None);
        json!({"error": {"code": "invocation_failed", "message": format!("handler error: {payload}")}})
    }

    /// What a fake build channel sends its reader, in order, before it closes.
    enum Frame {
        Text(String),
        Binary(Vec<u8>),
        /// A one-byte keepalive every 50 ms, until the reader goes.
        KeepAlive,
    }

    /// A fake engine for `worker::build`. `answer` answers each call as `fake_engine`'s does,
    /// by function id (`true` leaves it unanswered), and every call goes to the receiver. A
    /// reader of the build channel gets `frames`, then a close; `gone` is notified when it
    /// closed its end first.
    async fn build_engine(
        answer: impl Fn(&str, &Value) -> Value + Send + Sync + 'static,
        frames: Vec<Frame>,
    ) -> (
        IIIClient,
        tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
        Arc<tokio::sync::Notify>,
    ) {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::{
            Message,
            handshake::server::{Request, Response},
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let answer = Arc::new(answer);
        let frames = Arc::new(frames);
        let gone = Arc::new(tokio::sync::Notify::new());
        let (calls, called) = tokio::sync::mpsc::unbounded_channel();
        let reader_gone = gone.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (calls, answer, frames, gone) = (
                    calls.clone(),
                    answer.clone(),
                    frames.clone(),
                    reader_gone.clone(),
                );
                tokio::spawn(async move {
                    let mut path = String::new();
                    // tungstenite fixes the callback's error type.
                    #[allow(clippy::result_large_err)]
                    let accept = |request: &Request, response: Response| {
                        path = request.uri().path().to_string();
                        Ok(response)
                    };
                    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, accept).await
                    else {
                        return;
                    };
                    if path.starts_with("/ws/channels/") {
                        for frame in frames.iter() {
                            let message = match frame {
                                Frame::Text(text) => Message::Text(text.clone().into()),
                                Frame::Binary(bytes) => Message::Binary(bytes.clone().into()),
                                Frame::KeepAlive => loop {
                                    tokio::select! {
                                        read = socket.next() => match read {
                                            Some(Ok(Message::Close(_)) | Err(_)) | None => {
                                                gone.notify_one();
                                                return;
                                            }
                                            Some(Ok(_)) => {}
                                        },
                                        () = tokio::time::sleep(Duration::from_millis(50)) => {
                                            let keepalive = Message::Binary(vec![0].into());
                                            if socket.send(keepalive).await.is_err() {
                                                gone.notify_one();
                                                return;
                                            }
                                        }
                                    }
                                },
                            };
                            if socket.send(message).await.is_err() {
                                return;
                            }
                        }
                        let _ = socket.close(None).await;
                        return;
                    }
                    while let Some(Ok(frame)) = socket.next().await {
                        let Ok(message) = serde_json::from_slice::<Value>(&frame.into_data())
                        else {
                            continue;
                        };
                        let function = message["function_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        if message["type"] != "invokefunction"
                            || (function.starts_with("engine::workers::")
                                && function != "engine::workers::list")
                        {
                            continue;
                        }
                        let end = |direction| json!({"channel_id": "c", "access_key": "k", "direction": direction});
                        let mut reply = match function.as_str() {
                            "engine::channels::create" => {
                                json!({"result": {"writer": end("write"), "reader": end("read")}})
                            }
                            _ => answer(&function, &message["data"]),
                        };
                        let _ = calls.send((function.clone(), message["data"].clone()));
                        if reply == json!(true) {
                            continue;
                        }
                        reply["type"] = json!("invocationresult");
                        reply["invocation_id"] = message["invocation_id"].clone();
                        reply["function_id"] = json!(function);
                        let _ = socket.send(Message::Text(reply.to_string().into())).await;
                    }
                });
            }
        });
        let client =
            iii_sdk::register_worker(&format!("ws://{address}"), iii_sdk::InitOptions::default());
        (client, called, gone)
    }

    /// A shutdown that never comes.
    fn never() -> Shutdown {
        Shutdown::manual().1
    }

    fn signal_after(delay: Duration, number: i32) -> Shutdown {
        let (sender, shutdown) = Shutdown::manual();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            sender.send_replace(Some((number, Instant::now())));
        });
        shutdown
    }

    /// A printer into a buffer, as `stream: line`.
    fn captured() -> (Printer, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        let printer = Printer(Arc::new(move |record: &LogRecord| {
            let stream = match record.stream {
                LogStream::Stdout => "stdout",
                LogStream::Stderr => "stderr",
            };
            sink.lock()
                .unwrap()
                .push(format!("{stream}: {}", record.line));
        }));
        (printer, lines)
    }

    fn info(features: &[&str]) -> AdapterInfo {
        AdapterInfo {
            adapter: "sandbox-libkrun".into(),
            adapter_version: "0.3.0".into(),
            contract: ContractVersion::CURRENT,
            features: features.iter().map(|f| f.to_string()).collect(),
        }
    }

    fn status(state: State, exit_code: Option<i32>, signal: Option<i32>) -> Value {
        answered(WorkerStatus {
            vm_id: "v".into(),
            instance: 1,
            state,
            exit_code,
            signal,
            started_at_ms: 0,
            ended_at_ms: None,
        })
    }

    /// An adapter that boots instance 1 of VM `v`, keeps it running and stops it on SIGTERM,
    /// except where `overrides` answers a call (by function suffix) itself. An override of
    /// `true` leaves the call unanswered.
    fn adapter(
        overrides: impl Fn(&str, &Value) -> Option<Value> + Send + Sync + 'static,
    ) -> impl Fn(&str, &Value) -> Value + Send + Sync + 'static {
        move |function, payload| {
            let suffix = function.trim_start_matches("sandbox-libkrun::");
            overrides(suffix, payload).unwrap_or_else(|| match suffix {
                "adapter::info" => answered(info(&REQUIRED_FEATURES)),
                "worker::start" => answered(json!({"vm_id": "v", "instance": 1})),
                "worker::status" => status(State::Running, None, None),
                "worker::stop" => status(State::Stopped, None, Some(15)),
                _ => answered(json!({})),
            })
        }
    }

    fn calls(
        called: &mut tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
    ) -> Vec<(String, Value)> {
        let mut calls = Vec::new();
        while let Ok((function, payload)) = called.try_recv() {
            calls.push((
                function.trim_start_matches("sandbox-libkrun::").into(),
                payload,
            ));
        }
        calls
    }

    fn payloads<'a>(calls: &'a [(String, Value)], function: &str) -> Vec<&'a Value> {
        calls
            .iter()
            .filter(|(called, _)| called == function)
            .map(|(_, payload)| payload)
            .collect()
    }

    fn payload<'a>(calls: &'a [(String, Value)], function: &str) -> &'a Value {
        payloads(calls, function)
            .first()
            .copied()
            .unwrap_or_else(|| panic!("{function} was not called: {calls:?}"))
    }

    #[test]
    fn a_container_without_the_adapter_grant_is_refused() {
        let mut vars = compose_vars();
        vars.remove(TOKEN_ENV);
        let error = Context::from_vars(&vars, None).unwrap_err();
        assert!(
            error.contains(TOKEN_ENV) && error.contains("runtime.base_image"),
            "{error}"
        );
    }

    /// What Compose hands the container is what this command reads.
    #[test]
    fn compose_hands_the_container_everything_the_command_reads() {
        let dir = tempfile::tempdir().unwrap();
        crate::sandbox::tests::manifest(
            dir.path(),
            "api",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let file = crate::config::ComposeFile::parse(
            "stop_timeout: 7s\ncontainers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  api:\n    worker: path://./api\n    start_after: [sb]\n    environment: {API_KEY: k, PATH: /opt/tools}\n    scripts: {run: iii sandbox run}\n",
            dir.path().join("worker-compose.yaml"),
        )
        .unwrap();
        let state_dir = std::path::Path::new("/state");
        let mut user_env = file.containers["api"].resolve_user_env("api").unwrap();
        sandbox::layer_own_vm_env(&file, "api", "project", state_dir, "orders", &mut user_env)
            .unwrap();
        let start = crate::manifest::StartSpec::Shell("iii sandbox run".into());
        let plan = crate::spawn::spawn_plan(&crate::spawn::SpawnCtx {
            engine_url: "ws://engine.test:1",
            namespace: "orders",
            compose_namespace: "compose",
            compose_file: &file.path,
            container_key: "api",
            start: &start,
            config_name: Some("api-config"),
            working_dir: dir.path(),
            user_env: &user_env,
        });

        let ctx = Context::from_vars(&plan.env, None).unwrap();
        let key = sandbox::state_key(state_dir, "api");
        assert_eq!(ctx.target.state_key, key);
        assert_eq!(
            ctx.target.token,
            iii_sandbox_contract::scoped_auth_token("project", &key)
        );
        assert_eq!(ctx.target.engine_url, "ws://engine.test:1");
        assert_eq!(ctx.target.adapter, "sandbox-libkrun");
        assert_eq!(ctx.target.adapter_namespace, "orders");
        assert_eq!(ctx.worker_name, "api");
        assert_eq!(ctx.stop_timeout, Duration::from_secs(7));
        let WorkerSource::Local {
            host_path,
            run_override: None,
        } = &ctx.source
        else {
            panic!("{:?}", ctx.source);
        };
        assert_eq!(
            std::path::Path::new(host_path),
            file.containers["api"].worker_dir().unwrap()
        );

        // The guest gets its own variables and the reserved contract: without III_NAMESPACE
        // it would register where readiness does not look.
        assert_eq!(
            ctx.env.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "API_KEY",
                "III_COMPOSE_DIR",
                "III_COMPOSE_FILE",
                "III_COMPOSE_NAMESPACE",
                "III_CONFIG_NAME",
                "III_NAMESPACE",
                "III_WORKER_NAME",
                "PATH",
            ]
        );
        assert_eq!(ctx.env["III_NAMESPACE"], "orders");
        assert_eq!(ctx.env["III_CONFIG_NAME"], "api-config");

        // Only the scoped token reaches this container.
        assert!(!plan.env.contains_key(PROJECT_TOKEN_ENV));
        assert!(!plan.env.values().any(|value| value == "project"));
        assert!(!plan.env[WORKER_ENV_KEYS_ENV].contains(PROJECT_TOKEN_ENV));

        // `iii` is this binary, and the operator's PATH follows it.
        let path: Vec<_> = std::env::split_paths(&plan.env["PATH"]).collect();
        assert_eq!(path[0], std::env::current_exe().unwrap().parent().unwrap());
        assert_eq!(path[1..], [std::path::PathBuf::from("/opt/tools")]);
    }

    #[test]
    fn a_container_that_does_not_run_its_own_vm_gets_none_of_it() {
        let dir = tempfile::tempdir().unwrap();
        crate::sandbox::tests::manifest(dir.path(), "api", "scripts: {start: run}\n");
        let file = crate::config::ComposeFile::parse(
            "containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  api: {worker: path://./api, start_after: [sb]}\n",
            dir.path().join("worker-compose.yaml"),
        )
        .unwrap();
        for key in ["api", "sb"] {
            let mut env = BTreeMap::new();
            sandbox::layer_own_vm_env(&file, key, "project", dir.path(), "orders", &mut env)
                .unwrap();
            assert!(env.is_empty(), "{key}: {env:?}");
        }
    }

    #[test]
    fn only_the_listed_worker_variables_reach_the_guest() {
        let expected: BTreeMap<String, String> = [("API_KEY", "k"), ("III_WORKER_NAME", "api")]
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        assert_eq!(context().env, expected);
    }

    #[test]
    fn a_path_worker_may_replace_its_start_command_and_a_bundle_may_not() {
        let ctx = Context::from_vars(&compose_vars(), Some("python main.py".into())).unwrap();
        assert!(matches!(
            ctx.source,
            WorkerSource::Local { run_override: Some(ref command), .. } if command == "python main.py"
        ));

        let mut bundle = compose_vars();
        bundle.remove(WORKER_DIR_ENV);
        bundle.insert(PACKAGE_DIR_ENV.into(), "/cache/onboarding".into());
        bundle.insert(PACKAGE_NAME_ENV.into(), "onboarding".into());
        assert!(matches!(
            Context::from_vars(&bundle, None).unwrap().source,
            WorkerSource::Bundle { ref package, .. } if package == "onboarding"
        ));
        assert!(Context::from_vars(&bundle, Some("x".into())).is_err());
    }

    #[test]
    fn a_stop_ends_inside_the_stop_timeout() {
        let at = Instant::now();
        assert_eq!(context().stop_deadline(at) - at, Duration::from_secs(9));
        // An absurd timeout is a far deadline, not a panic.
        let far = context_stopping_within(&u64::MAX.to_string()).stop_deadline(at);
        assert!(far > at + Duration::from_secs(24 * 60 * 60));
    }

    #[cfg(unix)]
    #[test]
    fn exit_codes_read_like_a_shell_child() {
        assert_eq!(exit_code(sandbox::vm_exit(Some(3), None)), 3);
        assert_eq!(exit_code(sandbox::vm_exit(None, Some(15))), 143);
        assert_eq!(exit_code(sandbox::vm_exit(None, None)), 137);
        assert_eq!(exit_code(signalled(2)), 130);
    }

    #[test]
    fn an_adapter_without_the_features_is_refused() {
        let mut adapter = info(&[]);
        let failure = check_adapter(&adapter, "sandbox-libkrun").unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (SANDBOX_BUILD_UNSUPPORTED, RUNNER_FAILED)
        );
        let error = failure.message;
        assert!(
            error.contains(FEATURE_SCOPED_AUTH) && error.contains(FEATURE_BUILD),
            "{error}"
        );
        assert!(error.contains("sandbox contract 1.3"), "{error}");
        // The adapter's name is not its container key, and only a package can move.
        assert!(
            error.contains("A package adapter moves")
                && error.contains("iii call compose::update worker=<its container key>@latest")
                && !error.contains("worker=sandbox-libkrun"),
            "{error}"
        );
        // `latest` may be the version it runs: the advice does not promise a move.
        assert!(error.contains("no release has them yet"), "{error}");
        adapter.features = REQUIRED_FEATURES.iter().map(|f| f.to_string()).collect();
        assert!(check_adapter(&adapter, "sandbox-libkrun").is_ok());
    }

    /// A contract 1.2 adapter, or sandbox-libkrun in legacy mode: everything but the build.
    #[test]
    fn an_adapter_without_worker_build_is_refused_and_told_why() {
        let adapter = info(&[FEATURE_SCOPED_AUTH, FEATURE_LEASE, FEATURE_LOGS_FROM_END]);
        let failure = check_adapter(&adapter, "sandbox-libkrun").unwrap_err();
        assert_eq!(failure.code, SANDBOX_BUILD_UNSUPPORTED);
        assert!(
            failure.message.contains("does not support worker.build,")
                && failure.message.contains("III_ROOTFS_MODE=legacy"),
            "{failure}"
        );
    }

    #[test]
    fn an_adapter_of_another_contract_major_is_refused() {
        let mut adapter = info(&REQUIRED_FEATURES);
        adapter.contract.major = CONTRACT_MAJOR + 1;
        let failure = check_adapter(&adapter, "sandbox-libkrun").unwrap_err();
        assert_eq!(failure.code, SANDBOX_CONTRACT_UNSUPPORTED);
        assert_eq!(failure.exit, RUNNER_FAILED);
    }

    #[test]
    fn windows_refuses_run_and_build() {
        let failure = vm_platform(true).unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (SANDBOX_UNSUPPORTED_PLATFORM, RUNNER_FAILED)
        );
        assert!(vm_platform(false).is_ok());
    }

    #[test]
    fn run_boots_the_build_its_build_dir_records() {
        let records = FakeRecords::holding(record(Some("sha256:7d41")));
        let (ctx, build) = boot_plan(&compose_vars(), None, &records).unwrap();
        assert_eq!(ctx.worker_name, "api");
        assert_eq!(build.build_id, "sha256:7d41");
        assert_eq!(build.build_dir, "/work/.build/api");
        assert_eq!(
            *records.read_from.lock().unwrap(),
            [PathBuf::from("/work/.build/api")]
        );
    }

    #[test]
    fn run_without_a_build_is_build_missing() {
        let failure = boot_plan(&compose_vars(), None, &FakeRecords::default()).unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (BUILD_MISSING, RUNNER_FAILED)
        );
        assert!(
            failure.message.contains("/work/.build/api")
                && failure.message.contains("iii sandbox build api"),
            "{failure}"
        );
        // A host build records no image to boot.
        let host = FakeRecords::holding(record(None));
        let failure = boot_plan(&compose_vars(), None, &host).unwrap_err();
        assert_eq!(failure.code, BUILD_MISSING);
        assert!(failure.message.contains("host build"), "{failure}");
    }

    #[test]
    fn run_without_a_build_dir_is_refused() {
        let mut vars = compose_vars();
        vars.remove(BUILD_DIR_ENV);
        let failure = boot_plan(&vars, None, &FakeRecords::default()).unwrap_err();
        assert_eq!(failure.code, SANDBOX_ENV_INVALID);
        assert!(failure.message.contains(BUILD_DIR_ENV), "{failure}");
    }

    /// The command itself: without a build it exits 125 before it reaches the engine.
    #[cfg(unix)]
    #[tokio::test]
    async fn run_without_a_build_exits_125_before_calling_anything() {
        let (url, mut called) = fake_engine_url(adapter(|_, _| None)).await;
        let mut vars = compose_vars();
        vars.insert(URL_ENV.into(), url);
        let failure = run_command(&vars, None, &FakeRecords::default())
            .await
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (BUILD_MISSING, RUNNER_FAILED)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(calls(&mut called).is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn run_is_refused_on_windows() {
        let records = FakeRecords::holding(record(Some("sha256:7d41")));
        let failure = run_command(&compose_vars(), None, &records)
            .await
            .unwrap_err();
        assert_eq!(failure.code, SANDBOX_UNSUPPORTED_PLATFORM);
    }

    #[test]
    fn the_build_reads_its_directory_compose_file_and_timeout_from_compose() {
        let bctx = build_context();
        assert_eq!(bctx.build_dir, "/work/.build/api");
        assert_eq!(
            bctx.compose_file,
            PathBuf::from("/work/worker-compose.yaml")
        );
        assert_eq!(bctx.timeout, Duration::from_secs(60));
        assert_eq!(bctx.worker_dir(), Path::new("/work/api"));

        let mut vars = compose_vars();
        vars.remove(BUILD_TIMEOUT_ENV);
        let default = BuildContext::from_vars(&vars).unwrap().timeout;
        assert_eq!(default, Duration::from_secs(20 * 60));
        vars.insert(BUILD_TIMEOUT_ENV.into(), "20m".into());
        let error = BuildContext::from_vars(&vars).unwrap_err();
        assert!(error.contains(BUILD_TIMEOUT_ENV), "{error}");
        for name in [BUILD_DIR_ENV, BUILD_COMPOSE_FILE_ENV] {
            let mut vars = compose_vars();
            vars.remove(name);
            let error = BuildContext::from_vars(&vars).unwrap_err();
            assert!(error.contains(name), "{error}");
        }

        // A bundle builds from its package directory.
        let mut bundle = compose_vars();
        bundle.remove(WORKER_DIR_ENV);
        bundle.insert(PACKAGE_DIR_ENV.into(), "/work/.workers/api".into());
        bundle.insert(PACKAGE_NAME_ENV.into(), "onboarding".into());
        let bctx = BuildContext::from_vars(&bundle).unwrap();
        assert_eq!(bctx.worker_dir(), Path::new("/work/.workers/api"));
    }

    #[test]
    fn inside_compose_the_build_is_the_containers_own() {
        let bctx = build_context();
        assert!(own_container(&bctx, None).is_ok());
        assert!(own_container(&bctx, Some("api")).is_ok());
        let failure = own_container(&bctx, Some("web")).unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (SANDBOX_INVALID_ARGUMENTS, RUNNER_FAILED)
        );
    }

    #[test]
    fn the_last_line_of_a_failed_build_is_one_line_compose_can_read() {
        let line = build_failure_line(TIMED_OUT, "did not finish\n\n  in time  \n");
        assert_eq!(
            line,
            "iii sandbox: build failed: TIMED_OUT: did not finish; in time"
        );
        assert_eq!(
            parse_build_failure(&line),
            Some((TIMED_OUT, "did not finish; in time"))
        );
        assert_eq!(
            parse_build_failure("iii sandbox: built api (sha256:7d41)"),
            None
        );
    }

    #[test]
    fn what_the_adapter_answers_is_the_builds_failure_and_the_rest_is_this_commands() {
        let target = &context().target;
        let remote = |code: &str, message: String| iii_sdk::Error::Remote {
            code: code.into(),
            message,
            stacktrace: None,
        };
        let payload =
            contract::error_payload(SandboxErrorCode::S509, "a build is running", None, None);
        let s509 = remote("invocation_failed", format!("handler error: {payload}"));
        let failure = build_call_failure(&s509, target);
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            ("S509", BUILD_FAILED_EXIT)
        );
        assert!(
            failure
                .message
                .starts_with("worker::build: a build is running"),
            "{failure}"
        );

        let crashed = remote("invocation_failed", "the build VM crashed".into());
        let failure = build_call_failure(&crashed, target);
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            ("invocation_failed", BUILD_FAILED_EXIT)
        );

        for unreached in [
            iii_sdk::Error::Timeout,
            iii_sdk::Error::NotConnected,
            remote("function_not_found", "gone".into()),
        ] {
            let failure = build_call_failure(&unreached, target);
            assert_eq!(
                (failure.code.as_str(), failure.exit),
                (ENGINE_CALL_FAILED, RUNNER_FAILED),
                "{unreached}"
            );
        }
        let failure = build_call_failure(&iii_sdk::Error::Timeout, target);
        assert!(
            failure
                .message
                .contains("is the engine at ws://127.0.0.1:49134"),
            "{failure}"
        );
    }

    #[tokio::test]
    async fn build_prints_text_frames_only_and_records_the_image_after_the_answer() {
        let records = FakeRecords::default();
        let events = records.events.clone();
        let frames = vec![
            Frame::Binary(vec![0]),
            Frame::Text(log_line("stdout", "npm ci", 1)),
            // A record in a binary frame is no record: records come in text frames only.
            Frame::Binary(log_line("stdout", "not a record", 2).into_bytes()),
            Frame::Text("not json".into()),
            Frame::Binary(vec![0]),
            Frame::Text(log_line("stderr", "added 3 packages", 3)),
        ];
        let (client, mut called, _) = build_engine(
            adapter(move |suffix, _| {
                (suffix == "worker::build").then(|| {
                    events.lock().unwrap().push("answered".into());
                    built()
                })
            }),
            frames,
        )
        .await;
        let (printer, printed) = captured();
        let record = build_vm(&build_context(), &records, &client, &never(), printer)
            .await
            .unwrap();

        // Both of the guest's streams are this command's stdout.
        assert_eq!(
            *printed.lock().unwrap(),
            ["stdout: npm ci", "stdout: added 3 packages"]
        );

        let calls = calls(&mut called);
        let request = payload(&calls, "worker::build");
        assert_eq!(request["output_dir"], "/work/.build/api");
        assert_eq!(request["state_key"], "9f2c1e");
        assert_eq!(request["auth"], "scoped");
        assert_eq!(request["worker_name"], "api");
        assert_eq!(
            request["source"],
            json!({"kind": "local", "host_path": "/work/api", "run_override": null})
        );
        assert_eq!(
            request["env"],
            json!({"API_KEY": "k", "III_WORKER_NAME": "api"})
        );
        assert_eq!(request["logs"]["direction"], "write");
        for not_called in ["worker::prepare", "worker::start"] {
            assert!(payloads(&calls, not_called).is_empty(), "{calls:?}");
        }

        let expected = BuildRecord {
            schema: build_record::SCHEMA,
            container: "api".into(),
            source: BuildSource::Package {
                name: "onboarding".into(),
                version: "0.2.0".into(),
                sha256: "sha256:8c34".into(),
            },
            command: "iii sandbox build".into(),
            dependency_files: [("package.json".to_string(), "sha256:2f1c".to_string())].into(),
            image: Some(BuildImage {
                file: "image.erofs".into(),
                build_id: "sha256:7d41".into(),
                base_image: BuildBaseImage {
                    declared: Some("docker.io/iiidev/python".into()),
                    reference: "docker.io/iiidev/python:latest".into(),
                    digest: Some("sha256:b1e2".into()),
                },
                adapter: BuildAdapter {
                    name: "sandbox-libkrun".into(),
                    version: "0.3.0".into(),
                },
                contract: BuildContract {
                    major: CONTRACT_MAJOR,
                    minor: contract::CONTRACT_MINOR,
                },
            }),
            built_at: "2023-11-14T22:13:20Z".into(),
        };
        assert_eq!(record, expected);
        assert_eq!(
            *records.written.lock().unwrap(),
            [(PathBuf::from("/work/.build/api"), expected)]
        );
        // The inputs are read before the build; the record is written after its answer.
        assert_eq!(
            *records.events.lock().unwrap(),
            [
                "source /work/worker-compose.yaml api",
                "declared_base_image /work/api",
                "dependency_files /work/api",
                "answered",
                "write",
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_build_exits_1_with_the_adapters_code_and_records_nothing() {
        let records = FakeRecords::default();
        let frames = vec![Frame::Text(log_line(
            "stderr",
            "npm ERR! missing script",
            1,
        ))];
        let (client, _called, _) = build_engine(
            adapter(|suffix, _| {
                (suffix == "worker::build").then(|| {
                    refused_with(
                        SandboxErrorCode::S507,
                        "scripts.install exited 1:\nnpm ERR! missing script",
                    )
                })
            }),
            frames,
        )
        .await;
        let (printer, printed) = captured();
        let failure = build_vm(&build_context(), &records, &client, &never(), printer)
            .await
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            ("S507", BUILD_FAILED_EXIT)
        );
        assert_eq!(
            *printed.lock().unwrap(),
            ["stdout: npm ERR! missing script"]
        );
        assert!(records.written.lock().unwrap().is_empty());

        let line = build_failure_line(&failure.code, &failure.message);
        let (code, message) = parse_build_failure(&line).unwrap();
        assert_eq!(code, "S507");
        assert!(
            message
                .starts_with("worker::build: scripts.install exited 1:; npm ERR! missing script"),
            "{line}"
        );
    }

    #[tokio::test]
    async fn a_build_past_its_timeout_closes_the_channel_and_fails_with_timed_out() {
        let records = FakeRecords::default();
        let (client, _called, gone) = build_engine(
            adapter(|suffix, _| (suffix == "worker::build").then_some(json!(true))),
            vec![Frame::KeepAlive],
        )
        .await;
        let (printer, printed) = captured();
        let begun = Instant::now();
        let failure = build_vm(
            &build_context_within("700"),
            &records,
            &client,
            &never(),
            printer,
        )
        .await
        .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (TIMED_OUT, BUILD_FAILED_EXIT)
        );
        assert!(failure.message.contains("700ms"), "{failure}");
        assert!(begun.elapsed() < Duration::from_secs(3));
        // Closing the channel is what stops the adapter's build.
        tokio::time::timeout(Duration::from_secs(5), gone.notified())
            .await
            .expect("the build channel was left open");
        assert!(
            printed.lock().unwrap().is_empty(),
            "a keepalive is no output"
        );
        assert!(records.written.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_signal_closes_the_build_channel_and_exits_130() {
        let records = FakeRecords::default();
        let (client, _called, gone) = build_engine(
            adapter(|suffix, _| (suffix == "worker::build").then_some(json!(true))),
            vec![Frame::KeepAlive],
        )
        .await;
        let (printer, _) = captured();
        let shutdown = signal_after(Duration::from_millis(500), 2);
        let failure = build_vm(&build_context(), &records, &client, &shutdown, printer)
            .await
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (BUILD_CANCELLED, CANCELLED_EXIT)
        );
        assert!(failure.message.contains("SIGINT"), "{failure}");
        tokio::time::timeout(Duration::from_secs(5), gone.notified())
            .await
            .expect("the build channel was left open");
        assert!(records.written.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn build_on_an_adapter_without_worker_build_builds_nothing() {
        let records = FakeRecords::default();
        let (client, mut called, _) = build_engine(
            adapter(|suffix, _| {
                (suffix == "adapter::info").then(|| {
                    answered(info(&[
                        FEATURE_SCOPED_AUTH,
                        FEATURE_LEASE,
                        FEATURE_LOGS_FROM_END,
                    ]))
                })
            }),
            Vec::new(),
        )
        .await;
        let (printer, _) = captured();
        let failure = build_vm(&build_context(), &records, &client, &never(), printer)
            .await
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (SANDBOX_BUILD_UNSUPPORTED, RUNNER_FAILED)
        );
        let calls = calls(&mut called);
        assert!(
            calls
                .iter()
                .all(|(function, _)| function == "adapter::info"),
            "nothing but adapter::info may be called: {calls:?}"
        );
        assert!(records.events.lock().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn build_is_refused_on_windows() {
        let args = BuildArgs { container: None };
        let failure = build_command(&compose_vars(), args, &FakeRecords::default())
            .await
            .unwrap_err();
        assert_eq!(failure.code, SANDBOX_UNSUPPORTED_PLATFORM);
    }

    /// What Compose hands `iii sandbox build` (spec 8.3) is what it reads, and none of it reaches
    /// the guest.
    #[test]
    #[ignore = "needs KAN-280: scripts.build and the III_BUILD_* variables in layer_own_vm_env"]
    fn compose_hands_the_build_everything_it_reads() {
        let dir = tempfile::tempdir().unwrap();
        crate::sandbox::tests::manifest(
            dir.path(),
            "api",
            "runtime: {base_image: python}\nscripts: {start: run}\n",
        );
        let file = crate::config::ComposeFile::parse(
            "containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  api:\n    worker: path://./api\n    start_after: [sb]\n    scripts: {build: iii sandbox build, build_timeout: 90s, run: iii sandbox run}\n",
            dir.path().join("worker-compose.yaml"),
        )
        .unwrap();
        let state_dir = Path::new("/state");
        let mut user_env = file.containers["api"].resolve_user_env("api").unwrap();
        sandbox::layer_own_vm_env(&file, "api", "project", state_dir, "orders", &mut user_env)
            .unwrap();
        let start = crate::manifest::StartSpec::Shell("iii sandbox build".into());
        let plan = crate::spawn::spawn_plan(&crate::spawn::SpawnCtx {
            engine_url: "ws://engine.test:1",
            namespace: "orders",
            compose_namespace: "compose",
            compose_file: &file.path,
            container_key: "api",
            start: &start,
            config_name: Some("api-config"),
            working_dir: dir.path(),
            user_env: &user_env,
        });

        let bctx = BuildContext::from_vars(&plan.env).unwrap();
        let build_dir = Path::new(&bctx.build_dir);
        assert!(
            build_dir.is_absolute() && build_dir.ends_with(".build/api"),
            "{}",
            bctx.build_dir
        );
        assert_eq!(
            bctx.compose_file,
            PathBuf::from(&plan.env["III_COMPOSE_FILE"])
        );
        assert_eq!(bctx.timeout, Duration::from_secs(90));
        let keys = &plan.env[WORKER_ENV_KEYS_ENV];
        for name in [BUILD_DIR_ENV, BUILD_COMPOSE_FILE_ENV, BUILD_TIMEOUT_ENV] {
            assert!(!keys.split(',').any(|key| key == name), "{name} in {keys}");
            assert!(
                !bctx.container.env.contains_key(name),
                "{name} reaches the guest"
            );
        }
    }

    /// The real `build_record`: a `path://` container's build records its declared directory and
    /// base image where `iii sandbox run` reads them.
    #[tokio::test]
    #[ignore = "needs KAN-281: build_record::{source, declared_base_image, dependency_files, write, read}"]
    async fn a_build_is_recorded_where_run_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        crate::sandbox::tests::manifest(
            dir.path(),
            "api",
            "runtime: {base_image: docker.io/iiidev/python}\nscripts: {start: run}\n",
        );
        std::fs::write(dir.path().join("api/requirements.txt"), "flask\n").unwrap();
        let compose_file = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_file,
            "containers:\n  sb: {worker: package://sandbox-libkrun, version: 1.0.0}\n  api: {worker: path://./api, start_after: [sb]}\n",
        )
        .unwrap();
        let build_dir = dir.path().join(".build/api");
        std::fs::create_dir_all(&build_dir).unwrap();
        let mut vars = compose_vars();
        let path = |path: &Path| path.to_str().unwrap().to_string();
        vars.insert(WORKER_DIR_ENV.into(), path(&dir.path().join("api")));
        vars.insert(BUILD_DIR_ENV.into(), path(&build_dir));
        vars.insert(BUILD_COMPOSE_FILE_ENV.into(), path(&compose_file));
        let bctx = BuildContext::from_vars(&vars).unwrap();
        let (client, _called, _) = build_engine(
            adapter(|suffix, _| (suffix == "worker::build").then(built)),
            Vec::new(),
        )
        .await;
        let (printer, _) = captured();
        let record = build_vm(&bctx, &BuildRecords, &client, &never(), printer)
            .await
            .unwrap();
        assert_eq!(
            record.source,
            BuildSource::Path {
                dir: "./api".into()
            }
        );
        let image = record.image.as_ref().unwrap();
        assert_eq!(
            image.base_image.declared.as_deref(),
            Some("docker.io/iiidev/python")
        );
        assert!(record.dependency_files.contains_key("requirements.txt"));
        assert_eq!(build_record::read(&build_dir).unwrap(), Some(record));
        let (_, boot) = boot_plan(&vars, None, &BuildRecords).unwrap();
        assert_eq!(boot.build_id, "sha256:7d41");
    }

    /// The real `build_record::read`: a build directory without `build.json` holds no build.
    #[test]
    #[ignore = "needs KAN-281: build_record::read"]
    fn a_build_dir_without_build_json_is_build_missing() {
        let dir = tempfile::tempdir().unwrap();
        let failure = built_image(dir.path(), "api", &BuildRecords).unwrap_err();
        assert_eq!(failure.code, BUILD_MISSING);
    }

    /// The real handlers, in a subprocess so the test runner never handles a signal: a SIGTERM
    /// right after they are installed is a stop request, and a second one cannot cut it short.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_signal_is_handled_from_the_moment_the_handlers_are_installed() {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader, Lines};
        use tokio::process::ChildStdout;

        async fn wait_for(lines: &mut Lines<BufReader<ChildStdout>>, wanted: &str) {
            let seen = tokio::time::timeout(Duration::from_secs(10), async {
                while let Some(line) = lines.next_line().await.unwrap() {
                    // Single-threaded, libtest prints the test's name before its output on
                    // the same line.
                    if line.ends_with(wanted) {
                        return;
                    }
                }
                panic!("the fixture exited before printing {wanted:?}");
            })
            .await;
            assert!(seen.is_ok(), "no {wanted:?} from the fixture");
        }

        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sandbox_cli::tests::signal_fixture",
                "--ignored",
                "--nocapture",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = Pid::from_raw(child.id().unwrap() as i32);
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        wait_for(&mut lines, "signals-ready").await;
        kill(pid, Signal::SIGTERM).unwrap();
        wait_for(&mut lines, "stop-requested 15").await;
        kill(pid, Signal::SIGTERM).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "a second SIGTERM ended the command during its stop"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "subprocess fixture for process-wide signal handling"]
    async fn signal_fixture() {
        let shutdown = Shutdown::signals().unwrap();
        println!("signals-ready");
        std::io::stdout().flush().unwrap();
        // Run on its own (`--include-ignored`), nothing signals it: it ends instead of hanging.
        let Ok((number, _)) =
            tokio::time::timeout(Duration::from_secs(10), shutdown.requested()).await
        else {
            return;
        };
        println!("stop-requested {number}");
        std::io::stdout().flush().unwrap();
        // Alive long enough for the test to see a second signal absorbed.
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    #[tokio::test]
    async fn run_boots_the_vm_prints_its_output_once_and_exits_with_its_code() {
        let (client, mut called) = fake_engine(adapter(|suffix, _| {
            (suffix == "worker::status").then(|| status(State::Exited, Some(3), None))
        }))
        .await;
        let (printer, printed) = captured();
        let status = run_vm(&context(), &booted(), &client, &never(), printer)
            .await
            .unwrap();
        assert_eq!(status.code(), Some(3));
        // The fake stream holds one stderr record at cursor 12; the stream reopened after the
        // exit asks from there, so the record is printed once.
        assert_eq!(*printed.lock().unwrap(), ["stderr: no /dev/kvm"]);

        let calls = calls(&mut called);
        assert!(
            payloads(&calls, "worker::prepare").is_empty(),
            "a build boots as it is: {calls:?}"
        );
        let start = payload(&calls, "worker::start");
        assert_eq!(
            start["build"],
            json!({"build_id": "sha256:7d41", "build_dir": "/work/.build/api"})
        );
        assert_eq!(start["state_key"], "9f2c1e");
        assert_eq!(start["auth"], "scoped");
        assert_eq!(start["worker_name"], "api");
        assert_eq!(
            start["env"],
            json!({"API_KEY": "k", "III_WORKER_NAME": "api"})
        );
        let logs = payloads(&calls, "worker::logs");
        assert!(logs[0].get("from_end").is_none() && logs[0]["cursor"].is_null());
        assert!(
            logs[1..].iter().all(|reopened| reopened["cursor"] == 12),
            "{logs:?}"
        );
    }

    #[tokio::test]
    async fn a_live_boot_is_reattached_to_without_replaying_or_losing_logs() {
        let statuses = AtomicUsize::new(0);
        let (client, mut called) = fake_engine(adapter(move |suffix, payload| match suffix {
            "worker::start" => Some(refused(SandboxErrorCode::S502, 4)),
            // The log's last record is 12 when the reattach asks from the end.
            "worker::logs" if payload["from_end"] == true => Some(answered(json!({"cursor": 12}))),
            "worker::status" if statuses.fetch_add(1, Ordering::SeqCst) >= 3 => {
                Some(status(State::Exited, Some(0), None))
            }
            _ => None,
        }))
        .await;
        let (printer, printed) = captured();
        let status = run_vm(&context(), &booted(), &client, &never(), printer)
            .await
            .unwrap();
        assert_eq!(status.code(), Some(0));
        assert!(
            printed.lock().unwrap().is_empty(),
            "the old record was replayed"
        );

        let calls = calls(&mut called);
        assert_eq!(
            payloads(&calls, "worker::start").len(),
            1,
            "a reattach boots nothing"
        );
        assert_eq!(payload(&calls, "worker::status")["instance"], 4);
        // The from_end stream closes empty while the VM runs; the reopened one resumes where it
        // started, not at a later end that would skip what the guest printed in between.
        let logs = payloads(&calls, "worker::logs");
        assert_eq!(logs[0]["from_end"], true);
        assert!(logs.len() >= 2, "{logs:?}");
        for reopened in &logs[1..] {
            assert!(reopened.get("from_end").is_none(), "{reopened}");
            assert_eq!(reopened["cursor"], 12);
        }
    }

    /// The exit code a run that ends on `signal` reports: 128 + signal, as a unix shell
    /// reports a child's; Windows has no signals, and `vm_exit` reads one as exit code 1.
    fn exit_code_on_signal(signal: i32) -> i32 {
        if cfg!(unix) { 128 + signal } else { 1 }
    }

    #[tokio::test]
    async fn a_signal_while_running_stops_the_vm_within_the_grace() {
        let (client, mut called) = fake_engine(adapter(|_, _| None)).await;
        let shutdown = signal_after(Duration::from_millis(300), 15);
        let (printer, _) = captured();
        let status = run_vm(&context(), &booted(), &client, &shutdown, printer)
            .await
            .unwrap();
        assert_eq!(exit_code(status), exit_code_on_signal(15));
        let grace = payload(&calls(&mut called), "worker::stop")["grace_ms"]
            .as_u64()
            .unwrap();
        assert!((7_800..=8_000).contains(&grace), "grace {grace}");
    }

    #[tokio::test]
    async fn a_signal_while_the_adapter_answers_boots_nothing() {
        let (sender, shutdown) = Shutdown::manual();
        let (client, mut called) = fake_engine(adapter(move |suffix, _| {
            (suffix == "adapter::info").then(|| {
                sender.send_replace(Some((15, Instant::now())));
                json!(true)
            })
        }))
        .await;
        let (printer, _) = captured();
        let begun = Instant::now();
        let status = run_vm(&context(), &booted(), &client, &shutdown, printer)
            .await
            .unwrap();
        assert_eq!(exit_code(status), exit_code_on_signal(15));
        assert!(begun.elapsed() < Duration::from_secs(2));
        let calls = calls(&mut called);
        assert!(payloads(&calls, "worker::start").is_empty(), "{calls:?}");
    }

    #[tokio::test]
    async fn a_signal_during_start_stops_the_instance_it_boots() {
        let (sender, shutdown) = Shutdown::manual();
        let (client, mut called) = fake_engine(adapter(move |suffix, _| {
            (suffix == "worker::start").then(|| {
                sender.send_replace(Some((15, Instant::now())));
                answered(json!({"vm_id": "v", "instance": 7}))
            })
        }))
        .await;
        let (printer, _) = captured();
        let status = run_vm(&context(), &booted(), &client, &shutdown, printer)
            .await
            .unwrap();
        assert_eq!(exit_code(status), exit_code_on_signal(15));
        assert_eq!(payload(&calls(&mut called), "worker::stop")["instance"], 7);
    }

    #[tokio::test]
    async fn an_unanswered_stop_ends_inside_the_stop_timeout() {
        let (client, _called) = fake_engine(adapter(|suffix, _| {
            (suffix == "worker::stop").then_some(json!(true))
        }))
        .await;
        let shutdown = signal_after(Duration::from_millis(200), 15);
        let (printer, _) = captured();
        let ctx = context_stopping_within("2000");
        let status = run_vm(&ctx, &booted(), &client, &shutdown, printer)
            .await
            .unwrap();
        let (_, signalled_at) = shutdown.requested().await;
        let took = signalled_at.elapsed();
        // The deadline is at 1800 ms: 200 ms of slack for the return path on a loaded runner.
        assert!(took < Duration::from_millis(2000), "the stop took {took:?}");
        assert!(
            took >= Duration::from_millis(1750),
            "gave up early: {took:?}"
        );
        assert_eq!(exit_code(status), exit_code_on_signal(9));
    }

    #[tokio::test]
    async fn a_lost_adapter_ends_the_run() {
        let (client, _called) = fake_engine(adapter(|suffix, _| {
            (suffix == "worker::status")
                .then(|| json!({"error": {"code": "function_not_found", "message": "gone"}}))
        }))
        .await;
        let (printer, _) = captured();
        let begun = Instant::now();
        let error = run_vm(&context(), &booted(), &client, &never(), printer)
            .await
            .unwrap_err();
        assert!(error.message.contains("has not answered"), "{error}");
        assert!(begun.elapsed() < LOST_AFTER + Duration::from_secs(2));
    }

    #[tokio::test]
    async fn an_adapter_without_scoped_tokens_never_boots_anything() {
        let (client, mut called) = fake_engine(adapter(|suffix, _| {
            (suffix == "adapter::info").then(|| answered(info(&[])))
        }))
        .await;
        let (printer, _) = captured();
        let error = run_vm(&context(), &booted(), &client, &never(), printer)
            .await
            .unwrap_err();
        assert!(error.message.contains("does not support"), "{error}");
        assert!(
            calls(&mut called).iter().all(|(f, _)| f == "adapter::info"),
            "nothing but adapter::info may be called"
        );
    }

    #[tokio::test]
    async fn run_on_an_adapter_without_worker_build_boots_nothing() {
        let (client, mut called) = fake_engine(adapter(|suffix, _| {
            (suffix == "adapter::info").then(|| {
                answered(info(&[
                    FEATURE_SCOPED_AUTH,
                    FEATURE_LEASE,
                    FEATURE_LOGS_FROM_END,
                ]))
            })
        }))
        .await;
        let (printer, _) = captured();
        let failure = run_vm(&context(), &booted(), &client, &never(), printer)
            .await
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit),
            (SANDBOX_BUILD_UNSUPPORTED, RUNNER_FAILED)
        );
        assert!(
            calls(&mut called).iter().all(|(f, _)| f == "adapter::info"),
            "nothing but adapter::info may be called"
        );
    }

    /// The adapter refuses a `build_id` that is not the image's: its code, and the way out.
    #[tokio::test]
    async fn a_build_mismatch_is_reported_with_the_rebuild() {
        let (client, _called) = fake_engine(adapter(|suffix, _| {
            (suffix == "worker::start").then(|| refused(SandboxErrorCode::S508, 1))
        }))
        .await;
        let (printer, _) = captured();
        let failure = run_vm(&context(), &booted(), &client, &never(), printer)
            .await
            .unwrap_err();
        assert_eq!(failure.code, "S508");
        assert!(
            failure.message.starts_with("worker::start: refused")
                && failure
                    .message
                    .ends_with("rebuild it with `iii sandbox build api`"),
            "{failure}"
        );
    }
}
