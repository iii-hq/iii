//! `ListRequest` is kept as a struct (rather than `()`) so older
//! clients that send `{"show_all": ...}` or similar fields continue to
//! deserialize cleanly. serde ignores unknown fields by default; no
//! wire-compat break.

use crate::sandbox_daemon::registry::SandboxRegistry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListRequest {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SandboxSummary {
    pub sandbox_id: String,
    pub name: Option<String>,
    pub image: String,
    pub age_secs: u64,
    /// Any exec running. NOTE: this no longer implies the next exec will be
    /// refused — a sandbox admits `max_concurrent_exec_per_sandbox` at once.
    /// Use `exec_in_flight` / `exec_slots_free` for admission decisions; a
    /// caller that polls this waiting for `false` will wait forever against a
    /// resident process while slots sit idle.
    pub exec_in_progress: bool,
    /// Execs running right now.
    pub exec_in_flight: u32,
    /// Execs this sandbox would admit before returning S003.
    pub exec_slots_free: u32,
    pub stopped: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResponse {
    pub sandboxes: Vec<SandboxSummary>,
}

pub async fn handle_list(_req: ListRequest, registry: &SandboxRegistry) -> ListResponse {
    let all = registry.list().await;
    let max_exec = registry.max_exec_in_flight();
    let now = std::time::Instant::now();
    let sandboxes = all
        .into_iter()
        .map(|s| SandboxSummary {
            sandbox_id: s.id.to_string(),
            // Wire field stays a bool; the record behind it is now a count.
            exec_in_progress: s.exec_in_progress(),
            exec_in_flight: s.exec_in_flight,
            exec_slots_free: max_exec.saturating_sub(s.exec_in_flight),
            age_secs: now.saturating_duration_since(s.created_at).as_secs(),
            name: s.name,
            image: s.image,
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

    use uuid::Uuid;

    fn make(id: Uuid) -> SandboxState {
        let mut s = crate::sandbox_daemon::registry::sandbox_state_for_test(id);
        s.rootfs = PathBuf::new();
        s.workdir = PathBuf::new();
        s.shell_sock = PathBuf::new();
        s
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
