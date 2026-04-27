//! `ListRequest` is kept as a struct (rather than `()`) so older
//! clients that send `{"show_all": ...}` or similar fields continue to
//! deserialize cleanly. serde ignores unknown fields by default; no
//! wire-compat break.

use crate::sandbox_daemon::registry::SandboxRegistry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
pub struct ListRequest {}

#[derive(Debug, Serialize)]
pub struct SandboxSummary {
    pub sandbox_id: String,
    pub name: Option<String>,
    pub image: String,
    pub age_secs: u64,
    pub exec_in_progress: bool,
    pub stopped: bool,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub sandboxes: Vec<SandboxSummary>,
}

pub async fn handle_list(_req: ListRequest, registry: &SandboxRegistry) -> ListResponse {
    let all = registry.list().await;
    let now = std::time::Instant::now();
    let sandboxes = all
        .into_iter()
        .map(|s| SandboxSummary {
            sandbox_id: s.id.to_string(),
            name: s.name,
            image: s.image,
            age_secs: now.saturating_duration_since(s.created_at).as_secs(),
            exec_in_progress: s.exec_in_progress,
            stopped: s.stopped,
        })
        .collect();
    ListResponse { sandboxes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::registry::SandboxState;
    use std::path::PathBuf;
    use std::time::Instant;
    use uuid::Uuid;

    fn make(id: Uuid) -> SandboxState {
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::new(),
            workdir: PathBuf::new(),
            shell_sock: PathBuf::new(),
            vm_pid: Some(1),
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_progress: false,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn list_returns_every_sandbox() {
        let reg = SandboxRegistry::new();
        reg.insert(make(Uuid::new_v4())).await;
        reg.insert(make(Uuid::new_v4())).await;
        let resp = handle_list(ListRequest::default(), &reg).await;
        assert_eq!(resp.sandboxes.len(), 2);
    }

    #[tokio::test]
    async fn empty_registry_returns_empty_list() {
        let reg = SandboxRegistry::new();
        let resp = handle_list(ListRequest::default(), &reg).await;
        assert!(resp.sandboxes.is_empty());
    }

    #[tokio::test]
    async fn ignores_unknown_request_fields() {
        // Wire-compat: older SDK / CLI clients that send `show_all` or
        // `owner_subscriber_id` must keep working. serde drops unknown
        // fields by default; this test pins that behavior so nobody
        // accidentally flips on `deny_unknown_fields`.
        let json = serde_json::json!({
            "show_all": true,
            "owner_subscriber_id": "legacy-caller"
        });
        let req: ListRequest = serde_json::from_value(json).expect("parse");
        let reg = SandboxRegistry::new();
        reg.insert(make(Uuid::new_v4())).await;
        let resp = handle_list(req, &reg).await;
        assert_eq!(resp.sandboxes.len(), 1);
    }
}
