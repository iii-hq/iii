// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iii_sdk::{IIIError, RegisterFunctionMessage};
use iii_shell_proto::{FsEntry, FsOp, FsResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::SandboxError, fs::adapter::FsRunner, registry::SandboxRegistry,
};

#[derive(Debug, Deserialize)]
pub struct StatRequest {
    pub sandbox_id: String,
    pub path: String,
}

/// Mirrors `FsEntry`. Note: the shell protocol does not carry uid/gid;
/// those fields are absent at this trigger level.
#[derive(Debug, Serialize)]
pub struct StatResponse {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mode: String,
    pub mtime: i64,
    pub is_symlink: bool,
}

impl From<FsEntry> for StatResponse {
    fn from(e: FsEntry) -> Self {
        StatResponse {
            name: e.name,
            is_dir: e.is_dir,
            size: e.size,
            mode: e.mode,
            mtime: e.mtime,
            is_symlink: e.is_symlink,
        }
    }
}

pub async fn handle_stat<R: FsRunner + ?Sized>(
    req: StatRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<StatResponse, SandboxError> {
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
        .fs_call(state.shell_sock, FsOp::Stat { path: req.path })
        .await?;

    match result {
        FsResult::Stat(entry) => Ok(StatResponse::from(entry)),
        other => Err(SandboxError::FsIo(format!(
            "expected Stat result, got {other:?}"
        ))),
    }
}

pub(super) fn register(
    iii: &iii_sdk::III,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let handler = move |payload: Value| {
        let registry = registry.clone();
        let runner = runner.clone();
        Box::pin(async move {
            let req: StatRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match handle_stat(req, &registry, &*runner).await {
                Ok(resp) => serde_json::to_value(resp)
                    .map_err(|e| IIIError::Handler(format!("serialize: {e}"))),
                Err(e) => Err(IIIError::Handler(
                    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string()),
                )),
            }
        }) as Pin<Box<dyn Future<Output = Result<Value, IIIError>> + Send>>
    };
    let _ = iii.register_function_with(
        RegisterFunctionMessage {
            id: "sandbox::fs::stat".to_string(),
            description: Some("Stat a path inside a sandbox".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        handler,
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
        entry: FsEntry,
    }

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            Ok(FsResult::Stat(self.entry.clone()))
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

    fn fake_entry() -> FsEntry {
        FsEntry {
            name: "foo.txt".into(),
            is_dir: false,
            size: 42,
            mode: "0644".into(),
            mtime: 100,
            is_symlink: false,
        }
    }

    #[tokio::test]
    async fn happy_path_returns_stat() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let runner = FakeRunner {
            entry: fake_entry(),
        };
        let req = StatRequest {
            sandbox_id: id.to_string(),
            path: "/workspace/foo.txt".into(),
        };
        let resp = handle_stat(req, &reg, &runner).await.unwrap();
        assert_eq!(resp.name, "foo.txt");
        assert_eq!(resp.size, 42);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner {
            entry: fake_entry(),
        };
        let err = handle_stat(
            StatRequest {
                sandbox_id: "not-a-uuid".into(),
                path: "/".into(),
            },
            &reg,
            &runner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn missing_sandbox_returns_s002() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner {
            entry: fake_entry(),
        };
        let err = handle_stat(
            StatRequest {
                sandbox_id: Uuid::new_v4().to_string(),
                path: "/".into(),
            },
            &reg,
            &runner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }
}
