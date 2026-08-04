//! Sandbox reaper. Background task that periodically scans the registry
//! and reaps sandboxes that crossed their idle-timeout.

use crate::sandbox_daemon::{overlay::OverlayLayout, registry::SandboxRegistry, stop::VmStopper};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub async fn run_reaper_loop<S: VmStopper>(
    registry: SandboxRegistry,
    stopper: Arc<S>,
    scan_interval: Duration,
) {
    loop {
        tokio::time::sleep(scan_interval).await;
        let now = Instant::now();
        let all = registry.list().await;
        for state in all {
            if state.stopped {
                continue;
            }
            // Busy is not idle. `last_exec_at` is stamped when an exec
            // STARTS, and the default per-exec deadline (300s) equals the
            // default idle timeout — so without this a long exec has its VM
            // killed out from under it the moment those cross. Nothing
            // consulted the old in-progress bool at all.
            if state.exec_in_flight > 0 {
                continue;
            }
            let idle = now.saturating_duration_since(state.last_exec_at);
            if idle > Duration::from_secs(state.idle_timeout_secs) {
                tracing::info!(sandbox_id=%state.id, "reaping idle sandbox");
                registry.mark_stopped(state.id).await;
                if let Some(pid) = state.vm_pid {
                    let _ = stopper.stop(pid).await;
                }
                let _ = OverlayLayout::for_sandbox(state.id).cleanup();
                registry.remove(state.id).await;
            }
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
            shell_sock: PathBuf::new(),
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
            run_reaper_loop(reg_clone, stopper, scan_interval).await;
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
            run_reaper_loop(reg_clone, stopper, Duration::from_millis(10)).await;
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
            run_reaper_loop(reg_clone, stopper, Duration::from_millis(10)).await;
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
}
