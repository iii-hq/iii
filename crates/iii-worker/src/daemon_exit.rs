// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Shared self-exit watch for engine-spawned daemons (`worker-manager-daemon`,
//! `sandbox-daemon`).
//!
//! The engine spawns these daemons in their OWN session (setsid, see
//! engine/src/workers/external.rs) so it can clean-kill the whole process
//! group — but that detaches them from parent-death signals (an orphaned
//! session leader gets no SIGHUP from the kernel). Without a self-exit path,
//! an engine that dies abnormally (SIGKILL, OOM, crash, dev hard-restart that
//! skips kill_child) leaves the daemon reconnect-looping forever, and every
//! engine restart leaks another orphan. A real incident accumulated 19 of
//! them; an orphaned sandbox-daemon additionally keeps multi-GB libkrun VMs
//! alive.
//!
//! [`ExitWatch::wait`] resolves on ANY of: SIGINT, SIGTERM (what the engine's
//! kill_child actually sends — previously default-disposition death), SIGHUP
//! (terminal close on a hand-launched daemon), or engine death. Engine death
//! is detected two ways, in preference order:
//!
//! 1. **PID handshake** — the engine exports [`ENGINE_PID_ENV`] at spawn and
//!    the daemon polls that pid's existence directly. Immune to whatever sits
//!    between the engine and the daemon (wrapper scripts, spawn shims,
//!    debugger reparenting).
//! 2. **Reparent fallback** (env absent — e.g. version skew with an older
//!    engine) — the daemon snapshots `getppid()` at startup and exits when
//!    the ppid CHANGES *and* the original parent no longer exists. Both
//!    conditions are required: on macOS a debugger attach reparents the
//!    target to lldb (XNU `proc_reparent`), so a change-only check would kill
//!    the live daemon API under a debugger while the engine is healthy.
//!
//! Either way, a long-lived PID-reuse collision can only cause over-survival
//! (the old leak, rare and bounded), never a false exit.
//!
//! INVARIANT the fallback depends on: the engine must never fork or re-exec
//! itself after spawning a daemon — a parent that "moves" looks identical to
//! a parent that died.
//!
//! Unix-only: on non-Unix targets only Ctrl-C is watched and the orphan
//! self-exit does not exist.

#[cfg(unix)]
use std::time::Duration;

/// Env var the engine sets at daemon spawn (engine/src/workers/external.rs)
/// declaring its own pid, so daemons watch ENGINE liveness directly instead
/// of inferring it from `getppid()`.
pub const ENGINE_PID_ENV: &str = "III_ENGINE_PID";

/// Env var naming the file-descriptor number of an inherited LIFELINE PIPE
/// read end. The spawner holds the write end and never writes; when the
/// spawner dies — for ANY reason, SIGKILL included — the kernel closes the
/// write end and the child's read returns EOF instantly. Strictly stronger
/// than the PID watch: zero latency, no polling, no PID-reuse caveat, and no
/// dependence on parent/ppid semantics. Set by the engine for daemons
/// (engine/src/workers/external.rs — keep the literal there in sync) and by
/// the sandbox daemon for the `__vm-boot` VMs it launches.
pub const LIFELINE_FD_ENV: &str = "III_LIFELINE_FD";

/// Env var naming the SPAWNER's pid, set alongside [`LIFELINE_FD_ENV`] by
/// [`attach_lifeline`]. Children use it as the polling backstop for the one
/// lifeline failure mode: on macOS `pipe2` doesn't exist, so a concurrent
/// spawn on another thread can inherit a write end in the window before the
/// CLOEXEC fcntls land, delaying EOF until that bystander also exits. The
/// pid poll is immune to leaked fds.
pub const LIFELINE_SPAWNER_PID_ENV: &str = "III_LIFELINE_SPAWNER_PID";

/// Spawn-time facts captured BEFORE the tokio runtime exists. Reading the
/// lifeline fd out of the env requires `env::remove_var` (so children never
/// inherit it), which is only sound while the process is single-threaded —
/// `#[tokio::main]`-style entrypoints spawn worker threads before any user
/// code runs, so `main()` must call [`capture_early`] first, pre-runtime.
struct EarlyCapture {
    #[cfg(unix)]
    lifeline: std::sync::Mutex<Option<std::os::fd::OwnedFd>>,
    #[cfg(unix)]
    spawner_pid: Option<i32>,
}

static EARLY: std::sync::OnceLock<EarlyCapture> = std::sync::OnceLock::new();

/// Capture (and scrub from the env) the inherited lifeline facts. MUST be
/// the first thing `main()` does, before building the tokio runtime — this
/// is the only point where mutating the process environment is sound.
/// Idempotent; later calls are no-ops.
pub fn capture_early() {
    let _ = EARLY.get_or_init(|| EarlyCapture {
        #[cfg(unix)]
        lifeline: std::sync::Mutex::new(lifeline_fd_from_env()),
        #[cfg(unix)]
        spawner_pid: {
            let pid = std::env::var(LIFELINE_SPAWNER_PID_ENV)
                .ok()
                .and_then(|v| v.trim().parse::<i32>().ok())
                .filter(|p| *p > 1);
            if std::env::var_os(LIFELINE_SPAWNER_PID_ENV).is_some() {
                // SAFETY: pre-runtime, single-threaded (the contract of this
                // function); scrubbed so children never inherit it.
                unsafe { std::env::remove_var(LIFELINE_SPAWNER_PID_ENV) };
            }
            pid
        },
    });
}

/// Take the early-captured lifeline read end (None if [`capture_early`]
/// wasn't called or no valid lifeline was inherited).
#[cfg(unix)]
pub fn take_early_lifeline() -> Option<std::os::fd::OwnedFd> {
    EARLY.get()?.lifeline.lock().ok()?.take()
}

/// The early-captured spawner pid, for the polling backstop.
#[cfg(unix)]
pub fn early_spawner_pid() -> Option<i32> {
    EARLY.get()?.spawner_pid
}

/// Polling-backstop companion to [`blocking_wait_lifeline_eof`]: block until
/// `pid` no longer exists (ESRCH). For plain-thread contexts like
/// `__vm-boot`; the daemons use the async PID watch in [`ExitWatch`].
#[cfg(unix)]
pub fn blocking_wait_pid_gone(pid: i32) {
    loop {
        std::thread::sleep(ENGINE_POLL_INTERVAL);
        if process_is_gone(pid) {
            return;
        }
    }
}

/// Read-only engine pid from the env. Deliberately NOT scrubbed — unlike the
/// lifeline fd, [`ENGINE_PID_ENV`] flows down the entire spawn tree so that
/// detached processes (managed-worker `__vm-boot` VMs, `__watch-source`
/// sidecars) can anchor to ENGINE lifetime regardless of how many transient
/// spawners sit in between. Absent (e.g. a hand-run `iii worker start` in a
/// terminal) means "not engine-rooted": no watch, the process is the user's.
pub fn engine_pid_from_env() -> Option<i32> {
    std::env::var(ENGINE_PID_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<i32>().ok())
        .filter(|p| *p > 1)
}

/// How often the engine-liveness watch polls. The orphan-exit integration
/// test's deadline is sized against this value.
#[cfg(unix)]
pub const ENGINE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// One end of a spawner→child death-notification pipe. The holder is the
/// SPAWNER: dropping a `Lifeline` (or the spawner dying, however abruptly)
/// closes the write end and the child's lifeline watch sees EOF. Created by
/// [`attach_lifeline`] / [`attach_lifeline_std`].
#[derive(Debug)]
pub struct Lifeline {
    /// Held open, never written. Kernel closes it on process death.
    #[cfg(unix)]
    _write: std::os::fd::OwnedFd,
    /// The parent's copy of the read end. Kept so the raw fd number baked
    /// into the child's env/pre_exec stays valid until (and after) spawn;
    /// holding a READ end open never delays the child's EOF (EOF fires when
    /// all WRITE ends close).
    #[cfg(unix)]
    _read: std::os::fd::OwnedFd,
}

/// Create a lifeline pipe and wire `cmd` so the child inherits the read end:
/// both ends are CLOEXEC in this process (no leak into other children); a
/// `pre_exec` hook clears CLOEXEC on the read end for THIS child only, and
/// [`LIFELINE_FD_ENV`] tells the child which fd number it is.
///
/// Keep the returned [`Lifeline`] alive for as long as the child should
/// live; drop it to tell the child its spawner is gone.
///
/// macOS has no `pipe2`, so there is a tiny window between `pipe()` and the
/// CLOEXEC fcntls where a concurrent spawn on another thread could inherit
/// both ends, which would delay the child's EOF until that bystander also
/// exits. The pid-poll backstops cover that case: [`LIFELINE_SPAWNER_PID_ENV`]
/// is set here and consumed by [`ExitWatch::wait`] (daemons) and `__vm-boot`'s
/// [`blocking_wait_pid_gone`] thread (VMs).
#[cfg(unix)]
pub fn attach_lifeline(cmd: &mut tokio::process::Command) -> std::io::Result<Lifeline> {
    let (read, write) = new_cloexec_pipe()?;
    let read_raw = std::os::fd::AsRawFd::as_raw_fd(&read);
    cmd.env(LIFELINE_FD_ENV, read_raw.to_string());
    cmd.env(LIFELINE_SPAWNER_PID_ENV, std::process::id().to_string());
    // SAFETY: fcntl(F_SETFD) is async-signal-safe; runs in the
    // forked-but-not-yet-execed child to un-CLOEXEC its inherited copy.
    unsafe {
        cmd.pre_exec(move || clear_cloexec(read_raw));
    }
    Ok(Lifeline {
        _write: write,
        _read: read,
    })
}

/// [`attach_lifeline`] for `std::process::Command` spawn sites.
#[cfg(unix)]
pub fn attach_lifeline_std(cmd: &mut std::process::Command) -> std::io::Result<Lifeline> {
    use std::os::unix::process::CommandExt;
    let (read, write) = new_cloexec_pipe()?;
    let read_raw = std::os::fd::AsRawFd::as_raw_fd(&read);
    cmd.env(LIFELINE_FD_ENV, read_raw.to_string());
    cmd.env(LIFELINE_SPAWNER_PID_ENV, std::process::id().to_string());
    // SAFETY: as in `attach_lifeline`.
    unsafe {
        cmd.pre_exec(move || clear_cloexec(read_raw));
    }
    Ok(Lifeline {
        _write: write,
        _read: read,
    })
}

/// Pipe with both ends CLOEXEC: atomically via `pipe2` where it exists,
/// pipe+fcntl elsewhere (see the race note on [`attach_lifeline`]).
#[cfg(unix)]
fn new_cloexec_pipe() -> std::io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    #[cfg(target_os = "linux")]
    {
        let (r, w) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)?;
        Ok((r, w))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (r, w) = nix::unistd::pipe()?;
        set_cloexec(std::os::fd::AsRawFd::as_raw_fd(&r))?;
        set_cloexec(std::os::fd::AsRawFd::as_raw_fd(&w))?;
        Ok((r, w))
    }
}

#[cfg(unix)]
fn set_cloexec(fd: i32) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn clear_cloexec(fd: i32) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Child side: take ownership of the lifeline read end named by
/// [`LIFELINE_FD_ENV`], if any. Validates the fd is a live FIFO before
/// trusting it (a stale/corrupt env naming, say, stdin would otherwise turn
/// "read /dev/null EOF" into a false spawner-death). On success the fd gets
/// CLOEXEC set again and the env var is scrubbed, so OUR children inherit
/// neither — without that, e.g. a `__vm-boot` launched by the worker start
/// path would inherit the daemon's ENGINE lifeline and a detached worker VM
/// would wrongly die with the engine.
///
/// Single-call contract: takes ownership of the raw fd, so call at most once
/// per process (the daemons call it from `arm_at_startup`, `__vm-boot` from
/// its entrypoint).
#[cfg(unix)]
pub fn lifeline_fd_from_env() -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    let val = std::env::var(LIFELINE_FD_ENV).ok()?;
    // Scrub BEFORE validating: every rejection path below must also keep the
    // var away from our children, not just the success path.
    // SAFETY: startup-time, before this process spawns anything that could
    // race the env table (capture_early's pre-runtime contract).
    unsafe { std::env::remove_var(LIFELINE_FD_ENV) };
    let Ok(raw) = val.trim().parse::<i32>() else {
        tracing::warn!(value = ?val, "ignoring unparsable lifeline env");
        return None;
    };
    if raw <= 2 {
        tracing::warn!(fd = raw, "ignoring lifeline env naming a stdio fd");
        return None;
    }
    // Validate BEFORE taking ownership: if the env lies and names a foreign
    // fd, we must not adopt (and later close) something that isn't ours.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(raw, &mut stat) } != 0 {
        tracing::warn!(fd = raw, "ignoring lifeline env: fd is not open");
        return None;
    }
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
        tracing::warn!(fd = raw, "ignoring lifeline env: fd is not a pipe");
        return None;
    }
    // Ownership BEFORE the final fcntl: a failure there must close the
    // (genuinely ours, per protocol) fd rather than leak it open and
    // non-CLOEXEC into every child we spawn.
    // SAFETY: protocol gives this process sole ownership of the inherited fd.
    let owned = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
    if let Err(e) = set_cloexec(raw) {
        tracing::warn!(fd = raw, error = %e, "ignoring lifeline: cannot re-arm CLOEXEC");
        return None; // drop(owned) closes it
    }
    Some(owned)
}

/// Block until the lifeline reports the spawner is gone. Returns `true` on
/// EOF (all write ends closed — the spawner died or dropped its
/// [`Lifeline`]); `false` on an unrecoverable read error, which is NOT death
/// evidence (callers fall back to their other watches). Any bytes received
/// are ignored — the protocol never writes.
#[cfg(unix)]
pub fn blocking_wait_lifeline_eof(fd: std::os::fd::OwnedFd) -> bool {
    use std::io::Read;
    let mut file = std::fs::File::from(fd);
    let mut buf = [0u8; 16];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                tracing::warn!(error = %e, "lifeline read failed; falling back to PID watches");
                return false;
            }
        }
    }
}

/// Spawn-time facts needed to detect engine death. Construct with
/// [`ExitWatch::arm_at_startup`] as the FIRST statement of the daemon's
/// entrypoint, before any await: the parent right now is whoever launched us;
/// by the time the watch is polled (after SDK init + registrations) a dead
/// engine would already have been replaced by the adopter in `getppid()`.
pub struct ExitWatch {
    #[cfg(unix)]
    spawn_parent: i32,
    #[cfg(unix)]
    engine_pid: Option<i32>,
    /// From [`LIFELINE_SPAWNER_PID_ENV`]: pid-poll backstop when no engine
    /// pid was declared (lifeline-only spawners like [`attach_lifeline`]).
    #[cfg(unix)]
    spawner_pid: Option<i32>,
    #[cfg(unix)]
    lifeline: Option<std::os::fd::OwnedFd>,
}

impl ExitWatch {
    pub fn arm_at_startup() -> Self {
        #[cfg(unix)]
        {
            let engine_pid = engine_pid_from_env();
            Self {
                spawn_parent: nix::unistd::getppid().as_raw(),
                engine_pid,
                spawner_pid: early_spawner_pid(),
                // Prefer the pre-runtime capture (env already scrubbed,
                // single-threaded at the time); fall back to a live read for
                // in-process embedders (tests, the engine e2e harness) that
                // never go through main().
                lifeline: take_early_lifeline().or_else(lifeline_fd_from_env),
            }
        }
        #[cfg(not(unix))]
        {
            Self {}
        }
    }

    /// Block until the daemon should exit; returns a short reason for the
    /// log ("sigint" | "sigterm" | "sighup" | "engine-gone"). On the
    /// engine-gone path a one-line breadcrumb is appended to
    /// `~/.iii/logs/<daemon>.log` first: the daemon's stdout/stderr are pipes
    /// into the now-dead engine and the OTEL sink WAS that engine, so without
    /// it the self-exit is invisible in the field.
    pub async fn wait(&self, daemon: &'static str) -> &'static str {
        #[cfg(unix)]
        {
            use tokio::signal::unix::SignalKind;
            let reason = tokio::select! {
                _ = wait_for_sigint() => "sigint",
                _ = wait_for_unix_signal(SignalKind::terminate(), "SIGTERM") => "sigterm",
                _ = wait_for_unix_signal(SignalKind::hangup(), "SIGHUP") => "sighup",
                _ = self.wait_for_engine_gone(daemon) => "engine-gone",
            };
            if reason == "engine-gone" {
                write_exit_breadcrumb(daemon, self.engine_pid, self.spawn_parent);
            }
            reason
        }
        #[cfg(not(unix))]
        {
            let _ = daemon;
            let _ = tokio::signal::ctrl_c().await;
            "sigint"
        }
    }

    #[cfg(unix)]
    async fn wait_for_engine_gone(&self, daemon: &'static str) {
        // The PID watch (or reparent fallback) always runs as the backstop;
        // the lifeline, when present, races it as the primary signal — EOF is
        // instant and immune to PID semantics, while the backstop covers the
        // one lifeline failure mode (a leaked write end keeping EOF at bay).
        let backstop = async {
            match self.engine_pid.or(self.spawner_pid) {
                Some(pid) => wait_for_pid_exit(pid, daemon).await,
                None => wait_for_reparent(self.spawn_parent, daemon).await,
            }
        };
        tokio::pin!(backstop);

        if let Some(dup) = self.lifeline.as_ref().and_then(|fd| fd.try_clone().ok()) {
            tracing::info!(daemon, "lifeline exit-watch armed");
            tokio::select! {
                eof = tokio::task::spawn_blocking(move || blocking_wait_lifeline_eof(dup)) => {
                    if matches!(eof, Ok(true)) {
                        redirect_stdio_to_exit_log(daemon);
                        tracing::warn!(daemon, "lifeline closed: spawner gone");
                        return;
                    }
                    // Read error or a cancelled blocking task: not death
                    // evidence — fall through to the backstop.
                }
                _ = backstop.as_mut() => return,
            }
        }
        backstop.await;
    }
}

/// The engine that consumed this daemon's stdout/stderr just died, so fds
/// 1/2 are broken pipes. The very next write would panic the daemon: the
/// fmt layer swallows its own EPIPE, but its internal-error fallback is an
/// `eprintln!`, which panics when stderr is also broken — unwinding the
/// main task BETWEEN engine-death detection and the breadcrumb/reaper. A
/// real `killall -9 iii` left every managed worker running because the
/// reaper died to exactly this before stopping anything.
///
/// Re-point fds 1/2 at the durable exit log (same file as the breadcrumb)
/// so post-mortem writes succeed AND the reap pass becomes visible
/// forensics; fall back to /dev/null. Must run BEFORE the first
/// engine-is-gone log line. Best-effort: on total failure the panic risk
/// simply remains, which is no worse than before.
#[cfg(unix)]
fn redirect_stdio_to_exit_log(daemon: &str) {
    use std::os::fd::IntoRawFd;
    let log = exit_log_path(daemon)
        .and_then(|p| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        })
        .or_else(|| {
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .ok()
        });
    let Some(log) = log else { return };
    // Leak the fd deliberately: it must outlive this call as the process's
    // stdout/stderr for the remaining (short) life of the daemon.
    let fd = log.into_raw_fd();
    unsafe {
        libc::dup2(fd, 1);
        libc::dup2(fd, 2);
        if fd > 2 {
            libc::close(fd);
        }
    }
}

/// Resolve on Ctrl-C/SIGINT. If the handler can't be installed, log and park
/// forever instead of resolving — an installation Err is NOT an exit request,
/// and the other exit arms still cover shutdown. (An earlier version exited
/// rc 0 with reason "sigint" on Err, silently taking the daemon down at
/// startup.)
#[cfg(unix)]
async fn wait_for_sigint() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "ctrl_c handler failed; SIGINT exit arm disabled");
        std::future::pending::<()>().await
    }
}

/// Resolve when `kind` is delivered. Installation failure (or stream
/// exhaustion) logs and parks rather than resolving, for the same reason as
/// [`wait_for_sigint`].
#[cfg(unix)]
async fn wait_for_unix_signal(kind: tokio::signal::unix::SignalKind, name: &'static str) {
    match tokio::signal::unix::signal(kind) {
        Ok(mut sig) => {
            if sig.recv().await.is_none() {
                tracing::error!(signal = name, "signal stream closed; exit arm disabled");
                std::future::pending::<()>().await
            }
        }
        Err(e) => {
            tracing::error!(error = %e, signal = name, "failed to install signal handler; exit arm disabled");
            std::future::pending::<()>().await
        }
    }
}

/// PID-handshake watch: resolve once the engine's declared pid stops
/// existing. Immune to who our direct parent is.
#[cfg(unix)]
async fn wait_for_pid_exit(pid: i32, daemon: &'static str) {
    tracing::info!(engine_pid = pid, daemon, "engine exit-watch armed");
    loop {
        tokio::time::sleep(ENGINE_POLL_INTERVAL).await;
        if process_is_gone(pid) {
            redirect_stdio_to_exit_log(daemon);
            tracing::warn!(engine_pid = pid, daemon, "declared engine pid exited");
            return;
        }
    }
}

/// Reparent-fallback watch: resolve once the spawn-time parent is gone —
/// `getppid()` moved away from `initial` AND `initial` no longer exists.
///
/// If we were already orphaned at startup (ppid <= 1: init/launchd, or a
/// container where the engine is PID 1), there is nothing meaningful to
/// watch — the watch stays disarmed and the signal arms are the exits.
#[cfg(unix)]
async fn wait_for_reparent(initial: i32, daemon: &'static str) {
    if initial <= 1 {
        tracing::warn!(
            ppid = initial,
            daemon,
            "parent exit-watch disarmed: no watchable spawn parent"
        );
        return std::future::pending::<()>().await;
    }
    tracing::info!(ppid = initial, daemon, "parent exit-watch armed");
    loop {
        tokio::time::sleep(ENGINE_POLL_INTERVAL).await;
        let current = nix::unistd::getppid().as_raw();
        if reparented_away(initial, current) && process_is_gone(initial) {
            redirect_stdio_to_exit_log(daemon);
            tracing::warn!(was = initial, now = current, daemon, "spawn parent exited");
            return;
        }
    }
}

/// True when a fresh `getppid()` reading means we've been reparented away
/// from the parent that spawned us. Pure so the subtle parts are pinned by
/// tests: `initial <= 1` (no real parent at startup) never triggers, and we
/// key off a CHANGE from the startup parent — NOT `current == 1` — so
/// adoption by a subreaper rather than init still counts, and a daemon whose
/// real parent stays alive never false-exits. Callers must additionally
/// confirm the old parent is actually dead ([`process_is_gone`]) before
/// acting.
#[cfg(unix)]
fn reparented_away(initial: i32, current: i32) -> bool {
    initial > 1 && current != initial
}

/// Existence probe via `kill(pid, 0)`: true only on ESRCH (no such process).
/// EPERM means the process exists but isn't ours — counts as alive.
#[cfg(unix)]
fn process_is_gone(pid: i32) -> bool {
    matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

/// The durable per-daemon exit log: `~/.iii/logs/<daemon>.log`. Holds the
/// engine-gone breadcrumb and, post-redirect, the daemon's final output
/// (the reap pass). Creates the parent directory as a side effect.
#[cfg(unix)]
fn exit_log_path(daemon: &str) -> Option<std::path::PathBuf> {
    let dir = dirs::home_dir()?.join(".iii/logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{daemon}.log")))
}

/// Best-effort durable trace of an engine-gone self-exit (appends to
/// `~/.iii/logs/<daemon>.log`). Every failure mode is silently ignored — the
/// daemon is exiting either way.
#[cfg(unix)]
fn write_exit_breadcrumb(daemon: &str, engine_pid: Option<i32>, spawn_parent: i32) {
    use std::io::Write;
    let Some(path) = exit_log_path(daemon) else {
        return;
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!(
        "ts={ts} daemon={daemon} self-exit reason=engine-gone engine_pid={} spawn_parent={spawn_parent}\n",
        engine_pid.map_or_else(|| "none".to_string(), |p| p.to_string()),
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{process_is_gone, reparented_away};

    #[test]
    fn reparent_detection_keys_on_change_not_pid_one() {
        // Engine alive: ppid unchanged → keep running.
        assert!(!reparented_away(12345, 12345));
        // Engine died, adopted by launchd/init → exit.
        assert!(reparented_away(12345, 1));
        // Adopted by a subreaper (not init) → still a reparent → exit.
        assert!(reparented_away(12345, 999));
        // Started already orphaned (ppid 1) or unknown (0): nothing to watch,
        // never exit on this signal — guards the in-process e2e harness and
        // any daemon launched without a real parent.
        assert!(!reparented_away(1, 1));
        assert!(!reparented_away(1, 999));
        assert!(!reparented_away(0, 1));
    }

    #[test]
    fn process_is_gone_only_on_esrch() {
        // Our own process and our own parent demonstrably exist.
        assert!(!process_is_gone(std::process::id() as i32));
        assert!(!process_is_gone(nix::unistd::getppid().as_raw()));
        // PID 1 (launchd/init) always exists; we can't signal it → EPERM →
        // counts as alive, never as gone.
        assert!(!process_is_gone(1));
        // A child we spawn and fully reap is guaranteed dead at probe time.
        // (Its pid could in principle be recycled between reap and probe, in
        // which case the probe correctly says "exists" — so don't assert the
        // negative; just that the probe never panics.)
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id() as i32;
        child.wait().unwrap();
        let _ = process_is_gone(pid);
    }

    /// Serializes the env-mutating tests below: `arm_at_startup` reads BOTH
    /// env vars and `lifeline_fd_from_env` takes fd ownership, so unguarded
    /// parallel runs could double-own an fd or see each other's vars.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn arm_at_startup_parses_engine_pid_env() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        unsafe { std::env::remove_var(super::LIFELINE_FD_ENV) };

        unsafe { std::env::set_var(super::ENGINE_PID_ENV, "  4242 ") };
        let watch = super::ExitWatch::arm_at_startup();
        assert_eq!(watch.engine_pid, Some(4242));

        unsafe { std::env::set_var(super::ENGINE_PID_ENV, "1") }; // <= 1 rejected
        assert_eq!(super::ExitWatch::arm_at_startup().engine_pid, None);

        unsafe { std::env::set_var(super::ENGINE_PID_ENV, "not-a-pid") };
        assert_eq!(super::ExitWatch::arm_at_startup().engine_pid, None);

        unsafe { std::env::remove_var(super::ENGINE_PID_ENV) };
        assert_eq!(super::ExitWatch::arm_at_startup().engine_pid, None);
    }

    #[test]
    fn lifeline_env_rejects_garbage_and_non_pipes() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        unsafe { std::env::remove_var(super::LIFELINE_FD_ENV) };
        assert!(super::lifeline_fd_from_env().is_none());

        // Stdio fd numbers are refused outright.
        unsafe { std::env::set_var(super::LIFELINE_FD_ENV, "0") };
        assert!(super::lifeline_fd_from_env().is_none());

        // A live fd that is NOT a pipe (a regular file) is refused — a
        // corrupt env must not turn "EOF on some file" into engine death.
        let f = std::fs::File::open("/dev/null").unwrap();
        let raw = std::os::fd::AsRawFd::as_raw_fd(&f);
        unsafe { std::env::set_var(super::LIFELINE_FD_ENV, raw.to_string()) };
        assert!(super::lifeline_fd_from_env().is_none());

        // Closed/garbage fd numbers are refused.
        unsafe { std::env::set_var(super::LIFELINE_FD_ENV, "9999") };
        assert!(super::lifeline_fd_from_env().is_none());
        // The var is scrubbed even on rejection (children must not inherit).
        assert!(std::env::var(super::LIFELINE_FD_ENV).is_err());
    }

    #[test]
    fn lifeline_accepts_pipe_and_sees_eof_when_writer_drops() {
        use std::os::fd::IntoRawFd;
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let (read, write) = nix::unistd::pipe().unwrap();
        // Hand the raw number over as "inherited" — lifeline_fd_from_env
        // takes ownership, so release ours first.
        let raw = read.into_raw_fd();
        unsafe { std::env::set_var(super::LIFELINE_FD_ENV, raw.to_string()) };
        let owned = super::lifeline_fd_from_env().expect("pipe read end accepted");
        // Env scrubbed so OUR children can't inherit the lifeline.
        assert!(std::env::var(super::LIFELINE_FD_ENV).is_err());

        // EOF only after the write end drops.
        let watcher = std::thread::spawn(move || super::blocking_wait_lifeline_eof(owned));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!watcher.is_finished(), "no EOF while the writer is alive");
        drop(write);
        assert!(
            watcher.join().unwrap(),
            "dropping the write end must read as EOF (spawner gone)"
        );
    }
}
