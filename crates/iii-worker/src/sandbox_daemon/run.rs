// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `sandbox::run` — meta-function that composes
//! create + fs::write + sandbox::exec + sandbox::stop into a single call.
//!
//! Workflow TTHW for an AI agent: one tool call.
//!
//! ```text
//! agent  ──run({image, code, lang})──▶  sandbox::run
//!                                         │
//!                                         ├─ sandbox::create({image, env})        ──▶ sandbox_id
//!                                         ├─ for f in files:
//!                                         │    sandbox::fs::write({sandbox_id, path, content})
//!                                         ├─ sandbox::fs::write(/tmp/run.{ext}, code)
//!                                         ├─ sandbox::exec({sandbox_id, cmd: interpreter, args})
//!                                         └─ sandbox::stop({sandbox_id})  ◀ unless keep_sandbox
//! ```
//!
//! Failure model (E2 amendment from the locked plan):
//! - Any sub-step error short-circuits.
//! - If `keep_sandbox: false` (the default), the sandbox is stopped on
//!   either success or failure — symmetric, no leaks.
//! - If `keep_sandbox: true` and a sub-step fails, the error carries
//!   `fix.context` naming which step failed AND `fix.sandbox_id` so the
//!   caller can stop or inspect the partially-set-up sandbox.

use std::sync::Arc;

use iii_sdk::RegisterFunction;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::sandbox_daemon::{
    config::SandboxConfig,
    create::{CreateRequest, CreateResponse, handle_create},
    errors::{SandboxError, SandboxErrorWire},
    exec::{EnvShape, ExecRequest, ExecResponse, ShellRunner, handle_exec},
    fs::FsRunner,
    fs::write::{WriteContent, WriteRequest, handle_write},
    registry::SandboxRegistry,
    stop::{StopRequest, VmStopper, handle_stop},
};

/// Optional sibling file to drop into the sandbox before the main code runs.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunFile {
    /// Absolute path inside the sandbox guest where the file lands.
    pub path: String,
    /// UTF-8 file body. Streaming and base64 are not supported here;
    /// callers needing those should use `sandbox::create` + repeated
    /// `sandbox::fs::write` calls instead.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(example = "run_request_example")]
pub struct RunRequest {
    /// Catalog name of the image to boot (preset or `custom_images` key).
    /// Same value space as `sandbox::create`.
    pub image: String,
    /// The actual code to run. Written to a `/tmp/run.{ext}` file inside
    /// the sandbox; the chosen interpreter runs that file.
    pub code: String,
    /// Selects the interpreter and file extension. Required.
    /// Built-in values: `"node"`, `"python"`, `"shell"`.
    /// Any other string is treated as a literal interpreter binary path
    /// inside the VM and the file is written to `/tmp/run.txt`. There is
    /// no default — there's no honest universal answer to "what language
    /// is this code", and silently defaulting to shell makes Python or
    /// JS code produce confusing line-by-line failures.
    pub lang: String,
    /// Optional sibling files (extra source modules, config, fixtures).
    #[serde(default)]
    pub files: Vec<RunFile>,
    /// Env vars exposed to the interpreter. Accepts both `Vec<"K=V">`
    /// and `{ K: V }` map shapes (same as `sandbox::exec.env`).
    #[serde(default)]
    pub env: EnvShape,
    /// Base64-encoded bytes piped to the interpreter's stdin.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Kill-after window for the interpreter, in ms. Defaults to
    /// 300_000 (5 minutes); pass a smaller value to fail fast on
    /// quick probes.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// If true, the sandbox is NOT stopped after the run completes;
    /// `sandbox_id` is returned so the caller can poke around. Default
    /// false (the sandbox is torn down on either success or failure).
    #[serde(default)]
    pub keep_sandbox: bool,
}

fn run_request_example() -> serde_json::Value {
    serde_json::json!({
        "image": "node",
        "code": "console.log('hello world')",
        "lang": "node",
        "timeout_ms": 300000
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub success: bool,
    /// Present only if `keep_sandbox: true` was set on the request.
    /// Otherwise null (sandbox auto-stopped).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_id: Option<String>,
}

/// Pick `(interpreter, args_template)` for a given `lang`. For unknown
/// langs the value is treated as the literal interpreter binary path.
///
/// The args template uses `{file}` as a placeholder that
/// `interpolate_args` replaces with the run-script path.
fn interpreter_for(lang: &str) -> (&str, &[&'static str], &'static str) {
    match lang {
        "node" | "js" | "javascript" => ("node", &["{file}"], "js"),
        "python" | "py" => ("python3", &["{file}"], "py"),
        // Shell mode: invoke `/bin/sh` with the script file path as its
        // single positional argument. We avoid `-c <inline-code>` so we
        // don't reintroduce shell-escaping headaches. The script file's
        // shebang line isn't required. `bash` falls back to `/bin/sh`
        // because the bundled sandbox images aren't guaranteed to ship
        // bash; agents that require bash-specific behavior should set
        // `lang: "/bin/bash"` (treated as an unknown-lang interpreter
        // path by the catch-all arm below).
        "shell" | "sh" | "bash" => ("/bin/sh", &["{file}"], "sh"),
        // Unknown lang: treat lang as the binary path and assume it
        // takes a single file-path argument. Extension defaults to .txt
        // since we don't know what the interpreter expects.
        _ => (lang, &["{file}"], "txt"),
    }
}

fn interpolate_args(template: &[&'static str], file: &str) -> Vec<String> {
    template.iter().map(|t| t.replace("{file}", file)).collect()
}

/// Auto-stop the sandbox unless the caller asked to keep it. Best-effort:
/// if stop itself fails we log via tracing but do NOT alter the original
/// error returned to the caller (cleanup failures are observable in
/// logs, not in the response payload, because the original error is the
/// more useful signal to the agent).
async fn best_effort_stop<S: VmStopper>(sandbox_id: &str, registry: &SandboxRegistry, stopper: &S) {
    let req = StopRequest {
        sandbox_id: sandbox_id.to_string(),
        wait: false,
    };
    match handle_stop(req, registry, stopper).await {
        Ok(_) => {}
        Err(e) => tracing::warn!(
            sandbox_id = sandbox_id,
            error = %e,
            "sandbox::run cleanup stop failed; sandbox may linger until idle reaper",
        ),
    }
}

/// Composed handler. Returns a normal `RunResponse` on success, or a
/// `SandboxError` whose Display rendering (consumed by `SandboxErrorWire`)
/// already includes the step-attribution prefix.
#[allow(clippy::too_many_arguments)]
pub async fn handle_run<L, R, S>(
    req: RunRequest,
    cfg: &SandboxConfig,
    registry: &SandboxRegistry,
    launcher: &L,
    runner: &R,
    fs_runner: &(dyn FsRunner + Sync),
    stopper: &S,
    engine_address: &str,
) -> Result<RunResponse, SandboxError>
where
    L: crate::sandbox_daemon::create::VmLauncher,
    R: ShellRunner,
    S: VmStopper,
{
    // Step 1: create sandbox.
    let create_resp = handle_create(
        CreateRequest {
            image: req.image.clone(),
            cpus: None,
            memory_mb: None,
            name: None,
            network: None,
            idle_timeout_secs: None,
            env: req.env.clone(),
        },
        cfg,
        registry,
        launcher,
        |_| {},
    )
    .await
    .map_err(|e| step_error("create", None, e))?;
    // run_inner owns cleanup-on-failure: it inspects keep_sandbox on its
    // own and best-effort-stops the sandbox when a sub-step fails. So
    // handle_run just propagates the result verbatim — there's nothing
    // for an outer cleanup arm to do.
    run_inner(
        req,
        create_resp,
        registry,
        runner,
        fs_runner,
        stopper,
        engine_address,
    )
    .await
}

async fn run_inner<S: VmStopper>(
    req: RunRequest,
    create_resp: CreateResponse,
    registry: &SandboxRegistry,
    runner: &impl ShellRunner,
    fs_runner: &(dyn FsRunner + Sync),
    stopper: &S,
    engine_address: &str,
) -> Result<RunResponse, SandboxError> {
    let sandbox_id = create_resp.sandbox_id;
    let keep_sandbox = req.keep_sandbox;

    // Step 2: write sibling files. Cleanup on any failure.
    for f in &req.files {
        let write_req = WriteRequest {
            sandbox_id: sandbox_id.clone(),
            path: f.path.clone(),
            mode: "0644".to_string(),
            parents: true,
            content: Some(WriteContent::Utf8(f.content.clone())),
            content_b64: None,
        };
        if let Err(e) = handle_write(write_req, registry, fs_runner, engine_address).await {
            // Per E2: stop sandbox unless caller asked to keep it.
            if !keep_sandbox {
                best_effort_stop(&sandbox_id, registry, stopper).await;
            }
            return Err(step_error("fs::write (sibling file)", Some(&sandbox_id), e));
        }
    }

    // Step 3: write the main run script.
    let (interp, args_tmpl, ext) = interpreter_for(&req.lang);
    let run_file_path = format!("/tmp/run.{ext}");
    let write_code_req = WriteRequest {
        sandbox_id: sandbox_id.clone(),
        path: run_file_path.clone(),
        mode: "0644".to_string(),
        parents: true,
        content: Some(WriteContent::Utf8(req.code.clone())),
        content_b64: None,
    };
    if let Err(e) = handle_write(write_code_req, registry, fs_runner, engine_address).await {
        if !keep_sandbox {
            best_effort_stop(&sandbox_id, registry, stopper).await;
        }
        return Err(step_error("fs::write (code)", Some(&sandbox_id), e));
    }

    // Step 4: invoke the interpreter.
    let exec_args = interpolate_args(args_tmpl, &run_file_path);
    let exec_req = ExecRequest {
        sandbox_id: sandbox_id.clone(),
        cmd: interp.to_string(),
        args: exec_args,
        argv: Vec::new(),
        stdin: req.stdin,
        env: req.env,
        timeout_ms: req.timeout_ms,
        workdir: None,
    };
    let started = std::time::Instant::now();
    let resp: ExecResponse = match handle_exec(exec_req, registry, runner).await {
        Ok(r) => r,
        Err(e) => {
            if !keep_sandbox {
                best_effort_stop(&sandbox_id, registry, stopper).await;
            }
            return Err(step_error("exec", Some(&sandbox_id), e));
        }
    };

    // Step 5: cleanup the sandbox unless asked to keep.
    if !keep_sandbox {
        best_effort_stop(&sandbox_id, registry, stopper).await;
    }

    let _ = started; // duration_ms comes from the exec response, not here.

    Ok(RunResponse {
        stdout: resp.stdout,
        stderr: resp.stderr,
        exit_code: resp.exit_code,
        timed_out: resp.timed_out,
        duration_ms: resp.duration_ms,
        success: resp.success,
        sandbox_id: if keep_sandbox { Some(sandbox_id) } else { None },
    })
}

/// Wraps a sub-step error with structured step + sandbox_id attribution.
/// The wire-level S-code is preserved via `RunStepFailed::inner_code`;
/// `to_payload` renders `fix.context` so agents don't have to grep the
/// message to know which sub-step failed.
fn step_error(step: &str, sandbox_id: Option<&str>, e: SandboxError) -> SandboxError {
    SandboxError::RunStepFailed {
        step: step.to_string(),
        // When step="create" no sandbox exists yet, so sandbox_id is None.
        // Render that as the empty string in the wire payload so the JSON
        // shape is stable; agents can branch on `sandbox_id == ""`.
        sandbox_id: sandbox_id.unwrap_or("").to_string(),
        message: e.to_string(),
        inner_code: e.code(),
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub(super) fn register(
    iii: &iii_sdk::IIIClient,
    registry: Arc<SandboxRegistry>,
    cfg: Arc<SandboxConfig>,
    launcher: Arc<crate::sandbox_daemon::adapters::IiiWorkerLauncher>,
    runner: Arc<crate::sandbox_daemon::adapters::ShellProtoRunner>,
    fs_runner: Arc<dyn FsRunner>,
    stopper: Arc<crate::sandbox_daemon::adapters::SignalStopper>,
) {
    let engine_address = iii.address().to_string();
    let _ = iii.register_function(
        "sandbox::run",
        RegisterFunction::new_async(move |req: RunRequest| {
            let registry = registry.clone();
            let cfg = cfg.clone();
            let launcher = launcher.clone();
            let runner = runner.clone();
            let fs_runner = fs_runner.clone();
            let stopper = stopper.clone();
            let engine_address = engine_address.clone();
            async move {
                let start = std::time::Instant::now();
                let result = handle_run(
                    req,
                    &cfg,
                    &registry,
                    &*launcher,
                    &*runner,
                    &*fs_runner,
                    &*stopper,
                    &engine_address,
                )
                .await;
                // On Ok the response carries sandbox_id only when
                // `keep_sandbox: true` was set; otherwise None. The trace
                // event still emits with an empty sandbox_id in that
                // case so the column shape stays uniform.
                let sid_owned = result.as_ref().ok().and_then(|r| r.sandbox_id.clone());
                crate::sandbox_daemon::log_handler_result(
                    "sandbox::run",
                    sid_owned.as_deref(),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Run code in an ephemeral sandbox in ONE call. \
             Composes create + fs::write + exec + stop. \
             `lang` selects the interpreter (`node`, `python`, `shell`, or a custom binary path). \
             Sandbox auto-stops on success and on failure unless `keep_sandbox: true`. \
             Example: { image: \"node\", code: \"console.log('hi')\", lang: \"node\" }",
        ),
    );
}
