// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iii_sdk::{IIIError, RegisterFunctionMessage};
use iii_shell_proto::{FsOp, FsResult, FsSedFileResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::SandboxError, fs::adapter::FsRunner, registry::SandboxRegistry,
};

/// Find-and-replace request, accepting either an explicit `files` list
/// or a `path` walked like grep does. Exactly one of those two must be
/// provided — `handle_sed` returns S210 otherwise.
#[derive(Debug, Deserialize)]
pub struct SedRequest {
    pub sandbox_id: String,
    /// Legacy form: explicit list of paths to rewrite.
    #[serde(default)]
    pub files: Vec<String>,
    /// New form: walk `path` like grep does. May be a directory or a
    /// single file. Mutually exclusive with `files`.
    #[serde(default)]
    pub path: Option<String>,
    /// Whether to descend into subdirectories. Only meaningful with
    /// `path`. Defaults to `true` so the path-form behaves like grep.
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Gitignore-style include filter applied to paths relative to
    /// `path`. Only meaningful with `path`.
    #[serde(default)]
    pub include_glob: Vec<String>,
    /// Gitignore-style exclude filter applied to paths relative to
    /// `path`. Only meaningful with `path`.
    #[serde(default)]
    pub exclude_glob: Vec<String>,
    pub pattern: String,
    pub replacement: String,
    #[serde(default = "default_true")]
    pub regex: bool,
    #[serde(default)]
    pub first_only: bool,
    #[serde(default)]
    pub ignore_case: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct SedResponse {
    pub results: Vec<FsSedFileResult>,
    pub total_replacements: u64,
}

pub async fn handle_sed<R: FsRunner + ?Sized>(
    req: SedRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<SedResponse, SandboxError> {
    let id = Uuid::parse_str(&req.sandbox_id).map_err(|_| {
        SandboxError::InvalidRequest(format!(
            "sandbox_id is not a valid UUID: {}",
            req.sandbox_id
        ))
    })?;

    // Mutually-exclusive form check before we touch the registry. We
    // treat empty `files` as "not the legacy form" so old guests that
    // always sent `files: [..]` keep working, and new callers can pass
    // `path` instead. Both/neither -> S210.
    match (req.files.is_empty(), req.path.as_ref()) {
        (false, None) | (true, Some(_)) => {}
        (false, Some(_)) => {
            return Err(SandboxError::FsInvalidRequest(
                "sed: provide exactly one of files or path, not both".into(),
            ));
        }
        (true, None) => {
            return Err(SandboxError::FsInvalidRequest(
                "sed: must provide exactly one of files or path".into(),
            ));
        }
    }

    let state = registry.get(id).await?;
    if state.stopped {
        return Err(SandboxError::AlreadyStopped(id.to_string()));
    }
    registry.bump_last_exec(id).await;

    let result = runner
        .fs_call(
            state.shell_sock,
            FsOp::Sed {
                files: req.files,
                path: req.path,
                recursive: req.recursive,
                include_glob: req.include_glob,
                exclude_glob: req.exclude_glob,
                pattern: req.pattern,
                replacement: req.replacement,
                regex: req.regex,
                first_only: req.first_only,
                ignore_case: req.ignore_case,
            },
        )
        .await?;

    match result {
        FsResult::Sed {
            results,
            total_replacements,
        } => Ok(SedResponse {
            results,
            total_replacements,
        }),
        other => Err(SandboxError::FsIo(format!(
            "expected Sed result, got {other:?}"
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
            let req: SedRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match handle_sed(req, &registry, &*runner).await {
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
            id: "sandbox::fs::sed".to_string(),
            description: Some("Search-and-replace in files inside a sandbox".to_string()),
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
    use iii_shell_proto::{FsOp, FsReadMeta, FsResult, FsSedFileResult};
    use std::path::PathBuf;
    use std::time::Instant;

    struct FakeRunner;

    #[async_trait::async_trait]
    impl FsRunner for FakeRunner {
        async fn fs_call(&self, _shell_sock: PathBuf, _op: FsOp) -> Result<FsResult, SandboxError> {
            Ok(FsResult::Sed {
                results: vec![FsSedFileResult {
                    path: "a.py".into(),
                    replacements: 2,
                    success: true,
                    error: None,
                }],
                total_replacements: 2,
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

    fn req_files(sandbox_id: String, files: Vec<String>) -> SedRequest {
        SedRequest {
            sandbox_id,
            files,
            path: None,
            recursive: true,
            include_glob: Vec::new(),
            exclude_glob: Vec::new(),
            pattern: "hello".into(),
            replacement: "world".into(),
            regex: true,
            first_only: false,
            ignore_case: false,
        }
    }

    #[tokio::test]
    async fn happy_path_returns_results() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(make_state(id)).await;
        let req = req_files(id.to_string(), vec!["/workspace/a.py".into()]);
        let resp = handle_sed(req, &reg, &FakeRunner).await.unwrap();
        assert_eq!(resp.total_replacements, 2);
        assert_eq!(resp.results.len(), 1);
    }

    #[tokio::test]
    async fn bad_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        // Note: bad-uuid returns S001 *before* the form check. Use a
        // valid form to confirm we're testing the UUID path.
        let err = handle_sed(
            req_files("bad".into(), vec!["/workspace/a.py".into()]),
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
        let err = handle_sed(
            req_files(Uuid::new_v4().to_string(), vec!["/workspace/a.py".into()]),
            &reg,
            &FakeRunner,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }

    #[tokio::test]
    async fn sed_request_with_both_files_and_path_returns_s210() {
        // Form-check happens after UUID parse but before registry hit,
        // so a valid UUID with no matching sandbox is fine here.
        let reg = SandboxRegistry::new();
        let mut req = req_files(Uuid::new_v4().to_string(), vec!["/workspace/a.py".into()]);
        req.path = Some("/workspace".into());
        let err = handle_sed(req, &reg, &FakeRunner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S210");
    }

    #[tokio::test]
    async fn sed_request_with_neither_returns_s210() {
        let reg = SandboxRegistry::new();
        let req = req_files(Uuid::new_v4().to_string(), Vec::new());
        // path is already None via req_files default; files is empty.
        let err = handle_sed(req, &reg, &FakeRunner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S210");
    }
}
