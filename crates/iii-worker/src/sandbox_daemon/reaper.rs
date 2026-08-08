//! Sandbox reaper. Background task that periodically scans the registry
//! and reaps sandboxes that crossed their idle-timeout.
//!
//! "Idle" means no activity of ANY daemon-visible kind: no exec
//! (`last_exec_at`, also bumped by fs::* ops) and no network payload. The
//! network signal arrives through the per-sandbox `net-activity` beacon file
//! (see `registry::net_activity_path`): the smoltcp stack lives in the
//! `__vm-boot` child process, and the beacon's mtime — refreshed there
//! whenever guest payload is relayed — is its only channel back to this
//! daemon. Before the beacon existed, a sandbox serving pure network traffic
//! was reaped mid-service (measured: 22s against a 20s timeout after 112
//! successful proxied calls).

use crate::sandbox_daemon::{
    overlay::OverlayLayout,
    registry::{SandboxRegistry, net_activity_path},
    stop::VmStopper,
};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Time since the sandbox's network beacon was last refreshed. `None` when
/// no beacon exists (networking off, old VM, or creation failed) — the
/// sandbox is then judged on exec-idleness alone. An mtime in the future
/// (host clock stepped) reads as "active now": the safe direction, deferring
/// the reap by at most one timeout rather than killing a live sandbox.
fn net_idle(shell_sock: &Path) -> Option<Duration> {
    let mtime = std::fs::metadata(net_activity_path(shell_sock))
        .ok()?
        .modified()
        .ok()?;
    Some(
        SystemTime::now()
            .duration_since(mtime)
            .unwrap_or(Duration::ZERO),
    )
}

/// Reap idle sandboxes.
///
/// `busy_grace` is how long past its idle timeout a sandbox with execs in
/// flight is protected. Busy is not idle — `last_exec_at` is stamped when an
/// exec STARTS and the default per-exec deadline equals the default idle
/// timeout, so without protection a long exec has its VM killed out from
/// under it the moment those cross. But the protection has to end: a leaked
/// `exec_in_flight` or a wedged child would otherwise pin the VM forever,
/// and the reaper is the only thing that reclaims one.
///
/// The network exemption carries no analogous bound on purpose: a beacon
/// mtime cannot leak the way a counter can. It stays fresh only while
/// payload actually moves, and a sandbox moving payload is doing work — the
/// same standing as one exec'ing in a loop.
pub async fn run_reaper_loop<S: VmStopper>(
    registry: SandboxRegistry,
    stopper: Arc<S>,
    scan_interval: Duration,
    busy_grace: Duration,
) {
    loop {
        tokio::time::sleep(scan_interval).await;
        let now = Instant::now();
        // Snapshot only to enumerate ids (shell_sock is immutable per
        // sandbox, so carrying it out of the snapshot is safe). Every
        // decision is re-made under the registry lock by
        // `try_claim_for_reap`, because this loop yields between entries
        // (each stop sleeps STOP_GRACE_MS, plus overlay cleanup I/O) and an
        // exec admitted in that window must not be killed.
        let candidates: Vec<_> = registry
            .list()
            .await
            .into_iter()
            .map(|s| (s.id, s.shell_sock))
            .collect();
        for (id, shell_sock) in candidates {
            let net_idle = net_idle(&shell_sock);
            let Some(state) = registry
                .try_claim_for_reap(id, now, busy_grace, net_idle)
                .await
            else {
                continue;
            };
            if state.exec_in_flight > 0 {
                tracing::warn!(
                    sandbox_id = %state.id,
                    exec_in_flight = state.exec_in_flight,
                    "reaping a sandbox still marked busy past its grace period — an exec \
                     slot leaked or a child is wedged; reclaiming rather than pinning the VM"
                );
            } else {
                tracing::info!(sandbox_id=%state.id, "reaping idle sandbox");
            }
            if let Some(pid) = state.vm_pid {
                let _ = stopper.stop(pid).await;
            }
            let _ = OverlayLayout::for_sandbox(state.id).cleanup();
            registry.remove(state.id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::{errors::SandboxError, registry::SandboxState};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use uuid::Uuid;

    struct CountingStopper(Arc<AtomicU32>);
    #[async_trait::async_trait]
    impl crate::sandbox_daemon::stop::VmStopper for CountingStopper {
        async fn stop(&self, _pid: u32) -> Result<(), SandboxError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn state(id: Uuid, idle_ago_secs: u64, timeout: u64) -> SandboxState {
        let past = Instant::now()
            .checked_sub(Duration::from_secs(idle_ago_secs))
            .unwrap_or(Instant::now());
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::new(),
            workdir: PathBuf::new(),
            // A per-id path whose net-activity sibling never exists, so the
            // beacon stat is deterministically None. `PathBuf::new()` would
            // make it a bare relative "net-activity" — CWD-dependent.
            shell_sock: std::env::temp_dir()
                .join(format!("iii-reaper-test-{id}"))
                .join("shell.sock"),
            vm_pid: Some(1),
            lifeline: None,
            created_at: past,
            last_exec_at: past,
            exec_in_flight: 0,
            idle_timeout_secs: timeout,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn reaps_idle_sandboxes() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state(id, 600, 300)).await; // 10 min idle, 5 min timeout
        let count = Arc::new(AtomicU32::new(0));
        let stopper = Arc::new(CountingStopper(count.clone()));
        let scan_interval = Duration::from_millis(10);
        let reg_clone = reg.clone();
        let task = tokio::spawn(async move {
            run_reaper_loop(reg_clone, stopper, scan_interval, Duration::from_secs(3600)).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    /// The reaper must not kill a sandbox that is mid-exec. `last_exec_at` is
    /// stamped when the exec STARTS and the default per-exec deadline equals
    /// the default idle timeout (both 300s), so a long exec crosses the line
    /// while still running. Nothing consulted the old in-progress bool.
    #[tokio::test]
    async fn does_not_reap_a_sandbox_with_an_exec_in_flight() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        // Idle far past its timeout, but busy.
        let mut busy = state(id, 600, 300);
        busy.exec_in_flight = 1;
        reg.insert(busy).await;

        let count = Arc::new(AtomicU32::new(0));
        let stopper = Arc::new(CountingStopper(count.clone()));
        let reg_clone = reg.clone();
        let task = tokio::spawn(async move {
            run_reaper_loop(
                reg_clone,
                stopper,
                Duration::from_millis(10),
                Duration::from_secs(3600),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();

        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "must not reap a busy sandbox"
        );
        assert!(reg.get(id).await.is_ok(), "the record must survive");
    }

    /// ...and once it finishes, the same sandbox is reapable again.
    #[tokio::test]
    async fn reaps_once_the_exec_finishes() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        let mut busy = state(id, 600, 300);
        busy.exec_in_flight = 1;
        reg.insert(busy).await;

        let count = Arc::new(AtomicU32::new(0));
        let stopper = Arc::new(CountingStopper(count.clone()));
        let reg_clone = reg.clone();
        let task = tokio::spawn(async move {
            run_reaper_loop(
                reg_clone,
                stopper,
                Duration::from_millis(10),
                Duration::from_secs(3600),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);

        // Drop the in-flight count without refreshing last_exec_at, so the
        // sandbox is immediately eligible.
        {
            let mut s = reg.get(id).await.unwrap();
            s.exec_in_flight = 0;
            reg.insert(s).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();
        assert_eq!(count.load(Ordering::SeqCst), 1, "reapable once idle again");
    }

    /// The busy exemption must END. A leaked `exec_in_flight` — a panic that
    /// skipped `end_exec`, a wedged child — would otherwise pin the VM
    /// forever, and the reaper is the only thing that reclaims one.
    #[tokio::test]
    async fn reaps_a_sandbox_stuck_busy_past_the_grace_period() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        // Idle 600s against a 300s timeout, and permanently "busy".
        let mut wedged = state(id, 600, 300);
        wedged.exec_in_flight = 1;
        reg.insert(wedged).await;

        let count = Arc::new(AtomicU32::new(0));
        let stopper = Arc::new(CountingStopper(count.clone()));
        let reg_clone = reg.clone();
        // Grace of 10s: 600s idle is well past 300 + 10.
        let task = tokio::spawn(async move {
            run_reaper_loop(
                reg_clone,
                stopper,
                Duration::from_millis(10),
                Duration::from_secs(10),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();

        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "a leaked exec slot must not pin the VM forever"
        );
        assert!(reg.get(id).await.is_err(), "record must be removed");
    }

    /// The gap this branch's PR originally shipped around, now closed: a
    /// sandbox serving only NETWORK traffic was reaped as idle (measured at
    /// 22s against a 20s timeout, after 112 successful proxied calls),
    /// because the smoltcp stack lives in `__vm-boot` and nothing
    /// daemon-visible recorded its activity. A fresh net-activity beacon
    /// must protect the sandbox; once the beacon goes stale past the
    /// timeout, the sandbox is genuinely idle and must be reclaimed.
    #[tokio::test]
    async fn network_activity_defers_reaping_until_the_beacon_goes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let shell_sock = dir.path().join("shell.sock");
        let beacon = crate::sandbox_daemon::registry::net_activity_path(&shell_sock);
        std::fs::write(&beacon, b"").unwrap(); // mtime = now: traffic is flowing

        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        // Exec-idle far past its timeout — only the beacon protects it.
        let mut s = state(id, 600, 300);
        s.shell_sock = shell_sock;
        reg.insert(s).await;

        let count = Arc::new(AtomicU32::new(0));
        let stopper = Arc::new(CountingStopper(count.clone()));
        let reg_clone = reg.clone();
        let task = tokio::spawn(async move {
            run_reaper_loop(
                reg_clone,
                stopper,
                Duration::from_millis(10),
                Duration::from_secs(3600),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "a network-active sandbox must not be reaped"
        );
        assert!(reg.get(id).await.is_ok(), "the record must survive");

        // Traffic stops: age the beacon past the idle timeout. No exec, no
        // network => genuinely idle => reclaimed.
        std::fs::File::options()
            .write(true)
            .open(&beacon)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "a stale beacon must not pin the VM"
        );
        assert!(reg.get(id).await.is_err(), "record must be removed");
    }

    /// ...but a sandbox busy *within* the grace window is still protected.
    #[tokio::test]
    async fn protects_a_busy_sandbox_inside_the_grace_window() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        // 310s idle against a 300s timeout — 10s over, inside a 600s grace.
        let mut busy = state(id, 310, 300);
        busy.exec_in_flight = 1;
        reg.insert(busy).await;

        let count = Arc::new(AtomicU32::new(0));
        let stopper = Arc::new(CountingStopper(count.clone()));
        let reg_clone = reg.clone();
        let task = tokio::spawn(async move {
            run_reaper_loop(
                reg_clone,
                stopper,
                Duration::from_millis(10),
                Duration::from_secs(600),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();

        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "still legitimately running"
        );
        assert!(reg.get(id).await.is_ok());
    }
}
