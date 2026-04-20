// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Child-process lifecycle for the in-VM supervisor.
//!
//! Holds a shared handle to the currently-running worker subprocess.
//! Spawns it, kills it, respawns it. Nothing else. Signal handling and
//! control-channel decoding live in sibling modules.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Acquire the state lock without poisoning the whole VM on panic.
///
/// `Mutex::lock()` returns `Err` when a previous holder panicked while
/// the guard was live. `.expect(...)` turns that into a fresh panic,
/// which on PID-1 terminates the VM. The state behind the mutex is
/// small (`Child` handle + restart counter) and the supervisor is
/// already designed to tolerate out-of-band child reaping, so recovering
/// the inner data is strictly better than crashing: at worst we observe
/// partially-updated state for one RPC, which the host either retries
/// or escalates to a full VM restart.
fn lock_or_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => {
            tracing::error!("inner mutex was poisoned; recovering");
            poisoned.into_inner()
        }
    }
}

/// Grace period between SIGTERM and SIGKILL when cycling the worker.
///
/// 500ms is the sweet spot for a dev-loop restart:
/// - Workers that install a SIGTERM handler (most modern runtimes —
///   Node, Python with uvicorn/fastapi lifecycle hooks, Go's signal.Notify)
///   clean up and exit in 10–100ms. The poll loop catches them almost
///   immediately, so there's no observable latency cost for well-behaved
///   workers.
/// - Workers that don't install a handler get the default SIGTERM
///   behavior (immediate termination with status 15), also caught fast.
/// - Workers that ignore SIGTERM entirely (e.g. stuck in a tight loop,
///   or buggy handler that swallows signals) hit the 500ms deadline and
///   get SIGKILL. 500ms is tight enough that the dev loop doesn't feel
///   sluggish even in the pathological case.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// How often to check whether the child exited after SIGTERM. 10ms keeps
/// the poll overhead negligible (a try_wait is a single waitpid(WNOHANG)
/// syscall, ~µs) while giving up to 50 chances to catch a fast exit
/// before escalating to SIGKILL.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Delay between the initial spawn attempt and the retry in
/// `spawn_with_one_retry`. Short enough that the host-side
/// `supervisor_ctl` 500ms read timeout does not fire while a single
/// transient EAGAIN/ENOMEM is being absorbed, long enough that the
/// kernel has a fair chance to free a fork-table slot.
const RESPAWN_RETRY_DELAY: Duration = Duration::from_millis(50);

/// Configuration captured once at supervisor startup. Immutable after.
#[derive(Debug, Clone)]
pub struct Config {
    /// Shell command line to run as the user's worker. Executed via
    /// `/bin/sh -c` so the user's existing `iii.worker.yaml` run_cmd
    /// (e.g. `npm run dev`, `uvicorn app:main`) works verbatim.
    pub run_cmd: String,
    /// Working directory for the child. `/workspace` for local-path
    /// workers.
    pub workdir: String,
}

/// Mutable supervisor state protected by a single mutex.
#[derive(Debug)]
struct Inner {
    child: Option<Child>,
    restarts: u32,
}

/// Shareable handle to the supervisor's process state. Cheap to clone.
#[derive(Clone, Debug)]
pub struct State {
    config: Config,
    inner: Arc<Mutex<Inner>>,
}

impl State {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            inner: Arc::new(Mutex::new(Inner {
                child: None,
                restarts: 0,
            })),
        }
    }

    /// Spawn the worker subprocess for the first time. Call once during
    /// supervisor boot, before entering the control loop. Returns an
    /// error if spawn fails; supervisor should exit in that case so the
    /// host can observe the VM going down and fall back.
    ///
    /// Intentionally does NOT retry on transient errors — boot-time
    /// spawn failure already escalates to the host by way of the
    /// supervisor exiting, and the host's full-VM restart path provides
    /// its own retry layer. Masking boot-time EAGAIN here would hide a
    /// real "wrong run_cmd / workdir / binary missing" class of bug
    /// under a 50ms stall. The restart-time path
    /// (`kill_and_respawn` → `spawn_with_one_retry`) retries because
    /// the cost of escalation — a full VM teardown that breaks the
    /// dev-loop — is disproportionate to a single transient fork
    /// failure.
    pub fn spawn_initial(&self) -> anyhow::Result<u32> {
        let mut guard = lock_or_recover(&self.inner);
        let child = Self::spawn_child(&self.config)?;
        let pid = child.id();
        guard.child = Some(child);
        Ok(pid)
    }

    /// Kill the current child (if any) and spawn a fresh one with the
    /// same config. Increments the restart counter. Returns the new
    /// pid on success.
    ///
    /// Used from the control-channel handler when the host sends
    /// `Restart`. Idempotent on a dead child — if the child already
    /// exited, we still respawn cleanly.
    ///
    /// Termination is graceful: SIGTERM first, poll for exit up to
    /// [`SHUTDOWN_GRACE`], escalate to SIGKILL if the child is still
    /// alive. See [`terminate_gracefully`] for the rationale.
    pub fn kill_and_respawn(&self) -> anyhow::Result<u32> {
        // Terminate the old child under the lock so no concurrent
        // reader observes a torn `Inner { child: Some(old), … }` state.
        {
            let mut guard = lock_or_recover(&self.inner);
            if let Some(mut old) = guard.child.take() {
                terminate_gracefully(&mut old);
            }
        }

        // Spawn the replacement WITHOUT holding the lock. The retry
        // path below sleeps up to `RESPAWN_RETRY_DELAY` between
        // attempts — if that sleep happened under the mutex, every
        // concurrent `Status`/`Ping`/`Restart` RPC would block for
        // the sleep duration. Since the host-side `supervisor_ctl`
        // has a 500ms read timeout, a chained retry under real
        // fork-table pressure could trip that timeout and escalate
        // to a full VM teardown — exactly the outcome the retry is
        // meant to avoid.
        let child = Self::spawn_with_one_retry(&self.config)?;
        let pid = child.id();

        // Re-acquire the lock to insert the new child. Because the
        // control loop is single-threaded and the only other writers
        // are `spawn_initial` (called once at boot) and
        // `kill_for_shutdown` (terminal), no racing writer can have
        // re-populated `child` in the window above. Assert that
        // invariant defensively: if we ever grow concurrent writers,
        // we'd rather fail loud here than silently leak the child we
        // just spawned.
        let mut guard = lock_or_recover(&self.inner);
        debug_assert!(
            guard.child.is_none(),
            "kill_and_respawn: racing writer re-populated child during respawn"
        );
        guard.child = Some(child);
        guard.restarts = guard.restarts.saturating_add(1);
        Ok(pid)
    }

    /// Spawn the worker with a single retry after `RESPAWN_RETRY_DELAY`.
    ///
    /// Retries transient spawn failures (EAGAIN under fork storms,
    /// ENOMEM under memory pressure). A persistent failure still
    /// propagates: the PID-1 reap loop sees `state.pid() == None`,
    /// marks the exit terminal, and the VM goes down so the host
    /// watcher can fall back to a full restart. Bounded retry lets a
    /// dev-loop edit survive brief fork-table exhaustion without a
    /// teardown cycle.
    ///
    /// Factored out of `kill_and_respawn` so the retry contract can
    /// be regression-tested without mocking the state machine (see
    /// `spawn_with_one_retry_retries_once_then_succeeds` and
    /// `spawn_with_one_retry_propagates_persistent_failure`).
    fn spawn_with_one_retry(config: &Config) -> anyhow::Result<Child> {
        match Self::spawn_child(config) {
            Ok(c) => Ok(c),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "spawn_child failed during restart; retrying once after {}ms",
                    RESPAWN_RETRY_DELAY.as_millis()
                );
                std::thread::sleep(RESPAWN_RETRY_DELAY);
                Self::spawn_child(config).map_err(|e2| {
                    tracing::error!(
                        error = %e2,
                        "spawn_child retry also failed; VM will exit and host will full-restart"
                    );
                    e2
                })
            }
        }
    }

    /// Kill the current child, do NOT respawn, mark supervisor as
    /// shutting down. Caller should then return from the control loop
    /// so `main` exits 0, which triggers the VM's poweroff path.
    ///
    /// Same SIGTERM-then-SIGKILL escalation as `kill_and_respawn`;
    /// dev-time shutdowns should still give the worker a chance to
    /// flush stdio and close sockets before hard-killing it.
    pub fn kill_for_shutdown(&self) -> anyhow::Result<()> {
        let mut guard = lock_or_recover(&self.inner);
        if let Some(mut old) = guard.child.take() {
            terminate_gracefully(&mut old);
        }
        Ok(())
    }

    /// Current child pid, if alive. `None` during the restart window or
    /// after an unexpected child exit that `kill_and_respawn` hasn't yet
    /// been called to recover from.
    pub fn pid(&self) -> Option<u32> {
        let mut guard = lock_or_recover(&self.inner);
        // Check if the stored child has died on its own. `try_wait`
        // non-destructively reaps dead children so we don't report a
        // stale pid. On `Err(ECHILD)` the child was reaped by someone
        // else (typically the PID-1 `waitpid(-1)` loop in supervisor
        // mode) — the handle is stale and the kernel may have already
        // recycled the pid, so returning `child.id()` would lie about
        // liveness (and could coincide with an unrelated same-uid
        // process). Treat any try_wait error as "handle is stale" and
        // drop it.
        if let Some(child) = guard.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    guard.child = None;
                    return None;
                }
                Ok(None) => return Some(child.id()),
                Err(_) => {
                    guard.child = None;
                    return None;
                }
            }
        }
        None
    }

    /// Total restart count since supervisor boot.
    pub fn restarts(&self) -> u32 {
        lock_or_recover(&self.inner).restarts
    }

    fn spawn_child(config: &Config) -> anyhow::Result<Child> {
        use std::os::unix::process::CommandExt;
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&config.run_cmd)
            .current_dir(&config.workdir)
            // Inherit stdio directly — the supervisor's stdout/stderr
            // are piped to the VM's console, which goes to the host's
            // ~/.iii/logs/<name>/stdout.log. Passing through preserves
            // log ordering without userspace copying.
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            // Put the child in its own process group (setpgid(0, 0) in
            // the fork'd child before execve). `terminate_gracefully`
            // uses this pgid to killpg(2) the *entire* worker subtree —
            // npm, shells, tsx, node, esbuild, whatever — in one signal.
            //
            // Without process-group isolation, descendants that don't
            // propagate SIGTERM (npm is famous for this) or that sit
            // under a non-exec'd wrapper shell end up orphaned to PID 1
            // on `kill_and_respawn` and keep running alongside the new
            // worker. The engine then sees two worker_ids registering
            // the same function name and logs
            //   "Function ... is already registered. Overwriting."
            // while the old tsx keeps serving pre-edit code — the exact
            // "reload does nothing" symptom in the bug report.
            //
            // This makes the double-wrapping of `/bin/sh -c …` inside
            // `III_WORKER_CMD` cosmetic rather than correctness-critical:
            // regardless of how many sh layers sit between the group
            // leader and the eventual worker, killpg torches the whole
            // tree. Simplifying the wrapper shape is still worthwhile
            // for ps-output clarity — tracked as a follow-up in
            // crates/iii-worker/src/cli/vm_boot.rs (build_worker_cmd).
            .process_group(0)
            .spawn()?;
        Ok(child)
    }
}

/// Send SIGTERM to the worker process group, poll for the leader to
/// exit up to [`SHUTDOWN_GRACE`], then SIGKILL the group if the leader
/// is still alive. Always reaps the leader before returning so no zombie
/// is left behind for the PID-1 waitpid loop to clean up.
///
/// `killpg` instead of `kill` because `spawn_child` puts each worker in
/// its own process group (`Command::process_group(0)`). Signalling the
/// whole group kills npm, its forked dev script shell, tsx, node,
/// esbuild — the entire worker subtree — in one shot. The old
/// `kill(pid, …)` version only signalled the direct child and leaked
/// every descendant on restart, which is what produced two coexisting
/// `npm run dev` trees and duplicate engine registrations.
///
/// SIGTERM first so worker shutdown hooks (SIGTERM handlers, atexit
/// callbacks, async runtime graceful-shutdown paths) get a chance to
/// run. SIGKILL is the safety net for workers that ignore SIGTERM.
///
/// If the signal fails with ESRCH the group is already gone — we still
/// need to reap the leader's zombie via `wait`.
fn terminate_gracefully(child: &mut Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    // If the child was already reaped out-of-band (PID-1 waitpid(-1)
    // loop, another wait, etc.), `child.id()` still returns the stale
    // PID — which the kernel may have recycled into a totally unrelated
    // process group by now. Signalling it would hit an innocent
    // bystander. Check liveness first and bail if we've lost the right
    // to signal.
    match child.try_wait() {
        Ok(Some(_)) => return, // already exited + reaped
        Err(_) => return,      // ECHILD → reaped elsewhere
        Ok(None) => {}         // still running, proceed
    }

    // Leader's pid == pgid (set by `process_group(0)` in spawn_child).
    let pgid = Pid::from_raw(child.id() as i32);
    // Ignore the result — ESRCH (group already gone) is benign, and
    // EPERM shouldn't happen for our own group. Any failure just means
    // we skip straight to reaping below.
    let _ = killpg(pgid, Signal::SIGTERM);

    let deadline = Instant::now() + SHUTDOWN_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return, // leader exited + reaped by try_wait
            Ok(None) => {
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(SHUTDOWN_POLL_INTERVAL);
            }
            Err(_) => return, // already reaped elsewhere (e.g. PID-1 waitpid)
        }
    }

    // Grace exhausted — wipe the whole group with SIGKILL, then reap
    // the leader's Rust handle. Group members are reaped by the PID-1
    // waitpid(-1) loop.
    let _ = killpg(pgid, Signal::SIGKILL);
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn sleep_config() -> Config {
        Config {
            run_cmd: "sleep 5".to_string(),
            workdir: "/tmp".to_string(),
        }
    }

    #[test]
    fn spawn_initial_reports_live_pid() {
        let state = State::new(sleep_config());
        let pid = state.spawn_initial().expect("spawn");
        assert!(pid > 0);
        assert_eq!(state.pid(), Some(pid));
        assert_eq!(state.restarts(), 0);
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn kill_and_respawn_changes_pid_and_bumps_counter() {
        let state = State::new(sleep_config());
        let pid1 = state.spawn_initial().unwrap();
        let pid2 = state.kill_and_respawn().unwrap();
        assert_ne!(pid1, pid2, "pid must change on respawn");
        assert_eq!(state.restarts(), 1);
        let pid3 = state.kill_and_respawn().unwrap();
        assert_ne!(pid2, pid3);
        assert_eq!(state.restarts(), 2);
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn kill_for_shutdown_clears_pid() {
        let state = State::new(sleep_config());
        state.spawn_initial().unwrap();
        state.kill_for_shutdown().unwrap();
        assert_eq!(state.pid(), None);
    }

    #[test]
    fn pid_none_after_child_exits_on_its_own() {
        // Child that exits immediately: supervisor should detect the
        // exit via try_wait and report None rather than a stale pid.
        let state = State::new(Config {
            run_cmd: "true".to_string(),
            workdir: "/tmp".to_string(),
        });
        state.spawn_initial().unwrap();
        // Give the child a moment to exit.
        thread::sleep(Duration::from_millis(100));
        assert_eq!(state.pid(), None, "exited child must not report a pid");
    }

    #[test]
    fn pid_clears_handle_when_child_reaped_out_of_band() {
        // Regression for the stale-dead-pid bug: in supervisor mode the
        // PID-1 `waitpid(-1)` loop reaps the child before `State::pid()`
        // is queried. `try_wait()` on a reaped child returns `Err(ECHILD)`;
        // we must treat that as "handle is stale" and return None,
        // rather than falling through to `child.id()` and reporting a
        // pid the kernel may have already recycled.
        let state = State::new(Config {
            run_cmd: "true".to_string(),
            workdir: "/tmp".to_string(),
        });
        let pid = state.spawn_initial().unwrap();
        // Reap the child from the outside so the stored Rust handle
        // becomes stale — mirrors what the PID-1 waitpid(-1) loop does.
        use nix::sys::wait::{WaitPidFlag, waitpid};
        use nix::unistd::Pid;
        let deadline = std::time::Instant::now() + Duration::from_millis(1000);
        loop {
            match waitpid(Pid::from_raw(pid as i32), Some(WaitPidFlag::WNOHANG)) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) => {}
                Ok(_) => break,
                Err(nix::Error::ECHILD) => break,
                Err(e) => panic!("waitpid: {e}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child {pid} never exited"
            );
            thread::sleep(Duration::from_millis(10));
        }
        // Now the internal Child handle references a reaped pid. pid()
        // must not lie by returning the stale value.
        assert_eq!(
            state.pid(),
            None,
            "stale reaped child must not report a live pid"
        );
    }

    #[test]
    fn spawn_with_one_retry_returns_ok_without_sleeping_on_happy_path() {
        // When the first spawn succeeds the helper must not sleep at
        // all. A regression that unconditionally slept would double the
        // dev-loop restart latency.
        let start = Instant::now();
        let mut child = State::spawn_with_one_retry(&sleep_config()).unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < RESPAWN_RETRY_DELAY / 2,
            "happy path must not sleep, elapsed = {:?}",
            elapsed
        );
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn spawn_with_one_retry_retries_exactly_once_on_persistent_failure() {
        // Bad workdir makes `Command::spawn` fail at chdir-in-child
        // time: the stdlib's exec-failure pipe returns the errno from
        // the child before `.spawn()` returns, so the parent observes
        // a proper Err. Every attempt with this config fails the same
        // way, which lets us pin the retry contract:
        //   - exactly one retry (not zero, not two)
        //   - sleep of RESPAWN_RETRY_DELAY between attempts
        //   - final error propagates
        let bad = Config {
            run_cmd: "true".to_string(),
            workdir: "/nonexistent/__iii_supervisor_retry_test__".to_string(),
        };

        let start = Instant::now();
        let result = State::spawn_with_one_retry(&bad);
        let elapsed = start.elapsed();

        assert!(result.is_err(), "persistent spawn failure must propagate");
        assert!(
            elapsed >= RESPAWN_RETRY_DELAY,
            "persistent failure must sleep at least the retry delay, elapsed = {:?}",
            elapsed
        );
        // Upper bound: well under 2 * delay so a refactor that loops
        // forever or adds a second retry trips this.
        assert!(
            elapsed < RESPAWN_RETRY_DELAY * 3,
            "persistent failure must NOT retry more than once, elapsed = {:?}",
            elapsed
        );
    }

    #[test]
    fn kill_and_respawn_is_idempotent_on_already_dead_child() {
        // Child exits immediately; we call kill_and_respawn against a
        // dead child. Should still spawn a fresh one without erroring.
        let state = State::new(Config {
            run_cmd: "true".to_string(),
            workdir: "/tmp".to_string(),
        });
        state.spawn_initial().unwrap();
        thread::sleep(Duration::from_millis(100));
        let pid_new = state.kill_and_respawn().expect("respawn from dead");
        assert!(pid_new > 0);
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn state_handle_is_clonable_and_shared() {
        let state = State::new(sleep_config());
        let pid = state.spawn_initial().unwrap();
        let clone = state.clone();
        assert_eq!(clone.pid(), Some(pid));
        state.kill_for_shutdown().unwrap();
        // The clone observes the shared state.
        assert_eq!(clone.pid(), None);
    }

    /// Give the child `sh` a moment to parse its script and install its
    /// `trap` handler. Without this delay, our SIGTERM may arrive at
    /// the fresh `sh` process before `trap '' TERM` has executed, and
    /// the default signal action takes over — which defeats the point
    /// of these tests. 100ms is overkill for shell startup (<10ms on
    /// any sane system) but keeps the tests robust under CI load.
    fn let_trap_install() {
        std::thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn terminate_respects_sigterm_from_cooperative_worker() {
        // A worker that installs a SIGTERM handler and exits cleanly
        // should be reaped inside the grace window, not force-killed.
        //
        // Bash quirk: a trap handler doesn't fire while the shell is
        // `wait`ing on a foreground child. Putting `sleep` in the
        // background and running the `wait` builtin lets SIGTERM run
        // the trap immediately, which is the realistic case — any
        // real worker that handles SIGTERM does so via its own signal
        // handler, not a trapped bash builtin wait.
        let state = State::new(Config {
            run_cmd: "trap 'exit 0' TERM; sleep 30 & wait".to_string(),
            workdir: "/tmp".to_string(),
        });
        state.spawn_initial().unwrap();
        let_trap_install();

        let start = std::time::Instant::now();
        let pid_new = state.kill_and_respawn().unwrap();
        let elapsed = start.elapsed();
        assert!(pid_new > 0);
        // A cooperative shell catches SIGTERM in <100ms easily.
        // Budget well under SHUTDOWN_GRACE (500ms) to prove we exit
        // on signal, not on grace-period timeout.
        assert!(
            elapsed < Duration::from_millis(300),
            "graceful shutdown took {:?}, expected <300ms",
            elapsed
        );
        state.kill_for_shutdown().unwrap();
    }

    #[test]
    fn terminate_escalates_to_sigkill_on_ignored_sigterm() {
        // A worker that ignores SIGTERM must still be terminated —
        // just after the grace window via SIGKILL.
        //
        // Because `terminate_gracefully` now killpg's the whole group,
        // a plain `trap '' TERM; sleep 30` no longer exercises the
        // escalation path: the foreground `sleep 30` gets SIGTERM
        // too, dies on its default handler, and the shell falls
        // through to exit. To actually force the escalation we need
        // every process in the group to ignore TERM — easiest via a
        // loop that re-spawns sleeps, so even if the current sleep
        // child dies from SIGTERM the shell immediately replaces it
        // and stays alive until SIGKILL razes the whole group.
        let state = State::new(Config {
            run_cmd: "trap '' TERM; while :; do sleep 1; done".to_string(),
            workdir: "/tmp".to_string(),
        });
        state.spawn_initial().unwrap();
        let_trap_install();

        let start = std::time::Instant::now();
        state.kill_and_respawn().unwrap();
        let elapsed = start.elapsed();
        // Must have waited out the grace, then SIGKILLed. Lower bound
        // is SHUTDOWN_GRACE; upper bound is a bit more to absorb poll
        // granularity + wait() latency.
        assert!(
            elapsed >= SHUTDOWN_GRACE,
            "escalation happened too fast: {:?}",
            elapsed
        );
        assert!(
            elapsed < SHUTDOWN_GRACE + Duration::from_millis(500),
            "escalation took way too long: {:?}",
            elapsed
        );
        state.kill_for_shutdown().unwrap();
    }

    /// Probe PID liveness without sending a real signal. `kill(pid, 0)`
    /// returns `Ok` iff the process exists and the caller has permission
    /// to signal it. For a dead-and-reaped pid the kernel returns
    /// `ESRCH`. Anything else (rare EPERM in practice) we conservatively
    /// treat as "alive" so the test fails loud rather than silent.
    fn pid_alive(pid: i32) -> bool {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        match kill(Pid::from_raw(pid), None) {
            Ok(()) => true,
            Err(nix::Error::ESRCH) => false,
            Err(_) => true,
        }
    }

    #[test]
    fn kill_and_respawn_kills_grandchildren_via_process_group() {
        // Regression guard for the "reload leaves two worker trees
        // coexisting" bug. Pre-fix, `terminate_gracefully` sent
        // `kill(pid, …)` to the direct sh child only; npm + its
        // tsx/node/esbuild grandchildren got orphaned to PID 1 on
        // cycle and kept running alongside the newly-spawned worker,
        // producing duplicate engine registrations and stale source.
        //
        // Shape here mirrors `npm run dev`: a parent shell that forks
        // a backgrounded "real worker" (sleep 30) and blocks on
        // `wait`. Post-fix, `spawn_child` puts the child in its own
        // process group and `terminate_gracefully` killpg's the whole
        // group — grandchild dies with parent. Pre-fix the grandchild
        // stayed alive and this assertion fired.
        use std::fs;

        let tmp = tempfile::tempdir().expect("tempdir");
        let pidfile = tmp.path().join("grandchild.pid");
        let run_cmd = format!("sleep 30 & echo $! > {}; wait", pidfile.display());

        let state = State::new(Config {
            run_cmd,
            workdir: tmp.path().to_string_lossy().into_owned(),
        });
        state.spawn_initial().unwrap();

        // Block until the backgrounded sleep has written its pid so
        // we're not racing the shell's startup.
        let deadline = std::time::Instant::now() + Duration::from_millis(2000);
        let grandchild_pid: i32 = loop {
            if let Ok(content) = fs::read_to_string(&pidfile)
                && let Ok(pid) = content.trim().parse::<i32>()
                && pid > 0
            {
                break pid;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "grandchild never wrote pidfile at {}",
                pidfile.display()
            );
            thread::sleep(Duration::from_millis(25));
        };

        assert!(
            pid_alive(grandchild_pid),
            "grandchild {grandchild_pid} should be alive before cycle"
        );

        // The cycle. Pre-fix: only the sh is killed, grandchild
        // orphaned to PID 1. Post-fix: killpg wipes the group.
        state.kill_and_respawn().unwrap();

        // Signal + reap window. SIGKILL delivery is synchronous but
        // zombie reaping by the system PID-1 init can take a tick.
        thread::sleep(Duration::from_millis(300));

        assert!(
            !pid_alive(grandchild_pid),
            "grandchild {grandchild_pid} must die when its group leader is cycled"
        );

        state.kill_for_shutdown().unwrap();
    }
}
