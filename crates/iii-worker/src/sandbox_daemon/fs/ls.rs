// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::sync::Arc;

use iii_sdk::RegisterFunction;
use iii_shell_proto::{FsEntry, FsOp, FsResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::{SandboxError, SandboxErrorWire},
    fs::adapter::FsRunner,
    registry::SandboxRegistry,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(example = "ls_request_example")]
pub struct LsRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path of the directory to list inside the sandbox guest.
    pub path: String,
}

fn ls_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app"
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LsResponse {
    pub entries: Vec<FsEntry>,
}

pub async fn handle_ls<R: FsRunner + ?Sized>(
    req: LsRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<LsResponse, SandboxError> {
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
        .fs_call(state.shell_sock, FsOp::Ls { path: req.path })
        .await?;

    match result {
        FsResult::Ls { entries } => Ok(LsResponse { entries }),
        other => Err(SandboxError::FsIo(format!(
            "expected Ls result, got {other:?}"
        ))),
    }
}

pub(super) fn register(
    iii: &iii_sdk::III,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let _ = iii.register_function(
        "sandbox::fs::ls",
        RegisterFunction::new_async(move |req: LsRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result = handle_ls(req, &registry, &*runner).await;
                crate::sandbox_daemon::log_handler_result(
                    "sandbox::fs::ls",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "List directory contents inside a sandbox. \
             Example: { sandbox_id: \"...\", path: \"/home/app\" }",
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::{fs::adapter::FsRunner, registry::SandboxState};
    use iii_shell_proto::{FsEntry, FsReadMeta, FsResult};
    use std::path::PathBuf;
    use std::time::Instant;

    struct FakeRunner {
        entries: Vec<FsEntry>,
    }

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            Ok(FsResult::Ls {
                entries: self.entries.clone(),
            })
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
    async fn happy_path_returns_entries() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let runner = FakeRunner {
            entries: vec![FsEntry {
                name: "hello.txt".into(),
                is_dir: false,
                size: 5,
                mode: "0644".into(),
                mtime: 0,
                is_symlink: false,
            }],
        };
        let req = LsRequest {
            sandbox_id: id.to_string(),
            path: "/workspace".into(),
        };
        let resp = handle_ls(req, &reg, &runner).await.unwrap();
        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].name, "hello.txt");
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner { entries: vec![] };
        let req = LsRequest {
            sandbox_id: "not-a-uuid".into(),
            path: "/".into(),
        };
        let err = handle_ls(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn missing_sandbox_returns_s002() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner { entries: vec![] };
        let req = LsRequest {
            sandbox_id: Uuid::new_v4().to_string(),
            path: "/".into(),
        };
        let err = handle_ls(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }
}
