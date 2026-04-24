use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use iii_shell_client::{RequestSpec, Session, VecSink};

use crate::sandbox_daemon::create::{BootHandle, BootParams, VmLauncher};
use crate::sandbox_daemon::errors::SandboxError;
use crate::sandbox_daemon::exec::{ExecRequest, ExecResponse, ShellRunner};
use crate::sandbox_daemon::stop::VmStopper;

/// How often to stat the shell socket while waiting for __vm-boot's
/// `shell_relay` to bind it. Unix socket existence is a cheap fs stat,
/// so polling aggressively is harmless; the previous 500ms interval
/// added up to ~499ms of pure lag after the socket actually bound.
const BOOT_SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(10);
const BOOT_SOCKET_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_EXEC_TIMEOUT_MS: u64 = 30_000;
/// Grace between SIGTERM and SIGKILL. Kept short because the sandbox VM
/// is ephemeral — nothing needs to flush to persistent storage. Values
/// larger than a few hundred ms directly translate into user-visible
/// `sandbox run` latency.
const STOP_GRACE_MS: u64 = 200;
/// Cap sandbox output at 1 MiB per stream (same as vm_client).
const OUTPUT_CAP: usize = 1_048_576;

/// Set once the first boot has completed its per-host provisioning
/// (codesign entitlement + libkrunfw dylib placement). Subsequent boots
/// skip the work, which is idempotent but touches the filesystem on
/// every call.
static PROVISION_DONE: AtomicBool = AtomicBool::new(false);

pub struct IiiWorkerLauncher;

#[async_trait::async_trait]
impl VmLauncher for IiiWorkerLauncher {
    async fn boot(&self, params: &BootParams) -> Result<BootHandle, SandboxError> {
        let t_boot_start = Instant::now();
        // We're running inside the iii-worker binary ourselves, so the
        // path to fork+exec for __vm-boot is our own executable. More
        // reliable than a PATH lookup: current_exe() is guaranteed to
        // resolve (unlike PATH, which can be empty in service managers)
        // and cannot disagree with the version we compiled against.
        let bin = std::env::current_exe()
            .map_err(|e| SandboxError::BootFailed(format!("current_exe() failed: {e}")))?;

        // Per-host provisioning: codesign (macOS Hypervisor entitlement)
        // and libkrunfw dylib placement. Both are idempotent but touch
        // the filesystem, and this runs on every sandbox::create --
        // when the user is issuing a stream of short runs, that's
        // wasteful lag on the hot path. Cache completion in an atomic
        // so only the first boot of each daemon lifetime pays for it.
        //
        // If the cached bit is missing (first boot, or restart after
        // the user wiped ~/.iii/lib/), we run both steps. If it's set,
        // we trust the previous boot's side-effects — if the user
        // manually deletes the dylib mid-session, __vm-boot will surface
        // a `libkrunfw: load` error which is diagnostic enough.
        if !PROVISION_DONE.load(Ordering::Acquire) {
            #[cfg(target_os = "macos")]
            {
                let t0 = Instant::now();
                if let Err(e) =
                    crate::cli::worker_manager::platform::ensure_macos_entitlements(&bin)
                {
                    tracing::warn!(error = %e, "failed to codesign iii-worker for Hypervisor entitlement");
                }
                tracing::info!(
                    ms = t0.elapsed().as_millis() as u64,
                    "boot_phase: codesign (first boot)"
                );
            }
        } else {
            tracing::debug!("boot_phase: codesign (skipped, cached)");
        }

        // Mirrors `worker_manager::libkrun::run_dev` arg surface. Flag-name
        // alignment is not cosmetic: VmBootArgs declares `vcpus` / `ram` /
        // `exec`, so the older `--cpus` / `--memory-mb` and missing
        // `--exec` caused clap to reject the args, the child to exit
        // instantly, and a 30s `shell.sock` wait that ended in an opaque
        // S300.
        //
        // Ensure `libkrunfw.<soname>` is discoverable by the loader.
        // Managed workers call this at startup from
        // worker_manager::libkrun and local_worker; the sandbox path
        // skips it entirely. When iii-worker ships with embed-libkrunfw,
        // this extracts the embedded bytes to ~/.iii/lib/ on first use;
        // otherwise it falls back to a GitHub-release download. Without
        // this step, dlopen(libkrunfw.X.dylib) fails inside the spawned
        // __vm-boot subprocess, libkrun's vm.enter() returns "build
        // error: libkrunfw: load", shell_relay binds shell.sock briefly
        // and then exits, and the caller sees the classic S300
        // "Connection refused" on the socket that should still be live.
        if !PROVISION_DONE.load(Ordering::Acquire) {
            let t_fw = Instant::now();
            // Hard-fail: without libkrunfw the child `__vm-boot` will
            // briefly bind shell.sock during init, then exit when
            // `vm.enter()` can't dlopen the dylib. Our outer wait loop
            // sees the stale socket and returns "boot OK", then the
            // next `sandbox::exec` hits Connection refused — a very
            // expensive way to surface a missing dependency. Bail now
            // with a real error the user can act on.
            crate::cli::firmware::download::ensure_libkrunfw()
                .await
                .map_err(|e| {
                    SandboxError::BootFailed(format!(
                        "ensure_libkrunfw failed (vm.enter would crash with \
                         dlopen error): {e}"
                    ))
                })?;
            tracing::info!(
                ms = t_fw.elapsed().as_millis() as u64,
                "boot_phase: ensure_libkrunfw (first boot)"
            );
            // Mark provisioning done only after libkrunfw is in place.
            // Codesign above is macOS-only and soft-failing; guarding
            // this flag on libkrunfw is the correctness-critical step.
            // With the hard-fail on libkrunfw, we never set this on a
            // partially-provisioned host.
            PROVISION_DONE.store(true, Ordering::Release);
        } else {
            tracing::debug!("boot_phase: ensure_libkrunfw (skipped, cached)");
        }

        // Self-heal `init.krun` on disk. For iii-worker built WITHOUT
        // --features embed-init, iii-filesystem's virtual passthrough
        // serves nothing for /init.krun, and vm_boot's pre-boot check
        // demands the file exist on the rootfs. For embed-init builds,
        // has_init() returns true and this block is a no-op.
        if !iii_filesystem::init::has_init() {
            let dest = params.rootfs.join("init.krun");
            if !dest.exists() {
                let src = crate::cli::firmware::download::ensure_init_binary()
                    .await
                    .map_err(|e| {
                        SandboxError::BootFailed(format!("ensure_init_binary failed: {e}"))
                    })?;
                std::fs::copy(&src, &dest).map_err(|e| {
                    SandboxError::BootFailed(format!("copy init.krun to {}: {e}", dest.display()))
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                }
            }
        }

        // `--control-sock` is what flips the in-VM iii-init into
        // supervisor mode so it binds `shell.sock`. Pairing the control
        // socket with the shell socket in the same sandbox dir keeps
        // reaper semantics simple.
        let control_sock = params.shell_sock.with_file_name("control.sock");

        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg("__vm-boot")
            .arg("--rootfs")
            .arg(&params.rootfs)
            .arg("--exec")
            .arg("/bin/sh")
            // Keep PID 1 alive; iii-init supervisor serves every
            // `sb.exec()` through `shell.sock` independently of PID 1's
            // foreground command.
            .arg("--arg")
            .arg("-c")
            .arg("--arg")
            .arg("exec sleep infinity")
            // `params.workdir` is a host path (the sandbox's overlay
            // merged dir) and does NOT exist inside the VM. iii-init's
            // supervisor chdir's here before spawning PID-1, producing
            // `spawn_initial: No such file or directory (os error 2)`.
            // The VM's rootfs defines its own semantics; pass `/` as a
            // universally-valid cwd and let sandbox::exec requests carry
            // their own `cwd` when callers care.
            .arg("--workdir")
            .arg("/")
            .arg("--vcpus")
            .arg(params.cpus.to_string())
            .arg("--ram")
            .arg(params.memory_mb.to_string())
            .arg("--shell-sock")
            .arg(&params.shell_sock)
            .arg("--control-sock")
            .arg(&control_sock);

        if params.network {
            cmd.arg("--network");
        }
        for e in &params.env {
            cmd.arg("--env").arg(e);
        }

        // Capture stderr to a per-sandbox log. Dropping to /dev/null
        // masked clap parse errors and libkrun panics as opaque 30s
        // timeouts.
        let log_path = params.shell_sock.with_file_name("vm-boot.stderr.log");
        let stderr = std::fs::File::create(&log_path).map_err(|e| {
            SandboxError::BootFailed(format!(
                "cannot create vm-boot stderr log at {}: {e}",
                log_path.display()
            ))
        })?;

        let t_spawn = Instant::now();
        let child = cmd
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|e| SandboxError::BootFailed(format!("spawn iii-worker __vm-boot: {e}")))?;

        let vm_pid = child
            .id()
            .ok_or_else(|| SandboxError::BootFailed("child exited immediately".to_string()))?;

        // Detach — the child is the VM process; let it run independently.
        // Dropping `tokio::process::Child` with its default `kill_on_drop:
        // false` releases tokio's bookkeeping (fds + the background reap
        // future) without signaling the process. `sandbox::stop` later
        // sends SIGTERM/SIGKILL by `vm_pid` directly, so we don't need
        // the handle. (mem::forget here would leak the reap state
        // permanently, once per `sandbox::create`.)
        drop(child);
        tracing::info!(
            ms = t_spawn.elapsed().as_millis() as u64,
            pid = vm_pid,
            "boot_phase: spawn __vm-boot"
        );

        // Wait up to 30 s for the shell socket to appear AND accept a
        // connection. File-existence alone is not enough: the previous
        // implementation broke out of the loop the instant shell_relay
        // called `bind()`, before it had a chance to die on a missing
        // libkrunfw. That surfaced downstream as S300 "Connection
        // refused" on the next `sandbox::exec`.
        //
        // Three ways this loop exits:
        //   1. a test connect() succeeds — VM is live; return Ok.
        //   2. the child PID is no longer alive — VM died; bail with
        //      the tail of the stderr log so the user can see why.
        //   3. 30 s elapsed — bail with a useful error.
        let t_sock = Instant::now();
        let sock = params.shell_sock.clone();
        let deadline = Instant::now() + BOOT_SOCKET_TIMEOUT;
        loop {
            if sock.exists() {
                // Try to connect. If the listener is actually
                // accepting, we're done.
                match tokio::net::UnixStream::connect(&sock).await {
                    Ok(_stream) => break,
                    // Connection refused = socket file present but no
                    // listener, likely because the relay bound then
                    // crashed. Keep looping only if the child PID is
                    // still alive; otherwise bail fast.
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                        if !pid_alive(vm_pid) {
                            let hint = read_stderr_tail(&log_path);
                            return Err(SandboxError::BootFailed(format!(
                                "__vm-boot child {vm_pid} exited before the shell relay \
                                 accepted connections. Last stderr lines:\n{hint}"
                            )));
                        }
                    }
                    // Any other connect error is unusual — treat it
                    // the same as a timeout so the caller sees a
                    // specific error instead of an opaque hang.
                    Err(e) => {
                        let hint = read_stderr_tail(&log_path);
                        return Err(SandboxError::BootFailed(format!(
                            "connect({}) failed unexpectedly: {e}. Last stderr:\n{hint}",
                            sock.display()
                        )));
                    }
                }
            } else if !pid_alive(vm_pid) {
                let hint = read_stderr_tail(&log_path);
                return Err(SandboxError::BootFailed(format!(
                    "__vm-boot child {vm_pid} exited before binding shell socket {}. \
                     Last stderr lines:\n{hint}",
                    sock.display()
                )));
            }
            if Instant::now() >= deadline {
                let hint = read_stderr_tail(&log_path);
                return Err(SandboxError::BootFailed(format!(
                    "shell socket {} did not start accepting within {:?}. Last stderr:\n{hint}",
                    sock.display(),
                    BOOT_SOCKET_TIMEOUT
                )));
            }
            tokio::time::sleep(BOOT_SOCKET_POLL_INTERVAL).await;
        }
        tracing::info!(
            ms = t_sock.elapsed().as_millis() as u64,
            total_ms = t_boot_start.elapsed().as_millis() as u64,
            "boot_phase: shell_sock_wait (boot total)"
        );

        Ok(BootHandle { vm_pid })
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` with signum 0 does not deliver a signal —
    // it only performs error checking. No side effects on the target.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    // Non-unix fallback: trust the timeout loop. `tokio::process` is
    // unix-only today anyway.
    true
}

fn read_stderr_tail(log_path: &std::path::Path) -> String {
    const TAIL_LINES: usize = 32;
    const MAX_BYTES: usize = 4096;
    let content = match std::fs::read_to_string(log_path) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return format!("  (stderr log at {} is empty)", log_path.display()),
        Err(e) => return format!("  (could not read stderr log {}: {e})", log_path.display()),
    };
    let trimmed: String = content
        .lines()
        .rev()
        .take(TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    // Cap size so a runaway log doesn't blow up the error message.
    if trimmed.len() > MAX_BYTES {
        let start = trimmed.len() - MAX_BYTES;
        format!("  ...(truncated)\n{}", &trimmed[start..])
    } else {
        trimmed
    }
}

/// Uses `tokio::task::spawn_blocking` + an inner `current_thread`
/// runtime so the `!Send` `&mut dyn OutputSink` reference never crosses
/// the outer multi-thread runtime's Send boundary.
pub struct ShellProtoRunner;

#[async_trait::async_trait]
impl ShellRunner for ShellProtoRunner {
    async fn run(&self, sock: PathBuf, req: &ExecRequest) -> Result<ExecResponse, SandboxError> {
        let cmd = req.cmd.clone();
        let args = req.args.clone();
        let env = req.env.clone();
        let cwd = req.workdir.clone();
        let timeout_ms = req.timeout_ms.unwrap_or(DEFAULT_EXEC_TIMEOUT_MS);

        let stdin: Option<Vec<u8>> = match &req.stdin {
            Some(s) if !s.is_empty() => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(s.as_bytes())
                    .map_err(|e| {
                        SandboxError::InvalidRequest(format!("stdin base64 decode: {e}"))
                    })?;
                Some(bytes)
            }
            _ => None,
        };

        let join: Result<ExecResponse, SandboxError> = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| SandboxError::BootFailed(format!("inner runtime build: {e}")))?;
            rt.block_on(async move {
                let started = Instant::now();

                let t_connect = Instant::now();
                let session = Session::connect(&sock)
                    .await
                    .map_err(|e| SandboxError::BootFailed(format!("shell connect: {e}")))?;
                tracing::info!(
                    ms = t_connect.elapsed().as_millis() as u64,
                    "exec_phase: shell_connect"
                );

                let spec = RequestSpec {
                    cmd,
                    args,
                    cwd,
                    env,
                    stdin,
                };

                let mut sink = VecSink::with_cap(OUTPUT_CAP);

                // Use `tokio::time::timeout` as the SOLE timeout
                // authority and pass `None` to `session.run` so
                // iii-shell-client does not add its own post-timeout
                // kill+grace tail (WRITE_TIMEOUT 5s + POST_KILL_GRACE
                // 1s at iii-shell-client/src/lib.rs:69,79). Without
                // this, a timed-out exec holds `exec_in_progress` on
                // the registry for up to ~1.5s after the user's
                // deadline, causing every subsequent sandbox::exec
                // on the same handle to return S003 ("concurrent
                // exec"). When the outer timeout fires, the
                // `session.run` future is dropped; its UnixStream
                // drops; the host-side socket closes; the relay
                // tears down its virtio-console view. `handle_exec`
                // then calls `end_exec` immediately, freeing the
                // sandbox for the next request.
                let t_run = Instant::now();
                let outer = tokio::time::timeout(
                    Duration::from_millis(timeout_ms),
                    session.run(spec, &mut sink, None),
                )
                .await;
                tracing::info!(
                    ms = t_run.elapsed().as_millis() as u64,
                    "exec_phase: shell_run (in-VM)"
                );

                let outcome = match outer {
                    Ok(Ok(o)) => o,
                    Ok(Err(e)) => {
                        return Err(SandboxError::BootFailed(format!("shell run: {e}")));
                    }
                    Err(_) => {
                        // Timeouts are expected outcomes of
                        // sandbox::exec (user commands can legitimately
                        // exceed the deadline). Kept at info because
                        // returning S200 already signals the caller —
                        // this log is only for operator awareness,
                        // not an error.
                        tracing::info!(timeout_ms, "exec timed out; session dropped, forcing S200");
                        return Err(SandboxError::ExecTimedOut { timeout_ms });
                    }
                };

                let duration_ms = started.elapsed().as_millis() as u64;
                let timed_out = outcome.status.timed_out;
                let exit_code = outcome.status.code;
                let success = exit_code == Some(0) && !timed_out;

                // `Session::run` can still surface timed_out=true if
                // iii-shell-client decides the stream hit some other
                // deadline; map it to S200 for caller parity.
                if timed_out {
                    return Err(SandboxError::ExecTimedOut { timeout_ms });
                }

                Ok(ExecResponse {
                    stdout: String::from_utf8_lossy(&sink.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&sink.stderr).into_owned(),
                    exit_code,
                    timed_out,
                    duration_ms,
                    success,
                })
            })
        })
        .await
        .map_err(|e| SandboxError::BootFailed(format!("spawn_blocking join: {e}")))?;

        join
    }
}

pub struct SignalStopper;

#[async_trait::async_trait]
impl VmStopper for SignalStopper {
    async fn stop(&self, vm_pid: u32) -> Result<(), SandboxError> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let t_stop = Instant::now();
        let pid = Pid::from_raw(vm_pid as i32);

        // SIGTERM first so iii-init/libkrun can close cleanly when they
        // do respond. The grace here is intentionally short: the sandbox
        // VM's upper layer is tmpfs and discarded on reap, so "allow
        // flush" isn't a real use case. Every millisecond of grace is
        // visible in `sandbox run` end-to-end latency.
        let _ = kill(pid, Signal::SIGTERM);

        tokio::time::sleep(Duration::from_millis(STOP_GRACE_MS)).await;

        let _ = kill(pid, Signal::SIGKILL);

        tracing::info!(
            ms = t_stop.elapsed().as_millis() as u64,
            pid = vm_pid,
            "stop_phase: SIGTERM+grace+SIGKILL"
        );
        Ok(())
    }
}
