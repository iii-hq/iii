// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! `iii sandbox {run, create, exec, list, stop}` handlers. Thin CLI wrapper
//! that calls the sandbox daemon directly via `iii.trigger(TriggerRequest{...})`.

use iii_sdk::{III, IIIError, InitOptions, TriggerRequest, register_worker};
use serde_json::{Value, json};

use crate::cli::rootfs_cache;
use crate::sandbox_daemon::catalog;

/// Upper bound for cold-start `sandbox::create` (image pull + VM boot).
const CREATE_TRIGGER_TIMEOUT_MS: u64 = 300_000;
/// Matches the daemon's default exec timeout. If the daemon changes, update here.
/// (See `sandbox_daemon::adapters::DEFAULT_EXEC_TIMEOUT_MS` — private there, so
/// we maintain a mirrored constant on the CLI side.)
const DAEMON_DEFAULT_EXEC_TIMEOUT_MS: u64 = 30_000;
/// Safety margin so the daemon's deadline fires before the trigger does.
const EXEC_TRIGGER_MARGIN_MS: u64 = 5_000;

/// Connect to the local engine on the given port. Returns the connected
/// `III` handle; callers are responsible for `iii.shutdown()` before return.
fn connect(port: u16) -> III {
    register_worker(&format!("ws://127.0.0.1:{port}"), InitOptions::default())
}

/// Pre-pull a preset image into the unified rootfs cache so
/// `pull_and_extract_rootfs`'s "Pulling image layers..." + layer-extract
/// progress bar renders directly on the user's terminal.
///
/// Why here and not on the daemon: the daemon runs in a separate process
/// and its stderr goes to the engine log, not the user's shell. The CLI
/// shares the same on-disk cache (`~/.iii/cache/<slug>/`) so pulling
/// here means the subsequent `sandbox::create` is a cache hit and the
/// spinner jumps straight to "Booting VM".
///
/// No-ops when:
/// - the image isn't a known preset (daemon rejects with S100 anyway, or
///   a future non-preset flow wires its own pre-flight)
/// - the rootfs is already cached (ensure_rootfs returns without
///   printing)
///
/// Any error is swallowed here -- the daemon will re-attempt the pull via
/// `auto_install_image` and surface a typed `SandboxError` through the
/// normal `create` path.
async fn preflight_pull_if_preset(image: &str) {
    let Some(oci_ref) = catalog::resolve_preset(image) else {
        return;
    };
    let hints = rootfs_cache::CacheHints {
        legacy_preset: Some(image),
        ..Default::default()
    };
    // on_pull_start is a no-op: pull_and_extract_rootfs already prints
    // "Pulling image layers..." itself; doubling the banner would look
    // silly.
    let _ = rootfs_cache::ensure_rootfs(oci_ref, &hints, || {}).await;
}

/// Extract a human-readable message from a `handler error: {...}` envelope.
/// The worker emits a flat payload shape
/// `{"type":"SandboxNotFound","code":"S002","message":"..."}` (see
/// `SandboxError::to_payload`), but we also tolerate a legacy nested
/// `{"error":{"message":"..."}}` wrapper for symmetry with the SDK parser in
/// `sdk/packages/rust/iii/src/sandbox.rs`. For anything else, fall back to
/// the raw error display.
fn handler_error_message(err: &IIIError) -> String {
    let raw = err.to_string();
    let stripped = raw.strip_prefix("handler error: ").unwrap_or(&raw);
    let Some(brace) = stripped.find('{') else {
        return raw;
    };
    let Ok(parsed): Result<Value, _> = serde_json::from_str(&stripped[brace..]) else {
        return raw;
    };
    let node = parsed.get("error").unwrap_or(&parsed);
    node.get("message")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .unwrap_or(raw)
}

/// `iii sandbox run <image> [--cpus N] [--memory MB] -- <cmd> [args...]`
pub async fn handle_run(image: String, cmd: Vec<String>, cpus: u32, memory: u32, port: u16) -> i32 {
    let (head, tail) = match cmd.split_first() {
        Some((h, t)) => (h.clone(), t.to_vec()),
        None => {
            eprintln!("error: sandbox run requires a command to execute");
            return 2;
        }
    };

    preflight_pull_if_preset(&image).await;
    let iii = connect(port);

    let create_resp: Value = match iii
        .trigger(TriggerRequest {
            function_id: "sandbox::create".into(),
            payload: json!({
                "image": image,
                "cpus": cpus,
                "memory_mb": memory,
            }),
            action: None,
            timeout_ms: Some(CREATE_TRIGGER_TIMEOUT_MS),
        })
        .await
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", handler_error_message(&e));
            iii.shutdown();
            return 1;
        }
    };

    let sandbox_id = match create_resp.get("sandbox_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            eprintln!("error: sandbox::create returned no sandbox_id");
            iii.shutdown();
            return 1;
        }
    };

    let exec_result = iii
        .trigger(TriggerRequest {
            function_id: "sandbox::exec".into(),
            payload: json!({
                "sandbox_id": sandbox_id,
                "cmd": head,
                "args": tail,
            }),
            action: None,
            timeout_ms: None,
        })
        .await;

    // Step 3: always attempt stop, even if exec failed.
    let _ = iii
        .trigger(TriggerRequest {
            function_id: "sandbox::stop".into(),
            payload: json!({
                "sandbox_id": sandbox_id,
                "wait": true,
            }),
            action: None,
            timeout_ms: None,
        })
        .await;

    let exit_code = match exec_result {
        Ok(out) => {
            if let Some(stdout) = out.get("stdout").and_then(|v| v.as_str()) {
                print!("{stdout}");
            }
            if let Some(stderr) = out.get("stderr").and_then(|v| v.as_str()) {
                eprint!("{stderr}");
            }
            if out
                .get("timed_out")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                // Match coreutils `timeout(1)`: 124 means the killer fired.
                124
            } else {
                out.get("exit_code")
                    .and_then(|v| v.as_i64())
                    .map(|c| c as i32)
                    .unwrap_or(1)
            }
        }
        Err(e) => {
            eprintln!("error: {}", handler_error_message(&e));
            1
        }
    };

    iii.shutdown();
    exit_code
}

/// `iii sandbox create <image> [flags]` -- prints the sandbox id on success.
/// The sandbox persists until `iii sandbox stop <id>` or the idle timeout
/// fires.
pub async fn handle_create(
    image: String,
    cpus: u32,
    memory: u32,
    idle_timeout: Option<u64>,
    name: Option<String>,
    network: bool,
    env: Vec<String>,
    port: u16,
) -> i32 {
    preflight_pull_if_preset(&image).await;
    let iii = connect(port);

    let mut payload = json!({
        "image": image,
        "cpus": cpus,
        "memory_mb": memory,
        "network": network,
        "env": env,
    });

    if let Some(t) = idle_timeout {
        payload["idle_timeout_secs"] = json!(t);
    }
    if let Some(n) = name {
        payload["name"] = json!(n);
    }

    let started_at = std::time::Instant::now();
    let code = match iii
        .trigger(TriggerRequest {
            function_id: "sandbox::create".into(),
            payload,
            action: None,
            timeout_ms: Some(CREATE_TRIGGER_TIMEOUT_MS),
        })
        .await
    {
        Ok(resp) => {
            let sandbox_id = resp
                .get("sandbox_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // On a TTY, leave a one-line "ready" breadcrumb on stderr so the
            // user sees what actually happened before the uuid appears. On a
            // pipe, stderr is silent and the uuid goes straight to stdout so
            // `SB=$(iii sandbox create ...)` still works unchanged.
            if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
                let elapsed = started_at.elapsed().as_millis() as f64 / 1000.0;
                eprintln!("✓ sandbox ready in {elapsed:.1}s");
            }
            println!("{sandbox_id}");
            0
        }
        Err(e) => {
            eprintln!("error: {}", handler_error_message(&e));
            1
        }
    };

    iii.shutdown();
    code
}

/// `iii sandbox exec <id> [--timeout DUR] [-e KEY=VAL]... -- <cmd> [args...]`
///
/// Stdin is not piped -- sandbox::exec is pipe-mode by protocol and the
/// current wire shape only carries base64-encoded stdin as an optional
/// field. Use `iii worker exec` against a managed worker for TTY
/// sessions.
pub async fn handle_exec(
    id: String,
    timeout: Option<String>,
    env: Vec<String>,
    port: u16,
    cmd: Vec<String>,
) -> i32 {
    let (head, tail) = match cmd.split_first() {
        Some((h, t)) => (h.clone(), t.to_vec()),
        None => {
            eprintln!("error: sandbox exec requires a command to run");
            return 2;
        }
    };

    let timeout_ms = match timeout {
        None => None,
        Some(s) => match humantime::parse_duration(&s) {
            Ok(d) => Some(d.as_millis() as u64),
            Err(e) => {
                eprintln!("error: invalid --timeout '{s}': {e}");
                return 2;
            }
        },
    };

    // When the user specifies a timeout, that value is passed to the handler;
    // when absent the handler defaults to DAEMON_DEFAULT_EXEC_TIMEOUT_MS. Either
    // way we add EXEC_TRIGGER_MARGIN_MS so the daemon's own deadline fires before
    // the trigger times out, ensuring proper timed_out signalling rather than a
    // bare IIIError::Timeout.
    let trigger_timeout_ms =
        Some(timeout_ms.unwrap_or(DAEMON_DEFAULT_EXEC_TIMEOUT_MS) + EXEC_TRIGGER_MARGIN_MS);

    let iii = connect(port);

    // Pass env through as-is in `KEY=VALUE` form (matches handle_create). The
    // daemon validates format on its end.
    let mut exec_payload = json!({
        "sandbox_id": id,
        "cmd": head,
        "args": tail,
        "env": env,
    });
    if let Some(ms) = timeout_ms {
        exec_payload["timeout_ms"] = json!(ms);
    }

    let result = iii
        .trigger(TriggerRequest {
            function_id: "sandbox::exec".into(),
            payload: exec_payload,
            action: None,
            timeout_ms: trigger_timeout_ms,
        })
        .await;

    let exit_code = match result {
        Ok(out) => {
            if let Some(stdout) = out.get("stdout").and_then(|v| v.as_str()) {
                print!("{stdout}");
            }
            if let Some(stderr) = out.get("stderr").and_then(|v| v.as_str()) {
                eprint!("{stderr}");
            }
            if out
                .get("timed_out")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                // Match coreutils `timeout(1)`: 124 means the killer fired.
                124
            } else {
                out.get("exit_code")
                    .and_then(|v| v.as_i64())
                    .map(|c| c as i32)
                    .unwrap_or(1)
            }
        }
        Err(e) => {
            eprintln!("error: {}", handler_error_message(&e));
            1
        }
    };

    iii.shutdown();
    exit_code
}

/// `iii sandbox list`
///
/// Sends an empty payload; the daemon's list handler returns every
/// sandbox unconditionally. The `--all` flag is a silent no-op, kept
/// so existing scripts that pass it don't error out on an unknown
/// arg.
pub async fn handle_list(_all: bool, port: u16) -> i32 {
    let iii = connect(port);

    let resp: Value = match iii
        .trigger(TriggerRequest {
            function_id: "sandbox::list".into(),
            payload: json!({}),
            action: None,
            timeout_ms: None,
        })
        .await
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", handler_error_message(&e));
            iii.shutdown();
            return 1;
        }
    };

    println!(
        "{:<36}  {:<10}  {:>8}  {}",
        "SANDBOX_ID", "IMAGE", "AGE_SECS", "NAME"
    );
    if let Some(arr) = resp.get("sandboxes").and_then(|v| v.as_array()) {
        for s in arr {
            let id = s.get("sandbox_id").and_then(|v| v.as_str()).unwrap_or("");
            let image = s.get("image").and_then(|v| v.as_str()).unwrap_or("");
            let age = s.get("age_secs").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
            println!("{id:<36}  {image:<10}  {age:>8}  {name}");
        }
    }

    iii.shutdown();
    0
}

/// `iii sandbox stop <id>`
pub async fn handle_stop(id: String, port: u16) -> i32 {
    let iii = connect(port);

    let code = match iii
        .trigger(TriggerRequest {
            function_id: "sandbox::stop".into(),
            payload: json!({
                "sandbox_id": id,
                "wait": true,
            }),
            action: None,
            timeout_ms: None,
        })
        .await
    {
        Ok(_) => {
            println!("stopped {id}");
            0
        }
        Err(e) => {
            eprintln!("error: {}", handler_error_message(&e));
            1
        }
    };

    iii.shutdown();
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use iii_sdk::IIIError;

    #[test]
    fn extracts_message_from_flat_payload() {
        let e = IIIError::Handler(
            r#"{"type":"SandboxNotFound","code":"S002","message":"sandbox abc not found"}"#.into(),
        );
        assert_eq!(handler_error_message(&e), "sandbox abc not found");
    }

    #[test]
    fn extracts_message_from_nested_payload() {
        let e = IIIError::Handler(
            r#"{"error":{"code":"S002","message":"sandbox abc not found"}}"#.into(),
        );
        assert_eq!(handler_error_message(&e), "sandbox abc not found");
    }

    #[test]
    fn falls_back_on_non_json_handler_body() {
        let e = IIIError::Handler("bad request: missing field".into());
        assert_eq!(
            handler_error_message(&e),
            "handler error: bad request: missing field"
        );
    }

    #[test]
    fn falls_back_on_non_handler_variant() {
        let e = IIIError::Timeout;
        assert_eq!(handler_error_message(&e), "invocation timed out");
    }
}
