// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration with the builtin `configuration` worker.
//!
//! The `iii-state` worker registers its config schema under the id `iii-state`,
//! seeds it from the config.yaml block only when no value is stored yet, reads
//! the live value (with `${VAR:default}` expansion) on boot, and hot-applies
//! `configuration:updated` events. After first boot the configuration worker
//! entry is the runtime source of truth; the config.yaml block is seed-only.
//!
//! Tiers (see `StateWorker::apply_config`): `triggers_enabled` and
//! `max_value_bytes` apply live; `save_interval_ms` respawns the kv save loop;
//! the storage `adapter` is restart-tier (applied at the next engine start via
//! the boot-read).

use std::path::Path;

use anyhow::anyhow;
use serde_json::{Value, json};

use super::{StateModuleConfig, StateWorker};
use crate::{
    engine::{Engine, EngineTrait},
    trigger::Trigger,
};

pub const CONFIG_ID: &str = "iii-state";
pub const CONFIG_FN_ID: &str = "iii-state::on-config-change";
pub const CONFIG_TRIGGER_ID: &str = "iii-state::config-watch";
pub const CONFIG_TRIGGER_TYPE: &str = "configuration";

/// Upper bound on every `configuration::*` bus call made by this worker.
/// `configuration::get`/`register` are overwrite-by-id on the bus, so a hung
/// provider must wedge neither the apply lock nor — worse — the serial
/// worker-startup loops in the boot and reload pipelines.
pub(super) const CONFIG_BUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Delay before the single retry of a timed-out apply (see `on_config_change`).
const APPLY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Register the `iii-state` configuration entry: schema and metadata refresh on
/// every boot; `initial_value` (the config.yaml seed, or built-in defaults) is
/// included only when nothing is stored yet, so runtime edits survive engine
/// restarts.
///
/// Makes unbounded bus calls — callers must wrap the call in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `StateWorker::start_background_tasks`).
pub async fn register_config(
    engine: &Engine,
    seed: Option<&StateModuleConfig>,
) -> anyhow::Result<()> {
    let mut payload = json!({
        "id": CONFIG_ID,
        "name": "State",
        "description": "State worker settings — storage adapter selection (restart-tier) plus live trigger fan-out (triggers_enabled), per-write size guard (max_value_bytes), and the file-backed kv save cadence (save_interval_ms).",
        "schema": serde_json::to_value(schemars::schema_for!(StateModuleConfig))?,
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
/// Makes an unbounded bus call — callers must wrap it in
/// `tokio::time::timeout(CONFIG_BUS_TIMEOUT, ...)` (see
/// `StateWorker::start_background_tasks` and `StateWorker::apply_config`).
pub async fn fetch_config(
    engine: &Engine,
    fallback: &StateModuleConfig,
) -> anyhow::Result<StateModuleConfig> {
    let Some(value) = try_get_value(engine).await? else {
        tracing::info!(
            "no `{}` configuration value stored; using static configuration",
            CONFIG_ID
        );
        // Normalized as invariant hardening: every current caller already
        // passes an already-normalized snapshot, but a future caller handing in
        // a raw config must not bypass the zero-knob clamp. Idempotent, so a
        // no-op for today's callers.
        return Ok(fallback.clone().normalized());
    };

    let config: StateModuleConfig = serde_json::from_value(value)
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

/// Handler body for `iii-state::on-config-change`. Delegates to `apply_config`,
/// which re-fetches the authoritative value under the apply lock instead of
/// trusting the trigger payload — the handler is a discoverable bus function,
/// and acting on a caller-supplied payload would let anyone repoint the worker
/// without updating persisted state. Any failure keeps the previous config.
pub async fn on_config_change(worker: &StateWorker) {
    match worker.apply_config().await {
        Ok(()) => tracing::info!("iii-state configuration re-applied after change"),
        // A timeout is transient: the stored value is valid but unapplied, and
        // the event will not fire again — so retry exactly once after a delay.
        // The retry calls `apply_config` directly (not this handler), so it
        // cannot loop. Other errors (malformed value) are deterministic;
        // retrying them would just repeat the failure.
        Err(err) if err.downcast_ref::<tokio::time::error::Elapsed>().is_some() => {
            tracing::error!(
                error = %err,
                "iii-state: configuration apply timed out; retrying once in {APPLY_RETRY_DELAY:?}"
            );
            let worker = worker.clone();
            tokio::spawn(async move {
                tokio::time::sleep(APPLY_RETRY_DELAY).await;
                if !worker.is_active() {
                    tracing::debug!(
                        "iii-state: worker no longer active; skipping configuration apply retry"
                    );
                    return;
                }
                match worker.apply_config().await {
                    Ok(()) => tracing::info!("iii-state configuration re-applied on retry"),
                    Err(err) => tracing::error!(
                        error = %err,
                        "iii-state: configuration apply retry failed; keeping previous config"
                    ),
                }
            });
        }
        Err(err) => tracing::error!(
            error = %err,
            "iii-state: failed to apply changed configuration; keeping previous config"
        ),
    }
}

/// Subscribe to `configuration:updated` events for the `iii-state` entry. The
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

/// Resolve the boot-time configuration: the persisted `iii-state` entry replaces
/// the config.yaml block when present and valid, so a runtime-edited adapter or
/// knob survives engine restarts. Falls back to the yaml block on a fresh boot,
/// an unreadable/missing file, or a malformed persisted value.
///
/// Because the first boot seeds the configuration entry from the yaml block
/// (`register_config`), the persisted value carries the yaml's `adapter`
/// selection forward — so "persisted replaces yaml" never silently drops the
/// configured storage backend.
///
/// Limitation: state's `create` does not receive the `EngineConfig`, so a
/// non-default `configuration` adapter directory is not discoverable here. The
/// read uses the fs adapter's default directory; if the configuration worker is
/// not file-backed there, the file is simply absent and the yaml block is used.
pub(super) fn resolve_boot_config(yaml_block: Option<Value>) -> Option<Value> {
    use crate::workers::configuration::adapters::fs;
    resolve_boot_config_from(Path::new(fs::DEFAULT_DIRECTORY), yaml_block)
}

/// Directory-parameterized core of [`resolve_boot_config`], so the boot-read can
/// be unit-tested against a tempdir instead of the process-wide default path.
fn resolve_boot_config_from(dir: &Path, yaml_block: Option<Value>) -> Option<Value> {
    let Some(persisted) = read_persisted_state_value_from(dir) else {
        return yaml_block;
    };
    match serde_json::from_value::<StateModuleConfig>(persisted.clone()) {
        Ok(_) => {
            tracing::info!(
                "Using persisted iii-state configuration entry (config.yaml block is seed-only)"
            );
            Some(persisted)
        }
        Err(err) => {
            tracing::warn!(
                "persisted iii-state configuration is invalid ({err}); using the config.yaml block"
            );
            yaml_block
        }
    }
}

/// Read the persisted `iii-state` value written by the configuration worker's
/// file-backed adapter, with the same `${VAR:default}` expansion
/// `configuration::get` applies. Returns `None` (boot falls back to the yaml
/// block) when the file is absent (fresh boot) or anything about it is unusable.
fn read_persisted_state_value_from(dir: &Path) -> Option<Value> {
    use crate::workers::configuration::adapters::fs;

    let path = dir.join(format!("{}.{}", CONFIG_ID, fs::FILE_EXTENSION));
    let bytes = std::fs::read(&path).ok()?; // absent: fresh boot, use yaml

    let entry: Value = match serde_yaml::from_slice(&bytes) {
        Ok(entry) => entry,
        Err(err) => {
            eprintln!(
                "persisted configuration entry {} is not valid YAML ({err}); using the config.yaml block",
                path.display()
            );
            return None;
        }
    };
    let value = entry.get("value").cloned().filter(|v| !v.is_null())?;

    // `expand_value` panics on a `${VAR}` placeholder with no default and no
    // env value. At runtime that fails one bus call; here it would brick every
    // engine start until the data file is hand-edited — so contain it and fall
    // back to the yaml block.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workers::configuration::store::expand_value(&value)
    })) {
        Ok(expanded) => Some(expanded),
        Err(_) => {
            eprintln!(
                "persisted configuration entry {} references an environment variable with no value and no default; using the config.yaml block",
                path.display()
            );
            None
        }
    }
}
