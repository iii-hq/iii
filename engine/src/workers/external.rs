// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! External worker support: spawns installed binaries from `iii_workers/` as child processes.
//!
//! When the engine encounters a worker class that isn't registered in the built-in
//! registry, it checks `iii.toml` for installed workers and spawns the corresponding
//! binary, passing its config via a temporary YAML file.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Deserialize;

use serde_json::Value;
use tokio::{
    process::Child,
    sync::Mutex,
    time::{Duration, timeout},
};

use crate::{engine::Engine, workers::traits::Worker};

/// Resolves an external worker class to a binary path.
///
/// Convention: `workers::image_resize::ImageResizeModule`
///   -> extract middle segment `image_resize`
///   -> convert underscores to hyphens: `image-resize`
///   -> binary at `iii_workers/image-resize`
///
/// Also checks `iii.toml` to verify the worker is actually installed.

#[derive(Deserialize)]
struct ManifestFile {
    workers: Option<BTreeMap<String, String>>,
}

/// A built-in external worker: a worker name that resolves to a binary
/// on $PATH plus extra args, instead of the conventional
/// `iii_workers/<name>` lookup. Used for daemons shipped as subcommands
/// of other iii binaries.
struct KnownExternal {
    /// Worker class slug as it appears in config.yaml (e.g. "iii-sandbox").
    name: &'static str,
    /// Binary to resolve via $PATH (fallback: ~/.local/bin/<binary>).
    binary: &'static str,
    /// Extra args prepended to the child's argv, before `--config <path>`.
    args: &'static [&'static str],
}

/// Workers shipped as subcommands of a single binary on $PATH, not as
/// standalone binaries in iii_workers/. Resolved in
/// `resolve_external_module_in` before the iii.toml lookup.
const KNOWN_EXTERNAL: &[KnownExternal] = &[
    KnownExternal {
        name: "iii-sandbox",
        binary: "iii-worker",
        args: &["sandbox-daemon"],
    },
    // Slug is "iii-worker-ops" to avoid clashing with the built-in
    // `WorkerManager` already registered as "iii-worker-manager".
    KnownExternal {
        name: "iii-worker-ops",
        binary: "iii-worker",
        args: &["worker-manager-daemon"],
    },
];

pub fn resolve_external_module(class: &str) -> Option<ExternalWorkerInfo> {
    let base_dir = std::env::current_dir().ok()?;
    resolve_external_module_in(&base_dir, class)
}

pub fn resolve_external_module_in(base_dir: &Path, class: &str) -> Option<ExternalWorkerInfo> {
    // Normalize the class to a binary-name candidate for KNOWN_EXTERNAL
    // matching. The real caller (`WorkerRegistry::create_worker`) passes
    // the bare `name` from config.yaml — e.g. "iii-sandbox". Tests and
    // the legacy iii.toml path use the `workers::<slug>` form. Handle
    // both by taking the last `::` segment (or the whole string if no
    // separator) and normalizing `_` -> `-`.
    let binary_name_candidate = class.rsplit("::").next().unwrap_or(class).replace('_', "-");

    // Check well-known externals first. These are shipped as
    // subcommands of iii-binaries on $PATH, so we skip the
    // iii_workers/<name> directory convention entirely.
    if let Some(hit) = KNOWN_EXTERNAL
        .iter()
        .find(|k| k.name == binary_name_candidate)
    {
        let binary_path = which::which(hit.binary)
            .or_else(|_| {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".local/bin").join(hit.binary))
                    .map_err(|_| which::Error::CannotFindBinaryPath)
                    .and_then(|p| {
                        if p.exists() {
                            Ok(p)
                        } else {
                            Err(which::Error::CannotFindBinaryPath)
                        }
                    })
            })
            .ok()?;

        return Some(ExternalWorkerInfo {
            name: binary_name_candidate,
            binary_path,
            extra_args: hit.args.iter().map(|s| (*s).to_string()).collect(),
            env: Vec::new(),
        });
    }

    // iii.toml lookup uses the `workers::<slug>` class-path form.
    let parts: Vec<&str> = class.split("::").collect();
    if parts.len() < 2 {
        return None;
    }

    let slug = parts.get(1)?;
    let binary_name = slug.replace('_', "-");

    // Parse iii.toml and check for exact worker key
    let manifest_path = base_dir.join("iii.toml");
    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path).ok()?;
        let parsed: ManifestFile = toml::from_str(&content).ok()?;
        let workers = parsed.workers.unwrap_or_default();
        if !workers.contains_key(&binary_name) {
            return None;
        }
    } else {
        return None;
    }

    let file_name = if cfg!(target_os = "windows") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.clone()
    };
    let binary_path = base_dir.join("iii_workers").join(&file_name);
    if !binary_path.exists() {
        tracing::warn!(
            "Worker '{}' is in iii.toml but binary not found at {}",
            binary_name,
            binary_path.display()
        );
        return None;
    }

    Some(ExternalWorkerInfo {
        name: binary_name,
        binary_path,
        extra_args: vec![],
        env: Vec::new(),
    })
}

pub struct ExternalWorkerInfo {
    pub name: String,
    pub binary_path: PathBuf,
    /// Args prepended to the child's argv, before `--config <path>`.
    /// Empty for conventional iii_workers/<binary> resolution; populated
    /// when the worker was matched via KNOWN_EXTERNAL.
    pub extra_args: Vec<String>,
    /// Extra environment variables set on the child (e.g. `III_CONFIG_PATH`
    /// so daemons that edit the project config file target the engine's
    /// actual file, not a hardcoded ./config.yaml).
    pub env: Vec<(String, String)>,
}

/// A worker implementation backed by an external binary from `iii_workers/`.
///
/// The binary is spawned as a child process during `start_background_tasks`
/// and killed during `destroy`. A supervisor task respawns it (with backoff)
/// if it exits unexpectedly — a dead worker-manager daemon must not take the
/// `worker::*` surface down until an engine restart (MOT-3857). The worker
/// config is serialized to a temporary YAML file and passed via
/// `--config <path>`.
#[derive(Clone)]
pub struct ExternalWorker {
    display_name: &'static str,
    name: String,
    binary_path: PathBuf,
    extra_args: Vec<String>,
    /// Effective `iii-worker-manager` port, from `engine.worker_manager_port()`.
    /// Exported to the child as III_ENGINE_URL/III_URL so builtin daemons
    /// (sandbox-daemon, worker-manager-daemon) connect back to THIS engine
    /// instead of their hardcoded ws://127.0.0.1:49134 default (MOT-3970).
    worker_manager_port: u16,
    env: Vec<(String, String)>,
    config: Option<Value>,
    child: Arc<Mutex<Option<Child>>>,
    config_file: Arc<Mutex<Option<PathBuf>>>,
    /// Write end of the child's lifeline pipe (see the spawn path). Held for
    /// the child's lifetime; the kernel closes it when THIS engine dies —
    /// any death, SIGKILL included — and the daemon's lifeline watch sees
    /// EOF instantly and self-exits. Dropped explicitly on shutdown/destroy
    /// so graceful teardown signals the child the same way.
    #[cfg(unix)]
    lifeline: Arc<Mutex<Option<std::os::fd::OwnedFd>>>,
    /// Terminal state: set by `destroy()` before it kills the child. The
    /// supervisor re-checks it under the child lock before every spawn so a
    /// destroy with no prior shutdown signal can never race a respawn into
    /// resurrecting a destroyed worker.
    stopped: Arc<AtomicBool>,
}

/// How often the supervisor polls the spawned child for unexpected exit.
const SUPERVISOR_POLL: Duration = Duration::from_millis(500);
/// Backoff between respawn attempts (exponential, capped).
const RESPAWN_BACKOFF_MIN: Duration = Duration::from_secs(1);
const RESPAWN_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// A child that stayed up this long resets the respawn backoff.
const BACKOFF_RESET_UPTIME: Duration = Duration::from_secs(60);

impl ExternalWorker {
    pub fn new(info: ExternalWorkerInfo, config: Option<Value>, worker_manager_port: u16) -> Self {
        let name = info.name.clone();
        let display_name = Box::leak(format!("ExternalWorker({})", &name).into_boxed_str());
        Self {
            display_name,
            name,
            binary_path: info.binary_path,
            extra_args: info.extra_args,
            worker_manager_port,
            env: info.env,
            config,
            child: Arc::new(Mutex::new(None)),
            config_file: Arc::new(Mutex::new(None)),
            #[cfg(unix)]
            lifeline: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Build the `Command` fresh — including a NEW lifeline pipe and
    /// `pre_exec` closure (a reused closure would capture a dead lifeline fd
    /// number) — spawn the child, wire the stdio forwarders, and store the
    /// child into `slot`.
    ///
    /// `slot` must be the contents of the `self.child` lock, held by the
    /// caller across this call: spawning under the lock means a concurrent
    /// `destroy()`/`kill_child` either blocks until the fresh child is
    /// stored (and then kills it) or completes first — it can never miss
    /// the new child.
    async fn spawn_child_into(
        &self,
        slot: &mut Option<Child>,
        config_path: Option<&Path>,
    ) -> anyhow::Result<()> {
        let mut cmd = tokio::process::Command::new(&self.binary_path);
        for arg in &self.extra_args {
            cmd.arg(arg);
        }
        if let Some(path) = config_path {
            cmd.arg("--config").arg(path);
        }
        for (key, value) in &self.env {
            cmd.env(key, value);
        }
        // Pipe stdio instead of inheriting the engine's TTY fds. External
        // workers (notably iii-sandbox) load libkrun, which raws the host
        // terminal when attaching the guest serial console — termios is per
        // tty, so any tcsetattr by the child mutates the engine's terminal
        // and scrambles tracing output until the VM exits. Piping isolates
        // libkrun's tcsetattr to non-tty fds (calls become no-ops with
        // ENOTTY) and forwarders below copy bytes line-atomic to the
        // engine's stdout/stderr. stdin is /dev/null because workers don't
        // read host stdin during normal operation.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Declare this engine's pid so daemons can watch ENGINE liveness
        // directly and self-exit when we die without running kill_child
        // (SIGKILL/OOM/crash). getppid() alone can't prove the parent is the
        // engine — wrappers and debugger reparenting break it — so iii-worker's
        // daemon_exit module prefers this handshake when present.
        cmd.env("III_ENGINE_PID", std::process::id().to_string());

        // Engine WS URL for the child. Builtin daemons' --engine args fall
        // back to this env (clap `env =`), so they connect to THIS engine's
        // actual worker-listener port rather than their hardcoded default —
        // two engines on one host each get their own daemon (MOT-3970).
        // Re-exported on every respawn (MOT-3857 supervisor) so a respawned
        // daemon keeps the same contract.
        let engine_url = format!("ws://127.0.0.1:{}", self.worker_manager_port);
        cmd.env("III_ENGINE_URL", &engine_url);
        cmd.env("III_URL", &engine_url);

        // Lifeline pipe: we hold the write end (never written) for the
        // child's lifetime; the kernel closes it the instant this engine
        // dies — ANY death, SIGKILL included — and the daemon's lifeline
        // watch sees EOF immediately. Env name + protocol live in
        // iii-worker's daemon_exit module (III_LIFELINE_FD); keep in sync.
        // Storing the new write end drops the previous child's — harmless,
        // that child is already dead by the time we respawn.
        #[cfg(unix)]
        let lifeline_read = match new_cloexec_pipe() {
            Ok((read, write)) => {
                use std::os::fd::AsRawFd;
                cmd.env("III_LIFELINE_FD", read.as_raw_fd().to_string());
                *self.lifeline.lock().await = Some(write);
                Some(read)
            }
            Err(e) => {
                // Non-fatal: the daemon's PID handshake still covers engine
                // death, just with poll latency instead of instant EOF.
                tracing::warn!("lifeline pipe for '{}' failed: {e}", self.name);
                None
            }
        };

        // Detach process group on Unix for clean termination
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let lifeline_raw = lifeline_read.as_ref().map(|fd| fd.as_raw_fd());
            // Captured pre-fork: in the child, "my parent is still the
            // engine" must compare against the ENGINE's pid, not against 1 —
            // in the standard container deployment the engine IS PID 1
            // (engine/Dockerfile has no init shim), so a `getppid()==1 →
            // exit` check would deterministically kill every worker spawn.
            let engine_pid = std::process::id() as i32;
            // SAFETY: everything in this hook is async-signal-safe per POSIX
            // (setsid, fcntl, prctl, getppid, _exit) and runs in the
            // forked-but-not-yet-execed child. setsid() gives the child its
            // own session so kill_child() can killpg() the whole group.
            unsafe {
                cmd.pre_exec(move || {
                    nix::unistd::setsid()
                        .map_err(|e| std::io::Error::other(format!("setsid failed: {e}")))?;
                    // Un-CLOEXEC the lifeline read end for THIS child only
                    // (the parent-side fds stay CLOEXEC so no other spawn
                    // inherits them).
                    if let Some(fd) = lifeline_raw {
                        let flags = libc::fcntl(fd, libc::F_GETFD);
                        if flags < 0
                            || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                        {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    // Linux belt-and-suspenders: kernel-delivered SIGKILL on
                    // parent death, covering even a wedged child that never
                    // polls its watches. Tied to the spawning THREAD — a
                    // tokio core worker thread, which lives as long as the
                    // runtime ≈ the engine process. If the engine died
                    // between fork and prctl, we were already reparented
                    // (getppid no longer the engine); exit now instead of
                    // leaking unprotected.
                    #[cfg(target_os = "linux")]
                    {
                        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                        if libc::getppid() != engine_pid {
                            libc::_exit(125);
                        }
                    }
                    #[cfg(not(target_os = "linux"))]
                    let _ = engine_pid;
                    Ok(())
                });
            }
        }

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn external worker '{}' ({}): {}",
                self.name,
                self.binary_path.display(),
                e
            )
        })?;

        // Forward child stdout/stderr to engine stdout/stderr line-atomically.
        // BufReader::read_until('\n') hands us complete lines (or trailing EOF
        // bytes). Each forwarded line is one Stdout::write_all / Stderr::write_all
        // call, which holds the global StdoutLock/StderrLock for the duration —
        // so worker lines never interleave mid-line with engine tracing output.
        // The forwarder tasks self-terminate on pipe EOF when the child dies,
        // so respawns don't leak them.
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(forward_pipe(stdout, false));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(forward_pipe(stderr, true));
        }

        tracing::info!(
            "Spawned external worker '{}' (pid: {:?})",
            self.name,
            child.id()
        );
        *slot = Some(child);

        Ok(())
    }
}

/// Pipe with both ends CLOEXEC. macOS has no `pipe2`, so there the CLOEXEC
/// fcntls leave a tiny window where a concurrent spawn on another thread can
/// inherit the fds; the daemon's PID-watch backstop covers that case (keep
/// in sync with iii-worker's `daemon_exit::new_cloexec_pipe`).
#[cfg(unix)]
fn new_cloexec_pipe() -> std::io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0i32; 2];
    #[cfg(target_os = "linux")]
    {
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        for fd in fds {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
            {
                let err = std::io::Error::last_os_error();
                unsafe {
                    libc::close(fds[0]);
                    libc::close(fds[1]);
                }
                return Err(err);
            }
        }
    }
    // SAFETY: fresh fds from pipe(2), owned exclusively here.
    Ok(unsafe {
        (
            std::os::fd::OwnedFd::from_raw_fd(fds[0]),
            std::os::fd::OwnedFd::from_raw_fd(fds[1]),
        )
    })
}

#[async_trait::async_trait]
impl Worker for ExternalWorker {
    fn name(&self) -> &'static str {
        self.display_name
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        Err(anyhow::anyhow!(
            "ExternalWorker::create should not be called directly"
        ))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!(
            "External worker '{}' initialized (binary: {})",
            self.name,
            self.binary_path.display()
        );
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let config_path = if let Some(ref config) = self.config {
            let path = crate::workers::secure_temp::write_engine_config_temp(&self.name, config)
                .map_err(|e| anyhow::anyhow!(e))?;
            tracing::debug!("Wrote external worker config to {}", path.display());
            *self.config_file.lock().await = Some(path.clone());
            Some(path)
        } else {
            None
        };

        // Wait for the engine to finish binding its listener, but bail early
        // if shutdown fires during the delay.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = shutdown_rx.changed() => {
                tracing::info!(
                    "External worker '{}' received shutdown before spawn",
                    self.name
                );
                return Ok(());
            }
        }

        // First spawn is synchronous so a missing/unspawnable binary still
        // hard-fails ReloadManager::start_worker instead of warning from a
        // background task.
        {
            let mut slot = self.child.lock().await;
            if self.stopped.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.spawn_child_into(&mut slot, config_path.as_deref())
                .await?;
        }

        // Supervisor: kill the child on shutdown, respawn it on unexpected
        // exit. Without the respawn, a dead worker-manager daemon takes the
        // whole worker::* surface down until a full engine restart
        // (MOT-3857).
        let worker = self.clone();
        tokio::spawn(async move {
            let mut backoff = RESPAWN_BACKOFF_MIN;
            let mut spawned_at = tokio::time::Instant::now();
            loop {
                tokio::select! {
                    // Ok or Err (sender dropped) — both mean shutdown.
                    _ = shutdown_rx.changed() => {
                        tracing::info!(
                            "External worker '{}' received shutdown signal",
                            worker.name
                        );
                        // SIGTERM first, lifeline-drop after: the daemon races
                        // its exit arms, and an already-pending EOF beats the
                        // signal — which would make every GRACEFUL shutdown
                        // take the "engine-gone" path and write the
                        // abnormal-death breadcrumb. Dropping after kill_child
                        // keeps EOF as a true engine-death signal (and is a
                        // no-op for the already-dead child).
                        kill_child(&worker.child).await;
                        #[cfg(unix)]
                        drop(worker.lifeline.lock().await.take());
                        return;
                    }
                    _ = tokio::time::sleep(SUPERVISOR_POLL) => {}
                }

                let status = {
                    let mut slot = worker.child.lock().await;
                    match slot.as_mut().map(|c| c.try_wait()) {
                        // Slot emptied by destroy()/kill_child — someone else
                        // owns the lifecycle now; stop supervising.
                        None => return,
                        // Still running.
                        Some(Ok(None)) => continue,
                        // Reap the corpse NOW: kill_child must never killpg a
                        // recycled pid, and the slot must be free to respawn
                        // into.
                        Some(Ok(Some(status))) => {
                            slot.take();
                            Some(status)
                        }
                        Some(Err(err)) => {
                            tracing::warn!(
                                "try_wait for external worker '{}' failed: {err}; treating as dead",
                                worker.name
                            );
                            slot.take();
                            None
                        }
                    }
                };

                if spawned_at.elapsed() >= BACKOFF_RESET_UPTIME {
                    backoff = RESPAWN_BACKOFF_MIN;
                }
                tracing::warn!(
                    "External worker '{}' exited unexpectedly ({}); respawning in {:?}",
                    worker.name,
                    status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "unknown status".to_string()),
                    backoff
                );

                // Respawn with backoff. Inner loop on purpose: the slot is
                // empty by OUR doing here, and the outer loop reads an empty
                // slot as "destroy ran".
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            #[cfg(unix)]
                            drop(worker.lifeline.lock().await.take());
                            return;
                        }
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(RESPAWN_BACKOFF_MAX);
                    // borrow(), not changed(): the signal may already have
                    // been consumed by an earlier select arm.
                    if *shutdown_rx.borrow() {
                        return;
                    }
                    let mut slot = worker.child.lock().await;
                    // Terminal-state check under the child lock: a destroy()
                    // with no prior shutdown signal (e.g. final engine
                    // teardown of reload-added workers) that lands during our
                    // backoff must not be raced by a respawn that resurrects
                    // the worker.
                    if worker.stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    match worker
                        .spawn_child_into(&mut slot, config_path.as_deref())
                        .await
                    {
                        Ok(()) => {
                            spawned_at = tokio::time::Instant::now();
                            break;
                        }
                        Err(err) => {
                            tracing::warn!("{err}; backing off");
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying external worker '{}'", self.name);
        // Terminal state BEFORE killing: the supervisor re-checks this under
        // the child lock, so once we flip it no respawn can resurrect the
        // worker even if destroy lands mid-backoff with no shutdown signal.
        self.stopped.store(true, Ordering::SeqCst);
        kill_child(&self.child).await;
        // After kill_child for breadcrumb accuracy — see the shutdown task.
        #[cfg(unix)]
        drop(self.lifeline.lock().await.take());

        if let Some(path) = self.config_file.lock().await.take()
            && let Err(e) = std::fs::remove_file(&path)
        {
            tracing::warn!("failed to remove temp config {}: {}", path.display(), e);
        }

        Ok(())
    }
}

async fn kill_child(child: &Arc<Mutex<Option<Child>>>) {
    if let Some(mut proc) = child.lock().await.take() {
        #[cfg(unix)]
        {
            if let Some(id) = proc.id() {
                let pgid = nix::unistd::Pid::from_raw(id as i32);
                let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM);
            }
        }

        #[cfg(not(unix))]
        {
            let _ = proc.kill().await;
        }

        // Wait briefly for graceful shutdown, then force kill
        let exited = timeout(Duration::from_secs(3), proc.wait()).await;
        if exited.is_err() {
            #[cfg(unix)]
            if let Some(id) = proc.id() {
                let pgid = nix::unistd::Pid::from_raw(id as i32);
                let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
            }
            #[cfg(not(unix))]
            {
                let _ = proc.kill().await;
            }
            let _ = proc.wait().await;
        }
    }
}

/// Copy a child process pipe to the engine's stdout/stderr line-atomically.
///
/// `read_until(b'\n', ...)` returns each complete line (with the trailing
/// newline) plus any unterminated tail at EOF. We then issue one
/// `write_all` call to the engine's stdout/stderr lock, which holds the
/// process-global `StdoutLock`/`StderrLock` for the full write — so a
/// worker log line never lands in the middle of an engine tracing event
/// and the terminal sees coherent line boundaries.
///
/// The task ends when the child closes its end of the pipe (Ok(0)) or
/// on any read error.
async fn forward_pipe<R>(reader: R, to_stderr: bool)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use std::io::Write;
    use tokio::io::AsyncBufReadExt;

    let mut buf = tokio::io::BufReader::new(reader);
    let mut line: Vec<u8> = Vec::with_capacity(512);
    loop {
        line.clear();
        match buf.read_until(b'\n', &mut line).await {
            Ok(0) => return,
            Ok(_) => {
                if to_stderr {
                    let _ = std::io::stderr().lock().write_all(&line);
                } else {
                    let _ = std::io::stdout().lock().write_all(&line);
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a temp dir with an `iii.toml` and optional binaries under `iii_workers/`.
    fn setup_manifest(workers: &[(&str, &str)], binaries: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();

        let mut toml = String::from("[workers]\n");
        for (name, version) in workers {
            toml.push_str(&format!("{} = \"{}\"\n", name, version));
        }
        std::fs::write(dir.path().join("iii.toml"), &toml).unwrap();

        let workers_dir = dir.path().join("iii_workers");
        std::fs::create_dir_all(&workers_dir).unwrap();
        for bin in binaries {
            std::fs::write(workers_dir.join(bin), b"fake-binary").unwrap();
        }

        dir
    }

    #[test]
    fn resolve_external_module_returns_none_for_builtin_name() {
        assert!(resolve_external_module("iii-stream").is_none());
    }

    #[test]
    fn resolve_external_module_returns_none_for_short_class() {
        assert!(resolve_external_module("SomeModule").is_none());
    }

    #[test]
    fn resolve_happy_path_three_segment_class() {
        let dir = setup_manifest(&[("image-resize", "1.0.0")], &["image-resize"]);

        let result =
            resolve_external_module_in(dir.path(), "workers::image_resize::ImageResizeModule");
        let info = result.expect("should resolve valid external module");
        assert_eq!(info.name, "image-resize");
        assert_eq!(
            info.binary_path,
            dir.path().join("iii_workers/image-resize")
        );
    }

    #[test]
    fn resolve_happy_path_two_segment_class() {
        let dir = setup_manifest(&[("my-worker", "2.0.0")], &["my-worker"]);

        let result = resolve_external_module_in(dir.path(), "workers::my_worker");
        let info = result.expect("two-segment class should resolve");
        assert_eq!(info.name, "my-worker");
    }

    #[test]
    fn resolve_underscore_to_hyphen_conversion() {
        let dir = setup_manifest(&[("data-transform", "0.1.0")], &["data-transform"]);

        let result =
            resolve_external_module_in(dir.path(), "workers::data_transform::DataTransformModule");
        let info = result.expect("underscores should convert to hyphens");
        assert_eq!(info.name, "data-transform");
    }

    #[test]
    fn resolve_selects_correct_worker_among_multiple() {
        let dir = setup_manifest(
            &[("alpha", "1.0.0"), ("beta", "2.0.0"), ("gamma", "3.0.0")],
            &["alpha", "beta", "gamma"],
        );

        let result = resolve_external_module_in(dir.path(), "workers::beta::BetaModule");
        let info = result.expect("should resolve 'beta' among multiple workers");
        assert_eq!(info.name, "beta");
    }

    #[test]
    fn resolve_slug_with_no_underscores_passes_through() {
        let dir = setup_manifest(&[("simple", "1.0.0")], &["simple"]);

        let result = resolve_external_module_in(dir.path(), "workers::simple::SimpleModule");
        let info = result.expect("slug without underscores should pass through unchanged");
        assert_eq!(info.name, "simple");
    }

    #[test]
    fn resolve_returns_none_for_empty_class() {
        let dir = setup_manifest(&[("x", "1.0.0")], &["x"]);
        assert!(resolve_external_module_in(dir.path(), "").is_none());
    }

    #[test]
    fn resolve_returns_none_for_single_segment_class() {
        let dir = setup_manifest(&[("x", "1.0.0")], &["x"]);
        assert!(resolve_external_module_in(dir.path(), "OnlyOneSegment").is_none());
    }

    #[test]
    fn resolve_returns_none_when_no_iii_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        // No iii.toml created
        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "should return None when iii.toml does not exist"
        );
    }

    #[test]
    fn resolve_returns_none_when_worker_not_in_manifest() {
        let dir = setup_manifest(&[("other-worker", "1.0.0")], &["other-worker"]);

        assert!(
            resolve_external_module_in(dir.path(), "workers::missing::MissingModule").is_none(),
            "should return None when worker key is not in iii.toml"
        );
    }

    #[test]
    fn resolve_returns_none_when_binary_missing_on_disk() {
        // Worker is in iii.toml but binary file doesn't exist
        let dir = setup_manifest(&[("ghost", "1.0.0")], &[]); // no binaries created

        assert!(
            resolve_external_module_in(dir.path(), "workers::ghost::GhostModule").is_none(),
            "should return None when binary is not on disk"
        );
    }

    #[test]
    fn resolve_returns_none_for_empty_workers_section() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.toml"), "[workers]\n").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "empty workers section should not match anything"
        );
    }

    #[test]
    fn resolve_returns_none_when_toml_has_no_workers_key() {
        let dir = tempfile::TempDir::new().unwrap();
        // Valid TOML but no [workers] section
        std::fs::write(dir.path().join("iii.toml"), "[package]\nname = \"test\"\n").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "should return None when iii.toml has no [workers] section"
        );
    }

    #[test]
    fn resolve_no_false_positive_on_substring() {
        let dir = setup_manifest(&[("image-resize", "1.0.0")], &["image-resize"]);

        // "workers::image::ImageModule" extracts slug "image" which should NOT match "image-resize"
        let result = resolve_external_module_in(dir.path(), "workers::image::ImageModule");
        assert!(
            result.is_none(),
            "Should not match 'image' when only 'image-resize' is installed"
        );
    }

    #[test]
    fn resolve_returns_none_for_malformed_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.toml"), "this is not valid toml {{{{").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "should return None for unparseable iii.toml"
        );
    }

    #[test]
    fn external_module_new_without_config() {
        let info = ExternalWorkerInfo {
            name: "test-worker".to_string(),
            binary_path: PathBuf::from("/tmp/test-worker"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);
        assert_eq!(module.name, "test-worker");
        assert_eq!(module.binary_path, PathBuf::from("/tmp/test-worker"));
        assert!(module.config.is_none());
    }

    #[test]
    fn external_module_new_with_config() {
        let config = serde_json::json!({"port": 8080, "debug": true});
        let info = ExternalWorkerInfo {
            name: "configured-worker".to_string(),
            binary_path: PathBuf::from("/tmp/configured-worker"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(
            info,
            Some(config.clone()),
            crate::workers::worker::DEFAULT_PORT,
        );
        assert_eq!(module.config, Some(config));
    }

    #[test]
    fn external_module_display_name_format() {
        let info = ExternalWorkerInfo {
            name: "my-worker".to_string(),
            binary_path: PathBuf::from("/tmp/my-worker"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);
        assert_eq!(module.name(), "ExternalWorker(my-worker)");
    }

    #[test]
    fn external_module_name_returns_consistent_pointer() {
        let info = ExternalWorkerInfo {
            name: "test-worker".to_string(),
            binary_path: PathBuf::from("/tmp/test-worker"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);
        let name1 = module.name();
        let name2 = module.name();
        assert_eq!(
            name1 as *const str, name2 as *const str,
            "name() should return the same pointer on repeated calls"
        );
    }

    #[test]
    fn external_module_clone_shares_child_and_config_file() {
        let info = ExternalWorkerInfo {
            name: "clone-test".to_string(),
            binary_path: PathBuf::from("/tmp/clone-test"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);
        let cloned = module.clone();

        // Arc pointers should be the same (shared state)
        assert!(Arc::ptr_eq(&module.child, &cloned.child));
        assert!(Arc::ptr_eq(&module.config_file, &cloned.config_file));
        assert_eq!(module.name, cloned.name);
    }

    #[tokio::test]
    async fn external_module_initialize_succeeds() {
        let info = ExternalWorkerInfo {
            name: "init-test".to_string(),
            binary_path: PathBuf::from("/tmp/init-test"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);
        assert!(module.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn external_module_destroy_succeeds_with_no_child() {
        let info = ExternalWorkerInfo {
            name: "destroy-test".to_string(),
            binary_path: PathBuf::from("/tmp/destroy-test"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);
        // destroy on a fresh module (no spawned child) should succeed
        assert!(module.destroy().await.is_ok());
    }

    #[tokio::test]
    async fn external_module_destroy_cleans_up_config_file() {
        let info = ExternalWorkerInfo {
            name: "cleanup-test".to_string(),
            binary_path: PathBuf::from("/tmp/cleanup-test"),
            extra_args: vec![],
            env: Vec::new(),
        };
        let module = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);

        // Simulate a config file being written
        let temp_config = std::env::temp_dir().join("iii-cleanup-test-config.yaml");
        std::fs::write(&temp_config, "test: true").unwrap();
        *module.config_file.lock().await = Some(temp_config.clone());

        assert!(
            temp_config.exists(),
            "config file should exist before destroy"
        );
        module.destroy().await.unwrap();
        assert!(
            !temp_config.exists(),
            "config file should be cleaned up after destroy"
        );
    }

    #[tokio::test]
    async fn kill_child_noop_when_no_child() {
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        // Should not panic or error
        kill_child(&child_handle).await;
        assert!(child_handle.lock().await.is_none());
    }

    #[test]
    fn external_worker_info_default_extra_args_is_empty() {
        let info = ExternalWorkerInfo {
            name: "unit-test".into(),
            binary_path: PathBuf::from("/tmp/fake"),
            extra_args: vec![],
            env: Vec::new(),
        };
        assert!(info.extra_args.is_empty());
    }

    #[test]
    fn known_external_iii_sandbox_dispatches_to_iii_worker() {
        let hit = KNOWN_EXTERNAL
            .iter()
            .find(|k| k.name == "iii-sandbox")
            .expect("iii-sandbox must be in KNOWN_EXTERNAL");
        assert_eq!(hit.binary, "iii-worker");
        assert_eq!(hit.args, &["sandbox-daemon"]);
    }

    #[test]
    fn known_external_iii_worker_ops_dispatches_to_iii_worker() {
        let hit = KNOWN_EXTERNAL
            .iter()
            .find(|k| k.name == "iii-worker-ops")
            .expect("iii-worker-ops must be in KNOWN_EXTERNAL");
        assert_eq!(hit.binary, "iii-worker");
        assert_eq!(hit.args, &["worker-manager-daemon"]);
    }

    #[test]
    #[serial_test::serial]
    fn resolves_iii_worker_ops_to_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("iii-worker");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var_os("PATH");
        // SAFETY: test is marked #[serial] so no other thread mutates env concurrently.
        unsafe {
            std::env::set_var("PATH", dir.path());
        }

        let result = resolve_external_module_in(dir.path(), "iii-worker-ops");

        // SAFETY: test is marked #[serial]; restoring original.
        unsafe {
            if let Some(v) = orig_path {
                std::env::set_var("PATH", v);
            } else {
                std::env::remove_var("PATH");
            }
        }

        let info = result.expect("bare 'iii-worker-ops' must resolve via KNOWN_EXTERNAL");
        assert_eq!(info.name, "iii-worker-ops");
        assert_eq!(info.binary_path, fake);
        assert_eq!(info.extra_args, vec!["worker-manager-daemon".to_string()]);
    }

    #[test]
    #[serial_test::serial]
    fn resolve_external_module_in_hits_known_external_when_binary_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("iii-worker");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var_os("PATH");
        // SAFETY: test is marked #[serial] so no other thread mutates env concurrently.
        unsafe {
            std::env::set_var("PATH", dir.path());
        }

        let result = resolve_external_module_in(dir.path(), "workers::iii_sandbox");

        // SAFETY: test is marked #[serial] so no other thread mutates env concurrently.
        unsafe {
            if let Some(v) = orig_path {
                std::env::set_var("PATH", v);
            } else {
                std::env::remove_var("PATH");
            }
        }

        let info = result.expect("iii-sandbox should resolve via KNOWN_EXTERNAL");
        assert_eq!(info.name, "iii-sandbox");
        assert_eq!(info.binary_path, fake);
        assert_eq!(info.extra_args, vec!["sandbox-daemon".to_string()]);
    }

    // Regression: `WorkerRegistry::create_worker` calls
    // `resolve_external_module(name)` with the bare `name` from
    // config.yaml — e.g. "iii-sandbox" — not the `workers::iii_sandbox`
    // class-path form. The KNOWN_EXTERNAL lookup must handle both.
    #[test]
    #[serial_test::serial]
    fn resolve_external_module_in_hits_known_external_with_bare_name() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("iii-worker");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var_os("PATH");
        // SAFETY: #[serial].
        unsafe {
            std::env::set_var("PATH", dir.path());
        }

        let result = resolve_external_module_in(dir.path(), "iii-sandbox");

        // SAFETY: #[serial]; restoring original.
        unsafe {
            if let Some(v) = orig_path {
                std::env::set_var("PATH", v);
            } else {
                std::env::remove_var("PATH");
            }
        }

        let info = result.expect("bare 'iii-sandbox' must resolve via KNOWN_EXTERNAL");
        assert_eq!(info.name, "iii-sandbox");
        assert_eq!(info.binary_path, fake);
        assert_eq!(info.extra_args, vec!["sandbox-daemon".to_string()]);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn external_worker_spawns_child_with_extra_args_before_config() {
        let dir = tempfile::tempdir().unwrap();
        let argv_log = dir.path().join("argv.txt");

        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/argv_probe.sh");

        let info = ExternalWorkerInfo {
            name: "probe".into(),
            binary_path: fixture,
            extra_args: vec!["first-arg".into(), "second-arg".into()],
            env: vec![("III_PROBE_EXTRA_ENV".into(), "probe-env-value".into())],
        };

        // Distinctive port: the assertion below proves the child received
        // THIS worker's port, not a hardcoded default.
        let worker = ExternalWorker::new(info, Some(serde_json::json!({"key": "val"})), 50123);

        // The probe reads ARGV_LOG from env. Set it on the parent so
        // the spawned child inherits.
        // SAFETY: edition 2024 requires unsafe wrap; test is #[serial].
        unsafe {
            std::env::set_var("ARGV_LOG", &argv_log);
        }

        let (tx, rx) = tokio::sync::watch::channel(false);
        worker.start_background_tasks(rx, tx.clone()).await.unwrap();

        // Poll for the argv file (probe writes immediately; worker may
        // sleep briefly before spawning).
        let mut got = None;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if let Ok(s) = std::fs::read_to_string(&argv_log) {
                got = Some(s.trim().to_string());
                break;
            }
        }
        let _ = tx.send(true);
        worker.destroy().await.unwrap();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::remove_var("ARGV_LOG");
        }

        let argv = got.expect("probe fixture never wrote argv");
        assert!(
            argv.starts_with("first-arg second-arg --config "),
            "expected extra args before --config, got: {argv:?}"
        );

        // Engine-attachment contract: the child must receive this engine's
        // pid AND an OPEN lifeline pipe fd (the probe checks /dev/fd/N from
        // inside the child — proving the pre_exec un-CLOEXEC actually made
        // the fd survive exec).
        #[cfg(unix)]
        {
            let env_line = argv.lines().nth(1).unwrap_or_default();
            assert!(
                env_line.contains(&format!("engine_pid={}", std::process::id())),
                "child must see this engine's pid; got: {env_line:?}"
            );
            assert!(
                env_line.contains("lifeline_open=yes"),
                "child's lifeline fd must be open after exec; got: {env_line:?}"
            );
            assert!(
                env_line.contains("engine_url=ws://127.0.0.1:50123"),
                "child must see this engine's actual WS URL, not a default; got: {env_line:?}"
            );
            // Info-declared env vars (e.g. III_CONFIG_PATH in production)
            // must reach the child.
            assert!(
                env_line.contains("extra_env=probe-env-value"),
                "child must see ExternalWorkerInfo.env vars; got: {env_line:?}"
            );
        }
    }

    #[cfg(unix)]
    fn read_pids(path: &std::path::Path) -> Vec<i32> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }

    #[cfg(unix)]
    async fn wait_for_pids(path: &std::path::Path, want: usize) -> Vec<i32> {
        // 10s ceiling: first spawn hides behind the 2s bind delay, a
        // respawn behind SUPERVISOR_POLL + 1s backoff.
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let pids = read_pids(path);
            if pids.len() >= want {
                return pids;
            }
        }
        read_pids(path)
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial_test::serial]
    async fn external_worker_respawns_killed_child_and_stops_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("pids.txt");

        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/respawn_probe.sh");

        let info = ExternalWorkerInfo {
            name: "respawn-probe".into(),
            binary_path: fixture,
            extra_args: vec![],
            env: Vec::new(),
        };
        // config: None — the builtin daemon path MOT-3857 is about.
        let worker = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);

        // SAFETY: edition 2024 requires unsafe wrap; test is #[serial].
        unsafe {
            std::env::set_var("RESPAWN_LOG", &log);
        }

        let (tx, rx) = tokio::sync::watch::channel(false);
        worker.start_background_tasks(rx, tx.clone()).await.unwrap();

        let pids = wait_for_pids(&log, 1).await;
        let pid1 = *pids.first().expect("probe never spawned");

        // SIGKILL = unexpected death, not engine shutdown.
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid1),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();

        let pids = wait_for_pids(&log, 2).await;
        assert!(
            pids.len() >= 2,
            "supervisor never respawned the killed child (pids: {pids:?})"
        );
        let pid2 = pids[1];
        assert_ne!(pid1, pid2, "respawn must be a fresh process");

        // Shutdown stops supervision: the fresh child is killed and no
        // further respawn happens.
        let spawns_before_shutdown = pids.len();
        let _ = tx.send(true);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert_eq!(
            read_pids(&log).len(),
            spawns_before_shutdown,
            "no respawn may happen after shutdown"
        );
        assert!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid2), None).is_err(),
            "child must be dead after shutdown"
        );

        worker.destroy().await.unwrap();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::remove_var("RESPAWN_LOG");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial_test::serial]
    async fn external_worker_destroy_during_backoff_prevents_respawn() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("pids.txt");

        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/respawn_probe.sh");

        let info = ExternalWorkerInfo {
            name: "destroy-probe".into(),
            binary_path: fixture,
            extra_args: vec![],
            env: Vec::new(),
        };
        let worker = ExternalWorker::new(info, None, crate::workers::worker::DEFAULT_PORT);

        // SAFETY: edition 2024 requires unsafe wrap; test is #[serial].
        unsafe {
            std::env::set_var("RESPAWN_LOG", &log);
        }

        let (tx, rx) = tokio::sync::watch::channel(false);
        worker.start_background_tasks(rx, tx.clone()).await.unwrap();

        let pids = wait_for_pids(&log, 1).await;
        let pid1 = *pids.first().expect("probe never spawned");

        // Unexpected death: supervisor detects within SUPERVISOR_POLL
        // (500ms), then waits RESPAWN_BACKOFF_MIN (1s) — earliest respawn is
        // ~1s after the kill.
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid1),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();

        // destroy() with NO shutdown signal, landing inside the backoff
        // window — the terminal `stopped` state must win over the respawn.
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        worker.destroy().await.unwrap();

        // Well past the 1s backoff: no second spawn may exist.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert_eq!(
            read_pids(&log).len(),
            1,
            "destroy() during backoff must not be raced by a respawn"
        );
        // SAFETY: test is #[serial].
        unsafe {
            std::env::remove_var("RESPAWN_LOG");
        }
    }
}
