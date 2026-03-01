// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::{
    engine::Engine,
    modules::stream::{
        StreamCoreModule, StreamIncomingMessage,
        adapters::{StreamAdapter, StreamConnection},
        connection::SocketStreamConnection,
        structs::{StreamAuthContext, StreamOutbound},
        trigger::StreamTriggers,
    },
};

pub struct StreamSocketManager {
    pub engine: Arc<Engine>,
    pub auth_function: Option<String>,
    adapter: Arc<dyn StreamAdapter>,
    stream_module: Arc<StreamCoreModule>,
    triggers: Arc<StreamTriggers>,
}

impl StreamSocketManager {
    pub fn new(
        engine: Arc<Engine>,
        adapter: Arc<dyn StreamAdapter>,
        stream_module: Arc<StreamCoreModule>,
        auth_function: Option<String>,
        triggers: Arc<StreamTriggers>,
    ) -> Self {
        Self {
            engine,
            adapter,
            stream_module,
            auth_function,
            triggers,
        }
    }

    pub async fn socket_handler(
        &self,
        socket: WebSocket,
        context: Option<StreamAuthContext>,
    ) -> anyhow::Result<()> {
        let (mut ws_tx, mut ws_rx) = socket.split();
        let (tx, mut rx) = mpsc::channel::<StreamOutbound>(64);
        let connection = SocketStreamConnection::new(
            self.stream_module.clone(),
            context,
            tx,
            self.engine.clone(),
            self.triggers.clone(),
        );

        let writer = tokio::spawn(async move {
            while let Some(outbound) = rx.recv().await {
                let send_result = match outbound {
                    StreamOutbound::Stream(msg) => match serde_json::to_string(&msg) {
                        Ok(payload) => ws_tx.send(WsMessage::Text(payload.into())).await,
                        Err(err) => {
                            tracing::error!(error = ?err, "serialize error");
                            continue;
                        }
                    },
                    StreamOutbound::Raw(frame) => ws_tx.send(frame).await,
                };

                if send_result.is_err() {
                    tracing::error!(error = ?send_result.err(), "Failed to send stream message");
                    break;
                }
            }
        });

        let connection_id = connection.id.to_string();
        let connection = Arc::new(connection);

        if let Err(e) = self
            .adapter
            .subscribe(connection_id.clone(), connection.clone())
            .await
        {
            tracing::error!(error = %e, "Failed to subscribe connection");
            return Err(anyhow::anyhow!("Failed to subscribe connection: {}", e));
        }

        while let Some(frame) = ws_rx.next().await {
            match frame {
                Ok(WsMessage::Text(text)) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<StreamIncomingMessage>(&text) {
                        Ok(msg) => connection.handle_socket_message(&msg).await?,
                        Err(err) => tracing::warn!(error = ?err, "json decode error"),
                    }
                }
                Ok(WsMessage::Binary(bytes)) => {
                    match serde_json::from_slice::<StreamIncomingMessage>(&bytes) {
                        Ok(msg) => connection.handle_socket_message(&msg).await?,
                        Err(err) => {
                            tracing::warn!(error = ?err, "binary decode error")
                        }
                    }
                }
                Ok(WsMessage::Close(_)) => {
                    tracing::debug!("Stream Websocket Connection closed");
                    break;
                }
                Ok(WsMessage::Ping(payload)) => {
                    let _ = connection
                        .sender
                        .send(StreamOutbound::Raw(WsMessage::Pong(payload)))
                        .await;
                }
                Ok(WsMessage::Pong(_)) => {}
                Err(_err) => {
                    break;
                }
            }
        }

        writer.abort();
        if let Err(e) = self.adapter.unsubscribe(connection_id).await {
            tracing::error!(error = %e, "Failed to unsubscribe connection");
        }
        connection.cleanup().await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use axum::{
        Router,
        extract::{State, ws::WebSocketUpgrade},
        response::IntoResponse,
        routing::get,
    };
    use futures_util::{SinkExt, StreamExt};
    use iii_sdk::{
        UpdateResult,
        types::{DeleteResult, SetResult},
    };
    use serde_json::{Value, json};
    use tokio::{
        net::TcpListener,
        sync::mpsc,
        task::JoinHandle,
        time::{sleep, timeout},
    };
    use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

    use crate::{
        engine::{EngineTrait, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        modules::{
            module::ConfigurableModule,
            observability::metrics::ensure_default_meter,
            stream::{
                StreamOutboundMessage, StreamWrapperMessage, config::StreamModuleConfig,
                structs::StreamIncomingMessageData,
            },
        },
        trigger::Trigger,
    };

    use super::*;

    struct TestSocketAdapter {
        get_result: Mutex<Option<Value>>,
        list_result: Mutex<Vec<Value>>,
        subscriptions: Mutex<HashMap<String, Arc<dyn StreamConnection>>>,
        subscribe_ids: Mutex<Vec<String>>,
        unsubscribe_ids: Mutex<Vec<String>>,
        fail_subscribe: AtomicBool,
    }

    impl Default for TestSocketAdapter {
        fn default() -> Self {
            Self {
                get_result: Mutex::new(None),
                list_result: Mutex::new(Vec::new()),
                subscriptions: Mutex::new(HashMap::new()),
                subscribe_ids: Mutex::new(Vec::new()),
                unsubscribe_ids: Mutex::new(Vec::new()),
                fail_subscribe: AtomicBool::new(false),
            }
        }
    }

    impl TestSocketAdapter {
        fn first_connection(&self) -> Arc<dyn StreamConnection> {
            self.subscriptions
                .lock()
                .expect("lock subscriptions")
                .values()
                .next()
                .cloned()
                .expect("expected subscribed connection")
        }
    }

    #[async_trait]
    impl StreamAdapter for TestSocketAdapter {
        async fn set(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
            data: Value,
        ) -> anyhow::Result<SetResult> {
            Ok(SetResult {
                old_value: None,
                new_value: data,
            })
        }

        async fn get(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
        ) -> anyhow::Result<Option<Value>> {
            Ok(self.get_result.lock().expect("lock get_result").clone())
        }

        async fn delete(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
        ) -> anyhow::Result<DeleteResult> {
            Ok(DeleteResult { old_value: None })
        }

        async fn get_group(
            &self,
            _stream_name: &str,
            _group_id: &str,
        ) -> anyhow::Result<Vec<Value>> {
            Ok(self.list_result.lock().expect("lock list_result").clone())
        }

        async fn list_groups(&self, _stream_name: &str) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }

        async fn list_all_stream(
            &self,
        ) -> anyhow::Result<Vec<crate::modules::stream::StreamMetadata>> {
            Ok(Vec::new())
        }

        async fn emit_event(&self, _message: StreamWrapperMessage) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            id: String,
            connection: Arc<dyn StreamConnection>,
        ) -> anyhow::Result<()> {
            if self.fail_subscribe.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("subscribe failed"));
            }

            self.subscribe_ids
                .lock()
                .expect("lock subscribe_ids")
                .push(id.clone());
            self.subscriptions
                .lock()
                .expect("lock subscriptions")
                .insert(id, connection);
            Ok(())
        }

        async fn unsubscribe(&self, id: String) -> anyhow::Result<()> {
            self.unsubscribe_ids
                .lock()
                .expect("lock unsubscribe_ids")
                .push(id.clone());
            self.subscriptions
                .lock()
                .expect("lock subscriptions")
                .remove(&id);
            Ok(())
        }

        async fn watch_events(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update(
            &self,
            _stream_name: &str,
            _group_id: &str,
            _item_id: &str,
            _ops: Vec<iii_sdk::UpdateOp>,
        ) -> anyhow::Result<UpdateResult> {
            Ok(UpdateResult {
                old_value: None,
                new_value: json!({}),
            })
        }
    }

    async fn ws_route(
        State(manager): State<Arc<StreamSocketManager>>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| async move {
            let _ = manager.socket_handler(socket, None).await;
        })
    }

    async fn spawn_server(
        adapter: Arc<TestSocketAdapter>,
    ) -> (Arc<Engine>, Arc<StreamCoreModule>, JoinHandle<()>, String) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let module = Arc::new(StreamCoreModule::build(
            engine.clone(),
            StreamModuleConfig::default(),
            adapter.clone(),
        ));
        let manager = Arc::new(StreamSocketManager::new(
            engine.clone(),
            adapter,
            module.clone(),
            None,
            module.triggers.clone(),
        ));

        let app = Router::new().route("/", get(ws_route)).with_state(manager);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test websocket listener");
        let addr = listener.local_addr().expect("listener local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve websocket app");
        });

        (engine, module, server, format!("ws://{addr}/"))
    }

    fn join_message(
        subscription_id: &str,
        stream_name: &str,
        group_id: &str,
        id: Option<&str>,
    ) -> ClientMessage {
        ClientMessage::Text(
            serde_json::to_string(&StreamIncomingMessage::Join {
                data: StreamIncomingMessageData {
                    subscription_id: subscription_id.to_string(),
                    stream_name: stream_name.to_string(),
                    group_id: group_id.to_string(),
                    id: id.map(str::to_string),
                },
            })
            .expect("serialize join message")
            .into(),
        )
    }

    async fn wait_for_subscription(adapter: &TestSocketAdapter) -> String {
        let started = tokio::time::Instant::now();
        loop {
            if let Some(id) = adapter
                .subscribe_ids
                .lock()
                .expect("lock subscribe_ids")
                .first()
                .cloned()
            {
                return id;
            }

            assert!(
                started.elapsed() < Duration::from_secs(1),
                "timed out waiting for subscribe"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_unsubscribe(adapter: &TestSocketAdapter) -> String {
        let started = tokio::time::Instant::now();
        loop {
            if let Some(id) = adapter
                .unsubscribe_ids
                .lock()
                .expect("lock unsubscribe_ids")
                .first()
                .cloned()
            {
                return id;
            }

            assert!(
                started.elapsed() < Duration::from_secs(1),
                "timed out waiting for unsubscribe"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn next_client_message(
        client: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> ClientMessage {
        timeout(Duration::from_secs(1), client.next())
            .await
            .expect("timed out waiting for websocket frame")
            .expect("websocket closed before frame")
            .expect("websocket frame should be valid")
    }

    #[tokio::test]
    async fn socket_handler_handles_ping_and_unsubscribes_on_close() {
        let adapter = Arc::new(TestSocketAdapter::default());
        let (_engine, _module, server, url) = spawn_server(adapter.clone()).await;
        let (mut client, _response) = connect_async(url).await.expect("connect websocket client");

        let subscribed_id = wait_for_subscription(&adapter).await;
        client
            .send(ClientMessage::Ping(vec![1, 2, 3].into()))
            .await
            .expect("send ping");
        assert!(matches!(
            next_client_message(&mut client).await,
            ClientMessage::Pong(payload) if payload.as_ref() == [1, 2, 3]
        ));

        client
            .send(ClientMessage::Pong(Vec::new().into()))
            .await
            .expect("send pong");
        client
            .send(ClientMessage::Close(None))
            .await
            .expect("send close");

        let unsubscribed_id = wait_for_unsubscribe(&adapter).await;
        assert_eq!(subscribed_id, unsubscribed_id);

        server.abort();
    }

    #[tokio::test]
    async fn socket_handler_ignores_invalid_frames_and_forwards_stream_messages() {
        let adapter = Arc::new(TestSocketAdapter::default());
        *adapter.list_result.lock().expect("lock list_result") = vec![json!({ "status": "open" })];

        let (_engine, _module, server, url) = spawn_server(adapter.clone()).await;
        let (mut client, _response) = connect_async(url).await.expect("connect websocket client");
        let _ = wait_for_subscription(&adapter).await;

        client
            .send(ClientMessage::Text(String::new().into()))
            .await
            .expect("send empty text");
        client
            .send(ClientMessage::Text("not-json".into()))
            .await
            .expect("send invalid text");
        client
            .send(ClientMessage::Binary(b"nope".to_vec().into()))
            .await
            .expect("send invalid binary");
        client
            .send(join_message("sub-1", "orders", "group-a", None))
            .await
            .expect("send join");

        let sync = next_client_message(&mut client).await;
        let sync = match sync {
            ClientMessage::Text(text) => {
                serde_json::from_str::<StreamWrapperMessage>(&text).expect("parse sync message")
            }
            other => panic!("expected text sync frame, got {other:?}"),
        };
        assert!(matches!(
            sync.event,
            StreamOutboundMessage::Sync { ref data }
                if data.as_array().is_some_and(|items| items == &vec![json!({ "status": "open" })])
        ));

        adapter
            .first_connection()
            .handle_stream_message(&StreamWrapperMessage {
                event_type: "stream".to_string(),
                timestamp: 42,
                stream_name: "orders".to_string(),
                group_id: "group-a".to_string(),
                id: None,
                event: StreamOutboundMessage::Event {
                    event: crate::modules::stream::structs::EventData {
                        event_type: "custom".to_string(),
                        data: json!({ "count": 1 }),
                    },
                },
            })
            .await
            .expect("forward server-side stream message");

        let event = next_client_message(&mut client).await;
        let event = match event {
            ClientMessage::Text(text) => {
                serde_json::from_str::<StreamWrapperMessage>(&text).expect("parse event message")
            }
            other => panic!("expected text event frame, got {other:?}"),
        };
        assert!(matches!(
            event.event,
            StreamOutboundMessage::Event { ref event }
                if event.event_type == "custom" && event.data == json!({ "count": 1 })
        ));

        client
            .send(ClientMessage::Close(None))
            .await
            .expect("send close");
        let _ = wait_for_unsubscribe(&adapter).await;

        server.abort();
    }

    #[tokio::test]
    async fn socket_handler_cleanup_triggers_leave_on_disconnect() {
        let adapter = Arc::new(TestSocketAdapter::default());
        *adapter.list_result.lock().expect("lock list_result") = vec![json!({ "status": "open" })];

        let (engine, module, server, url) = spawn_server(adapter.clone()).await;
        let (leave_tx, mut leave_rx) = mpsc::unbounded_channel::<String>();
        register_leave_handler(&engine, leave_tx);
        module
            .triggers
            .leave_triggers
            .write()
            .await
            .insert(Trigger {
                id: "leave-trigger".to_string(),
                trigger_type: crate::modules::stream::trigger::LEAVE_TRIGGER_TYPE.to_string(),
                function_id: "test::leave_handler".to_string(),
                config: json!({}),
                worker_id: None,
            });

        let (mut client, _response) = connect_async(url).await.expect("connect websocket client");
        let _ = wait_for_subscription(&adapter).await;
        client
            .send(join_message("sub-cleanup", "orders", "group-a", None))
            .await
            .expect("send join");
        let _ = next_client_message(&mut client).await;

        client
            .send(ClientMessage::Close(None))
            .await
            .expect("send close");

        let left_subscription = timeout(Duration::from_secs(1), leave_rx.recv())
            .await
            .expect("timed out waiting for leave trigger")
            .expect("leave trigger channel closed");
        assert_eq!(left_subscription, "sub-cleanup");
        let _ = wait_for_unsubscribe(&adapter).await;

        server.abort();
    }

    fn register_leave_handler(engine: &Arc<Engine>, leave_tx: mpsc::UnboundedSender<String>) {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::leave_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input| {
                let leave_tx = leave_tx.clone();
                async move {
                    leave_tx
                        .send(
                            input["subscription_id"]
                                .as_str()
                                .expect("subscription_id in leave payload")
                                .to_string(),
                        )
                        .expect("send leave payload");
                    FunctionResult::Success(None)
                }
            }),
        );
    }
}
