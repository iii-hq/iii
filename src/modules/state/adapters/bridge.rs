use std::sync::Arc;

use async_trait::async_trait;
use iii_sdk::{Bridge, UpdateOp, UpdateResult, types::SetResult};
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
    bridge: Arc<Bridge>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(bridge_url = %bridge_url, "Connecting to bridge");

        let bridge = Arc::new(Bridge::new(&bridge_url));
        let res = bridge.connect().await;

        if let Err(error) = res {
            panic!("Failed to connect to bridge: {}", error);
        }

        Ok(Self { bridge })
    }

    fn gen_key(&self, group_id: &str) -> String {
        group_id.to_string()
    }
}

#[async_trait]
impl StateAdapter for BridgeAdapter {
    async fn update(&self, group_id: &str, item_id: &str, ops: Vec<UpdateOp>) -> UpdateResult {
        let index = self.gen_key(group_id);
        let update_data = KvUpdateInput {
            index: index.clone(),
            key: item_id.to_string(),
            ops,
        };

        let update_result = self
            .bridge
            .invoke_function("kv_server.update", update_data)
            .await;

        match update_result {
            Ok(result) => serde_json::from_value::<UpdateResult>(result).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize update result");
                UpdateResult {
                    old_value: None,
                    new_value: Value::Null,
                }
            }),
            Err(e) => {
                tracing::error!(error = %e, index = %index, "Failed to update value in kv_server");
                UpdateResult {
                    old_value: None,
                    new_value: Value::Null,
                }
            }
        }
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.bridge.disconnect();
        Ok(())
    }

    async fn set(&self, group_id: &str, item_id: &str, data: Value) -> SetResult {
        let set_data = KvSetInput {
            index: self.gen_key(group_id),
            key: item_id.to_string(),
            value: data,
        };
        let set_result = self.bridge.invoke_function("kv_server.set", set_data).await;

        if let Err(e) = set_result {
            tracing::error!(error = %e, "Failed to set value in kv_server");

            return SetResult {
                old_value: None,
                new_value: Value::Null,
            };
        }

        match set_result {
            Ok(set_result) => serde_json::from_value::<SetResult>(set_result).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize set result");
                SetResult {
                    old_value: None,
                    new_value: Value::Null,
                }
            }),
            Err(e) => {
                tracing::error!(error = %e, "Failed to deserialize set result");

                return SetResult {
                    old_value: None,
                    new_value: Value::Null,
                };
            }
        }
    }

    async fn get(&self, group_id: &str, item_id: &str) -> Option<Value> {
        let data = KvGetInput {
            index: self.gen_key(group_id),
            key: item_id.to_string(),
        };
        let value = self.bridge.invoke_function("kv_server.get", data).await;

        match value {
            Ok(value) => serde_json::from_value::<Option<Value>>(value).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize get result");
                None
            }),
            Err(e) => {
                tracing::error!(error = %e, "Failed to get value from kv_server");
                None
            }
        }
    }

    async fn delete(&self, group_id: &str, item_id: &str) {
        let delete_data = KvDeleteInput {
            index: self.gen_key(group_id),
            key: item_id.to_string(),
        };
        let delete_result = self
            .bridge
            .invoke_function("kv_server.delete", delete_data)
            .await;

        if let Err(e) = delete_result {
            tracing::error!(error = %e, "Failed to delete value from kv_server");
        }
    }

    async fn list(&self, group_id: &str) -> Vec<Value> {
        let data = KvListInput {
            index: self.gen_key(group_id),
        };

        let value = self.bridge.invoke_function("kv_server.list", data).await;

        match value {
            Ok(value) => serde_json::from_value::<Vec<Value>>(value).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize list result");
                Vec::new()
            }),
            Err(e) => {
                tracing::error!(error = %e, "Failed to list values from kv_server");
                Vec::new()
            }
        }
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
