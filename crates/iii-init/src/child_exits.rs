// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Cross-module coordination for child process exits.
//!
//! iii-init is PID 1 and reaps every exited process via `waitpid(-1)`
//! inside `supervisor::run_legacy` / `supervisor::run_supervised`.
//! Without coordination, that loop consumes exit codes belonging to
//! **other** subsystems in iii-init — notably `shell_dispatcher`,
//! which spawns its own short-lived children for `iii worker exec`
//! and needs their exit statuses to emit terminal `Exited` frames.
//!
//! Design: a process-wide registry of `pid → mpsc::Sender<i32>`.
//! Owners (currently only `shell_dispatcher`) register a sender for
//! each pid they spawn; PID 1's reap loop calls
//! [`dispatch_exit`] after every `waitpid` observation. If the pid is
//! registered, its exit code is forwarded to the owner and removed
//! from the map — the caller should treat the exit as "handled" and
//! not apply its own termination logic. If the pid is unknown, PID 1
//! keeps its existing semantics (main-worker exit → terminate, else
//! → orphan reap).
//!
//! Race window: the owner must register **before** the spawn returns,
//! otherwise a fast-exiting child can be reaped by PID 1 before the
//! owner inserts its sender. Callers do this by holding the registry
//! lock for the span `spawn + insert` via [`register_spawn`].

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use std::sync::mpsc::{SyncSender, sync_channel};

/// Fixed-capacity channel used for one exit code delivery. Capacity 1
/// so the PID-1 reap loop never blocks when a waiter thread hasn't
/// received yet — the exit code sits buffered until the waiter picks
/// it up.
type ExitSender = SyncSender<i32>;

/// Process-wide registry. Lazily initialized on first touch.
static REGISTRY: OnceLock<Mutex<HashMap<u32, ExitSender>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u32, ExitSender>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register ownership of a child's exit. Returns a receiver that
/// produces the exit code exactly once, when PID 1's reap loop
/// observes the child exiting.
///
/// Use [`register_spawn`] instead when spawning — it closes the race
/// window by holding the lock across the spawn.
pub fn register(pid: u32) -> std::sync::mpsc::Receiver<i32> {
    let (tx, rx) = sync_channel::<i32>(1);
    registry()
        .lock()
        .expect("child_exits registry mutex poisoned")
        .insert(pid, tx);
    rx
}

/// Race-free registration around a spawn. The caller's closure runs
/// with the registry lock held; its return value (pid) is inserted
/// into the map before the lock is released, so PID 1 cannot observe
/// and discard the child's exit before we've claimed it.
///
/// `spawn_fn` must return the child pid (or `None` to indicate spawn
/// failed — in which case no entry is inserted and `None` is
/// returned).
pub fn register_spawn<F>(spawn_fn: F) -> Option<(u32, std::sync::mpsc::Receiver<i32>)>
where
    F: FnOnce() -> Option<u32>,
{
    let mut guard = registry()
        .lock()
        .expect("child_exits registry mutex poisoned");
    let pid = spawn_fn()?;
    let (tx, rx) = sync_channel::<i32>(1);
    guard.insert(pid, tx);
    Some((pid, rx))
}

/// Called from PID 1's reap loop after every `waitpid` observation.
/// Returns `true` if the pid was registered and its exit code was
/// dispatched — the caller should treat the exit as handled and not
/// apply its own termination logic.
///
/// Returns `false` for unknown pids; the caller falls back to its
/// existing behavior (main-worker exit → terminate, else → orphan).
pub fn dispatch_exit(pid: i32, code: i32) -> bool {
    let Ok(pid_u32) = u32::try_from(pid) else {
        return false;
    };
    let mut guard = registry()
        .lock()
        .expect("child_exits registry mutex poisoned");
    if let Some(tx) = guard.remove(&pid_u32) {
        // `try_send` would avoid blocking but the capacity-1 channel
        // shouldn't ever be full — we only send one value per pid.
        let _ = tx.send(code);
        true
    } else {
        false
    }
}

/// Best-effort unregister for the case where a caller abandons a
/// session without waiting. Used by `shell_dispatcher::spawn_tty_session`
/// when post-spawn setup (master fd clone) fails: we've already
/// registered the pid, so we must remove the entry before letting PID
/// 1's reap loop observe the exit, otherwise a waiterless sender would
/// silently buffer the exit code in the channel.
pub fn unregister(pid: u32) {
    let _ = registry()
        .lock()
        .expect("child_exits registry mutex poisoned")
        .remove(&pid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_dispatch_roundtrip() {
        let rx = register(12345);
        assert!(dispatch_exit(12345, 7));
        assert_eq!(rx.recv().unwrap(), 7);
    }

    #[test]
    fn dispatch_unknown_pid_returns_false() {
        assert!(!dispatch_exit(99999, 0));
    }

    #[test]
    fn second_dispatch_for_same_pid_is_noop() {
        let _rx = register(54321);
        assert!(dispatch_exit(54321, 0));
        assert!(!dispatch_exit(54321, 0));
    }
}
