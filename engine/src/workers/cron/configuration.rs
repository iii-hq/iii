// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration with the builtin `configuration` worker.
//!
//! The `iii-cron` worker registers its config schema under the id `iii-cron`,
//! seeds it from the config.yaml block only when no value is stored yet, reads
//! the live value (with `${VAR:default}` expansion) before building the
//! scheduler, and hot-applies `configuration:updated` events. After first boot
//! the configuration worker entry is the runtime source of truth; the
//! config.yaml block is seed-only.

use anyhow::anyhow;
use serde_json::{Value, json};

use super::{config::CronModuleConfig, cron::CronWorker};
use crate::{
    engine::{Engine, EngineTrait},
    trigger::Trigger,
};

pub const CONFIG_ID: &str = "iii-cron";
pub const CONFIG_FN_ID: &str = "iii-cron::on-config-change";
pub const CONFIG_TRIGGER_ID: &str = "iii-cron::config-watch";
pub const CONFIG_TRIGGER_TYPE: &str = "configuration";

/// Upper bound on every `configuration::*` bus call made by this worker. The
/// boot and reload pipelines await worker startup serially, so a hung
/// configuration provider must wedge neither the apply lock nor the startup
/// loop behind this worker.
pub(super) const CONFIG_BUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Delay before the single retry of a timed-out apply (see `on_config_change`).
const APPLY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Register the `iii-cron` configuration entry: schema and metadata refresh on
/// every boot; `initial_value` (the config.yaml seed, or built-in defaults) is
/// included only when nothing is stored yet, so runtime edits survive engine
/// restarts.
///
/// Makes unbounded bus calls — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `CronWorker::start_background_tasks`).
pub async fn register_config(
    engine: &Engine,
    seed: Option<&CronModuleConfig>,
) -> anyhow::Result<()> {
    let mut payload = json!({
        "id": CONFIG_ID,
        "name": "Cron",
        "description": "Cron worker settings — the distributed-lock backend (adapter/transport) used to coordinate scheduled job execution across engine instances.",
        "schema": serde_json::to_value(schemars::schema_for!(CronModuleConfig))?,
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
/// expanded by `configuration::get`. A missing or null value falls back to the
/// supplied config; a malformed stored value is an error so the caller keeps
/// its previous config.
///
/// Makes an unbounded bus call — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `CronWorker::start_background_tasks` and `CronWorker::apply_config`).
pub async fn fetch_config(
    engine: &Engine,
    fallback: &CronModuleConfig,
) -> anyhow::Result<CronModuleConfig> {
    let Some(value) = try_get_value(engine).await? else {
        tracing::info!(
            "no `{}` configuration value stored; using static configuration",
            CONFIG_ID
        );
        return Ok(fallback.clone());
    };

    let config: CronModuleConfig = serde_json::from_value(value)
        .map_err(|err| anyhow!("stored `{CONFIG_ID}` configuration is invalid: {err}"))?;
    Ok(config)
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

/// Handler body for `iii-cron::on-config-change`. Delegates to `apply_config`,
/// which re-fetches the authoritative value under the apply lock instead of
/// trusting the trigger payload — the handler is a discoverable bus function,
/// and acting on a caller-supplied payload would let anyone repoint the
/// listener without updating persisted state. Any failure keeps the previous
/// config and the running scheduler.
pub async fn on_config_change(worker: &CronWorker) {
    match worker.apply_config().await {
        Ok(()) => tracing::info!("iii-cron configuration re-applied after change"),
        // A timeout is transient: the stored value is valid but unapplied, and
        // the event will not fire again — so retry exactly once after a delay.
        // The retry calls `apply_config` directly (not this handler), so it
        // cannot loop. Other errors (malformed value, failed adapter rebuild)
        // are deterministic; retrying them would just repeat the failure.
        Err(err) if err.downcast_ref::<tokio::time::error::Elapsed>().is_some() => {
            tracing::error!(
                error = %err,
                "iii-cron: configuration apply timed out; retrying once in {APPLY_RETRY_DELAY:?}"
            );
            let worker = worker.clone();
            tokio::spawn(async move {
                tokio::time::sleep(APPLY_RETRY_DELAY).await;
                match worker.apply_config().await {
                    Ok(()) => tracing::info!("iii-cron configuration re-applied on retry"),
                    Err(err) => tracing::error!(
                        error = %err,
                        "iii-cron: configuration apply retry failed; keeping previous config"
                    ),
                }
            });
        }
        Err(err) => tracing::error!(
            error = %err,
            "iii-cron: failed to apply changed configuration; keeping previous config"
        ),
    }
}

/// Subscribe to `configuration:updated` events for the `iii-cron` entry. The
/// deterministic trigger id means re-registration replaces rather than
/// duplicates.
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
            namespace: crate::protocol::default_namespace(),
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
        workers::traits::AdapterEntry,
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

        let seed = CronModuleConfig {
            adapter: Some(AdapterEntry {
                name: "redis".to_string(),
                config: None,
            }),
        };
        register_config(&engine, Some(&seed)).await.unwrap();

        let payload = registered.recv().await.unwrap();
        assert_eq!(payload["id"], CONFIG_ID);
        assert_eq!(payload["initial_value"]["adapter"]["name"], "redis");
        // schemars derives deny_unknown_fields into the schema.
        assert_eq!(payload["schema"]["additionalProperties"], json!(false));
        // The adapter field's doc comment must flow into the schema so an agent
        // introspecting the config gets a description, not just a type.
        assert!(
            payload["schema"]["properties"]["adapter"]["description"].is_string(),
            "adapter field must carry a schema description: {payload}"
        );
    }

    #[tokio::test]
    async fn register_omits_initial_value_when_value_stored() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let mut registered =
            stub_configuration(&engine, Some(json!({ "adapter": { "name": "kv" } })));

        register_config(&engine, Some(&CronModuleConfig::default()))
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

        let fallback = CronModuleConfig {
            adapter: Some(AdapterEntry {
                name: "redis".to_string(),
                config: None,
            }),
        };
        let config = fetch_config(&engine, &fallback).await.unwrap();
        assert_eq!(config.adapter.map(|a| a.name), Some("redis".to_string()));
    }

    #[tokio::test]
    async fn fetch_config_falls_back_on_null_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(Value::Null));

        let config = fetch_config(&engine, &CronModuleConfig::default())
            .await
            .unwrap();
        assert!(config.adapter.is_none());
    }

    #[tokio::test]
    async fn fetch_config_errors_on_malformed_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        // `adapter.name` must be a string; a number fails deserialization.
        let _registered = stub_configuration(&engine, Some(json!({ "adapter": { "name": 42 } })));

        let result = fetch_config(&engine, &CronModuleConfig::default()).await;
        assert!(result.is_err(), "malformed value must surface as an error");
    }

    #[tokio::test]
    async fn on_config_change_keeps_previous_config_on_unresolvable_adapter() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        // An adapter name that is not registered deserializes fine and passes
        // the JSON schema, so it reaches the worker's apply gate — where
        // resolving the transport fails and the previous config must stand.
        let _registered = stub_configuration(
            &engine,
            Some(json!({ "adapter": { "name": "does-not-exist" } })),
        );

        let worker = CronWorker::for_test(engine.clone(), Some(json!({})))
            .await
            .expect("cron worker");

        on_config_change(&worker).await;

        assert!(
            worker.config_snapshot().adapter.is_none(),
            "an unresolvable adapter must not mutate the live config"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn on_config_change_retries_once_after_timeout() {
        use std::sync::atomic::{AtomicU64, Ordering};
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        // `configuration::get` hangs on the first call (forcing the apply to time
        // out) and returns a valid, adapter-unchanged value on the second, so the
        // one-shot retry scheduled by `on_config_change` re-fetches and succeeds.
        let calls = Arc::new(AtomicU64::new(0));
        let calls_handler = calls.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: Value| {
                let n = calls_handler.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n == 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    }
                    FunctionResult::Success(Some(json!({ "id": CONFIG_ID, "value": {} })))
                }
            }),
        );

        let worker = CronWorker::for_test(engine.clone(), Some(json!({})))
            .await
            .expect("cron worker");

        on_config_change(&worker).await;
        // The first apply timed out and scheduled a retry ~APPLY_RETRY_DELAY out.
        // Advance virtual time so the detached retry task runs and re-fetches.
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        tokio::task::yield_now().await;

        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "a timed-out apply must schedule exactly one retry that re-fetches"
        );
    }
}
