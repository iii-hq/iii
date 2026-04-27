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
            created_at: past,
            last_exec_at: past,
            exec_in_progress: false,
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
}
