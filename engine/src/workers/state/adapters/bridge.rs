// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use iii_sdk::{
    III, InitOptions, TriggerRequest, UpdateOp, UpdateResult, register_worker, types::SetResult,
};
use serde_json::Value;

use crate::{
    engine::Engine,
    workers::state::{
        adapters::StateAdapter,
        registry::{StateAdapterFuture, StateAdapterRegistration},
        structs::{
            StateDeleteInput, StateGetGroupInput, StateGetInput, StateListGroupsInput,
            StateListItem, StateSetInput, StateUpdateInput,
        },
    },
};

pub struct BridgeAdapter {
    bridge: Arc<III>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(bridge_url = %bridge_url, "Connecting to bridge");

        let bridge = Arc::new(register_worker(&bridge_url, InitOptions::default()));

        Ok(Self { bridge })
    }
}

fn decode_list_item(item: Value, index: usize) -> StateListItem {
    match item {
        Value::Array(mut tuple) if tuple.len() >= 2 => {
            if let Some(key) = tuple.first().and_then(Value::as_str).map(str::to_string) {
                let value = tuple.swap_remove(1);
                return StateListItem { key, value };
            }
            StateListItem {
                key: format!("(missing key {index})"),
                value: Value::Array(tuple),
            }
        }
        Value::Object(mut object) => {
            let key = object
                .get("key")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    object
                        .get("state_key")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            match key {
                Some(key) => {
                    let value = object
                        .remove("value")
                        .unwrap_or_else(|| Value::Object(object));
                    StateListItem { key, value }
                }
                None => StateListItem {
                    key: format!("(missing key {index})"),
                    value: Value::Object(object),
                },
            }
        }
        value => StateListItem {
            key: format!("(missing key {index})"),
            value,
        },
    }
}

fn decode_list_result(result: Value) -> anyhow::Result<Vec<StateListItem>> {
    if let Ok(items) = serde_json::from_value::<Vec<StateListItem>>(result.clone()) {
        return Ok(items);
    }

    let result = result.get("items").cloned().unwrap_or(result);

    if let Some(object) = result.as_object() {
        return Ok(object
            .iter()
            .map(|(key, value)| StateListItem {
                key: key.clone(),
                value: value.clone(),
            })
            .collect());
    }

    if let Value::Array(items) = result {
        return Ok(items
            .into_iter()
            .enumerate()
            .map(|(index, item)| decode_list_item(item, index))
            .collect());
    }

    anyhow::bail!("invalid state::list response: expected array, object, or items field")
}

#[async_trait]
impl StateAdapter for BridgeAdapter {
    async fn update(
        &self,
        scope: &str,
        key: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult> {
        let data = StateUpdateInput {
            scope: scope.to_string(),
            key: key.to_string(),
            ops,
        };

        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "state::update".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update value via bridge: {}", e))?;

        serde_json::from_value::<UpdateResult>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize update result: {}", e))
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.bridge.shutdown_async().await;
        Ok(())
    }

    async fn set(&self, scope: &str, key: &str, value: Value) -> anyhow::Result<SetResult> {
        let data = StateSetInput {
            scope: scope.to_string(),
            key: key.to_string(),
            value,
        };
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "state::set".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set value via bridge: {}", e))?;

        serde_json::from_value::<SetResult>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize set result: {}", e))
    }

    async fn get(&self, scope: &str, key: &str) -> anyhow::Result<Option<Value>> {
        let data = StateGetInput {
            scope: scope.to_string(),
            key: key.to_string(),
        };
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "state::get".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get value via bridge: {}", e))?;

        serde_json::from_value::<Option<Value>>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize get result: {}", e))
    }

    async fn delete(&self, scope: &str, key: &str) -> anyhow::Result<()> {
        let data = StateDeleteInput {
            scope: scope.to_string(),
            key: key.to_string(),
        };
        self.bridge
            .trigger(TriggerRequest {
                function_id: "state::delete".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete value via bridge: {}", e))?;
        Ok(())
    }

    async fn list(&self, scope: &str) -> anyhow::Result<Vec<StateListItem>> {
        let data = StateGetGroupInput {
            scope: scope.to_string(),
        };

        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "state::list".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list values via bridge: {}", e))?;

        decode_list_result(result)
    }

    async fn list_groups(&self) -> anyhow::Result<Vec<String>> {
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "state::list_groups".to_string(),
                payload: serde_json::to_value(StateListGroupsInput {})
                    .unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list groups via bridge: {}", e))?;

        let groups_value = result.get("groups").ok_or_else(|| {
            anyhow::anyhow!("invalid state::list_groups response: missing 'groups' field")
        })?;

        serde_json::from_value::<Vec<String>>(groups_value.clone()).map_err(|e| {
            anyhow::anyhow!(
                "invalid state::list_groups response: invalid 'groups' field: {}",
                e
            )
        })
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StateAdapterFuture {
    Box::pin(async move {
        let bridge_url = config
            .as_ref()
            .and_then(|c| c.get("bridge_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("ws://localhost:49134")
            .to_string();
        Ok(Arc::new(BridgeAdapter::new(bridge_url).await?) as Arc<dyn StateAdapter>)
    })
}

crate::register_adapter!(<StateAdapterRegistration> name: "bridge", make_adapter);
