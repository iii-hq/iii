// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use iii_sdk::{III, UpdateOp, UpdateResult, types::SetResult};
use serde_json::Value;

use crate::{
    engine::Engine,
    modules::{
        kv_server::{
            KvDeleteInput, KvSetInput,
            structs::{KvGetInput, KvListInput, KvUpdateInput},
        },
        state::{
            adapters::StateAdapter,
            registry::{StateAdapterFuture, StateAdapterRegistration},
        },
    },
};

pub struct BridgeAdapter {
    bridge: Arc<III>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(bridge_url = %bridge_url, "Connecting to bridge");

        let bridge = Arc::new(III::new(&bridge_url));
        let res = bridge.connect().await;

        if let Err(error) = res {
            panic!("Failed to connect to bridge: {}", error);
        }

        Ok(Self { bridge })
    }

    fn gen_key(&self, scope: &str) -> String {
        scope.to_string()
    }
}

#[async_trait]
impl StateAdapter for BridgeAdapter {
    async fn update(
        &self,
        scope: &str,
        key: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult> {
        let index = self.gen_key(scope);
        let update_data = KvUpdateInput {
            index: index.clone(),
            key: key.to_string(),
            ops,
        };

        let update_result = self
            .bridge
            .call("kv_server.update", update_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update value in kv_server: {}", e))?;

        serde_json::from_value::<UpdateResult>(update_result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize update result: {}", e))
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.bridge.shutdown_async().await;
        Ok(())
    }

    async fn set(&self, scope: &str, key: &str, data: Value) -> anyhow::Result<SetResult> {
        let set_data = KvSetInput {
            index: self.gen_key(scope),
            key: key.to_string(),
            value: data,
        };
        let set_result = self
            .bridge
            .call("kv_server.set", set_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set value in kv_server: {}", e))?;

        serde_json::from_value::<SetResult>(set_result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize set result: {}", e))
    }

    async fn get(&self, scope: &str, key: &str) -> anyhow::Result<Option<Value>> {
        let data = KvGetInput {
            index: self.gen_key(scope),
            key: key.to_string(),
        };
        let value = self
            .bridge
            .call("kv_server.get", data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get value from kv_server: {}", e))?;

        serde_json::from_value::<Option<Value>>(value)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize get result: {}", e))
    }

    async fn delete(&self, scope: &str, key: &str) -> anyhow::Result<()> {
        let delete_data = KvDeleteInput {
            index: self.gen_key(scope),
            key: key.to_string(),
        };
        self.bridge
            .call("kv_server.delete", delete_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete value from kv_server: {}", e))?;
        Ok(())
    }

    async fn list(&self, scope: &str) -> anyhow::Result<Vec<Value>> {
        let data = KvListInput {
            index: self.gen_key(scope),
        };

        let value = self
            .bridge
            .call("kv_server.list", data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list values from kv_server: {}", e))?;

        serde_json::from_value::<Vec<Value>>(value)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize list result: {}", e))
    }

    async fn list_groups(&self) -> anyhow::Result<Vec<String>> {
        let value = self
            .bridge
            .call("kv_server.list_groups", serde_json::json!({}))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list groups from kv_server: {}", e))?;

        // Validate that the response contains a "groups" field that is an array
        let groups_value = value.get("groups").ok_or_else(|| {
            anyhow::anyhow!("invalid kv_server.list_groups response: missing 'groups' field")
        })?;

        serde_json::from_value::<Vec<String>>(groups_value.clone()).map_err(|e| {
            anyhow::anyhow!(
                "invalid kv_server.list_groups response: invalid 'groups' field: {}",
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

crate::register_adapter!(<StateAdapterRegistration> "modules::state::adapters::Bridge", make_adapter);
