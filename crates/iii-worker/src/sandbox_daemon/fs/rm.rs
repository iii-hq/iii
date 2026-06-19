// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::sync::Arc;

use iii_sdk::RegisterFunction;
use iii_shell_proto::{FsOp, FsResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::{SandboxError, SandboxErrorWire},
    fs::adapter::FsRunner,
    registry::SandboxRegistry,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(example = "rm_request_example")]
pub struct RmRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path to remove inside the sandbox guest.
    pub path: String,
    /// Remove directories and their contents recursively (like `rm -rf`).
    #[serde(default)]
    pub recursive: bool,
}

fn rm_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/temp",
        "recursive": true
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RmResponse {
    pub removed: bool,
}

pub async fn handle_rm<R: FsRunner + ?Sized>(
    req: RmRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<RmResponse, SandboxError> {
    let id = Uuid::parse_str(&req.sandbox_id).map_err(|_| {
        SandboxError::InvalidRequest(format!(
            "sandbox_id is not a valid UUID: {}",
            req.sandbox_id
        ))
    })?;
    let state = registry.get(id).await?;
    if state.stopped {
        return Err(SandboxError::AlreadyStopped(id.to_string()));
    }
    registry.bump_last_exec(id).await;

    let result = runner
        .fs_call(
            state.shell_sock,
            FsOp::Rm {
                path: req.path,
                recursive: req.recursive,
            },
        )
        .await?;

    match result {
        FsResult::Rm { removed } => Ok(RmResponse { removed }),
        other => Err(SandboxError::FsIo(format!(
            "expected Rm result, got {other:?}"
        ))),
    }
}

pub(super) fn register(
    iii: &iii_sdk::IIIClient,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let _ = iii.register_function(
        "sandbox::fs::rm",
        RegisterFunction::new_async(move |req: RmRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result = handle_rm(req, &registry, &*runner).await;
                crate::sandbox_daemon::log_handler_result(
                    "sandbox::fs::rm",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Remove a file or directory inside a sandbox. Pass `recursive:true` to remove \
             directories with contents. Example: { sandbox_id: \"...\", path: \"/home/app/temp\", recursive: true }",
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::{fs::adapter::FsRunner, registry::SandboxState};
    use iii_shell_proto::{FsOp, FsReadMeta, FsResult};
    use std::path::PathBuf;
    use std::time::Instant;

    struct FakeRunner;

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            Ok(FsResult::Rm { removed: true })
        }
        async fn fs_write_stream(
            &self,
            _shell_sock: PathBuf,
            _path: String,
            _mode: String,
            _parents: bool,
            _reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        ) -> Result<FsResult, SandboxError> {
            unimplemented!()
        }
        async fn fs_read_stream(
            &self,
            _shell_sock: PathBuf,
            _path: String,
        ) -> Result<(FsReadMeta, Box<dyn tokio::io::AsyncRead + Unpin + Send>), SandboxError>
        {
            unimplemented!()
        }
    }

    fn make_state(id: Uuid) -> SandboxState {
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::from("/tmp/r"),
            workdir: PathBuf::from("/tmp/w"),
            shell_sock: PathBuf::from("/tmp/s"),
            vm_pid: Some(1),
            lifeline: None,
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_progress: false,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn happy_path_returns_removed() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let resp = handle_rm(
            RmRequest {
                sandbox_id: id.to_string(),
                path: "/workspace/file.txt".into(),
                recursive: false,
            },
            &reg,
            &FakeRunner,
        )
        .await
        .unwrap();
        assert!(resp.removed);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let err = handle_rm(
            RmRequest {
                sandbox_id: "bad".into(),
                path: "/".into(),
                recursive: false,
            },
            &reg,
            &FakeRunner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn missing_sandbox_returns_s002() {
        let reg = SandboxRegistry::new();
        let err = handle_rm(
            RmRequest {
                sandbox_id: Uuid::new_v4().to_string(),
                path: "/".into(),
                recursive: false,
            },
            &reg,
            &FakeRunner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }
}
