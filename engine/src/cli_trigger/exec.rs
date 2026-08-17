// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use iii::workers::telemetry::collector::CLI_WORKER_NAME_PREFIX;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{Error, InitOptions, register_worker};
use serde_json::Value;

use super::TriggerCliError;

/// Metadata for the trigger connection. Registering under a marked name is how
/// the engine counts this command in the heartbeat's CLI aggregate: the CLI
/// process is too short-lived to report anything itself, and the connection it
/// already opens carries the name.
///
/// Returns `None` when this CLI has opted out of telemetry. The engine counts
/// whatever name it receives, so marking it regardless would let an engine with
/// telemetry enabled record commands from a CLI that opted out.
fn cli_worker_metadata() -> Option<iii_sdk::iii::WorkerMetadata> {
    (!crate::cli::telemetry::is_telemetry_disabled()).then(|| iii_sdk::iii::WorkerMetadata {
        name: format!("{CLI_WORKER_NAME_PREFIX}trigger"),
        ..Default::default()
    })
}

pub async fn invoke(
    function_path: &str,
    payload: Value,
    address: &str,
    port: u16,
    timeout_ms: u64,
    namespace: Option<&str>,
) -> Result<(), TriggerCliError> {
    let url = format!("ws://{}:{}", address, port);
    let iii = register_worker(
        &url,
        InitOptions {
            metadata: cli_worker_metadata(),
            ..Default::default()
        },
    );

    let request = TriggerRequest {
        function_id: function_path.to_string(),
        payload,
        action: None,
        timeout_ms: Some(timeout_ms),
    };

    // Omitted when absent so the engine keeps resolving in `default`: the
    // caller's own namespace never influences routing.
    let result = match namespace {
        Some(namespace) => iii.trigger(request.namespace(namespace)).await,
        None => iii.trigger(request).await,
    };

    iii.shutdown_async().await;

    match result {
        Ok(value) => {
            if !value.is_null() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).map_err(|e| anyhow::anyhow!(e))?
                );
            }
            Ok(())
        }
        Err(Error::Remote {
            code,
            message,
            stacktrace,
        }) => {
            let err_obj = serde_json::json!({
                "code": code,
                "message": message,
                "stacktrace": stacktrace,
            });
            // Print structured JSON to stderr; main.rs translates the
            // RemoteAlreadyReported variant into a silent exit(1).
            eprintln!(
                "Error: {}",
                serde_json::to_string_pretty(&err_obj)
                    .unwrap_or_else(|_| "(unserializable error body)".to_string())
            );
            Err(TriggerCliError::RemoteAlreadyReported)
        }
        Err(e) => Err(TriggerCliError::Other(map_trigger_error(e))),
    }
}

fn map_trigger_error(e: Error) -> anyhow::Error {
    match e {
        Error::Timeout => anyhow::anyhow!(
            "Timed out waiting for the engine (no response within the timeout). Is the engine running at the given address and port?"
        ),
        Error::WebSocket(msg) => anyhow::anyhow!("WebSocket error: {}", msg),
        other => anyhow::Error::new(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iii::workers::telemetry::environment::CI_ENV_VARS;
    use serial_test::serial;

    /// Runs `body` with the process looking like a developer machine whose
    /// telemetry setting is `enabled`. `temp_env` restores every variable on the
    /// way out, including on a panic.
    ///
    /// The whole CI set has to be cleared, not just `CI`: `is_telemetry_disabled`
    /// treats any CI marker as an opt-out and `III_TELEMETRY_ENABLED=true` does
    /// not override it, so clearing one variable passes locally and fails on a
    /// runner that also sets `GITHUB_ACTIONS`. The list comes from the predicate's
    /// own constant so the two cannot drift apart.
    fn as_developer_machine(enabled: &str, body: impl FnOnce()) {
        let mut vars = vec![
            ("III_TELEMETRY_ENABLED", Some(enabled)),
            ("III_TELEMETRY_DEV", None),
        ];
        vars.extend(CI_ENV_VARS.iter().map(|var| (*var, None)));
        temp_env::with_vars(vars, body);
    }

    #[test]
    #[serial]
    fn cli_worker_metadata_marks_the_name_when_telemetry_is_enabled() {
        as_developer_machine("true", || {
            let metadata = cli_worker_metadata().expect("metadata when enabled");
            assert_eq!(metadata.name, "iii-cli:trigger");
        });
    }

    #[test]
    #[serial]
    fn cli_worker_metadata_is_absent_when_the_cli_opted_out() {
        // The engine counts whatever name it receives, so an opted-out CLI must
        // not mark itself even when the target engine has telemetry enabled.
        as_developer_machine("false", || {
            assert!(cli_worker_metadata().is_none());
        });
    }
}
