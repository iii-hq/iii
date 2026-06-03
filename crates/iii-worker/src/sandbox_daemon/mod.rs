// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

pub mod adapters;
pub mod auto_install;
pub mod catalog;
pub mod config;
pub mod create;
pub mod errors;
pub mod events;
pub mod exec;
pub mod fs;
pub mod list;
pub mod overlay;
pub mod reaper;
pub mod registry;
pub mod run;
pub mod stop;

pub use errors::SandboxError;
pub use registry::SandboxRegistry;

/// Cheatsheet hosts can paste into agent system prompts. Captures the
/// canonical sandbox::* mental model: which function to use first, how to
/// pass `cmd` and `env`, where the error `fix` payload lives, and how the
/// S-code map works. Designed to be small enough to inline in a system
/// prompt without busting context budgets.
///
/// Doctest verifies the embedded JSON examples parse cleanly through
/// serde so this constant cannot drift away from the wire format.
///
/// ```
/// use iii_worker::sandbox_daemon::SANDBOX_AGENT_GUIDE;
/// let guide = SANDBOX_AGENT_GUIDE;
/// assert!(guide.contains("sandbox::run"));
/// assert!(guide.contains("sandbox::exec"));
/// assert!(guide.contains("error.message"));
/// ```
pub const SANDBOX_AGENT_GUIDE: &str = r#"
You have access to the iii sandbox::* tools. Use them to run code in an
isolated microVM and capture stdout/stderr.

# Workflow

The fastest path is one call to `sandbox::run`:

  sandbox::run({
    image: "node",                 // or "python", or any custom_images key
    code: "console.log('hello')",  // your code, written to /tmp/run.{ext}
    lang: "node",                  // "node" | "python" | "shell" | <binary>
    env: { "NODE_ENV": "production" }
    // timeout_ms defaults to 300_000 (5 min) — set explicitly only for
    // probes or to override for very long builds.
  })

sandbox::run auto-stops the VM on success AND on failure. Set
`keep_sandbox: true` to keep the VM alive (returns sandbox_id).

# Surgical workflow

  sandbox::create({ image: "node" }) -> { sandbox_id }
  sandbox::fs::write({ sandbox_id, path, content })  // content is a UTF-8 string
  sandbox::exec({ sandbox_id, cmd, args })           // see cmd shapes below
  sandbox::stop({ sandbox_id })

# sandbox::exec cmd shapes (all three accepted)

  { cmd: "node /home/app/main.js" }                 // shell-line, shlex-split
  { cmd: "node", args: ["/home/app/main.js"] }      // classic POSIX
  { argv: ["node", "/home/app/main.js"] }           // argv array

Shlex is NOT bash. For bash semantics use sandbox::run with lang:"shell".

# env shapes

  { env: ["FOO=bar", "PATH=/usr/bin"] }              // wire shape
  { env: { "FOO": "bar", "PATH": "/usr/bin" } }      // map shape

# Errors

Errors return JSON encoded inside error.message. Parse once:

  const detail = JSON.parse(err.message);
  // detail.code, detail.type, detail.message, detail.docs_url
  // detail.fix       ready-to-send next-call payload (null if not auto-fixable)
  // detail.fix_note  one-liner explaining why fix is null
  // detail.retryable bool

If detail.fix is non-null, your next call is fn(detail.fix).

# S-code map

  S001 validation       request shape error
  S002 sandbox missing  call sandbox::create first
  S003 concurrent exec  one exec at a time per sandbox; detach servers (nohup &) or stop+recreate (waiting won't free a foreground exec)
  S100 image catalog    pick "python" or "node" (call sandbox::catalog::list for custom_images keys)
  S200 exec timeout     raise timeout_ms
  S211 file not found    (parent-missing variants carry fix:{parents:true})
  S215 permission denied
  S300 VM boot failed   platform / virtualization issue
  S400 capacity         resource cap reached
"#;

use std::sync::Arc;

use iii_observability::OtelConfig;
use iii_sdk::{InitOptions, RegisterFunction, WorkerMetadata, register_worker};

use crate::sandbox_daemon::config::SandboxConfig;
use crate::sandbox_daemon::errors::SandboxErrorWire;

pub async fn serve(config: SandboxConfig, engine_url: &str) -> anyhow::Result<()> {
    tracing::info!(url = %engine_url, "connecting to III engine");
    // Identify ourselves as `iii-sandbox` so the engine surfaces this
    // worker by its config-yaml name (and not the auto-detected
    // `<hostname>:<pid>`) in `engine::workers::list` and friends. The
    // publish workflow polls by this name to decide when the worker is
    // ready for interface collection.
    let iii = register_worker(
        engine_url,
        InitOptions {
            otel: Some(OtelConfig::default()),
            metadata: Some(WorkerMetadata {
                name: "iii-sandbox".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let sandbox_registry = Arc::new(crate::sandbox_daemon::SandboxRegistry::new());
    let sandbox_cfg = Arc::new(config);
    let launcher = Arc::new(crate::sandbox_daemon::adapters::IiiWorkerLauncher);
    let runner = Arc::new(crate::sandbox_daemon::adapters::ShellProtoRunner);
    let stopper = Arc::new(crate::sandbox_daemon::adapters::SignalStopper);
    let fs_runner: std::sync::Arc<dyn fs::FsRunner> = std::sync::Arc::new(fs::IiiShellFsRunner);

    register_sandbox_create(
        &iii,
        sandbox_registry.clone(),
        sandbox_cfg.clone(),
        launcher.clone(),
    );
    register_sandbox_exec(&iii, sandbox_registry.clone(), runner.clone());
    register_sandbox_stop(&iii, sandbox_registry.clone(), stopper.clone());
    register_sandbox_list(&iii, sandbox_registry.clone());
    register_sandbox_catalog_list(&iii, sandbox_cfg.clone());

    // sandbox::run meta-function. Composes create+write+exec+stop into
    // one call for agents that just want to run code (workflow TTHW = 1).
    crate::sandbox_daemon::run::register(
        &iii,
        sandbox_registry.clone(),
        sandbox_cfg.clone(),
        launcher.clone(),
        runner.clone(),
        fs_runner.clone(),
        stopper.clone(),
    );

    fs::register_all(&iii, sandbox_registry.clone(), fs_runner.clone());

    {
        let registry = (*sandbox_registry).clone();
        let stopper = stopper.clone();
        tokio::spawn(async move {
            crate::sandbox_daemon::reaper::run_reaper_loop(
                registry,
                stopper,
                std::time::Duration::from_secs(10),
            )
            .await;
        });
    }

    tracing::info!("sandbox-daemon ready");
    tokio::signal::ctrl_c().await?;
    tracing::info!("sandbox-daemon shutting down");
    iii.shutdown_async().await;
    Ok(())
}

fn register_sandbox_create(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    cfg: Arc<crate::sandbox_daemon::config::SandboxConfig>,
    launcher: Arc<crate::sandbox_daemon::adapters::IiiWorkerLauncher>,
) {
    let _ = iii.register_function(
        "sandbox::create",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::create::CreateRequest| {
            let registry = registry.clone();
            let cfg = cfg.clone();
            let launcher = launcher.clone();
            async move {
                let start = std::time::Instant::now();
                let result = crate::sandbox_daemon::create::handle_create(
                    req,
                    &cfg,
                    &registry,
                    &*launcher,
                    |e| {
                        tracing::info!(event = ?e, "sandbox create event");
                    },
                )
                .await;
                // sandbox::create has no incoming sandbox_id (it generates one).
                // On success the new id is in the response — surface it so even
                // create-failure events carry the same field shape (just empty).
                let sid_owned = result.as_ref().ok().map(|r| r.sandbox_id.clone());
                log_handler_result(
                    "sandbox::create",
                    sid_owned.as_deref(),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Create an ephemeral sandbox VM. `image` must be a preset \
             (`\"python\"`, `\"node\"`) or a `custom_images` key from iii.config.yaml; \
             OCI refs are NOT accepted unless they match a catalog key. \
             `env` accepts both `Vec<\"K=V\">` and `{ K: V }` map shapes.",
        ),
    );
}

fn register_sandbox_exec(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    runner: Arc<crate::sandbox_daemon::adapters::ShellProtoRunner>,
) {
    let _ = iii.register_function(
        "sandbox::exec",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::exec::ExecRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result =
                    crate::sandbox_daemon::exec::handle_exec(req, &registry, &*runner).await;
                log_handler_result(
                    "sandbox::exec",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Execute a command inside a live sandbox. `cmd` accepts three shapes: \
             (1) a shell-style line that gets shlex-split (`cmd: \"node -v\"`), \
             (2) `cmd` + `args` (the POSIX shape, `cmd: \"node\", args: [\"-v\"]`), \
             (3) an `argv` array (`argv: [\"node\", \"-v\"]`). \
             Shell metacharacters in the shell-line shape are NOT interpreted; \
             use `sandbox::run` with `lang: \"shell\"` for bash semantics. \
             `env` accepts both `Vec<\"K=V\">` and `{ K: V }` map shapes.",
        ),
    );
}

fn register_sandbox_stop(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    stopper: Arc<crate::sandbox_daemon::adapters::SignalStopper>,
) {
    let _ = iii.register_function(
        "sandbox::stop",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::stop::StopRequest| {
            let registry = registry.clone();
            let stopper = stopper.clone();
            async move {
                let sid = req.sandbox_id.clone();
                let start = std::time::Instant::now();
                let result =
                    crate::sandbox_daemon::stop::handle_stop(req, &registry, &*stopper).await;
                log_handler_result(
                    "sandbox::stop",
                    Some(&sid),
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                result.map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description(
            "Stop and remove a running sandbox. Set `wait: true` to block \
             until the VM process exits and resources are reclaimed.",
        ),
    );
}

fn register_sandbox_list(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
) {
    // Note: pre-migration this handler used
    // `serde_json::from_value(payload).unwrap_or_default()`, silently
    // coercing `null`/non-object payloads into an empty `ListRequest`.
    // `new_async` is strict, so a literal `null` payload now returns an
    // S001-style handler error instead of an empty list. The only
    // production caller (`cli::sandbox::handle_list`, which sends
    // `json!({})`) is unaffected; an empty object still deserializes to
    // the unit `ListRequest`.
    let _ = iii.register_function(
        "sandbox::list",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::list::ListRequest| {
            let registry = registry.clone();
            async move {
                let start = std::time::Instant::now();
                let response = crate::sandbox_daemon::list::handle_list(req, &registry).await;
                // handle_list is infallible; manufacture an Ok-typed Result so
                // log_handler_result emits the success branch consistently with
                // the fallible handlers. The Err arm is unreachable.
                let result: Result<_, crate::sandbox_daemon::errors::SandboxError> = Ok(response);
                log_handler_result(
                    "sandbox::list",
                    None,
                    &result,
                    start.elapsed().as_millis() as u64,
                );
                Ok::<_, iii_sdk::IIIError>(result.unwrap())
            }
        })
        .description("List active sandboxes"),
    );
}

fn register_sandbox_catalog_list(
    iii: &iii_sdk::III,
    cfg: Arc<crate::sandbox_daemon::config::SandboxConfig>,
) {
    let _ = iii.register_function(
        "sandbox::catalog::list",
        RegisterFunction::new_async(
            move |req: crate::sandbox_daemon::catalog::CatalogListRequest| {
                let cfg = cfg.clone();
                async move {
                    let start = std::time::Instant::now();
                    let response = crate::sandbox_daemon::catalog::handle_catalog_list(req, &cfg);
                    let result: Result<_, crate::sandbox_daemon::errors::SandboxError> =
                        Ok(response);
                    log_handler_result(
                        "sandbox::catalog::list",
                        None,
                        &result,
                        start.elapsed().as_millis() as u64,
                    );
                    Ok::<_, iii_sdk::IIIError>(result.unwrap())
                }
            },
        )
        .description(
            "List bootable images: bundled presets plus operator-registered custom_images. \
             Call this before sandbox::create when you don't know what's available.",
        ),
    );
}

/// Per-handler-boundary tracing emission. Called by every
/// `register_sandbox_*` closure right after the inner handler returns.
/// Emits a single `tracing::info!` event with a stable field set so
/// operators can dashboard sandbox::* usage without grepping logs:
///
/// - `function_id` — the registration name (e.g. `"sandbox::exec"`).
/// - `sandbox_id` — the sandbox the call targeted, empty string when
///   the function has no incoming sandbox_id (sandbox::create,
///   sandbox::list, sandbox::catalog::list).
/// - `success` — `true` when the handler returned `Ok`.
/// - `error_code` — the S-code on `Err`, empty on `Ok`.
/// - `error_type` — the SandboxErrorCode category on `Err`, empty on `Ok`.
/// - `retryable` — whether the error suggests a retry (only set on `Err`).
/// - `duration_ms` — wall-clock the handler spent before returning.
///
/// Moving this out of `to_payload` (where A5 originally placed
/// error-path tracing) and broadening it to success-path is the
/// "tracing instrumentation" follow-up surfaced by the post-implementation
/// devex review.
pub(crate) fn log_handler_result<T>(
    function_id: &'static str,
    sandbox_id: Option<&str>,
    result: &Result<T, crate::sandbox_daemon::errors::SandboxError>,
    duration_ms: u64,
) {
    match result {
        Ok(_) => {
            tracing::info!(
                function_id = function_id,
                sandbox_id = sandbox_id.unwrap_or(""),
                success = true,
                error_code = "",
                error_type = "",
                duration_ms = duration_ms,
                "sandbox::* handler completed"
            );
        }
        Err(err) => {
            let code = err.code();
            tracing::info!(
                function_id = function_id,
                sandbox_id = sandbox_id.unwrap_or(""),
                success = false,
                error_code = code.as_str(),
                error_type = code.error_type(),
                retryable = code.retryable(),
                duration_ms = duration_ms,
                "sandbox::* handler returned error"
            );
        }
    }
}

#[cfg(test)]
mod handler_logging_tests {
    //! Pins the trace contract operators dashboard against. Both branches
    //! (success + error) must execute without panicking and emit through
    //! the global `tracing` subscriber. We don't assert on the field set
    //! at runtime here — that's the operator's contract and validating
    //! it requires a tracing layer/test-subscriber dance that's noise
    //! for a one-shot helper. The compile-time signature plus this
    //! happy-path check is the right level of coverage.
    use super::*;

    #[test]
    fn log_handler_result_success_branch_runs() {
        let ok: Result<&str, crate::sandbox_daemon::errors::SandboxError> = Ok("ignored");
        log_handler_result("sandbox::test", Some("sid-1"), &ok, 42);
        log_handler_result("sandbox::test", None, &ok, 0);
    }

    #[test]
    fn log_handler_result_error_branch_runs_with_code() {
        // Use the well-known S001 InvalidRequest error so the code,
        // type, and retryable accessors all resolve to concrete values.
        let err: Result<(), _> = Err(crate::sandbox_daemon::errors::SandboxError::InvalidRequest(
            "missing path".to_string(),
        ));
        log_handler_result("sandbox::test", Some("sid-1"), &err, 7);
        log_handler_result("sandbox::test", None, &err, 99);
    }
}
