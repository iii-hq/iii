// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration with the builtin `configuration` worker.
//!
//! The `iii-queue` worker registers its config schema under the id `iii-queue`,
//! seeds it from the config.yaml block only when no value is stored yet, reads
//! the live value (with `${VAR:default}` expansion) before starting consumers,
//! and hot-applies `configuration:updated` events. After first boot the
//! configuration worker entry is the runtime source of truth; the config.yaml
//! block is seed-only.

use anyhow::anyhow;
use serde_json::{Value, json};

use super::{config::QueueModuleConfig, queue::QueueWorker};
use crate::{
    engine::{Engine, EngineTrait},
    trigger::Trigger,
};

pub const CONFIG_ID: &str = "iii-queue";
pub const CONFIG_FN_ID: &str = "iii-queue::on-config-change";
pub const CONFIG_TRIGGER_ID: &str = "iii-queue::config-watch";
pub const CONFIG_TRIGGER_TYPE: &str = "configuration";

/// Upper bound on every `configuration::*` bus call made by this worker. The
/// boot and reload pipelines await worker startup serially, so a hung
/// configuration provider must wedge neither the apply lock nor the startup
/// loop behind this worker.
pub(super) const CONFIG_BUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Delay before the single retry of a timed-out apply (see `on_config_change`).
const APPLY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Register the `iii-queue` configuration entry: schema and metadata refresh on
/// every boot; `initial_value` (the config.yaml seed, or built-in defaults) is
/// included only when nothing is stored yet, so runtime edits survive engine
/// restarts.
///
/// Makes unbounded bus calls — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `QueueWorker::start_background_tasks`).
pub async fn register_config(
    engine: &Engine,
    seed: Option<&QueueModuleConfig>,
) -> anyhow::Result<()> {
    let mut payload = json!({
        "id": CONFIG_ID,
        "name": "Queue",
        "description": "Queue worker settings — the backing adapter/transport and per-queue config (retries, concurrency, FIFO ordering, backoff).",
        "schema": serde_json::to_value(schemars::schema_for!(QueueModuleConfig))?,
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
/// its previous config. The always-present `default` queue is provisioned on
/// every returned value (the queue analogue of normalization).
///
/// Makes an unbounded bus call — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `QueueWorker::start_background_tasks` and `QueueWorker::apply_config`).
pub async fn fetch_config(
    engine: &Engine,
    fallback: &QueueModuleConfig,
) -> anyhow::Result<QueueModuleConfig> {
    let Some(value) = try_get_value(engine).await? else {
        tracing::info!(
            "no `{}` configuration value stored; using static configuration",
            CONFIG_ID
        );
        let mut config = fallback.clone();
        config.ensure_default_queue();
        return Ok(config);
    };

    let mut config: QueueModuleConfig = serde_json::from_value(value)
        .map_err(|err| anyhow!("stored `{CONFIG_ID}` configuration is invalid: {err}"))?;
    config.ensure_default_queue();
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

/// Handler body for `iii-queue::on-config-change`. Delegates to `apply_config`,
/// which re-fetches the authoritative value under the apply lock instead of
/// trusting the trigger payload — the handler is a discoverable bus function,
/// and acting on a caller-supplied payload would let anyone repoint the
/// listener without updating persisted state. Any failure keeps the previous
/// config and the running consumers.
pub async fn on_config_change(worker: &QueueWorker) {
    match worker.apply_config().await {
        Ok(()) => tracing::info!("iii-queue configuration re-applied after change"),
        // A timeout is transient: the stored value is valid but unapplied, and
        // the event will not fire again — so retry exactly once after a delay.
        // The retry calls `apply_config` directly (not this handler), so it
        // cannot loop. Other errors (malformed value, failed adapter rebuild)
        // are deterministic; retrying them would just repeat the failure.
        Err(err) if err.downcast_ref::<tokio::time::error::Elapsed>().is_some() => {
            tracing::error!(
                error = %err,
                "iii-queue: configuration apply timed out; retrying once in {APPLY_RETRY_DELAY:?}"
            );
            let worker = worker.clone();
            tokio::spawn(async move {
                tokio::time::sleep(APPLY_RETRY_DELAY).await;
                match worker.apply_config().await {
                    Ok(()) => tracing::info!("iii-queue configuration re-applied on retry"),
                    Err(err) => tracing::error!(
                        error = %err,
                        "iii-queue: configuration apply retry failed; keeping previous config"
                    ),
                }
            });
        }
        Err(err) => tracing::error!(
            error = %err,
            "iii-queue: failed to apply changed configuration; keeping previous config"
        ),
    }
}

/// Subscribe to `configuration:updated` events for the `iii-queue` entry. The
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
        workers::queue::config::DEFAULT_QUEUE_NAME,
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

        let mut queue_configs = std::collections::HashMap::new();
        queue_configs.insert(
            "reports".to_string(),
            crate::workers::queue::config::FunctionQueueConfig {
                concurrency: 7,
                ..Default::default()
            },
        );
        let seed = QueueModuleConfig {
            adapter: None,
            queue_configs,
        };
        register_config(&engine, Some(&seed)).await.unwrap();

        let payload = registered.recv().await.unwrap();
        assert_eq!(payload["id"], CONFIG_ID);
        assert_eq!(
            payload["initial_value"]["queue_configs"]["reports"]["concurrency"],
            7
        );
        // schemars derives deny_unknown_fields into the schema.
        assert_eq!(payload["schema"]["additionalProperties"], json!(false));
        // Field doc comments must flow into the schema so an agent
        // introspecting the config gets descriptions, not just types.
        assert!(
            payload["schema"]["properties"]["queue_configs"]["description"].is_string(),
            "queue_configs field must carry a schema description: {payload}"
        );
        // Per-queue entries must reject unknown keys at set time too — a typo'd
        // queue option silently ignored is a configuration footgun.
        assert_eq!(
            payload["schema"]["definitions"]["FunctionQueueConfig"]["additionalProperties"],
            json!(false),
            "FunctionQueueConfig schema must deny unknown fields: {payload}"
        );
        assert!(
            payload["schema"]["definitions"]["FunctionQueueConfig"]["properties"]["concurrency"]
                ["description"]
                .is_string(),
            "concurrency field must carry a schema description: {payload}"
        );
    }

    #[tokio::test]
    async fn register_omits_initial_value_when_value_stored() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let mut registered =
            stub_configuration(&engine, Some(json!({ "queue_configs": { "x": {} } })));

        register_config(&engine, Some(&QueueModuleConfig::default()))
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

        let config = fetch_config(&engine, &QueueModuleConfig::default())
            .await
            .unwrap();
        // The always-present default queue is provisioned even on the fallback.
        assert!(config.queue_configs.contains_key(DEFAULT_QUEUE_NAME));
    }

    #[tokio::test]
    async fn fetch_config_falls_back_on_null_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(Value::Null));

        let config = fetch_config(&engine, &QueueModuleConfig::default())
            .await
            .unwrap();
        assert!(config.queue_configs.contains_key(DEFAULT_QUEUE_NAME));
    }

    #[tokio::test]
    async fn fetch_config_errors_on_malformed_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(
            &engine,
            Some(json!({ "queue_configs": { "x": { "concurrency": "not-a-number" } } })),
        );

        let result = fetch_config(&engine, &QueueModuleConfig::default()).await;
        assert!(result.is_err(), "malformed value must surface as an error");
    }

    #[tokio::test]
    async fn fetch_config_ensures_default_queue() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let _registered = stub_configuration(&engine, Some(json!({ "queue_configs": {} })));

        let config = fetch_config(&engine, &QueueModuleConfig::default())
            .await
            .unwrap();
        assert!(
            config.queue_configs.contains_key(DEFAULT_QUEUE_NAME),
            "a stored value lacking `default` must still gain it on read"
        );
    }

    #[tokio::test]
    async fn on_config_change_keeps_previous_config_on_invalid_value() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        // FIFO without `message_group_field` deserializes fine and passes the
        // JSON schema (which can't express the cross-field rule), so it reaches
        // the worker's `validate()` gate — which must reject it and keep the
        // previous live config.
        let _registered = stub_configuration(
            &engine,
            Some(json!({ "queue_configs": { "orders": { "type": "fifo" } } })),
        );

        let worker = QueueWorker::for_test(
            engine.clone(),
            Some(json!({ "queue_configs": { "default": { "concurrency": 3 } } })),
        )
        .await
        .expect("queue worker");

        on_config_change(&worker).await;

        let config = worker.config_snapshot();
        assert_eq!(
            config.queue_configs.get("default").map(|c| c.concurrency),
            Some(3),
            "an invalid stored value must not mutate the live config"
        );
        assert!(
            !config.queue_configs.contains_key("orders"),
            "the invalid fifo queue must not be applied"
        );
    }
}
