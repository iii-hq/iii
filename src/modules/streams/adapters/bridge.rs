use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{Bridge, UpdateOp, UpdateResult, types::SetResult};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::pubsub_lite::BuiltInPubSubLite,
    engine::Engine,
    modules::{
        kv_server::{
            KvDeleteInput, KvSetInput,
            structs::{KvGetInput, KvListInput, KvListKeysWithPrefixInput, KvUpdateInput},
        },
        pubsub::{PubSubInput, SubscribeTrigger},
        streams::{
            StreamOutboundMessage, StreamWrapperMessage,
            adapters::{StreamAdapter, StreamConnection},
            registry::{StreamAdapterFuture, StreamAdapterRegistration},
        },
    },
};

type TopicName = String;
type GroupId = String;
type ItemId = String;
type ItemsDataAsString = HashMap<ItemId, String>;
type StoreKey = (TopicName, GroupId);

#[derive(Clone, Debug, Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub struct Storage(HashMap<StoreKey, ItemsDataAsString>);

pub const STREAMS_EVENTS_TOPIC: &str = "streams.events";

pub struct BridgeAdapter {
    pub_sub: Arc<BuiltInPubSubLite>,
    handler_function_path: String,
    bridge: Arc<Bridge>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(bridge_url = %bridge_url, "Connecting to bridge");

        let bridge = Arc::new(Bridge::new(&bridge_url));
        let handler_function_path = format!("streams::bridge::on_pub::{}", uuid::Uuid::new_v4());
        let res = bridge.connect().await;

        if let Err(error) = res {
            panic!("Failed to connect to bridge: {}", error);
        }

        Ok(Self {
            bridge,
            pub_sub: Arc::new(BuiltInPubSubLite::new(None)),
            handler_function_path,
        })
    }

    fn gen_key(&self, stream_name: &str, group_id: &str) -> String {
        format!("{}::{}", stream_name, group_id)
    }
}

#[async_trait]
impl StreamAdapter for BridgeAdapter {
    async fn update(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult> {
        let index = self.gen_key(stream_name, group_id);
        let update_data = KvUpdateInput {
            index: index.clone(),
            key: item_id.to_string(),
            ops,
        };

        let update_result = self
            .bridge
            .invoke_function("kv_server.update", update_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update value in kv_server: {}", e))?;

        serde_json::from_value::<UpdateResult>(update_result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize update result: {}", e))
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        self.bridge.disconnect();
        Ok(())
    }

    async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()> {
        let data = PubSubInput {
            topic: STREAMS_EVENTS_TOPIC.to_string(),
            data: serde_json::to_value(&message)
                .map_err(|e| anyhow::anyhow!("Failed to serialize message: {}", e))?,
        };

        tracing::debug!(data = ?data.clone(), "Emitting event");

        self.bridge
            .invoke_function("publish", data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish event: {}", e))?;
        Ok(())
    }

    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> anyhow::Result<SetResult> {
        let set_data = KvSetInput {
            index: self.gen_key(stream_name, group_id),
            key: item_id.to_string(),
            value: data.clone(),
        };
        let set_result = self
            .bridge
            .invoke_function("kv_server.set", set_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set value in kv_server: {}", e))?;

        let set_result = serde_json::from_value::<SetResult>(set_result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize set result: {}", e))?;

        let data_clone = set_result.new_value.clone();
        let event = if set_result.old_value.is_some() {
            StreamOutboundMessage::Update { data: data_clone }
        } else {
            StreamOutboundMessage::Create { data: data_clone }
        };
        let message = StreamWrapperMessage {
            timestamp: chrono::Utc::now().timestamp_millis(),
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            id: Some(item_id.to_string()),
            event,
        };

        self.emit_event(message).await?;

        Ok(set_result)
    }

    async fn get(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<Option<Value>> {
        let data = KvGetInput {
            index: self.gen_key(stream_name, group_id),
            key: item_id.to_string(),
        };
        let value = self
            .bridge
            .invoke_function("kv_server.get", data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get value from kv_server: {}", e))?;

        serde_json::from_value::<Option<Value>>(value)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize get result: {}", e))
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) -> anyhow::Result<()> {
        let delete_data = KvDeleteInput {
            index: self.gen_key(stream_name, group_id),
            key: item_id.to_string(),
        };
        let delete_result = self
            .bridge
            .invoke_function("kv_server.delete", delete_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete value from kv_server: {}", e))?;

        let delete_result = serde_json::from_value::<Option<Value>>(delete_result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize delete result: {}", e))?;

        if delete_result.is_some() {
            self.emit_event(StreamWrapperMessage {
                timestamp: chrono::Utc::now().timestamp_millis(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: Some(item_id.to_string()),
                event: StreamOutboundMessage::Delete {
                    data: serde_json::json!({ "id": item_id }),
                },
            })
            .await?;
        }

        Ok(())
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> anyhow::Result<Vec<Value>> {
        let data = KvListInput {
            index: self.gen_key(stream_name, group_id),
        };

        let value = self
            .bridge
            .invoke_function("kv_server.list", data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get group from kv_server: {}", e))?;

        serde_json::from_value::<Vec<Value>>(value)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize get group result: {}", e))
    }

    async fn list_groups(&self, stream_name: &str) -> anyhow::Result<Vec<String>> {
        let data = KvListKeysWithPrefixInput {
            prefix: self.gen_key(stream_name, ""),
        };
        let value = self
            .bridge
            .invoke_function("kv_server.list_keys_with_prefix", data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list groups from kv_server: {}", e))?;

        serde_json::from_value::<Vec<String>>(value).map_err(|e| {
            anyhow::anyhow!("Failed to deserialize list keys with prefix result: {}", e)
        })
    }

    async fn subscribe(
        &self,
        id: String,
        connection: Arc<dyn StreamConnection>,
    ) -> anyhow::Result<()> {
        self.pub_sub.subscribe(id, connection).await;
        Ok(())
    }
    async fn unsubscribe(&self, id: String) -> anyhow::Result<()> {
        self.pub_sub.unsubscribe(id).await;
        Ok(())
    }

    async fn watch_events(&self) -> anyhow::Result<()> {
        let handler_function_path = self.handler_function_path.clone();
        let pub_sub = self.pub_sub.clone();
        self.bridge
            .register_function(handler_function_path.clone(), move |data| {
                let pub_sub = pub_sub.clone();

                async move {
                    match serde_json::from_value::<StreamWrapperMessage>(data) {
                        Ok(data) => {
                            tracing::debug!(data = ?data.clone(), "Event: Received event");
                            pub_sub.send_msg(data);
                            Ok(Value::Null)
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to deserialize stream message");
                            Err(iii_sdk::BridgeError::Remote {
                                code: "DESERIALIZATION_ERROR".to_string(),
                                message: format!("Failed to deserialize stream message: {}", e),
                            })
                        }
                    }
                }
            });

        let _ = self.bridge.register_trigger(
            "subscribe",
            handler_function_path,
            SubscribeTrigger {
                topic: STREAMS_EVENTS_TOPIC.to_string(),
            },
        );

        self.pub_sub.watch_events().await;
        Ok(())
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StreamAdapterFuture {
    Box::pin(async move {
        let bridge_url = config
            .as_ref()
            .and_then(|c| c.get("bridge_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("ws://localhost:49134")
            .to_string();
        Ok(Arc::new(BridgeAdapter::new(bridge_url).await?) as Arc<dyn StreamAdapter>)
    })
}

crate::register_adapter!(<StreamAdapterRegistration> "modules::streams::adapters::Bridge", make_adapter);
