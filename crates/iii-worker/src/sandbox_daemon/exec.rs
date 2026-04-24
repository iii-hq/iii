//! Exec serialization invariant: per-sandbox, only one exec runs at a
//! time. `SandboxRegistry::begin_exec` / `end_exec` holds that guard.

use crate::sandbox_daemon::{errors::SandboxError, registry::SandboxRegistry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ExecRequest {
    pub sandbox_id: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub stdin: Option<String>, // base64-encoded stdin bytes
    #[serde(default)]
    pub env: Vec<String>, // "K=V" entries
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub workdir: Option<String>,
}

#[derive(Debug, Serialize)]
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
}
