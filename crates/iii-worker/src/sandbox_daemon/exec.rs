//! Exec serialization invariant: per-sandbox, only one exec runs at a
//! time. `SandboxRegistry::begin_exec` / `end_exec` holds that guard.

use crate::sandbox_daemon::{errors::SandboxError, registry::SandboxRegistry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// The binary to execute as a single string — NOT a shell line.
    /// `"node"` is correct; `"node -v"` is not (put `-v` in `args`).
    /// `handle_exec` rejects values containing whitespace with S001.
    /// Shell metacharacters (`;`, `|`, `&&`, `>`, etc.) in `cmd` are not
    /// interpreted — the runner spawns `cmd` literally. Use a wrapper
    /// script inside the VM if you need shell behavior.
    pub cmd: String,
    /// Argv tail passed to `cmd` (each entry is one argv slot).
    #[serde(default)]
    pub args: Vec<String>,
    /// Base64-encoded bytes piped to the child's stdin.
    #[serde(default)]
    pub stdin: Option<String>,
    /// `"K=V"` entries (NOT a map) added to the child's environment.
    #[serde(default)]
    pub env: Vec<String>,
    /// Kill-after window in ms; daemon default applies when omitted.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Working directory inside the sandbox; image default when omitted.
    #[serde(default)]
    pub workdir: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExecResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub success: bool,
}

#[async_trait::async_trait]
pub trait ShellRunner: Send + Sync + 'static {
    async fn run(
        &self,
        state_shell_sock: std::path::PathBuf,
        req: &ExecRequest,
    ) -> Result<ExecResponse, SandboxError>;
}

pub async fn handle_exec<R: ShellRunner>(
    req: ExecRequest,
    registry: &SandboxRegistry,
    runner: &R,
) -> Result<ExecResponse, SandboxError> {
    let id = Uuid::parse_str(&req.sandbox_id).map_err(|_| {
        SandboxError::InvalidRequest(format!(
            "sandbox_id is not a valid UUID: {}",
            req.sandbox_id
        ))
    })?;
    // Reject `cmd` containing whitespace. The runner spawns `cmd` as a
    // single binary (no shell expansion), so `cmd: "node -v"` looks for a
    // binary literally named `node -v` and fails inside the VM. In
    // practice the failure mode observed downstream is a shell-relay
    // disconnect (S300 "vm disconnected mid-run") rather than a clean
    // not-found error, leaving agents stuck retrying a doomed call.
    // Catching it here converts that into a recoverable S001 with a
    // hint, so the caller learns the right shape on the first miss.
    if req.cmd.chars().any(|c| c.is_whitespace()) {
        return Err(SandboxError::InvalidRequest(format!(
            "cmd must be a single binary, not a shell line: got {:?}. Put arguments in `args` (Vec<String>) instead — e.g. {{ cmd: \"node\", args: [\"-v\"] }}.",
            req.cmd
        )));
    }
    if req.cmd.is_empty() {
        return Err(SandboxError::InvalidRequest(
            "cmd is required and must be non-empty".to_string(),
        ));
    }
    let state = registry.begin_exec(id).await?;

    // Always clear exec flag on exit (success OR error).
    let result = runner.run(state.shell_sock.clone(), &req).await;
    registry.end_exec(id).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_daemon::registry::SandboxState;
    use std::path::PathBuf;
    use std::time::Instant;

    struct FakeRunner {
        stdout: String,
        exit: i32,
    }
    #[async_trait::async_trait]
    impl ShellRunner for FakeRunner {
        async fn run(
            &self,
            _sock: std::path::PathBuf,
            _r: &ExecRequest,
        ) -> Result<ExecResponse, SandboxError> {
            Ok(ExecResponse {
                stdout: self.stdout.clone(),
                stderr: String::new(),
                exit_code: Some(self.exit),
                timed_out: false,
                duration_ms: 1,
                success: self.exit == 0,
            })
        }
    }

    fn state_for(id: Uuid) -> SandboxState {
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
    async fn happy_path_runs_and_clears_flag() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "hi\n".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: "/bin/true".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        };
        let resp = handle_exec(req, &reg, &runner).await.unwrap();
        assert_eq!(resp.stdout, "hi\n");
        let state = reg.get(id).await.unwrap();
        assert!(!state.exec_in_progress);
    }

    #[tokio::test]
    async fn invalid_uuid_returns_s001() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner {
            stdout: "".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: "not-a-uuid".into(),
            cmd: "/bin/true".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn missing_sandbox_returns_s002() {
        let reg = SandboxRegistry::new();
        let runner = FakeRunner {
            stdout: "".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: Uuid::new_v4().to_string(),
            cmd: "/bin/true".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }

    #[tokio::test]
    async fn cmd_with_whitespace_returns_s001_with_hint() {
        // Regression: kimi-k2.6 (and other LLMs) frequently pass
        // `cmd: "node -v"` instead of `cmd: "node", args: ["-v"]`. Before
        // the guard this reached the shell relay and surfaced as
        // S300 "vm disconnected mid-run" — opaque, retried in a loop,
        // and left the operator's QA report blocked.
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: "node -v".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
        let payload = err.to_payload();
        // Message must steer the caller to the correct shape.
        let msg = payload["message"].as_str().unwrap();
        assert!(msg.contains("single binary"), "msg={msg}");
        assert!(msg.contains("args"), "msg={msg}");
        // begin_exec must NOT have flipped — a doomed call should not
        // hold the per-sandbox exec lock.
        let state = reg.get(id).await.unwrap();
        assert!(!state.exec_in_progress);
    }

    #[tokio::test]
    async fn cmd_with_tab_or_newline_also_rejected() {
        for bad in ["node\t-v", "node\n-v"] {
            let reg = SandboxRegistry::new();
            let id = Uuid::new_v4();
            reg.insert(state_for(id)).await;
            let runner = FakeRunner {
                stdout: "".into(),
                exit: 0,
            };
            let req = ExecRequest {
                sandbox_id: id.to_string(),
                cmd: bad.into(),
                args: vec![],
                stdin: None,
                env: vec![],
                timeout_ms: None,
                workdir: None,
            };
            let err = handle_exec(req, &reg, &runner).await.unwrap_err();
            assert_eq!(err.code().as_str(), "S001", "input={bad:?}");
        }
    }

    #[tokio::test]
    async fn empty_cmd_returns_s001() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: "".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }
}
