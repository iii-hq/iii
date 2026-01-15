use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::{
    engine::Engine,
    modules::streams::{
        StreamIncomingMessage, StreamWrapperMessage,
        adapters::{StreamAdapter, StreamConnection},
        registry::{StreamAdapterFuture, StreamAdapterRegistration},
        structs::StreamIncomingMessageData,
    },
};

const DEFAULT_BRIDGE_URL: &str = "ws://127.0.0.1:49134";
const DEFAULT_EVENT_FUNCTION: &str = "streams.event";
const DEFAULT_CHANNEL_SIZE: usize = 256;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct BridgeStreamAdapterConfig {
    url: Option<String>,
    event_function: Option<String>,
    channel_size: Option<usize>,
    stream_ws_url: Option<String>,
}

type Subscribers = HashMap<String, Arc<dyn StreamConnection>>;

pub struct BridgeStreamAdapter {
    bridge: iii_sdk::Bridge,
    subscribers: Arc<RwLock<Subscribers>>,
    events_tx: broadcast::Sender<StreamWrapperMessage>,
    join_sender: Option<mpsc::UnboundedSender<StreamIncomingMessage>>,
}

impl BridgeStreamAdapter {
    pub fn new(
        bridge: iii_sdk::Bridge,
        channel_size: usize,
        event_function: &str,
        stream_ws_url: Option<String>,
    ) -> Self {
        let (events_tx, _events_rx) = broadcast::channel(channel_size);
        let subscribers = Arc::new(RwLock::new(HashMap::new()));
        let join_sender = stream_ws_url.map(|url| spawn_stream_ws_joiner(url, events_tx.clone()));

        let events_tx_clone = events_tx.clone();
        bridge.register_function(event_function, move |input| {
            let events_tx = events_tx_clone.clone();
            async move {
                let message: StreamWrapperMessage =
                    serde_json::from_value(input).map_err(|err| {
                        iii_sdk::BridgeError::Handler(format!(
                            "failed to decode stream event: {}",
                            err
                        ))
                    })?;

                if let Err(err) = events_tx.send(message) {
                    tracing::warn!(error = ?err, "Stream event dropped");
                }

                Ok(Value::Null)
            }
        });

        Self {
            bridge,
            subscribers,
            events_tx,
            join_sender,
        }
    }

    async fn add_subscriber(&self, id: String, connection: Arc<dyn StreamConnection>) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.insert(id, connection);
    }

    async fn remove_subscriber(&self, id: String) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&id);
    }

    async fn forward_events(&self) {
        let mut events_rx = self.events_tx.subscribe();
        let subscribers = self.subscribers.clone();

        tokio::spawn(async move {
            loop {
                match events_rx.recv().await {
                    Ok(message) => {
                        let subs = subscribers.read().await;
                        for connection in subs.values() {
                            if let Err(err) = connection.handle_stream_message(&message).await {
                                tracing::error!(error = ?err, "Failed to handle stream message");
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        tracing::warn!("Lagged in receiving stream events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::error!("Stream events channel closed");
                        break;
                    }
                }
            }
        });
    }
}

#[async_trait]
impl StreamAdapter for BridgeStreamAdapter {
    async fn destroy(&self) -> anyhow::Result<()> {
        let connections = {
            let mut subscribers = self.subscribers.write().await;
            let connections = subscribers.values().cloned().collect::<Vec<_>>();
            subscribers.clear();
            connections
        };

        for connection in connections {
            connection.cleanup().await;
        }

        Ok(())
    }

    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        match self
            .bridge
            .invoke_function(
                "streams.set",
                serde_json::json!({
                    "stream_name": stream_name,
                    "group_id": group_id,
                    "item_id": item_id,
                    "data": data,
                }),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("BridgeStreamAdapter set error: {}", e);
                return;
            }
        }
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        match self
            .bridge
            .invoke_function(
                "streams.get",
                serde_json::json!({
                    "stream_name": stream_name,
                    "group_id": group_id,
                    "item_id": item_id,
                }),
            )
            .await
        {
            Ok(result) => Some(result),
            Err(e) => {
                tracing::error!("BridgeStreamAdapter get error: {}", e);
                None
            }
        }
    }

    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        match self
            .bridge
            .invoke_function(
                "streams.delete",
                serde_json::json!({
                    "stream_name": stream_name,
                    "group_id": group_id,
                    "item_id": item_id,
                }),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("BridgeStreamAdapter delete error: {}", e)
            }
        }
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        match self
            .bridge
            .invoke_function(
                "streams.getGroup",
                serde_json::json!({
                    "stream_name": stream_name,
                    "group_id": group_id,
                }),
            )
            .await
        {
            Ok(result) => match result {
                Value::Array(arr) => arr,
                _ => vec![],
            },
            Err(e) => {
                tracing::error!("BridgeStreamAdapter get_group error: {}", e);
                vec![]
            }
        }
    }

    async fn list_groups(&self, stream_name: &str) -> Vec<String> {
        match self
            .bridge
            .invoke_function(
                "streams.listGroups",
                serde_json::json!({
                    "stream_name": stream_name,
                }),
            )
            .await
        {
            Ok(result) => match result {
                Value::Array(arr) => arr
                    .into_iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
                _ => vec![],
            },
            Err(e) => {
                tracing::error!("BridgeStreamAdapter list_groups error: {}", e);
                vec![]
            }
        }
    }

    async fn emit_event(&self, message: StreamWrapperMessage) {
        if let Err(err) = self.events_tx.send(message) {
            tracing::warn!(error = ?err, "Stream event dropped");
        }
    }

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        self.add_subscriber(id, connection).await;
    }

    async fn unsubscribe(&self, id: String) {
        self.remove_subscriber(id).await;
    }

    async fn watch_events(&self) {
        self.forward_events().await;
    }

    async fn notify_join(&self, data: &StreamIncomingMessageData) {
        let Some(join_sender) = &self.join_sender else {
            return;
        };

        let message = StreamIncomingMessage::Join { data: data.clone() };
        if let Err(err) = join_sender.send(message) {
            tracing::warn!(error = ?err, "Failed to queue stream join message");
        }
    }
}

fn spawn_stream_ws_joiner(
    url: String,
    events_tx: broadcast::Sender<StreamWrapperMessage>,
) -> mpsc::UnboundedSender<StreamIncomingMessage> {
    let (tx, mut rx) = mpsc::unbounded_channel::<StreamIncomingMessage>();

    tokio::spawn(async move {
        loop {
            match tokio_tungstenite::connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    tracing::info!(stream_ws_url = %url, "Connected to stream WebSocket");
                    let (mut ws_tx, mut ws_rx) = ws_stream.split();
                    let events_tx = events_tx.clone();

                    loop {
                        tokio::select! {
                            maybe_msg = rx.recv() => {
                                let msg = match maybe_msg {
                                    Some(msg) => msg,
                                    None => return,
                                };
                                let payload = match serde_json::to_string(&msg) {
                                    Ok(payload) => payload,
                                    Err(err) => {
                                        tracing::error!(error = ?err, "Failed to serialize stream join");
                                        continue;
                                    }
                                };
                                if let Err(err) = ws_tx.send(WsMessage::Text(payload)).await {
                                    tracing::warn!(error = ?err, "Failed to send stream join");
                                    break;
                                }
                            }
                            incoming = ws_rx.next() => {
                                match incoming {
                                    Some(Ok(WsMessage::Ping(payload))) => {
                                        let _ = ws_tx.send(WsMessage::Pong(payload)).await;
                                    }
                                    Some(Ok(WsMessage::Text(payload))) => {
                                        match serde_json::from_str::<StreamWrapperMessage>(&payload) {
                                            Ok(message) => {
                                                if let Err(err) = events_tx.send(message) {
                                                    tracing::warn!(error = ?err, "Stream event dropped");
                                                }
                                            }
                                            Err(err) => {
                                                tracing::warn!(error = ?err, "Failed to parse stream event payload");
                                            }
                                        }
                                    }
                                    Some(Ok(WsMessage::Binary(payload))) => {
                                        match serde_json::from_slice::<StreamWrapperMessage>(&payload) {
                                            Ok(message) => {
                                                if let Err(err) = events_tx.send(message) {
                                                    tracing::warn!(error = ?err, "Stream event dropped");
                                                }
                                            }
                                            Err(err) => {
                                                tracing::warn!(error = ?err, "Failed to parse stream event payload");
                                            }
                                        }
                                    }
                                    Some(Ok(WsMessage::Close(_))) => {
                                        break;
                                    }
                                    Some(Ok(_)) => {}
                                    Some(Err(err)) => {
                                        tracing::warn!(error = ?err, "Stream WebSocket error");
                                        break;
                                    }
                                    None => break,
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = ?err, stream_ws_url = %url, "Failed to connect to stream WebSocket");
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    tx
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StreamAdapterFuture {
    Box::pin(async move {
        let config: BridgeStreamAdapterConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let url = config
            .url
            .or_else(|| std::env::var("III_BRIDGE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BRIDGE_URL.to_string());

        let event_function = config
            .event_function
            .unwrap_or_else(|| DEFAULT_EVENT_FUNCTION.to_string());

        let channel_size = config.channel_size.unwrap_or(DEFAULT_CHANNEL_SIZE);
        let stream_ws_url = config
            .stream_ws_url
            .or_else(|| std::env::var("STREAMS_WS_URL").ok());

        let bridge = iii_sdk::Bridge::new(&url);
        let adapter =
            BridgeStreamAdapter::new(bridge.clone(), channel_size, &event_function, stream_ws_url);

        bridge
            .connect()
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        Ok(Arc::new(adapter) as Arc<dyn StreamAdapter>)
    })
}

crate::register_adapter!(
    <StreamAdapterRegistration> "modules::streams::adapters::BridgeAdapter",
    make_adapter
);
