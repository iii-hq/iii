//! Events emitted by sandbox::create as it progresses from "pulling the
//! image" to "VM ready". Clients stream these if they want progress UX.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SandboxCreateEvent {
    PullingImage {
        image_ref: String,
        progress_bytes: u64,
        total_bytes: Option<u64>,
    },
    Unpacking,
    BootingVm,
    Ready {
        sandbox_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_pulling_image() {
        let e = SandboxCreateEvent::PullingImage {
            image_ref: "docker.io/iiidev/python:latest".into(),
            progress_bytes: 42,
            total_bytes: Some(100),
        };
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("\"kind\":\"pulling_image\""));
        assert!(s.contains("\"progress_bytes\":42"));
    }

    #[test]
    fn serializes_ready() {
        let e = SandboxCreateEvent::Ready {
            sandbox_id: "abc".into(),
        };
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("\"kind\":\"ready\""));
        assert!(s.contains("\"sandbox_id\":\"abc\""));
    }
}
