use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::Bridge;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::{BuiltInPubSubAdapter, filters::UpdateOp},
    engine::Engine,
    modules::{
        kv_server::{
            KvSetInput,
            structs::{KvGetInput, KvListInput, KvListKeysWithPrefixInput},
        },
        pubsub::{PubSubInput, SubscribeTrigger},
        streams::{
            StreamOutboundMessage, StreamWrapperMessage,
            adapters::{StreamAdapter, StreamConnection, UpdateResult},
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
    pub_sub: Arc<BuiltInPubSubAdapter>,
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
            pub_sub: Arc::new(BuiltInPubSubAdapter::new(None)),
            handler_function_path,
        })
    }

    fn gen_key(&self, stream_name: &str, group_id: &str) -> String {
        format!("{}::{}", stream_name, group_id)
    }
}

#[async_trait]
impl StreamAdapter for BridgeAdapter {
    async fn destroy(&self) -> anyhow::Result<()> {
        self.bridge.disconnect();
        Ok(())
    }

    async fn emit_event(&self, message: StreamWrapperMessage) {
        let data = PubSubInput {
            topic: STREAMS_EVENTS_TOPIC.to_string(),
            data: serde_json::to_value(&message).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to serialize message");
                serde_json::json!({ "error": e.to_string() })
            }),
        };

        tracing::debug!(data = ?data.clone(), "Emitting event");

        let _ = self.bridge.invoke_function("publish", data).await;
    }

    async fn update(
        &self,
        stream_name: &str,
        group_id: &str,
        ops: Vec<UpdateOp>,
    ) -> Option<UpdateResult> {
        let key = self.gen_key(stream_name, group_id);
        let data = KvGetInput { key: key.clone() };
        let value = self.bridge.invoke_function("kv_server.get", data).await;

        let existing_value = match value {
            Ok(value) => serde_json::from_value::<Option<Value>>(value).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize update get result");
                None
            }),
            Err(e) => {
                tracing::error!(error = %e, "Failed to get value from kv_server for update");
                None
            }
        }?;

        let old_value = existing_value.clone();
        let mut updated_value = existing_value;
        for op in ops {
            match op {
                UpdateOp::Set { path, value } => {
                    if path.0.is_empty() {
                        updated_value = value;
                    } else if let Value::Object(ref mut map) = updated_value {
                        map.insert(path.0, value);
                    } else {
                        tracing::warn!(
                            "Set operation with path requires existing value to be a JSON object"
                        );
                    }
                }
                UpdateOp::Merge { path, value } => {
                    if path.is_none() || path.as_ref().unwrap().0.is_empty() {
                        if let (Value::Object(existing_map), Value::Object(new_map)) =
                            (&mut updated_value, value)
                        {
                            for (k, v) in new_map {
                                existing_map.insert(k, v);
                            }
                        } else {
                            tracing::warn!(
                                "Merge operation requires both existing and new values to be JSON objects"
                            );
                        }
                    } else {
                        tracing::warn!("Only root-level merge is supported");
                    }
                }
                UpdateOp::Increment { path, by } => {
                    if let Value::Object(ref mut map) = updated_value {
                        if let Some(existing_val) = map.get_mut(&path.0) {
                            if let Some(num) = existing_val.as_i64() {
                                *existing_val = Value::Number(serde_json::Number::from(num + by));
                            } else {
                                tracing::warn!(
                                    "Increment operation requires existing value to be a number"
                                );
                            }
                        } else {
                            map.insert(path.0, Value::Number(serde_json::Number::from(by)));
                        }
                    } else {
                        tracing::warn!(
                            "Increment operation requires existing value to be a JSON object"
                        );
                    }
                }
                UpdateOp::Decrement { path, by } => {
                    if let Value::Object(ref mut map) = updated_value {
                        if let Some(existing_val) = map.get_mut(&path.0) {
                            if let Some(num) = existing_val.as_i64() {
                                *existing_val = Value::Number(serde_json::Number::from(num - by));
                            } else {
                                tracing::warn!(
                                    "Decrement operation requires existing value to be a number"
                                );
                            }
                        } else {
                            map.insert(path.0, Value::Number(serde_json::Number::from(-by)));
                        }
                    } else {
                        tracing::warn!(
                            "Decrement operation requires existing value to be a JSON object"
                        );
                    }
                }
                UpdateOp::Remove { path } => {
                    if let Value::Object(ref mut map) = updated_value {
                        map.remove(&path.0);
                    } else {
                        tracing::warn!(
                            "Remove operation requires existing value to be a JSON object"
                        );
                    }
                }
            }
        }

        let set_data = KvSetInput {
            key,
            value: updated_value.clone(),
        };
        let set_result = self.bridge.invoke_function("kv_server.set", set_data).await;
        match set_result {
            Ok(_) => Some(UpdateResult {
                old_value: Some(old_value),
                new_value: updated_value,
            }),
            Err(e) => {
                tracing::error!(error = %e, "Failed to set updated value in kv_server");
                None
            }
        }
    }

    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        let key = self.gen_key(stream_name, group_id);
        let value = self
            .bridge
            .invoke_function("kv_server.get", KvGetInput { key: key.clone() })
            .await;

        let mut map = match value {
            Ok(value) => match serde_json::from_value::<Option<Value>>(value).unwrap_or(None) {
                Some(Value::Object(map)) => map,
                Some(other) => {
                    tracing::warn!(
                        value = ?other,
                        "Expected group value to be a JSON object, overwriting"
                    );
                    serde_json::Map::new()
                }
                None => serde_json::Map::new(),
            },
            Err(e) => {
                tracing::error!(error = %e, "Failed to get group value from kv_server");
                return;
            }
        };

        let existed = map.contains_key(item_id);
        map.insert(item_id.to_string(), data.clone());

        let set_data = KvSetInput {
            key,
            value: Value::Object(map),
        };
        let set_result = self.bridge.invoke_function("kv_server.set", set_data).await;

        match set_result {
            Ok(_) => {
                let event = if existed {
                    StreamOutboundMessage::Update { data }
                } else {
                    StreamOutboundMessage::Create { data }
                };
                let message = StreamWrapperMessage {
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    stream_name: stream_name.to_string(),
                    group_id: group_id.to_string(),
                    id: Some(item_id.to_string()),
                    event,
                };
                self.emit_event(message).await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to set group value in kv_server");
                return;
            }
        }
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = self.gen_key(stream_name, group_id);
        let data = KvGetInput { key };
        let value = self.bridge.invoke_function("kv_server.get", data).await;

        match value {
            Ok(value) => match serde_json::from_value::<Option<Value>>(value).unwrap_or(None) {
                Some(Value::Object(map)) => map.get(item_id).cloned(),
                Some(other) => {
                    tracing::warn!(
                        value = ?other,
                        "Expected group value to be a JSON object"
                    );
                    None
                }
                None => None,
            },
            Err(e) => {
                tracing::error!(error = %e, "Failed to get value from kv_server");
                None
            }
        }
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let key = self.gen_key(stream_name, group_id);
        let value = self
            .bridge
            .invoke_function("kv_server.get", KvGetInput { key: key.clone() })
            .await;

        let mut map = match value {
            Ok(value) => match serde_json::from_value::<Option<Value>>(value).unwrap_or(None) {
                Some(Value::Object(map)) => map,
                Some(other) => {
                    tracing::warn!(
                        value = ?other,
                        "Expected group value to be a JSON object"
                    );
                    return;
                }
                None => return,
            },
            Err(e) => {
                tracing::error!(error = %e, "Failed to get group value from kv_server");
                return;
            }
        };

        if map.remove(item_id).is_none() {
            return;
        }

        let set_data = KvSetInput {
            key,
            value: Value::Object(map),
        };
        let set_result = self.bridge.invoke_function("kv_server.set", set_data).await;

        match set_result {
            Ok(_) => {
                self.emit_event(StreamWrapperMessage {
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    stream_name: stream_name.to_string(),
                    group_id: group_id.to_string(),
                    id: Some(item_id.to_string()),
                    event: StreamOutboundMessage::Delete {
                        data: serde_json::json!({ "id": item_id }),
                    },
                })
                .await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to set updated group in kv_server");
                return;
            }
        }
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let data = KvListInput {
            key: self.gen_key(stream_name, group_id),
        };

        let value = self.bridge.invoke_function("kv_server.list", data).await;

        match value {
            Ok(value) => serde_json::from_value::<Vec<Value>>(value).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize get group result");
                Vec::new()
            }),
            Err(e) => {
                tracing::error!(error = %e, "Failed to get value from kv_server");
                Vec::new()
            }
        }
    }

    async fn list_groups(&self, stream_name: &str) -> Vec<String> {
        let data = KvListKeysWithPrefixInput {
            prefix: self.gen_key(stream_name, ""),
        };
        let value = self
            .bridge
            .invoke_function("kv_server.list_keys_with_prefix", data)
            .await;

        match value {
            Ok(value) => {
                let prefix = self.gen_key(stream_name, "");
                serde_json::from_value::<Vec<String>>(value)
                    .unwrap_or_else(|e| {
                        tracing::error!(error = %e, "Failed to deserialize list keys with prefix result");
                        Vec::new()
                    })
                    .into_iter()
                    .filter_map(|key| key.strip_prefix(&prefix).map(|s| s.to_string()))
                    .collect()
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to list keys with prefix from kv_server");
                Vec::new()
            }
        }
    }

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        self.pub_sub.subscribe(id, connection).await;
    }
    async fn unsubscribe(&self, id: String) {
        self.pub_sub.unsubscribe(id).await;
    }

    async fn watch_events(&self) {
        let handler_function_path = self.handler_function_path.clone();
        let pub_sub = self.pub_sub.clone();
        let _ = self
            .bridge
            .register_function(handler_function_path.clone(), move |data| {
                let pub_sub = pub_sub.clone();

                async move {
                    let data = serde_json::from_value::<StreamWrapperMessage>(data).unwrap();

                    tracing::debug!(data = ?data.clone(), "Event: Received event");

                    let _ = pub_sub.send_msg(data);

                    Ok(Value::Null)
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
