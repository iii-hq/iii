// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Bridge adapter — delegates `configuration::*` to a remote III instance
//! and rebroadcasts that instance's `configuration` trigger events into the
//! local fan-out so subscribers see remote-originated changes too.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use iii_sdk::{
    III, InitOptions, RegisterFunction, RegisterTriggerInput, TriggerRequest, register_worker,
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::engine::Engine;
use crate::workers::configuration::adapters::{
    ConfigurationAdapter, ExternalChange, ExternalChangeSender, RegisterKind, RegisterOutcome,
    SetOutcome,
};
use crate::workers::configuration::registry::{
    ConfigurationAdapterFuture, ConfigurationAdapterRegistration,
};
use crate::workers::configuration::structs::{
    ConfigurationEntry, ConfigurationEventData, ConfigurationEventType, ConfigurationGetInput,
    ConfigurationListInput, ConfigurationListResult, ConfigurationRegisterInput,
    ConfigurationSetInput,
};

const DEFAULT_BRIDGE_URL: &str = "ws://localhost:49134";
const RELAY_FUNCTION_ID: &str = "configuration::__bridge_relay";
/// Bounded timeout for every remote `configuration::*` call so an
/// unresponsive remote engine can't hang the local worker indefinitely.
/// 30 s is generous for a control-plane call and matches the order of magnitude
/// of other engine-to-engine timeouts.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

pub struct BridgeAdapter {
    bridge: Arc<III>,
    /// Holds onto the relay [`iii_sdk::Trigger`] handle so the SDK keeps
    /// the remote subscription alive for the worker's lifetime.
    relay_trigger: Mutex<Option<iii_sdk::Trigger>>,
    /// Set lazily by `watch` — used by the relay function below.
    sender: OnceCell<ExternalChangeSender>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(
            bridge_url = %bridge_url,
            "Connecting configuration bridge to remote engine"
        );
        let bridge = Arc::new(register_worker(&bridge_url, InitOptions::default()));
        Ok(Self {
            bridge,
            relay_trigger: Mutex::new(None),
            sender: OnceCell::new(),
        })
    }

    async fn call<I: Serialize>(&self, function_id: &str, input: I) -> anyhow::Result<Value> {
        let payload =
            serde_json::to_value(input).map_err(|e| anyhow::anyhow!("encode payload: {}", e))?;
        self.bridge
            .trigger(TriggerRequest {
                function_id: function_id.to_string(),
                payload,
                action: None,
                timeout_ms: Some(DEFAULT_TIMEOUT_MS),
            })
            .await
            .map_err(|e| anyhow::anyhow!("remote {} failed: {}", function_id, e))
    }
}

#[async_trait]
impl ConfigurationAdapter for BridgeAdapter {
    async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
        let raw = self
            .call(
                "configuration::register",
                ConfigurationRegisterInput {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    description: entry.description.clone(),
                    schema: entry.schema.clone(),
                    initial_value: Some(entry.value.clone()),
                    metadata: entry.metadata.clone(),
                },
            )
            .await?;
        let returned: ConfigurationEntry = serde_json::from_value(raw)
            .map_err(|e| anyhow::anyhow!("decode register response: {}", e))?;
        Ok(RegisterOutcome {
            kind: RegisterKind::Replaced,
            entry: returned,
            old_value: None,
        })
    }

    async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome> {
        let raw = self
            .call(
                "configuration::set",
                ConfigurationSetInput {
                    id: id.to_string(),
                    value,
                },
            )
            .await?;
        let old_value = raw.get("old_value").cloned();
        let new_value = raw
            .get("new_value")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("remote set response missing 'new_value'"))?;

        let mut entry = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("remote set succeeded but get returned None"))?;
        entry.value = new_value;
        Ok(SetOutcome { entry, old_value })
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
        // We treat any remote error as "absent" here because the SDK's
        // `IIIError` is already string-wrapped by `call` above, so we can't
        // cleanly distinguish a NOT_FOUND from a network/timeout failure
        // without a wider refactor. Log the underlying error so transient
        // remote failures aren't completely silent.
        let value_resp = match self
            .call(
                "configuration::get",
                ConfigurationGetInput {
                    id: id.to_string(),
                    raw: true,
                },
            )
            .await
        {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    configuration_id = %id,
                    error = %err,
                    "Bridge configuration::get failed; treating as absent"
                );
                return Ok(None);
            }
        };
        let value = value_resp.get("value").cloned().unwrap_or(Value::Null);

        let schema_resp = match self
            .call("configuration::schema", serde_json::json!({ "id": id }))
            .await
        {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    configuration_id = %id,
                    error = %err,
                    "Bridge configuration::schema failed; treating as absent"
                );
                return Ok(None);
            }
        };
        Ok(Some(ConfigurationEntry {
            id: id.to_string(),
            name: schema_resp
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            description: schema_resp
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            schema: schema_resp.get("schema").cloned().unwrap_or(Value::Null),
            value,
            metadata: schema_resp.get("metadata").cloned(),
        }))
    }

    async fn delete(&self, _id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
        // The remote `configuration::*` surface does not expose `delete`
        // — cleanup happens through TTL on the remote engine. Surface a
        // clear error so callers know to operate on the remote engine
        // directly when they want a hard removal.
        Err(anyhow::anyhow!(
            "bridge adapter cannot delete configurations on the remote engine; \
             rely on TTL or operate on the remote engine directly"
        ))
    }

    async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
        let raw = self
            .call("configuration::list", ConfigurationListInput {})
            .await?;
        let listed: ConfigurationListResult =
            serde_json::from_value(raw).map_err(|e| anyhow::anyhow!("decode list: {}", e))?;
        let mut out = Vec::with_capacity(listed.configurations.len());
        for view in listed.configurations {
            let value = self
                .call(
                    "configuration::get",
                    ConfigurationGetInput {
                        id: view.id.clone(),
                        raw: true,
                    },
                )
                .await
                .ok()
                .and_then(|raw| raw.get("value").cloned())
                .unwrap_or(Value::Null);
            out.push(ConfigurationEntry {
                id: view.id,
                name: view.name,
                description: view.description,
                schema: view.schema,
                value,
                metadata: view.metadata,
            });
        }
        Ok(out)
    }

    async fn watch(&self, sender: ExternalChangeSender) -> anyhow::Result<()> {
        // Stash the sender so the relay function below (which is called by
        // the remote engine via this WebSocket) can forward events.
        self.sender
            .set(sender)
            .map_err(|_| anyhow::anyhow!("watch already started"))?;
        let sender_lookup = self.sender.clone();

        // Register the relay handler on this bridge worker — when the
        // remote engine fires the `configuration` trigger we registered
        // below, it'll route the call to this function.
        self.bridge.register_function(
            RELAY_FUNCTION_ID,
            RegisterFunction::new_async(move |payload: Value| {
                let sender_lookup = sender_lookup.clone();
                async move {
                    let event: ConfigurationEventData = serde_json::from_value(payload)
                        .map_err(|e| iii_sdk::IIIError::Handler(e.to_string()))?;
                    if let Some(tx) = sender_lookup.get() {
                        let entry = ConfigurationEntry {
                            id: event.id.clone(),
                            name: event.name.clone(),
                            description: event.description.clone(),
                            schema: event.schema.clone(),
                            value: event.new_value.clone().unwrap_or(Value::Null),
                            metadata: event.metadata.clone(),
                        };
                        let change = match event.event_type {
                            ConfigurationEventType::Registered => ExternalChange::Registered(entry),
                            ConfigurationEventType::Updated => ExternalChange::Updated {
                                entry,
                                old_value: event.old_value.clone(),
                            },
                            ConfigurationEventType::Deleted => ExternalChange::Deleted { entry },
                        };
                        let _ = tx.send(change);
                    }
                    Ok::<Value, iii_sdk::IIIError>(Value::Null)
                }
            }),
        );

        let trigger = self
            .bridge
            .register_trigger(RegisterTriggerInput {
                trigger_type: "configuration".to_string(),
                function_id: RELAY_FUNCTION_ID.to_string(),
                config: serde_json::json!({}),
                metadata: None,
            })
            .map_err(|e| {
                anyhow::anyhow!("failed to subscribe to remote configuration trigger: {}", e)
            })?;
        *self.relay_trigger.lock().expect("relay trigger lock") = Some(trigger);
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        if let Some(trigger) = self
            .relay_trigger
            .lock()
            .expect("relay trigger lock")
            .take()
        {
            trigger.unregister();
        }
        self.bridge.shutdown_async().await;
        Ok(())
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> ConfigurationAdapterFuture {
    Box::pin(async move {
        let bridge_url = config
            .as_ref()
            .and_then(|c| c.get("bridge_url"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_BRIDGE_URL)
            .to_string();
        Ok(Arc::new(BridgeAdapter::new(bridge_url).await?) as Arc<dyn ConfigurationAdapter>)
    })
}

crate::register_adapter!(<ConfigurationAdapterRegistration> name: "bridge", make_adapter);
