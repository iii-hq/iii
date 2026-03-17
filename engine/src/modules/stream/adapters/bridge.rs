// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use iii_sdk::{
    III, InitOptions, RegisterFunctionMessage, RegisterTriggerInput, TriggerRequest, UpdateOp,
    UpdateResult, register_worker,
    types::{DeleteResult, SetResult},
};
use serde_json::Value;

use crate::{
    builtins::pubsub_lite::BuiltInPubSubLite,
    engine::Engine,
    modules::{
        pubsub::{PubSubInput, SubscribeTrigger},
        stream::{
            StreamMetadata, StreamWrapperMessage,
            adapters::{StreamAdapter, StreamConnection},
            registry::{StreamAdapterFuture, StreamAdapterRegistration},
            structs::{
                StreamDeleteInput, StreamGetInput, StreamListGroupsInput, StreamListInput,
                StreamSetInput, StreamUpdateInput,
            },
        },
    },
};

pub const STREAM_EVENTS_TOPIC: &str = "stream.events";

pub struct BridgeAdapter {
    pub_sub: Arc<BuiltInPubSubLite>,
    handler_function_id: String,
    bridge: Arc<III>,
}

impl BridgeAdapter {
    pub async fn new(bridge_url: String) -> anyhow::Result<Self> {
        tracing::info!(bridge_url = %bridge_url, "Connecting to bridge");

        let bridge = Arc::new(register_worker(&bridge_url, InitOptions::default()));
        let handler_function_id = format!("stream::bridge::on_pub::{}", uuid::Uuid::new_v4());

        Ok(Self {
            bridge,
            pub_sub: Arc::new(BuiltInPubSubLite::new(None)),
            handler_function_id,
        })
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
        let data = StreamUpdateInput {
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            item_id: item_id.to_string(),
            ops,
        };

        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "stream::update".to_string(),
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

    async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()> {
        let data = PubSubInput {
            topic: STREAM_EVENTS_TOPIC.to_string(),
            data: serde_json::to_value(&message)
                .map_err(|e| anyhow::anyhow!("Failed to serialize message: {}", e))?,
        };

        tracing::debug!(data = ?data.clone(), "Emitting event");

        self.bridge
            .trigger(TriggerRequest {
                function_id: "publish".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
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
        let input = StreamSetInput {
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            item_id: item_id.to_string(),
            data,
        };
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "stream::set".to_string(),
                payload: serde_json::to_value(input).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set value via bridge: {}", e))?;

        serde_json::from_value::<SetResult>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize set result: {}", e))
    }

    async fn get(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<Option<Value>> {
        let data = StreamGetInput {
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            item_id: item_id.to_string(),
        };
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "stream::get".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get value via bridge: {}", e))?;

        serde_json::from_value::<Option<Value>>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize get result: {}", e))
    }

    async fn delete(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<DeleteResult> {
        let data = StreamDeleteInput {
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            item_id: item_id.to_string(),
        };
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "stream::delete".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete value via bridge: {}", e))?;

        serde_json::from_value::<DeleteResult>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize delete result: {}", e))
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> anyhow::Result<Vec<Value>> {
        let data = StreamListInput {
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
        };

        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "stream::list".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get group via bridge: {}", e))?;

        serde_json::from_value::<Vec<Value>>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize get group result: {}", e))
    }

    async fn list_groups(&self, stream_name: &str) -> anyhow::Result<Vec<String>> {
        let data = StreamListGroupsInput {
            stream_name: stream_name.to_string(),
        };
        let result = self
            .bridge
            .trigger(TriggerRequest {
                function_id: "stream::list_groups".to_string(),
                payload: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                action: None,
                timeout_ms: None,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list groups via bridge: {}", e))?;

        serde_json::from_value::<Vec<String>>(result)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize list groups result: {}", e))
    }

    async fn list_all_stream(&self) -> anyhow::Result<Vec<StreamMetadata>> {
        Ok(vec![])
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
        let handler_function_id = self.handler_function_id.clone();
        let pub_sub = self.pub_sub.clone();
        self.bridge.register_function(
            RegisterFunctionMessage {
                id: handler_function_id.clone(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
                invocation: None,
            },
            move |data| {
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
                            Err(iii_sdk::IIIError::Remote {
                                code: "DESERIALIZATION_ERROR".to_string(),
                                message: format!("Failed to deserialize stream message: {}", e),
                                stacktrace: None,
                            })
                        }
                    }
                }
            },
        );

        let _ = self.bridge.register_trigger(RegisterTriggerInput {
            trigger_type: "subscribe".to_string(),
            function_id: handler_function_id,
            config: serde_json::to_value(SubscribeTrigger {
                topic: STREAM_EVENTS_TOPIC.to_string(),
            })
            .unwrap_or_default(),
        });

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

crate::register_adapter!(<StreamAdapterRegistration> "modules::stream::adapters::Bridge", make_adapter);
