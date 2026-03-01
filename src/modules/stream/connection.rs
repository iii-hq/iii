// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    builtins::pubsub_lite::Subscriber,
    engine::{Engine, EngineTrait},
    function::FunctionResult,
    modules::stream::{
        StreamCoreModule, StreamIncomingMessage, StreamOutboundMessage, StreamWrapperMessage,
        Subscription,
        adapters::StreamConnection,
        structs::{
            StreamAuthContext, StreamGetInput, StreamIncomingMessageData, StreamJoinLeaveEvent,
            StreamJoinResult, StreamListInput, StreamOutbound,
        },
        trigger::{JOIN_TRIGGER_TYPE, LEAVE_TRIGGER_TYPE, StreamTriggers},
    },
};

pub struct SocketStreamConnection {
    pub id: String,
    pub sender: mpsc::Sender<StreamOutbound>,
    pub triggers: Arc<StreamTriggers>,
    subscriptions: Arc<DashMap<String, Subscription>>,
    stream_module: Arc<StreamCoreModule>,
    context: Option<StreamAuthContext>,
    engine: Arc<Engine>,
}

impl SocketStreamConnection {
    pub fn new(
        stream_module: Arc<StreamCoreModule>,
        context: Option<StreamAuthContext>,
        sender: mpsc::Sender<StreamOutbound>,
        engine: Arc<Engine>,
        triggers: Arc<StreamTriggers>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            subscriptions: Arc::new(DashMap::new()),
            sender,
            stream_module,
            context,
            engine,
            triggers,
        }
    }

    pub async fn handle_join_leave(&self, message: &StreamIncomingMessage) -> StreamJoinResult {
        let (stream_name, group_id, id, subscription_id, event_type, triggers) = match message {
            StreamIncomingMessage::Join { data } => {
                let triggers: Vec<_> = self
                    .triggers
                    .join_triggers
                    .read()
                    .await
                    .iter()
                    .cloned()
                    .collect();
                (
                    data.stream_name.clone(),
                    data.group_id.clone(),
                    data.id.clone(),
                    data.subscription_id.clone(),
                    JOIN_TRIGGER_TYPE,
                    triggers,
                )
            }
            StreamIncomingMessage::Leave { data } => {
                let triggers: Vec<_> = self
                    .triggers
                    .leave_triggers
                    .read()
                    .await
                    .iter()
                    .cloned()
                    .collect();
                (
                    data.stream_name.clone(),
                    data.group_id.clone(),
                    data.id.clone(),
                    data.subscription_id.clone(),
                    LEAVE_TRIGGER_TYPE,
                    triggers,
                )
            }
        };

        let event = StreamJoinLeaveEvent {
            subscription_id,
            stream_name,
            group_id,
            id,
            context: self
                .context
                .as_ref()
                .and_then(|context| context.context.clone()),
        };
        let mut result = StreamJoinResult {
            unauthorized: false,
        };

        let event_value = match serde_json::to_value(event) {
            Ok(event_value) => event_value,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to convert event to value");
                return result;
            }
        };

        for trigger in triggers {
            if trigger.trigger_type == event_type {
                tracing::debug!(function_id = %trigger.function_id, event_type = ?event_type, "Invoking trigger");

                let call_result = self
                    .engine
                    .call(&trigger.function_id, event_value.clone())
                    .await;

                tracing::debug!(call_result = ?call_result, "Call result");

                match call_result {
                    Ok(Some(call_result)) => {
                        if event_type == JOIN_TRIGGER_TYPE {
                            let unauthorized =
                                match call_result.get("unauthorized").and_then(|v| v.as_bool()) {
                                    Some(unauthorized) => unauthorized,
                                    None => {
                                        if event_type == JOIN_TRIGGER_TYPE {
                                            tracing::error!(
                                                error = "unauthorized must be a boolean",
                                                "Failed to get unauthorized from result"
                                            );
                                        }
                                        false
                                    }
                                };

                            if unauthorized {
                                result = StreamJoinResult { unauthorized: true };
                            }
                        }
                    }
                    Err(e) => {
                        if event_type == JOIN_TRIGGER_TYPE {
                            tracing::error!(error = ?e, "Failed to invoke trigger function");
                        }
                    }
                    _ => {}
                }
            }
        }

        result
    }

    pub async fn handle_socket_message(&self, msg: &StreamIncomingMessage) -> anyhow::Result<()> {
        match msg {
            StreamIncomingMessage::Join { data } => {
                let stream_name = data.stream_name.clone();
                let group_id = data.group_id.clone();
                let id = data.id.clone();
                let subscription_id = data.subscription_id.clone();
                let result = self.handle_join_leave(msg).await;
                let timestamp = chrono::Utc::now().timestamp_millis();

                if result.unauthorized {
                    let event = StreamOutboundMessage::Unauthorized {};
                    let message = StreamWrapperMessage {
                        event_type: "stream".to_string(),
                        timestamp,
                        stream_name,
                        group_id,
                        id,
                        event,
                    };
                    let outbound = StreamOutbound::Stream(message);
                    self.sender.send(outbound).await?;

                    return Ok(());
                }

                self.subscriptions.insert(
                    subscription_id.clone(),
                    Subscription {
                        subscription_id,
                        stream_name: stream_name.clone(),
                        group_id: group_id.clone(),
                        id: id.clone(),
                    },
                );

                if let Some(id) = id {
                    let data = self
                        .stream_module
                        .get(StreamGetInput {
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: id.clone(),
                        })
                        .await;

                    match data {
                        FunctionResult::Success(data) => {
                            self.sender
                                .send(StreamOutbound::Stream(StreamWrapperMessage {
                                    event_type: "stream".to_string(),
                                    timestamp,
                                    stream_name: stream_name.clone(),
                                    group_id: group_id.clone(),
                                    id: Some(id.clone()),
                                    event: StreamOutboundMessage::Sync {
                                        data: data.unwrap_or(Value::Null),
                                    },
                                }))
                                .await?;
                        }
                        FunctionResult::Failure(error) => {
                            tracing::error!(error = ?error, "Failed to get data");
                        }
                        FunctionResult::Deferred => {
                            tracing::error!(error = "Deferred result", "Failed to get data");
                        }
                        FunctionResult::NoResult => {
                            tracing::error!("No result");
                        }
                    }

                    return Ok(());
                } else {
                    let data = self
                        .stream_module
                        .list(StreamListInput {
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                        })
                        .await;

                    match data {
                        FunctionResult::Success(data) => {
                            self.sender
                                .send(StreamOutbound::Stream(StreamWrapperMessage {
                                    event_type: "stream".to_string(),
                                    timestamp,
                                    stream_name: stream_name.clone(),
                                    group_id: group_id.clone(),
                                    id: None,
                                    event: StreamOutboundMessage::Sync {
                                        data: serde_json::to_value(data).unwrap_or(Value::Null),
                                    },
                                }))
                                .await?;
                        }
                        FunctionResult::Failure(error) => {
                            tracing::error!(error = ?error, "Failed to get data");
                        }
                        FunctionResult::Deferred => {
                            tracing::error!(error = "Deferred result", "Failed to get data");
                        }
                        FunctionResult::NoResult => {
                            tracing::error!("No result");
                        }
                    }
                }

                Ok(())
            }
            StreamIncomingMessage::Leave { data } => {
                self.handle_join_leave(msg).await;
                self.subscriptions.remove(&data.subscription_id);
                Ok(())
            }
        }
    }
}

#[async_trait]
impl StreamConnection for SocketStreamConnection {
    async fn cleanup(&self) {
        let subscriptions: Vec<Subscription> = self
            .subscriptions
            .iter()
            .map(|entry| {
                let subscription = entry.value();
                Subscription {
                    subscription_id: subscription.subscription_id.clone(),
                    stream_name: subscription.stream_name.clone(),
                    group_id: subscription.group_id.clone(),
                    id: subscription.id.clone(),
                }
            })
            .collect();

        for subscription in subscriptions {
            tracing::debug!(subscription_id = %subscription.subscription_id, "Cleaning up subscription");

            let _ = self
                .handle_join_leave(&StreamIncomingMessage::Leave {
                    data: StreamIncomingMessageData {
                        subscription_id: subscription.subscription_id.clone(),
                        stream_name: subscription.stream_name.clone(),
                        group_id: subscription.group_id.clone(),
                        id: subscription.id.clone(),
                    },
                })
                .await;
        }
    }

    async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()> {
        tracing::debug!(msg = ?msg, "Sending stream message");

        let matching_senders: Vec<_> = self
            .subscriptions
            .iter()
            .filter(|entry| {
                let subscription = entry.value();
                subscription.stream_name == msg.stream_name
                    && subscription.group_id == msg.group_id
                    && (subscription.id.is_none() || subscription.id == msg.id)
            })
            .map(|_| self.sender.clone())
            .collect();

        for sender in matching_senders {
            if let Err(e) = sender.send(StreamOutbound::Stream(msg.clone())).await {
                tracing::error!(error = ?e.to_string(), "Failed to send stream message");
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Subscriber for SocketStreamConnection {
    async fn handle_message(&self, message: Arc<Value>) -> anyhow::Result<()> {
        let message = match serde_json::from_value::<StreamWrapperMessage>((*message).clone()) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to deserialize stream message");
                return Err(anyhow::anyhow!("Failed to deserialize stream message"));
            }
        };

        self.handle_stream_message(&message).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::{
        sync::{mpsc, oneshot},
        time::timeout,
    };

    use crate::{
        engine::{Handler, RegisterFunctionRequest},
        function::FunctionResult,
        modules::{
            module::ConfigurableModule,
            observability::metrics::ensure_default_meter,
            stream::{
                adapters::{StreamAdapter, StreamConnection, kv_store::BuiltinKvStoreAdapter},
                config::StreamModuleConfig,
            },
        },
        protocol::ErrorBody,
        trigger::Trigger,
    };

    use super::*;

    fn build_connection(
        context: Option<StreamAuthContext>,
    ) -> (
        Arc<Engine>,
        Arc<StreamCoreModule>,
        SocketStreamConnection,
        mpsc::Receiver<StreamOutbound>,
    ) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn StreamAdapter> = Arc::new(BuiltinKvStoreAdapter::new(None));
        let module = Arc::new(StreamCoreModule::build(
            engine.clone(),
            StreamModuleConfig::default(),
            adapter,
        ));
        let (tx, rx) = mpsc::channel(16);
        let connection = SocketStreamConnection::new(
            module.clone(),
            context,
            tx,
            engine.clone(),
            module.triggers.clone(),
        );

        (engine, module, connection, rx)
    }

    fn join_message(
        subscription_id: &str,
        stream_name: &str,
        group_id: &str,
        id: Option<&str>,
    ) -> StreamIncomingMessage {
        StreamIncomingMessage::Join {
            data: StreamIncomingMessageData {
                subscription_id: subscription_id.to_string(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: id.map(str::to_string),
            },
        }
    }

    fn leave_message(
        subscription_id: &str,
        stream_name: &str,
        group_id: &str,
        id: Option<&str>,
    ) -> StreamIncomingMessage {
        StreamIncomingMessage::Leave {
            data: StreamIncomingMessageData {
                subscription_id: subscription_id.to_string(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: id.map(str::to_string),
            },
        }
    }

    fn stream_message(
        stream_name: &str,
        group_id: &str,
        id: Option<&str>,
        event: StreamOutboundMessage,
    ) -> StreamWrapperMessage {
        StreamWrapperMessage {
            event_type: "stream".to_string(),
            timestamp: 1,
            stream_name: stream_name.to_string(),
            group_id: group_id.to_string(),
            id: id.map(str::to_string),
            event,
        }
    }

    async fn recv_stream_message(rx: &mut mpsc::Receiver<StreamOutbound>) -> StreamWrapperMessage {
        match timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for stream outbound")
            .expect("channel closed before outbound")
        {
            StreamOutbound::Stream(message) => message,
            StreamOutbound::Raw(frame) => {
                panic!("expected stream outbound, got raw frame: {frame:?}")
            }
        }
    }

    fn register_function<F, Fut>(engine: &Arc<Engine>, function_id: &str, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut:
            std::future::Future<Output = FunctionResult<Option<Value>, ErrorBody>> + Send + 'static,
    {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(handler),
        );
    }

    #[tokio::test]
    async fn handle_join_leave_invokes_join_and_leave_triggers() {
        let context = StreamAuthContext {
            context: Some(json!({ "user": "alice" })),
        };
        let (engine, _module, connection, _rx) = build_connection(Some(context));

        let (events_tx, mut events_rx) = mpsc::unbounded_channel::<(String, Value)>();
        let join_events_tx = events_tx.clone();
        register_function(&engine, "test::join_guard", move |input| {
            let join_events_tx = join_events_tx.clone();
            async move {
                join_events_tx
                    .send(("join".to_string(), input))
                    .expect("send join event");
                FunctionResult::Success(Some(json!({ "unauthorized": true })))
            }
        });
        register_function(&engine, "test::leave_handler", move |input| {
            let events_tx = events_tx.clone();
            async move {
                events_tx
                    .send(("leave".to_string(), input))
                    .expect("send leave event");
                FunctionResult::Success(None)
            }
        });

        connection
            .triggers
            .join_triggers
            .write()
            .await
            .insert(Trigger {
                id: "join-trigger".to_string(),
                trigger_type: JOIN_TRIGGER_TYPE.to_string(),
                function_id: "test::join_guard".to_string(),
                config: json!({}),
                worker_id: None,
            });
        connection
            .triggers
            .leave_triggers
            .write()
            .await
            .insert(Trigger {
                id: "leave-trigger".to_string(),
                trigger_type: LEAVE_TRIGGER_TYPE.to_string(),
                function_id: "test::leave_handler".to_string(),
                config: json!({}),
                worker_id: None,
            });

        let join_result = connection
            .handle_join_leave(&join_message("sub-1", "orders", "group-a", Some("item-1")))
            .await;
        assert!(join_result.unauthorized);

        let leave_result = connection
            .handle_join_leave(&leave_message("sub-1", "orders", "group-a", Some("item-1")))
            .await;
        assert!(!leave_result.unauthorized);

        let (join_kind, join_payload) = events_rx.recv().await.expect("join trigger payload");
        assert_eq!(join_kind, "join");
        assert_eq!(join_payload["subscription_id"], "sub-1");
        assert_eq!(join_payload["stream_name"], "orders");
        assert_eq!(join_payload["group_id"], "group-a");
        assert_eq!(join_payload["id"], "item-1");
        assert_eq!(join_payload["context"], json!({ "user": "alice" }));

        let (leave_kind, leave_payload) = events_rx.recv().await.expect("leave trigger payload");
        assert_eq!(leave_kind, "leave");
        assert_eq!(leave_payload["subscription_id"], "sub-1");
        assert_eq!(leave_payload["stream_name"], "orders");
        assert_eq!(leave_payload["group_id"], "group-a");
        assert_eq!(leave_payload["id"], "item-1");
    }

    #[tokio::test]
    async fn handle_socket_message_unauthorized_join_emits_unauthorized() {
        let (engine, _module, connection, mut rx) = build_connection(None);

        register_function(&engine, "test::join_guard", |_input| async move {
            FunctionResult::Success(Some(json!({ "unauthorized": true })))
        });
        connection
            .triggers
            .join_triggers
            .write()
            .await
            .insert(Trigger {
                id: "join-guard".to_string(),
                trigger_type: JOIN_TRIGGER_TYPE.to_string(),
                function_id: "test::join_guard".to_string(),
                config: json!({}),
                worker_id: None,
            });

        connection
            .handle_socket_message(&join_message("sub-1", "orders", "group-a", Some("item-1")))
            .await
            .expect("unauthorized join should not fail");

        let outbound = recv_stream_message(&mut rx).await;
        assert_eq!(outbound.stream_name, "orders");
        assert_eq!(outbound.group_id, "group-a");
        assert_eq!(outbound.id.as_deref(), Some("item-1"));
        assert!(matches!(
            outbound.event,
            StreamOutboundMessage::Unauthorized {}
        ));
        assert!(connection.subscriptions.is_empty());

        connection
            .handle_stream_message(&stream_message(
                "orders",
                "group-a",
                Some("item-1"),
                StreamOutboundMessage::Update {
                    data: json!({ "ignored": true }),
                },
            ))
            .await
            .expect("fan-out without subscriptions should succeed");
        assert!(timeout(Duration::from_millis(50), rx.recv()).await.is_err());
    }

    #[tokio::test]
    async fn handle_socket_message_join_with_id_syncs_get_and_leave_removes_subscription() {
        let (_engine, module, connection, mut rx) = build_connection(None);

        let set_result = module
            .set(crate::modules::stream::structs::StreamSetInput {
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                item_id: "item-1".to_string(),
                data: json!({ "status": "open" }),
            })
            .await;
        assert!(matches!(set_result, FunctionResult::Success(_)));

        connection
            .handle_socket_message(&join_message("sub-1", "orders", "group-a", Some("item-1")))
            .await
            .expect("join with id should succeed");

        let sync = recv_stream_message(&mut rx).await;
        assert_eq!(sync.id.as_deref(), Some("item-1"));
        assert!(matches!(
            sync.event,
            StreamOutboundMessage::Sync { ref data } if data == &json!({ "status": "open" })
        ));
        assert_eq!(connection.subscriptions.len(), 1);

        let update_message = stream_message(
            "orders",
            "group-a",
            Some("item-1"),
            StreamOutboundMessage::Update {
                data: json!({ "status": "closed" }),
            },
        );
        connection
            .handle_stream_message(&update_message)
            .await
            .expect("matching stream message should fan out");

        let forwarded = recv_stream_message(&mut rx).await;
        assert!(matches!(
            forwarded.event,
            StreamOutboundMessage::Update { ref data } if data == &json!({ "status": "closed" })
        ));

        connection
            .handle_socket_message(&leave_message("sub-1", "orders", "group-a", Some("item-1")))
            .await
            .expect("leave should succeed");
        assert!(connection.subscriptions.is_empty());

        connection
            .handle_stream_message(&update_message)
            .await
            .expect("fan-out after leave should still succeed");
        assert!(timeout(Duration::from_millis(50), rx.recv()).await.is_err());
    }

    #[tokio::test]
    async fn handle_socket_message_join_without_id_syncs_list_and_cleanup_emits_leave_events() {
        let (engine, module, connection, mut rx) = build_connection(None);

        let set_result = module
            .set(crate::modules::stream::structs::StreamSetInput {
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                item_id: "item-1".to_string(),
                data: json!({ "status": "open" }),
            })
            .await;
        assert!(matches!(set_result, FunctionResult::Success(_)));

        let (leave_tx, mut leave_rx) = mpsc::unbounded_channel::<String>();
        register_function(&engine, "test::leave_handler", move |input| {
            let leave_tx = leave_tx.clone();
            async move {
                leave_tx
                    .send(
                        input["subscription_id"]
                            .as_str()
                            .expect("subscription_id in leave trigger")
                            .to_string(),
                    )
                    .expect("send leave subscription id");
                FunctionResult::Success(None)
            }
        });
        connection
            .triggers
            .leave_triggers
            .write()
            .await
            .insert(Trigger {
                id: "leave-trigger".to_string(),
                trigger_type: LEAVE_TRIGGER_TYPE.to_string(),
                function_id: "test::leave_handler".to_string(),
                config: json!({}),
                worker_id: None,
            });

        connection
            .handle_socket_message(&join_message("sub-list", "orders", "group-a", None))
            .await
            .expect("list join should succeed");
        let list_sync = recv_stream_message(&mut rx).await;
        assert!(matches!(
            list_sync.event,
            StreamOutboundMessage::Sync { ref data }
                if data.as_array().is_some_and(|items| items == &vec![json!({ "status": "open" })])
        ));

        connection
            .handle_socket_message(&join_message(
                "sub-item",
                "orders",
                "group-a",
                Some("item-1"),
            ))
            .await
            .expect("item join should succeed");
        let item_sync = recv_stream_message(&mut rx).await;
        assert!(matches!(
            item_sync.event,
            StreamOutboundMessage::Sync { ref data } if data == &json!({ "status": "open" })
        ));

        connection.cleanup().await;

        let first = leave_rx.recv().await.expect("first cleanup leave event");
        let second = leave_rx.recv().await.expect("second cleanup leave event");
        let mut received = vec![first, second];
        received.sort();
        assert_eq!(
            received,
            vec!["sub-item".to_string(), "sub-list".to_string()]
        );
    }

    #[tokio::test]
    async fn handle_message_filters_matches_and_reports_deserialize_errors() {
        let (_engine, module, connection, mut rx) = build_connection(None);

        let set_result = module
            .set(crate::modules::stream::structs::StreamSetInput {
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                item_id: "item-1".to_string(),
                data: json!({ "status": "open" }),
            })
            .await;
        assert!(matches!(set_result, FunctionResult::Success(_)));

        connection
            .handle_socket_message(&join_message("sub-all", "orders", "group-a", None))
            .await
            .expect("group subscription should succeed");
        let _ = recv_stream_message(&mut rx).await;

        connection
            .handle_socket_message(&join_message(
                "sub-item",
                "orders",
                "group-a",
                Some("item-1"),
            ))
            .await
            .expect("item subscription should succeed");
        let _ = recv_stream_message(&mut rx).await;

        connection
            .handle_socket_message(&join_message("sub-other", "orders", "group-b", None))
            .await
            .expect("non-matching subscription should succeed");
        let _ = recv_stream_message(&mut rx).await;

        let forwarded = stream_message(
            "orders",
            "group-a",
            Some("item-1"),
            StreamOutboundMessage::Event {
                event: crate::modules::stream::structs::EventData {
                    event_type: "custom".to_string(),
                    data: json!({ "count": 1 }),
                },
            },
        );
        connection
            .handle_message(Arc::new(
                serde_json::to_value(&forwarded).expect("serialize valid forwarded message"),
            ))
            .await
            .expect("valid subscriber payload should be handled");

        let first = recv_stream_message(&mut rx).await;
        let second = recv_stream_message(&mut rx).await;
        assert!(matches!(
            first.event,
            StreamOutboundMessage::Event { ref event }
                if event.event_type == "custom" && event.data == json!({ "count": 1 })
        ));
        assert!(matches!(
            second.event,
            StreamOutboundMessage::Event { ref event }
                if event.event_type == "custom" && event.data == json!({ "count": 1 })
        ));
        assert!(timeout(Duration::from_millis(50), rx.recv()).await.is_err());

        let invalid = connection
            .handle_message(Arc::new(json!({ "oops": true })))
            .await;
        assert!(invalid.is_err());

        drop(rx);
        connection
            .handle_stream_message(&forwarded)
            .await
            .expect("send failure should be swallowed");
    }

    #[tokio::test]
    async fn handle_join_leave_ignores_trigger_errors() {
        let (engine, _module, connection, _rx) = build_connection(None);
        let (payload_tx, payload_rx) = oneshot::channel::<Value>();
        let payload_tx = Arc::new(std::sync::Mutex::new(Some(payload_tx)));
        let payload_tx_clone = payload_tx.clone();

        register_function(&engine, "test::join_error", move |input| {
            let payload_tx_clone = payload_tx_clone.clone();
            async move {
                if let Some(tx) = payload_tx_clone.lock().expect("lock oneshot").take() {
                    let _ = tx.send(input);
                }
                FunctionResult::Failure(ErrorBody {
                    code: "JOIN".to_string(),
                    message: "boom".to_string(),
                })
            }
        });
        connection
            .triggers
            .join_triggers
            .write()
            .await
            .insert(Trigger {
                id: "join-error".to_string(),
                trigger_type: JOIN_TRIGGER_TYPE.to_string(),
                function_id: "test::join_error".to_string(),
                config: json!({}),
                worker_id: None,
            });

        let result = connection
            .handle_join_leave(&join_message("sub-err", "orders", "group-a", None))
            .await;
        assert!(!result.unauthorized);

        let payload = payload_rx.await.expect("join payload from failing trigger");
        assert_eq!(payload["subscription_id"], "sub-err");
        assert_eq!(payload["stream_name"], "orders");
        assert_eq!(payload["group_id"], "group-a");
    }

    #[tokio::test]
    async fn handle_join_leave_ignores_non_boolean_unauthorized_values() {
        let (engine, _module, connection, _rx) = build_connection(None);

        register_function(&engine, "test::join_invalid_auth", |_input| async move {
            FunctionResult::Success(Some(json!({ "unauthorized": "nope" })))
        });
        connection
            .triggers
            .join_triggers
            .write()
            .await
            .insert(Trigger {
                id: "join-invalid-auth".to_string(),
                trigger_type: JOIN_TRIGGER_TYPE.to_string(),
                function_id: "test::join_invalid_auth".to_string(),
                config: json!({}),
                worker_id: None,
            });

        let result = connection
            .handle_join_leave(&join_message("sub-invalid", "orders", "group-a", None))
            .await;
        assert!(!result.unauthorized);
    }

    #[tokio::test]
    async fn handle_socket_message_get_errors_do_not_emit_sync_messages() {
        let (engine, _module, connection, mut rx) = build_connection(None);

        register_function(&engine, "stream::get(orders)", |_input| async move {
            FunctionResult::Failure(ErrorBody {
                code: "GET_FAILED".to_string(),
                message: "custom get failed".to_string(),
            })
        });

        connection
            .handle_socket_message(&join_message(
                "sub-get-fail",
                "orders",
                "group-a",
                Some("item-1"),
            ))
            .await
            .expect("join with failing custom get should still succeed");

        assert_eq!(connection.subscriptions.len(), 1);
        assert!(timeout(Duration::from_millis(50), rx.recv()).await.is_err());
    }

    #[tokio::test]
    async fn handle_socket_message_list_errors_do_not_emit_sync_messages() {
        let (engine, _module, connection, mut rx) = build_connection(None);

        register_function(&engine, "stream::list(orders)", |_input| async move {
            FunctionResult::Failure(ErrorBody {
                code: "LIST_FAILED".to_string(),
                message: "custom list failed".to_string(),
            })
        });

        connection
            .handle_socket_message(&join_message("sub-list-fail", "orders", "group-a", None))
            .await
            .expect("join with failing custom list should still succeed");

        assert_eq!(connection.subscriptions.len(), 1);
        assert!(timeout(Duration::from_millis(50), rx.recv()).await.is_err());
    }
}
