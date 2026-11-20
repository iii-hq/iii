// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::io::IsTerminal;

use iii::workers::telemetry::collector::CLI_WORKER_NAME_PREFIX;
use iii_sdk::protocol::CallRequest;
use iii_sdk::{Error, IIIClient, InitOptions, register_worker};
use serde_json::{Value, json};

use super::CallCliError;
use super::streams::{Bindings, Carried, Interrupt, result_streams};

/// The call's timeout when nothing else names one.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// "No timeout" for a call that carries a stream: ten years, which the engine and the SDK still
/// add to an `Instant` without overflowing.
const NO_TIMEOUT_MS: u64 = 10 * 365 * 24 * 60 * 60 * 1000;

/// How one invocation runs.
pub struct Options<'a> {
    /// `--timeout-ms`: `None` is 30 s, or no limit for a call that binds a stream.
    pub timeout_ms: Option<u64>,
    pub namespace: Option<&'a str>,
    /// The fields `--stdout`, `--stderr` and `--stdin` bound.
    pub bindings: Bindings,
    /// Binds the descriptors no flag bound from the function's schema (`--no-bind` turns it off).
    pub auto_bind: bool,
    /// Follows the streams of the result (`--no-follow` turns it off).
    pub follow: bool,
    /// Prints the result after carried streams ended (`--print-result`).
    pub print_result: bool,
}

/// Metadata for the call connection. Registering under a marked name is how
/// the engine counts this command in the heartbeat's CLI aggregate: the CLI
/// process is too short-lived to report anything itself, and the connection it
/// already opens carries the name.
///
/// Returns `None` when this CLI has opted out of telemetry. The engine counts
/// whatever name it receives, so marking it regardless would let an engine with
/// telemetry enabled record commands from a CLI that opted out.
fn cli_worker_metadata() -> Option<iii_sdk::iii::WorkerMetadata> {
    (!crate::cli::telemetry::is_telemetry_disabled()).then(|| iii_sdk::iii::WorkerMetadata {
        name: format!("{CLI_WORKER_NAME_PREFIX}call"),
        ..Default::default()
    })
}

pub async fn invoke(
    function_path: &str,
    payload: Value,
    address: &str,
    port: u16,
    options: &Options<'_>,
) -> Result<(), CallCliError> {
    let url = format!("ws://{}:{}", address, port);
    let iii = register_worker(
        &url,
        InitOptions {
            metadata: cli_worker_metadata(),
            ..Default::default()
        },
    );
    let outcome = call(&iii, function_path, payload, options).await;
    iii.shutdown_async().await;
    outcome
}

async fn call(
    iii: &IIIClient,
    function_path: &str,
    mut payload: Value,
    options: &Options<'_>,
) -> Result<(), CallCliError> {
    let mut bindings = options.bindings.clone();
    if options.auto_bind && payload.is_object() {
        let budget = options
            .timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(DEFAULT_TIMEOUT_MS);
        match request_schema(iii, function_path, options.namespace, budget).await {
            Ok(Some(schema)) => bindings.complete_from_schema(&schema, &payload),
            Ok(None) => {}
            // An engine that cannot be reached for the schema cannot be reached for the call:
            // the same error, once, instead of a second wait.
            Err(error) => return Err(report_call_error(error)),
        }
    }

    let mut carried = Carried::default();
    let mut interrupt = None;
    if !bindings.is_empty() {
        carried
            .bind(iii, &bindings, &mut payload)
            .await
            .map_err(CallCliError::Usage)?;
        interrupt = Some(Interrupt::install().map_err(|error| anyhow::anyhow!(error))?);
    }
    let timeout_ms = options.timeout_ms.unwrap_or(if bindings.is_empty() {
        DEFAULT_TIMEOUT_MS
    } else {
        NO_TIMEOUT_MS
    });
    let request = CallRequest {
        function_id: function_path.to_string(),
        payload,
        action: None,
        timeout_ms: Some(timeout_ms),
    };
    // Omitted when absent so the engine keeps resolving in `default`: the
    // caller's own namespace never influences routing.
    let call = async {
        match options.namespace {
            Some(namespace) => iii.call(request.namespace(namespace)).await,
            None => iii.call(request).await,
        }
    };
    // A signal before the answer is a stop: the streams close, the function stops what it runs
    // and answers, and that answer is what this command exits with.
    let result = match &interrupt {
        None => call.await,
        Some(interrupt) => {
            tokio::pin!(call);
            tokio::select! {
                result = &mut call => result,
                _ = interrupt.requested() => {
                    carried.cancel().await;
                    call.await
                }
            }
        }
    };
    let answered_after_signal = interrupt.as_ref().and_then(Interrupt::received);

    let value = match result {
        Ok(value) => value,
        Err(error) => {
            carried.cancel().await;
            let _ = carried.finish().await;
            return Err(report_call_error(error));
        }
    };

    if options.follow {
        let streams = result_streams(&value);
        if !streams.is_empty() {
            if interrupt.is_none() {
                interrupt = Some(Interrupt::install().map_err(|error| anyhow::anyhow!(error))?);
            }
            carried.follow(iii, streams).map_err(CallCliError::Other)?;
        }
    }
    let streamed = !carried.is_empty();

    // The streams drain once the function closed its ends. A signal now, after the answer, has
    // nothing left to wait for.
    let drained = match &interrupt {
        Some(interrupt) if answered_after_signal.is_none() => {
            let ended = {
                let finish = carried.finish();
                tokio::pin!(finish);
                tokio::select! {
                    drained = &mut finish => Ok(drained),
                    signal = interrupt.requested() => Err(signal),
                }
            };
            match ended {
                Ok(drained) => drained,
                Err(signal) => {
                    carried.cancel().await;
                    let _ = carried.finish().await;
                    return Err(CallCliError::Interrupted(signal));
                }
            }
        }
        _ => carried.finish().await,
    };
    if answered_after_signal.is_none()
        && let Err(failure) = drained
    {
        return Err(CallCliError::Other(anyhow::anyhow!(failure)));
    }

    if (!streamed || options.print_result) && !value.is_null() {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| anyhow::anyhow!(e))?
        );
    }
    Ok(())
}

/// The function's request schema from `engine::functions::info`, to bind streams by. `None` when
/// the engine answers without one (an unknown function, an engine without the function): the
/// call itself then says what is wrong. An engine that does not answer is an error.
async fn request_schema(
    iii: &IIIClient,
    function_path: &str,
    namespace: Option<&str>,
    timeout_ms: u64,
) -> Result<Option<Value>, Error> {
    let mut payload = json!({ "function_id": function_path });
    if let Some(namespace) = namespace {
        payload["namespace"] = json!(namespace);
    }
    let info = match iii
        .call(CallRequest {
            function_id: "engine::functions::info".to_string(),
            payload,
            action: None,
            timeout_ms: Some(timeout_ms),
        })
        .await
    {
        Ok(info) => info,
        Err(Error::Remote { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(info
        .get("request_schema")
        .or_else(|| info.get("request_format"))
        .filter(|schema| schema.is_object())
        .cloned())
}

/// Prints a failed call and turns it into this command's error. A function's own error goes to
/// stderr as JSON, on one line when stderr is not a terminal so a caller can read it.
fn report_call_error(error: Error) -> CallCliError {
    match error {
        Error::Remote {
            code,
            message,
            stacktrace,
        } => {
            let err_obj = serde_json::json!({
                "code": code,
                "message": message,
                "stacktrace": stacktrace,
            });
            let rendered = if std::io::stderr().is_terminal() {
                serde_json::to_string_pretty(&err_obj)
            } else {
                serde_json::to_string(&err_obj)
            };
            // main.rs translates RemoteAlreadyReported into a silent exit(1).
            eprintln!(
                "Error: {}",
                rendered.unwrap_or_else(|_| "(unserializable error body)".to_string())
            );
            CallCliError::RemoteAlreadyReported
        }
        error => CallCliError::Other(map_call_error(error)),
    }
}

fn map_call_error(e: Error) -> anyhow::Error {
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
            assert_eq!(metadata.name, "iii-cli:call");
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
