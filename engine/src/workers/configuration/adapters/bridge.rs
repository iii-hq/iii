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
use iii_sdk::protocol::{RegisterTriggerInput, TriggerRequest};
use iii_sdk::{IIIClient, RegisterFunction, register_worker};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::engine::Engine;
use crate::workers::configuration::adapters::{
    AdapterEnsureOutcome, ConfigurationAdapter, EnsureCandidate, EnsureSupport, ExternalChange,
    ExternalChangeSender, RegisterKind, RegisterOutcome, SetOutcome,
};
use crate::workers::configuration::registry::{
    ConfigurationAdapterFuture, ConfigurationAdapterRegistration,
};
use crate::workers::configuration::structs::{
    ConfigurationEnsureInput, ConfigurationEnsureResult, ConfigurationEntry,
    ConfigurationEventData, ConfigurationEventType, ConfigurationGetInput, ConfigurationListInput,
    ConfigurationListResult, ConfigurationMigrateInput, ConfigurationMigrateResult,
    ConfigurationRegisterInput, ConfigurationSetInput,
};

const DEFAULT_BRIDGE_URL: &str = "ws://localhost:49134";
const RELAY_FUNCTION_ID: &str = "configuration::__bridge_relay";
/// Bounded timeout for every remote `configuration::*` call so an
/// unresponsive remote engine can't hang the local worker indefinitely.
/// 30 s is generous for a control-plane call and matches the order of magnitude
/// of other engine-to-engine timeouts.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

pub struct BridgeAdapter {
    bridge: Arc<IIIClient>,
    /// Holds onto the relay [`iii_sdk::trigger::Trigger`] handle so the SDK keeps
    /// the remote subscription alive for the worker's lifetime.
    relay_trigger: Mutex<Option<iii_sdk::trigger::Trigger>>,
    /// Set lazily by `watch` — used by the relay function below.
    sender: OnceCell<ExternalChangeSender>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(
            bridge_url = %bridge_url,
            "Connecting configuration bridge to remote engine"
        );
        let bridge = Arc::new(register_worker(
            &bridge_url,
            crate::workers::bridge::bridge_init_options("iii-configuration-bridge"),
        ));
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
    /// Forward explicit legacy registration, including its intentional value-replacement semantics.
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

    /// Initialization decisions belong to the remote engine rather than the local mirror.
    fn ensure_support(&self) -> EnsureSupport {
        // The authoritative store lives on the REMOTE engine; the local cache is
        // only a mirror the local `write_lock` cannot guard across processes.
        // The store must forward the decision to us so it happens where the
        // value actually lives.
        EnsureSupport::Delegated
    }

    /// Forward the untouched candidate to the remote atomic API; never retry as legacy register.
    async fn ensure(&self, candidate: EnsureCandidate) -> anyhow::Result<AdapterEnsureOutcome> {
        // Forward the ORIGINAL candidate to the REMOTE authoritative
        // `configuration::ensure` so the seed-vs-preserve decision is made where
        // the value actually lives. We deliberately never read the local cache
        // and never fall back to `configuration::register`: a remote engine
        // without `configuration::ensure` returns a hard error here (surfaced as
        // ADAPTER_ERROR), which is fail-closed — a legacy register could
        // overwrite operator state on the remote engine.
        let raw = self
            .call("configuration::ensure", build_ensure_input(candidate))
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "remote configuration::ensure failed — an engine without \
                     configuration::ensure is unsupported by the bridge (upgrade the remote \
                     engine; NOT falling back to configuration::register): {}",
                    e
                )
            })?;
        let result: ConfigurationEnsureResult = serde_json::from_value(raw)
            .map_err(|e| anyhow::anyhow!("decode remote ensure response: {}", e))?;
        Ok(AdapterEnsureOutcome {
            action: result.action,
            entry: result.entry,
        })
    }

    /// The remote authority migrates and archives the source; never copy through a local mirror.
    async fn migrate(
        &self,
        from_id: &str,
        to_id: &str,
    ) -> anyhow::Result<ConfigurationMigrateResult> {
        let capabilities = self.call("configuration::migration-capabilities", serde_json::json!({}))
            .await.map_err(|e| anyhow::anyhow!("upgrade remote configuration authority: migration capabilities unavailable: {e}"))?;
        anyhow::ensure!(
            capabilities
                .get("source_priority_archive_revision")
                .and_then(Value::as_u64)
                == Some(1),
            "upgrade remote configuration authority: source-priority archival contract is unknown"
        );
        let raw = self.call("configuration::migrate", ConfigurationMigrateInput {
            from_id: from_id.to_string(), to_id: to_id.to_string(),
        }).await.map_err(|e| anyhow::anyhow!("remote configuration::migrate failed; upgrade the remote engine if unavailable; no copy/delete fallback: {e}"))?;
        serde_json::from_value(raw).map_err(|e| anyhow::anyhow!("decode migration response: {e}"))
    }

    /// Replace the authoritative remote value through its validated configuration API.
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
        // `Error` is already string-wrapped by `call` above, so we can't
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
        let raw_reader = self.bridge.clone();

        // Register the relay handler on this bridge worker — when the
        // remote engine fires the `configuration` trigger we registered
        // below, it'll route the call to this function.
        self.bridge.register_function(
            RELAY_FUNCTION_ID,
            RegisterFunction::new_async(move |payload: Value| {
                let sender_lookup = sender_lookup.clone();
                let raw_reader = raw_reader.clone();
                async move {
                    let event: ConfigurationEventData = serde_json::from_value(payload)
                        .map_err(|e| iii_sdk::Error::Handler(e.to_string()))?;
                    if let Some(tx) = sender_lookup.get() {
                        // Events carry applied values. Re-read raw before updating
                        // the local store, or migration's template would become a secret.
                        let raw_value =
                            if matches!(event.event_type, ConfigurationEventType::Deleted) {
                                Value::Null
                            } else {
                                match raw_reader
                                    .trigger(TriggerRequest {
                                        function_id: "configuration::get".into(),
                                        payload: serde_json::json!({ "id": event.id, "raw": true }),
                                        action: None,
                                        timeout_ms: Some(DEFAULT_TIMEOUT_MS),
                                    })
                                    .await
                                {
                                    Ok(raw) => raw.get("value").cloned().ok_or_else(|| {
                                        iii_sdk::Error::Handler(
                                            "remote raw get omitted value".into(),
                                        )
                                    })?,
                                    Err(iii_sdk::Error::Remote { code, .. })
                                        if code == "NOT_FOUND" =>
                                    {
                                        return Ok(Value::Null);
                                    }
                                    Err(err) => return Err(err),
                                }
                            };
                        let entry = ConfigurationEntry {
                            id: event.id.clone(),
                            name: event.name.clone(),
                            description: event.description.clone(),
                            schema: event.schema.clone(),
                            value: raw_value,
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
                    Ok::<Value, iii_sdk::Error>(Value::Null)
                }
            }),
        );

        let trigger = self
            .bridge
            .register_trigger(RegisterTriggerInput::new(
                "configuration".to_string(),
                RELAY_FUNCTION_ID.to_string(),
                serde_json::json!({}),
            ))
            .map_err(|e| {
                anyhow::anyhow!("failed to subscribe to remote configuration trigger: {}", e)
            })?;
        *self.relay_trigger.lock().expect("relay trigger lock") = Some(trigger);
        Ok(())
    }

    /// Unregister the relay and close the remote client without deleting configuration entries.
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

/// Build the remote `configuration::ensure` input from a delegated candidate,
/// forwarding the ORIGINAL seed candidate verbatim. Kept as a free function so
/// it can be unit-tested without a live remote engine: the whole point of the
/// bridge ensure path is that the candidate reaches the remote unchanged and no
/// local cache value is ever substituted.
fn build_ensure_input(candidate: EnsureCandidate) -> ConfigurationEnsureInput {
    ConfigurationEnsureInput {
        id: candidate.id,
        name: candidate.name,
        description: candidate.description,
        schema: candidate.schema,
        initial_value: candidate.candidate,
        metadata: candidate.metadata,
    }
}

/// Build a remote-backed adapter from its configured URL, falling back to the default bridge address.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    /// Preserve the candidate and metadata without substituting a cached local value.
    fn build_ensure_input_forwards_original_candidate_verbatim() {
        let candidate = EnsureCandidate {
            id: "iii-stream".into(),
            name: "Stream".into(),
            description: "desc".into(),
            schema: json!({ "type": "object" }),
            candidate: Some(json!({ "port": 3112 })),
            metadata: Some(json!({ "owner": "team" })),
        };
        let input = build_ensure_input(candidate);
        // The remote engine — not the local cache — decides seed vs preserve, so
        // the candidate must reach it exactly as supplied.
        assert_eq!(input.id, "iii-stream");
        assert_eq!(input.initial_value, Some(json!({ "port": 3112 })));
        assert_eq!(input.schema, json!({ "type": "object" }));
        assert_eq!(input.metadata, Some(json!({ "owner": "team" })));
    }

    #[test]
    /// Missing candidates stay absent instead of becoming an explicit null seed.
    fn build_ensure_input_preserves_absent_candidate() {
        let candidate = EnsureCandidate {
            id: "demo".into(),
            name: "Demo".into(),
            description: String::new(),
            schema: json!({ "type": "object" }),
            candidate: None,
            metadata: None,
        };
        let input = build_ensure_input(candidate);
        assert_eq!(input.initial_value, None);
    }
}
