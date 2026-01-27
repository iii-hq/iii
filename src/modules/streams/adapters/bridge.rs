// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use iii_sdk::{Bridge, UpdateOp, UpdateResult, types::SetResult};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    builtins::pubsub::BuiltInPubSubAdapter,
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
    async fn update(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> Option<UpdateResult> {
        let index = self.gen_key(stream_name, group_id);
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
            Ok(result) => {
                serde_json::from_value::<Option<UpdateResult>>(result).unwrap_or_else(|e| {
                    tracing::error!(error = %e, "Failed to deserialize update result");
                    None
                })
            }
            Err(e) => {
                tracing::error!(error = %e, index = %index, "Failed to update value in kv_server");
                None
            }
        }
    }

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

    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> SetResult {
        let set_data = KvSetInput {
            index: self.gen_key(stream_name, group_id),
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

        let set_result = match set_result {
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
        };

        let data = set_result.new_value.clone();
        let event = if set_result.old_value.is_some() {
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

        set_result
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let data = KvGetInput {
            index: self.gen_key(stream_name, group_id),
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

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let delete_data = KvDeleteInput {
            index: self.gen_key(stream_name, group_id),
            key: item_id.to_string(),
        };
        let delete_result = self
            .bridge
            .invoke_function("kv_server.delete", delete_data)
            .await;

        match delete_result {
            Ok(delete_result) => {
                let delete_result = serde_json::from_value::<Option<Value>>(delete_result)
                    .unwrap_or_else(|e| {
                        tracing::error!(error = %e, "Failed to deserialize delete result");
                        None
                    });
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
                    .await;
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to delete value from kv_server");
                return;
            }
        }
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let data = KvListInput {
            index: self.gen_key(stream_name, group_id),
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
            Ok(value) => serde_json::from_value::<Vec<String>>(value).unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to deserialize list keys with prefix result");
                Vec::new()
            }),
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
