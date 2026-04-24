use crate::sandbox_daemon::{
    errors::SandboxError, overlay::OverlayLayout, registry::SandboxRegistry,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct StopRequest {
    pub sandbox_id: String,
    #[serde(default)]
    pub wait: bool,
}

#[derive(Debug, Serialize)]
pub struct StopResponse {
    pub sandbox_id: String,
    pub stopped: bool,
}

#[async_trait::async_trait]
pub trait VmStopper: Send + Sync + 'static {
    async fn stop(&self, vm_pid: u32) -> Result<(), SandboxError>;
}

pub async fn handle_stop<S: VmStopper>(
    req: StopRequest,
    registry: &SandboxRegistry,
    stopper: &S,
) -> Result<StopResponse, SandboxError> {
    let id = Uuid::parse_str(&req.sandbox_id).map_err(|_| {
        SandboxError::InvalidRequest(format!(
            "sandbox_id is not a valid UUID: {}",
            req.sandbox_id
        ))
    })?;
    let state = registry.get(id).await?;
    if state.stopped {
        return Ok(StopResponse {
            sandbox_id: id.to_string(),
            stopped: true,
        });
    }
    registry.mark_stopped(id).await;
    if let Some(pid) = state.vm_pid {
        stopper.stop(pid).await?;
    }
    let _ = OverlayLayout::for_sandbox(id).cleanup();
    registry.remove(id).await;
    Ok(StopResponse {
        sandbox_id: id.to_string(),
        stopped: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::registry::SandboxState;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    struct FakeStopper {
        called: Arc<AtomicBool>,
    }
    #[async_trait::async_trait]
    impl VmStopper for FakeStopper {
        async fn stop(&self, _pid: u32) -> Result<(), SandboxError> {
            self.called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn state(id: Uuid) -> SandboxState {
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::from("/tmp/r"),
            workdir: PathBuf::from("/tmp/w"),
            shell_sock: PathBuf::from("/tmp/s"),
            vm_pid: Some(1234),
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_progress: false,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn stop_happy_path_marks_and_calls_stopper() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state(id)).await;
        let called = Arc::new(AtomicBool::new(false));
        let stopper = FakeStopper {
            called: called.clone(),
        };
        let resp = handle_stop(
            StopRequest {
                sandbox_id: id.to_string(),
                wait: true,
            },
            &reg,
            &stopper,
        )
        .await
        .unwrap();
        assert!(resp.stopped);
        assert!(called.load(Ordering::SeqCst));
        assert!(reg.get(id).await.is_err());
    }

    #[tokio::test]
    async fn stop_nonexistent_returns_s002() {
        let reg = SandboxRegistry::new();
        let stopper = FakeStopper {
            called: Arc::new(AtomicBool::new(false)),
        };
        let err = handle_stop(
            StopRequest {
                sandbox_id: Uuid::new_v4().to_string(),
                wait: false,
            },
            &reg,
            &stopper,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }
}
