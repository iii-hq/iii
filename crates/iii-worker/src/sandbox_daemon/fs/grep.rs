// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iii_sdk::{IIIError, RegisterFunctionMessage};
use iii_shell_proto::{FsMatch, FsOp, FsResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::SandboxError, fs::adapter::FsRunner, registry::SandboxRegistry,
};

#[derive(Debug, Deserialize)]
pub struct GrepRequest {
    pub sandbox_id: String,
    pub path: String,
    pub pattern: String,
    #[serde(default = "default_recursive")]
    pub recursive: bool,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub include_glob: Vec<String>,
    #[serde(default)]
    pub exclude_glob: Vec<String>,
    #[serde(default = "default_max_matches")]
    pub max_matches: u64,
    #[serde(default = "default_max_line_bytes")]
    pub max_line_bytes: u64,
}

fn default_recursive() -> bool {
    true
}
fn default_max_matches() -> u64 {
    10_000
}
fn default_max_line_bytes() -> u64 {
    4096
}

#[derive(Debug, Serialize)]
pub struct GrepResponse {
    pub matches: Vec<FsMatch>,
    pub truncated: bool,
}

pub async fn handle_grep<R: FsRunner + ?Sized>(
    req: GrepRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<GrepResponse, SandboxError> {
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
            FsOp::Grep {
                path: req.path,
                pattern: req.pattern,
                recursive: req.recursive,
                ignore_case: req.ignore_case,
                include_glob: req.include_glob,
                exclude_glob: req.exclude_glob,
                max_matches: req.max_matches,
                max_line_bytes: req.max_line_bytes,
            },
        )
        .await?;

    match result {
        FsResult::Grep { matches, truncated } => Ok(GrepResponse { matches, truncated }),
        other => Err(SandboxError::FsIo(format!(
            "expected Grep result, got {other:?}"
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
            let req: GrepRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match handle_grep(req, &registry, &*runner).await {
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
            id: "sandbox::fs::grep".to_string(),
            description: Some("Search for a pattern in files inside a sandbox".to_string()),
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
    use iii_shell_proto::{FsMatch, FsOp, FsReadMeta, FsResult};
    use std::path::PathBuf;
    use std::time::Instant;

    struct FakeRunner {
        matches: Vec<FsMatch>,
    }

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            Ok(FsResult::Grep {
                matches: self.matches.clone(),
                truncated: false,
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
    async fn happy_path_returns_matches() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let runner = FakeRunner {
            matches: vec![FsMatch {
                path: "a.py".into(),
                line: 1,
                content: "hello".into(),
            }],
        };
        let req = GrepRequest {
            sandbox_id: id.to_string(),
            path: "/workspace".into(),
            pattern: "hello".into(),
            recursive: true,
            ignore_case: false,
            include_glob: vec![],
            exclude_glob: vec![],
            max_matches: 10_000,
            max_line_bytes: 4096,
        };
        let resp = handle_grep(req, &reg, &runner).await.unwrap();
        assert_eq!(resp.matches.len(), 1);
        assert!(!resp.truncated);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner { matches: vec![] };
        let req = GrepRequest {
            sandbox_id: "bad".into(),
            path: "/".into(),
            pattern: "x".into(),
            recursive: true,
            ignore_case: false,
            include_glob: vec![],
            exclude_glob: vec![],
            max_matches: 100,
            max_line_bytes: 4096,
        };
        let err = handle_grep(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn missing_sandbox_returns_s002() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner { matches: vec![] };
        let req = GrepRequest {
            sandbox_id: Uuid::new_v4().to_string(),
            path: "/".into(),
            pattern: "x".into(),
            recursive: true,
            ignore_case: false,
            include_glob: vec![],
            exclude_glob: vec![],
            max_matches: 100,
            max_line_bytes: 4096,
        };
        let err = handle_grep(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }
}
