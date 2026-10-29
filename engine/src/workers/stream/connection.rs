// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    builtins::pubsub_lite::Subscriber,
    engine::Engine,
    event_formats::StreamJoinLeaveEvent,
    function::FunctionResult,
    observer::events::{STREAM_ON_JOIN, STREAM_ON_LEAVE, payload_built},
    protocol::DEFAULT_NAMESPACE,
    workers::stream::{
        StreamIncomingMessage, StreamOutboundMessage, StreamWorker, StreamWrapperMessage,
        Subscription,
        adapters::StreamConnection,
        structs::{
            StreamAuthContext, StreamGetInput, StreamIncomingMessageData, StreamListInput,
            StreamOutbound,
        },
    },
};

pub struct SocketStreamConnection {
    pub id: String,
    pub sender: mpsc::Sender<StreamOutbound>,
    subscriptions: Arc<DashMap<String, Subscription>>,
    stream_module: Arc<StreamWorker>,
    context: Option<StreamAuthContext>,
    engine: Arc<Engine>,
}

impl SocketStreamConnection {
    pub fn new(
        stream_module: Arc<StreamWorker>,
        context: Option<StreamAuthContext>,
        sender: mpsc::Sender<StreamOutbound>,
        engine: Arc<Engine>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            subscriptions: Arc::new(DashMap::new()),
            sender,
            stream_module,
            context,
            engine,
        }
    }

    /// One call of `stream::on-join` or `stream::on-leave` for `message`, with
    /// the client's auth context. Built only when something observes the
    /// source.
    fn emit_join_leave(&self, message: &StreamIncomingMessage) {
        let (source, data) = match message {
            StreamIncomingMessage::Join { data } => (STREAM_ON_JOIN, data),
            StreamIncomingMessage::Leave { data } => (STREAM_ON_LEAVE, data),
        };
        self.engine.observers.emit(DEFAULT_NAMESPACE, source, || {
            payload_built();
            serde_json::to_value(StreamJoinLeaveEvent {
                subscription_id: data.subscription_id.clone(),
                stream_name: data.stream_name.clone(),
                group_id: data.group_id.clone(),
                id: data.id.clone(),
                context: self
                    .context
                    .as_ref()
                    .and_then(|context| context.context.clone()),
            })
            .ok()
        });
    }

    pub async fn handle_socket_message(&self, msg: &StreamIncomingMessage) -> anyhow::Result<()> {
        match msg {
            StreamIncomingMessage::Join { data } => {
                let stream_name = data.stream_name.clone();
                let group_id = data.group_id.clone();
                let id = data.id.clone();
                let subscription_id = data.subscription_id.clone();
                self.emit_join_leave(msg);
                let timestamp = chrono::Utc::now().timestamp_millis();

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
                        .get_item(StreamGetInput {
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            item_id: id.clone(),
                            caller_worker_id: None,
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
                        .list_items(StreamListInput {
                            stream_name: stream_name.clone(),
                            group_id: group_id.clone(),
                            caller_worker_id: None,
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
                self.emit_join_leave(msg);
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

            self.emit_join_leave(&StreamIncomingMessage::Leave {
                data: StreamIncomingMessageData {
                    subscription_id: subscription.subscription_id.clone(),
                    stream_name: subscription.stream_name.clone(),
                    group_id: subscription.group_id.clone(),
                    id: subscription.id.clone(),
                },
            });
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
        let message = match StreamWrapperMessage::deserialize(message.as_ref()) {
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
    use tokio::{sync::mpsc, time::timeout};

    use crate::{
        engine::{EngineTrait, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        observer::events::{register_stream_event_sources, test_support},
        protocol::ErrorBody,
        workers::{
            observability::metrics::ensure_default_meter,
            stream::{
                adapters::{StreamAdapter, StreamConnection, kv_store::BuiltinKvStoreAdapter},
                config::StreamModuleConfig,
            },
            traits::ConfigurableWorker,
        },
    };

    use super::*;

    fn build_connection(
        context: Option<StreamAuthContext>,
    ) -> (
        Arc<Engine>,
        Arc<StreamWorker>,
        SocketStreamConnection,
        mpsc::Receiver<StreamOutbound>,
    ) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter: Arc<dyn StreamAdapter> = Arc::new(BuiltinKvStoreAdapter::new(None));
        let module = Arc::new(StreamWorker::build(
            engine.clone(),
            StreamModuleConfig::default(),
            adapter,
        ));
        register_stream_event_sources(&engine);
        let (tx, rx) = mpsc::channel(16);
        let connection = SocketStreamConnection::new(module.clone(), context, tx, engine.clone());

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

    /// A join and a leave are calls of `stream::on-join` and
    /// `stream::on-leave`, with the client's auth context.
    #[tokio::test]
    async fn joins_and_leaves_are_engine_hosted_sources() {
        let context = StreamAuthContext {
            context: Some(json!({ "user": "alice" })),
        };
        let (engine, _module, connection, _rx) = build_connection(Some(context));
        let mut joins = test_support::capture(&engine, "test::on-join");
        let mut leaves = test_support::capture(&engine, "test::on-leave");
        let _join_socket = test_support::observe(
            &engine,
            "obs-join",
            STREAM_ON_JOIN,
            "test::on-join",
            Some(json!({ "input.stream_name": "orders" })),
        )
        .await;
        let _leave_socket = test_support::observe(
            &engine,
            "obs-leave",
            STREAM_ON_LEAVE,
            "test::on-leave",
            None,
        )
        .await;

        connection
            .handle_socket_message(&join_message("sub-other", "invoices", "group-a", None))
            .await
            .expect("join");
        connection
            .handle_socket_message(&join_message("sub-1", "orders", "group-a", None))
            .await
            .expect("join");
        assert_eq!(
            test_support::next(&mut joins).await,
            json!({
                "subscription_id": "sub-1",
                "stream_name": "orders",
                "group_id": "group-a",
                "id": null,
                "context": { "user": "alice" }
            }),
            "the filter skipped the invoices join"
        );

        connection
            .handle_socket_message(&leave_message("sub-1", "orders", "group-a", None))
            .await
            .expect("leave");
        let leave = test_support::next(&mut leaves).await;
        assert_eq!(leave["subscription_id"], "sub-1");
        assert_eq!(leave["context"], json!({ "user": "alice" }));
        assert!(connection.subscriptions.is_empty());
    }

    #[tokio::test]
    async fn handle_socket_message_join_with_id_syncs_get_and_leave_removes_subscription() {
        let (_engine, module, connection, mut rx) = build_connection(None);

        let set_result = module
            .set(crate::workers::stream::structs::StreamSetInput {
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                item_id: "item-1".to_string(),
                data: json!({ "status": "open" }),
                caller_worker_id: None,
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
            .set(crate::workers::stream::structs::StreamSetInput {
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                item_id: "item-1".to_string(),
                data: json!({ "status": "open" }),
                caller_worker_id: None,
            })
            .await;
        assert!(matches!(set_result, FunctionResult::Success(_)));

        let mut leaves = test_support::capture(&engine, "test::on-leave");
        let _leave_socket = test_support::observe(
            &engine,
            "obs-leave",
            STREAM_ON_LEAVE,
            "test::on-leave",
            None,
        )
        .await;

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

        let first = test_support::next(&mut leaves).await["subscription_id"].clone();
        let second = test_support::next(&mut leaves).await["subscription_id"].clone();
        let mut received = vec![
            first.as_str().expect("a subscription id").to_string(),
            second.as_str().expect("a subscription id").to_string(),
        ];
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
            .set(crate::workers::stream::structs::StreamSetInput {
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                item_id: "item-1".to_string(),
                data: json!({ "status": "open" }),
                caller_worker_id: None,
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
                event: crate::workers::stream::structs::EventData {
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

    /// Unobserved, a join builds no event.
    #[tokio::test]
    async fn an_unobserved_join_builds_nothing() {
        let (_engine, _module, connection, _rx) = build_connection(None);
        let before = crate::observer::events::payloads_built();
        connection
            .handle_socket_message(&join_message("sub-quiet", "orders", "group-a", None))
            .await
            .expect("join");
        assert_eq!(crate::observer::events::payloads_built(), before);
    }

    #[tokio::test]
    async fn handle_socket_message_get_errors_do_not_emit_sync_messages() {
        let (engine, _module, connection, mut rx) = build_connection(None);

        register_function(&engine, "stream::get(orders)", |_input| async move {
            FunctionResult::Failure(ErrorBody {
                code: "GET_FAILED".to_string(),
                message: "custom get failed".to_string(),
                stacktrace: None,
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
                stacktrace: None,
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
