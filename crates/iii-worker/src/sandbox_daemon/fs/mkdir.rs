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
#[schemars(example = "mkdir_request_example")]
pub struct MkdirRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path of the directory to create inside the sandbox guest.
    pub path: String,
    /// Octal permissions for the new directory (e.g. `"0755"`).
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Create intermediate parent directories like `mkdir -p`.
    #[serde(default)]
    pub parents: bool,
}

fn mkdir_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/cache",
        "mode": "0755",
        "parents": true
    })
}

fn default_mode() -> String {
    "0755".to_string()
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MkdirResponse {
    pub created: bool,
}

pub async fn handle_mkdir<R: FsRunner + ?Sized>(
    req: MkdirRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<MkdirResponse, SandboxError> {
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
            FsOp::Mkdir {
                path: req.path,
                mode: req.mode,
                parents: req.parents,
            },
        )
        .await?;

    match result {
        FsResult::Mkdir { created } => Ok(MkdirResponse { created }),
        other => Err(SandboxError::FsIo(format!(
            "expected Mkdir result, got {other:?}"
        ))),
    }
}

pub(super) fn register(
    iii: &iii_sdk::III,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let _ = iii.register_function(
        "sandbox::fs::mkdir",
        RegisterFunction::new_async(move |req: MkdirRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result = handle_mkdir(req, &registry, &*runner).await;
                crate::sandbox_daemon::log_handler_result(
                    "sandbox::fs::mkdir",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Create a directory inside a sandbox. Pass `parents:true` to make missing parents \
             like `mkdir -p`. Example: { sandbox_id: \"...\", path: \"/home/app/cache\", mode: \"0755\", parents: true }",
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
            Ok(FsResult::Mkdir { created: true })
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
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_progress: false,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn happy_path_returns_created() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let req = MkdirRequest {
            sandbox_id: id.to_string(),
            path: "/workspace/newdir".into(),
            mode: "0755".into(),
            parents: false,
        };
        let resp = handle_mkdir(req, &reg, &FakeRunner).await.unwrap();
        assert!(resp.created);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let err = handle_mkdir(
            MkdirRequest {
                sandbox_id: "not-a-uuid".into(),
                path: "/".into(),
                mode: "0755".into(),
                parents: false,
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
        let err = handle_mkdir(
            MkdirRequest {
                sandbox_id: Uuid::new_v4().to_string(),
                path: "/".into(),
                mode: "0755".into(),
                parents: false,
            },
            &reg,
            &FakeRunner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }
}
