//! In-memory registry of active sandboxes. Tracks UUID -> SandboxState
//! with an exec-in-progress flag for serialization.

use crate::sandbox_daemon::errors::SandboxError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SandboxState {
    pub id: Uuid,
    pub name: Option<String>,
    pub image: String,
    pub rootfs: PathBuf,
    pub workdir: PathBuf,
    pub shell_sock: PathBuf,
    pub vm_pid: Option<u32>,
    /// Write end of the VM's lifeline pipe (see `daemon_exit::Lifeline`).
    /// While any clone of this state is alive the VM keeps running; once the
    /// entry is dropped — or this whole daemon dies, however abruptly — the
    /// pipe closes and `__vm-boot` self-terminates.
    pub lifeline: Option<std::sync::Arc<crate::daemon_exit::Lifeline>>,
    pub created_at: Instant,
    pub last_exec_at: Instant,
    pub exec_in_progress: bool,
    pub idle_timeout_secs: u64,
    pub stopped: bool,
}

#[derive(Default, Clone)]
pub struct SandboxRegistry {
    inner: Arc<Mutex<HashMap<Uuid, SandboxState>>>,
}

impl SandboxRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, state: SandboxState) {
        let mut map = self.inner.lock().await;
        map.insert(state.id, state);
    }

    pub async fn get(&self, id: Uuid) -> Result<SandboxState, SandboxError> {
        let map = self.inner.lock().await;
        map.get(&id)
            .cloned()
            .ok_or_else(|| SandboxError::NotFound(id.to_string()))
    }

    /// Acquire exec slot (serialization). Returns S003 if another exec in flight.
    pub async fn begin_exec(&self, id: Uuid) -> Result<SandboxState, SandboxError> {
        let mut map = self.inner.lock().await;
        let state = map
            .get_mut(&id)
            .ok_or_else(|| SandboxError::NotFound(id.to_string()))?;
        if state.stopped {
            return Err(SandboxError::AlreadyStopped(id.to_string()));
        }
        if state.exec_in_progress {
            return Err(SandboxError::ConcurrentExec(id.to_string()));
        }
        state.exec_in_progress = true;
        state.last_exec_at = Instant::now();
        Ok(state.clone())
    }

    pub async fn end_exec(&self, id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(state) = map.get_mut(&id) {
            state.exec_in_progress = false;
            state.last_exec_at = Instant::now();
        }
    }

    pub async fn mark_stopped(&self, id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(state) = map.get_mut(&id) {
            state.stopped = true;
        }
    }

    pub async fn remove(&self, id: Uuid) -> Option<SandboxState> {
        let mut map = self.inner.lock().await;
        map.remove(&id)
    }

    pub async fn list(&self) -> Vec<SandboxState> {
        let map = self.inner.lock().await;
        map.values().cloned().collect()
    }

    pub async fn count(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Update `last_exec_at` to now. Called by fs::* trigger handlers (which
    /// don't use the exec-serialization guard) to keep the idle reaper from
    /// evicting a sandbox that is actively performing file operations.
    pub async fn bump_last_exec(&self, id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(state) = map.get_mut(&id) {
            state.last_exec_at = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixture(id: Uuid) -> SandboxState {
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::from("/tmp/rootfs"),
            workdir: PathBuf::from("/tmp/work"),
            shell_sock: PathBuf::from("/tmp/sock"),
            vm_pid: Some(1234),
            lifeline: None,
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_progress: false,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn insert_and_get() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let s = reg.get(id).await.unwrap();
        assert_eq!(s.id, id);
    }

    #[tokio::test]
    async fn get_missing_returns_s002() {
        let reg = SandboxRegistry::new();
        let err = reg.get(Uuid::new_v4()).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }

    #[tokio::test]
    async fn begin_exec_marks_in_progress() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let _ = reg.begin_exec(id).await.unwrap();
        let s = reg.get(id).await.unwrap();
        assert!(s.exec_in_progress);
    }

    #[tokio::test]
    async fn concurrent_begin_exec_returns_s003_with_actionable_recovery() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let _ = reg.begin_exec(id).await.unwrap();
        let err = reg.begin_exec(id).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S003");
        // The recovery hint must NOT tell the agent to merely "wait" — that
        // loops forever on a foreground server. It must name detach/stop and
        // flag the foreground case.
        let note = err
            .to_payload()
            .get("fix_note")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        assert!(
            note.contains("nohup") || note.contains("sandbox::stop"),
            "fix_note must name a real recovery (detach/stop), got: {note}"
        );
        assert!(
            note.to_lowercase().contains("foreground"),
            "fix_note must flag the foreground case, got: {note}"
        );
    }

    #[tokio::test]
    async fn end_exec_clears_flag() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let _ = reg.begin_exec(id).await.unwrap();
        reg.end_exec(id).await;
        let _ = reg.begin_exec(id).await.unwrap();
    }

    #[tokio::test]
    async fn stopped_sandbox_rejects_exec_with_s004() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        reg.mark_stopped(id).await;
        let err = reg.begin_exec(id).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S004");
    }
}
