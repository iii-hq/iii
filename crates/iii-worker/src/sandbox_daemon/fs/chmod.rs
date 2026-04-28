// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iii_sdk::{IIIError, RegisterFunctionMessage};
use iii_shell_proto::{FsOp, FsResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::SandboxError, fs::adapter::FsRunner, registry::SandboxRegistry,
};

#[derive(Debug, Deserialize)]
pub struct ChmodRequest {
    pub sandbox_id: String,
    pub path: String,
    pub mode: String,
    #[serde(default)]
    pub uid: Option<u32>,
    #[serde(default)]
    pub gid: Option<u32>,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Serialize)]
pub struct ChmodResponse {
    pub updated: u64,
}

pub async fn handle_chmod<R: FsRunner + ?Sized>(
    req: ChmodRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<ChmodResponse, SandboxError> {
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
            FsOp::Chmod {
                path: req.path,
                mode: req.mode,
                uid: req.uid,
                gid: req.gid,
                recursive: req.recursive,
            },
        )
        .await?;

    match result {
        FsResult::Chmod { updated } => Ok(ChmodResponse { updated }),
        other => Err(SandboxError::FsIo(format!(
            "expected Chmod result, got {other:?}"
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
            let req: ChmodRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match handle_chmod(req, &registry, &*runner).await {
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
            id: "sandbox::fs::chmod".to_string(),
            description: Some("Change file permissions inside a sandbox".to_string()),
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
    use iii_shell_proto::{FsOp, FsReadMeta, FsResult};
    use std::path::PathBuf;
    use std::time::Instant;

    struct FakeRunner;

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            Ok(FsResult::Chmod { updated: 3 })
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
    async fn happy_path_returns_updated() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let resp = handle_chmod(
            ChmodRequest {
                sandbox_id: id.to_string(),
                path: "/workspace".into(),
                mode: "0755".into(),
                uid: None,
                gid: None,
                recursive: false,
            },
            &reg,
            &FakeRunner,
        )
        .await
        .unwrap();
        assert_eq!(resp.updated, 3);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let err = handle_chmod(
            ChmodRequest {
                sandbox_id: "bad".into(),
                path: "/".into(),
                mode: "0755".into(),
                uid: None,
                gid: None,
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
        let err = handle_chmod(
            ChmodRequest {
                sandbox_id: Uuid::new_v4().to_string(),
                path: "/".into(),
                mode: "0755".into(),
                uid: None,
                gid: None,
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
