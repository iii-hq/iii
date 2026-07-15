// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii worker add --host <engine>`: route the add through a RUNNING iii
//! engine's `worker::add` trigger (served by its iii-worker-ops daemon)
//! instead of editing a config file relative to the CLI's cwd.
//!
//! The engine host applies the op against ITS project — config file,
//! artifacts, locks — so the CLI can run from any directory (or machine)
//! that can reach the engine. Without `--host`, `iii worker add` edits the
//! config file in the current directory and only a same-directory engine
//! picks the change up.

use colored::Colorize;

use crate::core::{AddOptions, AddOutcome};

/// Turn a `--host` value into an engine WebSocket URL.
///
/// Accepted shapes: `host`, `host:port`, `[v6addr]`, `[v6addr]:port`, and
/// full `ws://` / `wss://` URLs (passed through untouched). A missing port
/// defaults to the engine's default WS port.
pub fn host_to_ws_url(host: &str) -> Result<String, String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("--host cannot be empty".to_string());
    }
    if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        return Ok(trimmed.to_string());
    }
    if trimmed.contains("://") {
        return Err(format!(
            "unsupported scheme in --host '{trimmed}' (use host[:port], ws:// or wss://)"
        ));
    }

    // Bracketed IPv6: `[::1]` or `[::1]:49134`.
    if let Some(rest) = trimmed.strip_prefix('[') {
        let Some((addr, tail)) = rest.split_once(']') else {
            return Err(format!("unclosed '[' in --host '{trimmed}'"));
        };
        if addr.is_empty() {
            return Err(format!("empty IPv6 address in --host '{trimmed}'"));
        }
        let port = match tail.strip_prefix(':') {
            Some(p) => parse_port(p, trimmed)?,
            None if tail.is_empty() => super::app::DEFAULT_PORT,
            _ => return Err(format!("invalid --host '{trimmed}'")),
        };
        return Ok(format!("ws://[{addr}]:{port}"));
    }

    match trimmed.rsplit_once(':') {
        Some((h, p)) => {
            if h.is_empty() {
                return Err(format!("missing host in --host '{trimmed}'"));
            }
            if h.contains(':') {
                return Err(format!("IPv6 addresses need brackets: --host '[{h}]:{p}'"));
            }
            let port = parse_port(p, trimmed)?;
            Ok(format!("ws://{h}:{port}"))
        }
        None => Ok(format!("ws://{}:{}", trimmed, super::app::DEFAULT_PORT)),
    }
}

fn parse_port(p: &str, host: &str) -> Result<u16, String> {
    p.parse::<u16>()
        .map_err(|_| format!("invalid port '{p}' in --host '{host}'"))
}

/// `host:port` authority of a ws/wss URL, for the TCP reachability probe.
/// `None` when the URL has no explicit port (full-URL passthrough) — the
/// probe is skipped and the trigger timeout is the backstop.
fn probe_authority(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let auth = rest.split(['/', '?']).next().unwrap_or(rest);
    let has_port = if let Some(bracket_end) = auth.rfind(']') {
        auth[bracket_end..].contains(':')
    } else {
        auth.contains(':')
    };
    (has_port && !auth.is_empty()).then(|| auth.to_string())
}

/// Fail fast when nothing listens at the target: without this, a wrong
/// `--host` sits silent until the trigger timeout while the SDK retries the
/// connection in the background — exactly the hang this flag exists to fix.
async fn check_engine_reachable(url: &str) -> Result<(), String> {
    let Some(authority) = probe_authority(url) else {
        return Ok(());
    };
    let connect = tokio::net::TcpStream::connect(authority.clone());
    match tokio::time::timeout(std::time::Duration::from_secs(5), connect).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("cannot reach an iii engine at {url}: {e}")),
        Err(_) => Err(format!(
            "cannot reach an iii engine at {url}: connect timed out"
        )),
    }
}

/// Human-readable message from a failed `worker::*` trigger. The daemon
/// packs a `{ type, code, message, details }` envelope into
/// `Error::Remote.message`; prefer its `message` field over raw JSON.
fn remote_error_message(e: &iii_sdk::Error) -> String {
    match e {
        iii_sdk::Error::Remote { code, message, .. } => {
            let human = serde_json::from_str::<serde_json::Value>(message)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(|m| m.to_string())
                })
                .unwrap_or_else(|| message.clone());
            format!("[{code}] {human}")
        }
        iii_sdk::Error::Timeout => {
            "engine call timed out — the install may still be running server-side; \
             poll worker::status before retrying"
                .to_string()
        }
        other => other.to_string(),
    }
}

/// Install workers through the engine at `host`. `adds` pairs the
/// user-facing label (the argv token) with the fully-built options shipped
/// to `worker::add`. Returns a process exit code.
pub async fn handle_remote_add(host: &str, adds: Vec<(String, AddOptions)>) -> i32 {
    let url = match host_to_ws_url(host) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("{} {}", "error:".red(), e);
            return 2;
        }
    };

    if let Err(e) = check_engine_reachable(&url).await {
        eprintln!(
            "{} {}\n  Start the engine there first (`iii`), or fix --host.",
            "error:".red(),
            e
        );
        return 1;
    }

    let iii = iii_sdk::register_worker(
        &url,
        iii_sdk::InitOptions {
            // Attributable in the engine console instead of an anonymous
            // `<hostname>:<pid>` connection.
            metadata: Some(iii_sdk::runtime::WorkerMetadata {
                name: "iii-cli".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    // Keep the client-side deadline aligned with the daemon's advertised
    // worker::add budget (downloads + optional 120s ready-wait).
    let (timeout_ms, _) = super::worker_manager_daemon::op_metadata("worker::add");

    let total = adds.len();
    let mut fail_count = 0usize;
    for (i, (label, opts)) in adds.into_iter().enumerate() {
        if total > 1 {
            eprintln!("  [{}/{}] Adding {}...", i + 1, total, label.bold());
        }
        let waited = opts.wait;
        let payload = match serde_json::to_value(&opts) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "{} cannot encode add request for '{label}': {e}",
                    "error:".red()
                );
                fail_count += 1;
                continue;
            }
        };

        let spinner = super::spinner::Spinner::start(format!("Installing {label} via {url}..."));
        let result = iii
            .trigger(iii_sdk::protocol::TriggerRequest {
                function_id: "worker::add".to_string(),
                payload,
                action: None,
                timeout_ms: Some(timeout_ms),
            })
            .await;

        match result {
            Ok(value) => match serde_json::from_value::<AddOutcome>(value) {
                Ok(outcome) => {
                    let version = outcome
                        .version
                        .as_deref()
                        .map(|v| format!(" v{v}"))
                        .unwrap_or_default();
                    spinner.finish_ok(format!(
                        "{}{} installed via {} (engine config: {})",
                        outcome.name.bold(),
                        version,
                        url,
                        outcome.config_path.display(),
                    ));
                    if !waited {
                        eprintln!(
                            "  boots in the background — poll worker::status {{\"name\":\"{}\"}} on the engine",
                            outcome.name
                        );
                    }
                }
                Err(e) => {
                    // The op succeeded server-side; only the outcome decode
                    // failed (likely a version skew). Say so instead of
                    // claiming a failed install.
                    spinner.finish_ok(format!(
                        "{} installed via {} (unrecognized outcome shape: {e})",
                        label.bold(),
                        url,
                    ));
                }
            },
            Err(e) => {
                let msg = remote_error_message(&e);
                spinner.finish_err(format!("{label}: {msg}"));
                if msg.contains("not found") && msg.contains("worker::add") {
                    eprintln!(
                        "  The engine at {url} does not expose worker::add — its \
                         iii-worker-ops daemon isn't running (older engine, or \
                         IIIWORKER_DISABLE_BUILTIN_DAEMONS set)."
                    );
                }
                fail_count += 1;
            }
        }
    }

    iii.shutdown_async().await;
    if fail_count == 0 { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_host_gets_default_port() {
        assert_eq!(
            host_to_ws_url("localhost").unwrap(),
            format!("ws://localhost:{}", crate::cli::app::DEFAULT_PORT)
        );
    }

    #[test]
    fn host_port_passes_through() {
        assert_eq!(
            host_to_ws_url("localhost:49134").unwrap(),
            "ws://localhost:49134"
        );
        assert_eq!(
            host_to_ws_url("10.0.0.7:5000").unwrap(),
            "ws://10.0.0.7:5000"
        );
    }

    #[test]
    fn full_ws_urls_pass_through_untouched() {
        assert_eq!(
            host_to_ws_url("ws://engine.internal:49134").unwrap(),
            "ws://engine.internal:49134"
        );
        assert_eq!(
            host_to_ws_url("wss://engine.example.com").unwrap(),
            "wss://engine.example.com"
        );
    }

    #[test]
    fn ipv6_hosts_require_and_accept_brackets() {
        assert_eq!(host_to_ws_url("[::1]").unwrap(), {
            format!("ws://[::1]:{}", crate::cli::app::DEFAULT_PORT)
        });
        assert_eq!(host_to_ws_url("[::1]:5000").unwrap(), "ws://[::1]:5000");
        assert!(host_to_ws_url("::1").is_err());
    }

    #[test]
    fn invalid_hosts_are_rejected() {
        assert!(host_to_ws_url("").is_err());
        assert!(host_to_ws_url("localhost:notaport").is_err());
        assert!(host_to_ws_url("localhost:70000").is_err());
        assert!(host_to_ws_url(":49134").is_err());
        assert!(host_to_ws_url("http://engine:49134").is_err());
        assert!(host_to_ws_url("[::1").is_err());
    }

    #[test]
    fn probe_authority_extracts_host_port() {
        assert_eq!(
            probe_authority("ws://localhost:49134").as_deref(),
            Some("localhost:49134")
        );
        assert_eq!(
            probe_authority("ws://[::1]:49134").as_deref(),
            Some("[::1]:49134")
        );
        // No explicit port → probe skipped.
        assert_eq!(probe_authority("wss://engine.example.com"), None);
    }

    #[test]
    fn remote_error_message_prefers_envelope_message() {
        let e = iii_sdk::Error::Remote {
            code: "W110".into(),
            message: r#"{"type":"WorkerOpError","code":"W110","message":"Worker 'pdfkit' not found","details":{"name":"pdfkit"}}"#.into(),
            stacktrace: None,
        };
        assert_eq!(remote_error_message(&e), "[W110] Worker 'pdfkit' not found");
    }

    #[test]
    fn remote_error_message_falls_back_to_raw() {
        let e = iii_sdk::Error::Remote {
            code: "X1".into(),
            message: "plain text".into(),
            stacktrace: None,
        };
        assert_eq!(remote_error_message(&e), "[X1] plain text");
    }
}
