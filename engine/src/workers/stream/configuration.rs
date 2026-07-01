// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration with the builtin `configuration` worker.
//!
//! The `iii-stream` worker registers its config schema under the id
//! `iii-stream`, seeds it from the config.yaml block only when no value is
//! stored yet, reads the live value (with `${VAR:default}` expansion) before
//! binding, and hot-applies `configuration:updated` events. After first boot
//! the configuration worker entry is the runtime source of truth; the
//! config.yaml block is seed-only.

use anyhow::anyhow;
use serde_json::{Value, json};

use super::{config::StreamModuleConfig, stream::StreamWorker};
use crate::{
    engine::{Engine, EngineTrait},
    trigger::Trigger,
};

pub const CONFIG_ID: &str = "iii-stream";
pub const CONFIG_FN_ID: &str = "iii-stream::on-config-change";
pub const CONFIG_TRIGGER_ID: &str = "iii-stream::config-watch";
pub const CONFIG_TRIGGER_TYPE: &str = "configuration";

/// Upper bound on every `configuration::*` bus call made by this worker.
/// `configuration::get` is overwrite-by-id on the bus, so a hung provider
/// must wedge neither the apply lock nor — worse — the serial worker-startup
/// loops in the boot and reload pipelines.
pub(super) const CONFIG_BUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Delay before the single retry of a timed-out apply (see `on_config_change`).
const APPLY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Register the `iii-stream` configuration entry: schema and metadata refresh
/// on every boot; `initial_value` (the config.yaml seed, or built-in defaults)
/// is included only when nothing is stored yet, so runtime edits survive engine
/// restarts.
///
/// Makes unbounded bus calls — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `StreamWorker::start_background_tasks`).
pub async fn register_config(
    engine: &Engine,
    seed: Option<&StreamModuleConfig>,
) -> anyhow::Result<()> {
    let mut payload = json!({
        "id": CONFIG_ID,
        "name": "Stream",
        "description": "WebSocket stream server settings — host/port binding, connection auth function, and the pub/sub adapter backend.",
        "schema": serde_json::to_value(schemars::schema_for!(StreamModuleConfig))?,
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
/// `StreamWorker::start_background_tasks` and `StreamWorker::apply_config`).
pub async fn fetch_config(
    engine: &Engine,
    fallback: &StreamModuleConfig,
) -> anyhow::Result<StreamModuleConfig> {
    let Some(value) = try_get_value(engine).await? else {
        tracing::info!(
            "no `{}` configuration value stored; using static configuration",
            CONFIG_ID
        );
        return Ok(fallback.clone());
    };

    let config: StreamModuleConfig = serde_json::from_value(value)
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

/// Handler body for `iii-stream::on-config-change`. Delegates to `apply_config`,
/// which re-fetches the authoritative value under the apply lock instead of
/// trusting the trigger payload — the handler is a discoverable bus function,
/// and acting on a caller-supplied payload would let anyone repoint the
/// listener or pub/sub backend without updating persisted state. Any failure
/// keeps the previous config, server, and adapter.
pub async fn on_config_change(worker: &StreamWorker) {
    match worker.apply_config().await {
        Ok(()) => tracing::info!("iii-stream configuration re-applied after change"),
        // A timeout is transient: the stored value is valid but unapplied, and
        // the event will not fire again — so retry exactly once after a delay.
        // The retry calls `apply_config` directly (not this handler), so it
        // cannot loop. Other errors (malformed value, failed bind, unresolvable
        // adapter) are deterministic; retrying them would just repeat the
        // failure.
        Err(err) if err.downcast_ref::<tokio::time::error::Elapsed>().is_some() => {
            tracing::error!(
                error = %err,
                "iii-stream: configuration apply timed out; retrying once in {APPLY_RETRY_DELAY:?}"
            );
            let worker = worker.clone();
            tokio::spawn(async move {
                tokio::time::sleep(APPLY_RETRY_DELAY).await;
                match worker.apply_config().await {
                    Ok(()) => {
                        tracing::info!("iii-stream configuration re-applied on retry")
                    }
                    Err(err) => tracing::error!(
                        error = %err,
                        "iii-stream: configuration apply retry failed; keeping previous config"
                    ),
                }
            });
        }
        Err(err) => tracing::error!(
            error = %err,
            "iii-stream: failed to apply changed configuration; keeping previous config"
        ),
    }
}

/// Subscribe to `configuration:updated` events for the `iii-stream` entry. The
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

        let seed = StreamModuleConfig {
            port: 4242,
            ..StreamModuleConfig::default()
        };
        register_config(&engine, Some(&seed)).await.unwrap();

        let payload = registered.recv().await.unwrap();
        assert_eq!(payload["id"], CONFIG_ID);
        assert_eq!(payload["initial_value"]["port"], 4242);
        // schemars derives deny_unknown_fields into the schema.
        assert_eq!(payload["schema"]["additionalProperties"], json!(false));
        assert!(payload["schema"]["properties"]["port"].is_object());
        // Field doc comments must flow into the schema so an agent
        // introspecting the config gets descriptions, not just types.
        assert!(
            payload["schema"]["properties"]["port"]["description"].is_string(),
            "port field must carry a schema description: {payload}"
        );
        assert!(
            payload["schema"]["properties"]["adapter"]["description"].is_string(),
            "adapter field must carry a schema description: {payload}"
        );
    }

    #[tokio::test]
    async fn register_omits_initial_value_when_value_stored() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let mut registered = stub_configuration(&engine, Some(json!({ "port": 9999 })));

        register_config(&engine, Some(&StreamModuleConfig::default()))
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

        let fallback = StreamModuleConfig {
            port: 5555,
            ..StreamModuleConfig::default()
        };
        let config = fetch_config(&engine, &fallback).await.unwrap();
        assert_eq!(config.port, 5555);
    }

    #[tokio::test]
    async fn fetch_config_falls_back_on_null_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(Value::Null));

        let config = fetch_config(&engine, &StreamModuleConfig::default())
            .await
            .unwrap();
        assert_eq!(config.port, StreamModuleConfig::default().port);
    }

    #[tokio::test]
    async fn fetch_config_errors_on_malformed_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(json!({ "port": "not-a-port" })));

        let result = fetch_config(&engine, &StreamModuleConfig::default()).await;
        assert!(result.is_err(), "malformed value must surface as an error");
    }

    #[tokio::test]
    async fn fetch_config_reads_stored_adapter() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(
            &engine,
            Some(
                json!({ "adapter": { "name": "redis", "config": { "redis_url": "redis://x:6379" } } }),
            ),
        );

        let config = fetch_config(&engine, &StreamModuleConfig::default())
            .await
            .unwrap();
        let adapter = config.adapter.expect("stored adapter is read back");
        assert_eq!(adapter.name, "redis");
    }

    #[tokio::test]
    async fn on_config_change_keeps_previous_config_on_malformed_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(json!({ "port": "not-a-port" })));

        let worker = StreamWorker::for_test(
            engine.clone(),
            Some(json!({ "host": "127.0.0.1", "port": 4242 })),
        )
        .await
        .expect("stream worker");

        on_config_change(&worker).await;

        assert_eq!(
            worker.config_snapshot().port,
            4242,
            "a malformed stored value must not mutate the live config"
        );
    }

    #[tokio::test]
    async fn on_config_change_keeps_previous_config_on_unresolvable_adapter() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        // Deserializes and passes the schema, but no factory is registered for
        // this adapter name, so the apply gate rejects it.
        let _registered = stub_configuration(
            &engine,
            Some(json!({ "adapter": { "name": "does-not-exist" } })),
        );

        let worker = StreamWorker::for_test(
            engine.clone(),
            Some(json!({ "adapter": { "name": "kv", "config": { "lock_index": "keep" } } })),
        )
        .await
        .expect("stream worker");

        on_config_change(&worker).await;

        assert_eq!(
            worker
                .config_snapshot()
                .adapter
                .as_ref()
                .map(|a| a.name.as_str()),
            Some("kv"),
            "an unresolvable adapter must keep the previous config"
        );
    }
}
