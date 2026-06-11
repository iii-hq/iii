//! Exec serialization invariant: per-sandbox, only one exec runs at a
//! time. `SandboxRegistry::begin_exec` / `end_exec` holds that guard.

use crate::sandbox_daemon::{errors::SandboxError, registry::SandboxRegistry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Environment-variable input. Agents naturally pass `{ FOO: "bar" }`
/// (matching Docker/npm/k8s mental models); the original wire shape was
/// `Vec<"K=V">`. The untagged enum accepts both; `into_kv_vec()`
/// normalises to the canonical `Vec<String>` the runner expects.
///
/// `Default` is the empty vec form (the historical wire shape).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EnvShape {
    /// Original wire shape: `["FOO=bar", "PATH=/usr/bin"]`.
    Vec(Vec<String>),
    /// Agent-natural shape: `{ "FOO": "bar", "PATH": "/usr/bin" }`.
    /// Iteration is sorted by key (BTreeMap) so two callers passing the
    /// same map get the same env-var ordering.
    Map(std::collections::BTreeMap<String, String>),
}

impl Default for EnvShape {
    fn default() -> Self {
        // Empty Vec — matches the historical wire default. Cannot use
        // `#[default]` on the variant because schemars 0.8 chokes on
        // `#[default]` for tuple variants; manual impl keeps both
        // schemars and serde happy.
        EnvShape::Vec(Vec::new())
    }
}

impl EnvShape {
    /// Normalise to the canonical `Vec<String>` shape the runner consumes.
    /// Validates env-var names match `[A-Za-z_][A-Za-z0-9_]*` (POSIX shell
    /// portable names, plus the lowercase extension that `is_valid_env_name`
    /// allows for npm/pip/hg-style variables) for the Map form; returns
    /// `InvalidRequest` listing the bad keys.
    pub fn into_kv_vec(self) -> Result<Vec<String>, SandboxError> {
        match self {
            EnvShape::Vec(v) => Ok(v),
            EnvShape::Map(m) => {
                let mut bad = Vec::new();
                for k in m.keys() {
                    if !is_valid_env_name(k) {
                        bad.push(k.clone());
                    }
                }
                if !bad.is_empty() {
                    return Err(SandboxError::InvalidRequest(format!(
                        "env contains invalid variable name(s): {:?}. \
                         Names must match `[A-Za-z_][A-Za-z0-9_]*`.",
                        bad
                    )));
                }
                Ok(m.into_iter().map(|(k, v)| format!("{k}={v}")).collect())
            }
        }
    }
}

fn is_valid_env_name(name: &str) -> bool {
    // POSIX portable env-var names are `[A-Z_][A-Z0-9_]*`, but real-world
    // tooling routinely sets lowercase names (npm_config_*, hg_*, pip_*,
    // .env files in modern frameworks). Accept both cases so the
    // map-shape doesn't surprise agents passing what the tooling docs
    // say to use.
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(example = "exec_request_example")]
pub struct ExecRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// The binary to execute. Accepts three shapes (`handle_exec` picks
    /// one and normalises `cmd`/`args` before passing to the runner):
    ///
    /// 1. **Shell line**: `cmd: "node /home/app/index.js"`. If `cmd`
    ///    contains whitespace AND `args` is empty AND `argv` is empty,
    ///    `cmd` is shlex-split into `(head, tail)` and `head` becomes
    ///    the binary while `tail` becomes the argv.
    /// 2. **cmd + args**: `cmd: "node", args: ["-v"]`. The classic
    ///    POSIX shape; unchanged from earlier versions.
    /// 3. **argv array**: `argv: ["node", "/home/app/index.js"]`.
    ///    Wins over `cmd`/`args` if non-empty; the first element is
    ///    the binary, the rest are arguments.
    ///
    /// Shlex is NOT bash. Shell metacharacters (`;`, `|`, `&&`, `>`,
    /// pipes, redirects, variable expansion) inside the shell-line
    /// shape are split as text, not interpreted. Use `sandbox::run`
    /// with `lang: "shell"` if you need bash semantics.
    #[serde(default)]
    pub cmd: String,
    /// Argv tail passed to `cmd` (each entry is one argv slot).
    #[serde(default)]
    pub args: Vec<String>,
    /// Alternative input shape: a single argv array where the first
    /// element is the binary and the rest are arguments. Mutually
    /// exclusive with `cmd` having whitespace OR `args` being set.
    #[serde(default)]
    pub argv: Vec<String>,
    /// Base64-encoded bytes piped to the child's stdin.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Environment entries added to the child. Accepts either a
    /// `Vec<"K=V">` (the original wire shape) or a `{ K: V }` map.
    /// `handle_exec` normalises to `Vec<String>` before invoking the
    /// runner.
    #[serde(default)]
    pub env: EnvShape,
    /// Kill-after window in ms. Defaults to 300_000 (5 minutes) — sized
    /// for cold `npm install` / `pip install` / `cargo build`. Pass a
    /// smaller value (e.g. 10_000) for probes and version checks.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Working directory inside the sandbox; image default when omitted.
    #[serde(default)]
    pub workdir: Option<String>,
}

fn exec_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "cmd": "node",
        "args": ["/home/app/index.js"],
        "env": { "NODE_ENV": "production" },
        "timeout_ms": 300000
    })
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

    // 3-shape cmd resolution. The downstream runner is argv-only (no
    // shell), so we normalise here and hand it a (cmd, args) pair.
    //
    // Order of precedence:
    //   1. `argv` non-empty wins; reject if `cmd`/`args` also set.
    //   2. `cmd` containing whitespace AND `args` empty → shlex split.
    //   3. Plain `cmd` + `args` (the original shape).
    let (resolved_cmd, resolved_args) = resolve_cmd_shape(&req.cmd, &req.args, &req.argv)?;

    if resolved_cmd.is_empty() {
        return Err(SandboxError::InvalidRequest(
            "cmd is required and must be non-empty (or pass `argv` with at least one element)"
                .to_string(),
        ));
    }

    // Normalise env from EnvShape to Vec<String>.
    let resolved_env = req.env.into_kv_vec()?;

    // Build a normalised request to hand to the runner. The runner trait
    // reads `req.cmd` / `req.args` / `req.env` as Vec<String>, so we
    // construct a fresh `NormalisedExec` to pass through.
    let normalised = ExecRequest {
        sandbox_id: req.sandbox_id,
        cmd: resolved_cmd,
        args: resolved_args,
        argv: Vec::new(),
        stdin: req.stdin,
        env: EnvShape::Vec(resolved_env),
        timeout_ms: req.timeout_ms,
        workdir: req.workdir,
    };

    let state = registry.begin_exec(id).await?;
    let result = runner.run(state.shell_sock.clone(), &normalised).await;
    registry.end_exec(id).await;
    result
}

/// Pick the argv pair from the three accepted input shapes, returning
/// `(cmd, args)`. Errors are S001 (`InvalidRequest`) with hint prose.
///
/// Resolution precedence:
/// 1. `argv` wins if non-empty. Reject if `cmd` or `args` is also set
///    (ambiguous mix).
/// 2. Else if `cmd` has whitespace and `args` is empty, shlex-split `cmd`.
///    Reject on unbalanced quotes.
/// 3. Else use `cmd` + `args` as today.
pub(crate) fn resolve_cmd_shape(
    cmd: &str,
    args: &[String],
    argv: &[String],
) -> Result<(String, Vec<String>), SandboxError> {
    if !argv.is_empty() {
        if !cmd.is_empty() || !args.is_empty() {
            return Err(SandboxError::InvalidRequest(format!(
                "`argv` cannot be combined with `cmd` or `args`. Pick one shape. \
                 Got argv={argv:?}, cmd={cmd:?}, args={args:?}"
            )));
        }
        let mut it = argv.iter().cloned();
        let head = it.next().ok_or_else(|| {
            SandboxError::InvalidRequest("argv must contain at least one element".to_string())
        })?;
        return Ok((head, it.collect()));
    }

    let cmd_has_whitespace = cmd.chars().any(|c| c.is_whitespace());
    if cmd_has_whitespace {
        if !args.is_empty() {
            return Err(SandboxError::InvalidRequest(format!(
                "cmd contains whitespace (shell-line shape) AND `args` is set; that is ambiguous. \
                 Either remove whitespace from cmd and pass arguments via `args`, or leave `args` empty so cmd is shlex-split. \
                 Got cmd={cmd:?}, args={args:?}"
            )));
        }
        let parts = shlex::split(cmd).ok_or_else(|| {
            SandboxError::InvalidRequest(format!(
                "cmd has unbalanced quotes and cannot be shlex-split: {cmd:?}. \
                 Use `argv: [...]` for argv arrays with embedded quotes."
            ))
        })?;
        let mut it = parts.into_iter();
        let head = it.next().ok_or_else(|| {
            SandboxError::InvalidRequest(format!("cmd is empty after shlex split: {cmd:?}"))
        })?;
        return Ok((head, it.collect()));
    }

    // Plain cmd + args
    Ok((cmd.to_string(), args.to_vec()))
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
            lifeline: None,
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
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
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
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
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
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }

    #[tokio::test]
    async fn cmd_with_whitespace_now_shlex_splits() {
        // Post-D6: shell-line shape is accepted. `cmd: "node -v"` shlex-splits
        // into `cmd: "node", args: ["-v"]` and reaches the runner. Earlier the
        // handler rejected this with S001; the new contract pushes the burden
        // of choosing the right shape onto the resolver, not the agent.
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "ok".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: "node -v".into(),
            args: vec![],
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let resp = handle_exec(req, &reg, &runner).await.unwrap();
        assert_eq!(resp.exit_code, Some(0));
        let state = reg.get(id).await.unwrap();
        assert!(!state.exec_in_progress);
    }

    #[tokio::test]
    async fn cmd_whitespace_plus_args_is_ambiguous() {
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
            args: vec!["extra".into()],
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
        let msg = err.to_string();
        assert!(msg.contains("ambiguous"), "msg={msg}");
    }

    #[tokio::test]
    async fn argv_shape_used_when_present() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "ok".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: String::new(),
            args: vec![],
            argv: vec!["node".into(), "/home/app/index.js".into()],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let resp = handle_exec(req, &reg, &runner).await.unwrap();
        assert_eq!(resp.exit_code, Some(0));
    }

    #[tokio::test]
    async fn argv_combined_with_cmd_rejected() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: "node".into(),
            args: vec![],
            argv: vec!["python".into()],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }

    #[tokio::test]
    async fn cmd_with_unbalanced_quote_returns_s001() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(state_for(id)).await;
        let runner = FakeRunner {
            stdout: "".into(),
            exit: 0,
        };
        let req = ExecRequest {
            sandbox_id: id.to_string(),
            cmd: "node 'unterminated".into(),
            args: vec![],
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
        assert!(err.to_string().contains("unbalanced"), "msg={}", err);
    }

    #[tokio::test]
    async fn env_map_shape_normalises_to_vec() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert("FOO".to_string(), "bar".to_string());
        map.insert("BAZ".to_string(), "qux".to_string());
        let v = EnvShape::Map(map).into_kv_vec().unwrap();
        assert_eq!(v, vec!["BAZ=qux".to_string(), "FOO=bar".to_string()]);
    }

    #[tokio::test]
    async fn env_map_rejects_invalid_var_name() {
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        map.insert("9bad".to_string(), "x".to_string());
        let err = EnvShape::Map(map).into_kv_vec().unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
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
            argv: vec![],
            stdin: None,
            env: EnvShape::default(),
            timeout_ms: None,
            workdir: None,
        };
        let err = handle_exec(req, &reg, &runner).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S001");
    }
}
