// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration with the builtin `configuration` worker.
//!
//! The `iii-observability` worker registers its config schema under the id
//! `iii-observability`, seeds it from the config.yaml block (or
//! [`ObservabilityWorkerConfig::default`] when auto-injected with no block)
//! only when no value is stored yet, reads the live value (with
//! `${VAR:default}` expansion), and hot-applies `configuration:updated`
//! events per field tier (live globals, storage limits, sampler/alerts/
//! level/collapse swaps, log exporter task rebuilds). After first boot the
//! configuration worker entry is the runtime source of truth; the
//! config.yaml block is seed-only — restart-tier fields (trace exporter
//! wiring, resource identity, log format) are read from the persisted
//! entry at the next engine start.

use anyhow::anyhow;
use serde_json::{Value, json};

use super::{ObservabilityWorker, config::ObservabilityWorkerConfig};
use crate::{
    engine::{Engine, EngineTrait},
    trigger::Trigger,
};

pub const CONFIG_ID: &str = "iii-observability";
pub const CONFIG_FN_ID: &str = "iii-observability::on-config-change";
pub const CONFIG_TRIGGER_ID: &str = "iii-observability::config-watch";
pub const CONFIG_TRIGGER_TYPE: &str = "configuration";

/// Upper bound on every `configuration::*` bus call made by this worker.
/// Worker startup is awaited serially by the boot and reload pipelines, so a
/// hung provider must wedge neither the apply lock nor the startup loops.
pub(super) const CONFIG_BUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Delay before the single retry of a timed-out apply (see `on_config_change`).
const APPLY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Register the `iii-observability` configuration entry: schema and metadata
/// refresh on every boot; `initial_value` (the config.yaml seed, or
/// [`ObservabilityWorkerConfig::default`]) is included only when nothing is
/// stored yet, so runtime edits survive engine restarts.
///
/// Makes unbounded bus calls — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)`.
pub async fn register_config(
    engine: &Engine,
    seed: Option<&ObservabilityWorkerConfig>,
) -> anyhow::Result<()> {
    let mut payload = json!({
        "id": CONFIG_ID,
        "name": "Observability",
        "description": "OpenTelemetry settings — trace/metric/log exporters, sampling \
                        (ratio, per-operation rules, rate limits), in-memory store limits \
                        and retention, alert rules, span-collapse rules, and the engine \
                        log level/format.",
        "schema": serde_json::to_value(schemars::schema_for!(ObservabilityWorkerConfig))?,
    });

    if try_get_value(engine).await?.is_none() {
        payload["initial_value"] = serde_json::to_value(seed.cloned().unwrap_or_default())?;
    }

    engine
        .call("configuration::register", payload)
        .await
        .map_err(|err| {
            anyhow!(
                "configuration::register failed: {} ({})",
                err.message,
                err.code
            )
        })?;
    Ok(())
}

/// Read the live configuration value. `${VAR:default}` placeholders are
/// expanded by `configuration::get`. A missing or null value falls back to
/// the supplied config; a malformed stored value is an error so the caller
/// keeps its previous config. Out-of-range values that predate a schema
/// tightening (or were hand-edited on disk) are clamped by `normalized()`.
///
/// Makes an unbounded bus call — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)`.
pub async fn fetch_config(
    engine: &Engine,
    fallback: &ObservabilityWorkerConfig,
) -> anyhow::Result<ObservabilityWorkerConfig> {
    let Some(value) = try_get_value(engine).await? else {
        tracing::info!(
            "no `{}` configuration value stored; using static configuration",
            CONFIG_ID
        );
        return Ok(fallback.clone().normalized());
    };

    let config: ObservabilityWorkerConfig = serde_json::from_value(value)
        .map_err(|err| anyhow!("stored `{CONFIG_ID}` configuration is invalid: {err}"))?;
    Ok(config.normalized())
}

async fn try_get_value(engine: &Engine) -> anyhow::Result<Option<Value>> {
    match engine
        .call("configuration::get", json!({ "id": CONFIG_ID }))
        .await
    {
        Ok(response) => Ok(response
            .and_then(|body| body.get("value").cloned())
            .filter(|value| !value.is_null())),
        Err(err) if err.code == "NOT_FOUND" => Ok(None),
        Err(err) => Err(anyhow!(
            "configuration::get failed: {} ({})",
            err.message,
            err.code
        )),
    }
}

/// Handler body for `iii-observability::on-config-change`. Delegates to
/// `apply_config`, which re-fetches the authoritative value under the apply
/// lock instead of trusting the trigger payload — the handler is a
/// discoverable bus function, and acting on a caller-supplied payload would
/// let anyone repoint telemetry without updating persisted state. Any
/// failure keeps the previous configuration.
pub async fn on_config_change(worker: &ObservabilityWorker) {
    match worker.apply_config().await {
        Ok(()) => tracing::info!("iii-observability configuration re-applied after change"),
        // A timeout is transient: the stored value is valid but unapplied,
        // and the event will not fire again — so retry exactly once after a
        // delay. The retry calls `apply_config` directly (not this handler),
        // so it cannot loop. Other errors (malformed value) are
        // deterministic; retrying would just repeat the failure.
        Err(err) if err.downcast_ref::<tokio::time::error::Elapsed>().is_some() => {
            tracing::error!(
                error = %err,
                "iii-observability: configuration apply timed out; retrying once in {APPLY_RETRY_DELAY:?}"
            );
            let worker = worker.clone();
            tokio::spawn(async move {
                tokio::time::sleep(APPLY_RETRY_DELAY).await;
                // Best-effort: skip the retry if the worker was already
                // destroyed during the delay. This is a point-in-time check,
                // not a lock — a destroy that races just after it is still
                // safe: `apply_config` re-checks the worker lifecycle under its
                // lock before respawning any task, and the remaining live-tier
                // writes are idempotent global-snapshot swaps.
                if !worker.is_active() {
                    tracing::debug!(
                        "iii-observability: worker no longer active; skipping configuration apply retry"
                    );
                    return;
                }
                match worker.apply_config().await {
                    Ok(()) => {
                        tracing::info!("iii-observability configuration re-applied on retry")
                    }
                    Err(err) => tracing::error!(
                        error = %err,
                        "iii-observability: configuration apply retry failed; keeping previous config"
                    ),
                }
            });
        }
        Err(err) => tracing::error!(
            error = %err,
            "iii-observability: failed to apply changed configuration; keeping previous config"
        ),
    }
}

/// Subscribe to `configuration:updated` events for the `iii-observability`
/// entry. The deterministic trigger id means re-registration replaces rather
/// than duplicates.
pub async fn register_config_trigger(engine: &Engine) -> anyhow::Result<()> {
    engine
        .trigger_registry
        .register_trigger(Trigger {
            id: CONFIG_TRIGGER_ID.to_string(),
            trigger_type: CONFIG_TRIGGER_TYPE.to_string(),
            function_id: CONFIG_FN_ID.to_string(),
            config: json!({
                "configuration_id": CONFIG_ID,
                "event_types": ["configuration:updated"],
            }),
            worker_id: None,
            metadata: None,
        })
        .await
        .map_err(|err| anyhow!("failed to register configuration trigger: {err:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;
    use crate::{
        engine::{Handler, RegisterFunctionRequest},
        function::FunctionResult,
        workers::observability::metrics::ensure_default_meter,
    };

    /// Stub `configuration::get` to return a fixed stored value (`None` →
    /// NOT_FOUND) and capture `configuration::register` payloads.
    fn stub_configuration(
        engine: &Arc<Engine>,
        stored_value: Option<Value>,
    ) -> mpsc::UnboundedReceiver<Value> {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: Value| {
                let stored_value = stored_value.clone();
                async move {
                    match stored_value {
                        Some(value) => FunctionResult::Success(Some(
                            json!({ "id": CONFIG_ID, "value": value }),
                        )),
                        None => FunctionResult::Failure(crate::protocol::ErrorBody {
                            message: format!("configuration '{CONFIG_ID}' not found"),
                            code: "NOT_FOUND".to_string(),
                            stacktrace: None,
                        }),
                    }
                }
            }),
        );

        let (tx, rx) = mpsc::unbounded_channel::<Value>();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::register".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(input);
                    FunctionResult::Success(Some(json!({})))
                }
            }),
        );
        rx
    }

    #[tokio::test]
    async fn register_seeds_initial_value_when_nothing_stored() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let mut registered = stub_configuration(&engine, None);

        let seed = ObservabilityWorkerConfig {
            logs_max_count: Some(4242),
            ..ObservabilityWorkerConfig::default()
        };
        register_config(&engine, Some(&seed)).await.unwrap();

        let payload = registered.recv().await.unwrap();
        assert_eq!(payload["id"], CONFIG_ID);
        assert_eq!(payload["initial_value"]["logs_max_count"], 4242);
        // None fields must not be seeded as nulls.
        assert!(
            payload["initial_value"].get("endpoint").is_none(),
            "unset fields must not seed: {payload}"
        );
        // schemars derives deny_unknown_fields into the schema.
        assert_eq!(payload["schema"]["additionalProperties"], json!(false));
        // Field doc comments must flow into the schema so an agent
        // introspecting the config gets descriptions, not just types.
        assert!(
            payload["schema"]["properties"]["enabled"]["description"].is_string(),
            "enabled field must carry a schema description: {payload}"
        );
        // Range constraints reject dangerous values at configuration::set
        // time (a zero logs_batch_size would stall the exporter loop).
        assert_eq!(
            payload["schema"]["properties"]["logs_batch_size"]["minimum"],
            json!(1.0),
            "logs_batch_size must carry minimum 1: {payload}"
        );
        assert_eq!(
            payload["schema"]["properties"]["sampling_ratio"]["maximum"],
            json!(1.0),
            "sampling_ratio must carry maximum 1: {payload}"
        );
        // Nested rule types must reject unknown keys at set time too.
        assert_eq!(
            payload["schema"]["definitions"]["AlertRule"]["additionalProperties"],
            json!(false),
            "AlertRule schema must deny unknown fields: {payload}"
        );
    }

    #[tokio::test]
    async fn register_omits_initial_value_when_value_stored() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let mut registered = stub_configuration(&engine, Some(json!({ "logs_max_count": 9999 })));

        register_config(&engine, Some(&ObservabilityWorkerConfig::default()))
            .await
            .unwrap();

        let payload = registered.recv().await.unwrap();
        assert!(
            payload.get("initial_value").is_none(),
            "stored value must not be clobbered: {payload}"
        );
    }

    #[tokio::test]
    async fn fetch_config_falls_back_when_not_found() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, None);

        let fallback = ObservabilityWorkerConfig {
            logs_max_count: Some(5555),
            ..ObservabilityWorkerConfig::default()
        };
        let config = fetch_config(&engine, &fallback).await.unwrap();
        assert_eq!(config.logs_max_count, Some(5555));
    }

    #[tokio::test]
    async fn fetch_config_falls_back_on_null_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(Value::Null));

        let config = fetch_config(&engine, &ObservabilityWorkerConfig::default())
            .await
            .unwrap();
        assert_eq!(config, ObservabilityWorkerConfig::default());
    }

    #[tokio::test]
    async fn fetch_config_errors_on_malformed_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered =
            stub_configuration(&engine, Some(json!({ "logs_max_count": "not-a-count" })));

        let result = fetch_config(&engine, &ObservabilityWorkerConfig::default()).await;
        assert!(result.is_err(), "malformed value must surface as an error");
    }

    #[tokio::test]
    async fn fetch_config_normalizes_out_of_range_values() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        // A stored value that predates the schema range constraints.
        let _registered = stub_configuration(
            &engine,
            Some(json!({ "sampling_ratio": 7.5, "memory_max_spans": 0 })),
        );

        let config = fetch_config(&engine, &ObservabilityWorkerConfig::default())
            .await
            .unwrap();
        assert_eq!(config.sampling_ratio, Some(1.0));
        assert_eq!(config.memory_max_spans, None);
    }
}
