// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::sync::Arc;

use iii_sdk::RegisterFunction;
use iii_shell_proto::{FsMatch, FsOp, FsResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sandbox_daemon::{
    errors::{SandboxError, SandboxErrorWire},
    fs::adapter::FsRunner,
    registry::SandboxRegistry,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(example = "grep_request_example")]
pub struct GrepRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Root path to search inside the sandbox guest. Treated as a directory
    /// when `recursive: true`, else as a single file.
    pub path: String,
    /// Regex pattern (Rust regex syntax, anchored fragments allowed).
    pub pattern: String,
    /// Descend into subdirectories under `path`. Defaults to true.
    #[serde(default = "default_recursive")]
    pub recursive: bool,
    /// Case-insensitive match.
    #[serde(default)]
    pub ignore_case: bool,
    /// Gitignore-style include filter applied relative to `path`.
    #[serde(default)]
    pub include_glob: Vec<String>,
    /// Gitignore-style exclude filter applied relative to `path`.
    #[serde(default)]
    pub exclude_glob: Vec<String>,
    /// Maximum number of matches before truncation. Default 10_000.
    #[serde(default = "default_max_matches")]
    pub max_matches: u64,
    /// Maximum bytes per matched line before content is truncated with `…`.
    /// Default 4096.
    #[serde(default = "default_max_line_bytes")]
    pub max_line_bytes: u64,
}

fn grep_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/src",
        "pattern": "TODO|FIXME",
        "recursive": true,
        "include_glob": ["**/*.js", "**/*.ts"]
    })
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

#[derive(Debug, Serialize, JsonSchema)]
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
    iii: &iii_sdk::IIIClient,
    registry: Arc<SandboxRegistry>,
    runner: Arc<dyn FsRunner>,
) {
    let _ = iii.register_function(
        "sandbox::fs::grep",
        RegisterFunction::new_async(move |req: GrepRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result = handle_grep(req, &registry, &*runner).await;
                crate::sandbox_daemon::log_handler_result(
                    "sandbox::fs::grep",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Search for a regex pattern in files inside a sandbox. Walks `path` recursively \
             by default. Example: { sandbox_id: \"...\", path: \"/home/app/src\", pattern: \"TODO\" }",
        ),
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
            lifeline: None,
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
