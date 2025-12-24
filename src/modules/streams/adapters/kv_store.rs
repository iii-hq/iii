use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

use async_trait::async_trait;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

use crate::{
    engine::Engine,
    modules::streams::{
        StreamOutboundMessage, StreamWrapperMessage,
        adapters::{StreamAdapter, StreamConnection},
        registry::{StreamAdapterFuture, StreamAdapterRegistration},
    },
};

type Subscribers = Vec<Arc<dyn StreamConnection>>;
type TopicName = String;
type GroupId = String;
type ItemId = String;
type ItemsData = HashMap<ItemId, Value>;
type ItemsDataAsString = HashMap<ItemId, String>;
type StoreKey = (TopicName, GroupId);

#[derive(Clone, Debug, Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub struct Storage(HashMap<StoreKey, ItemsDataAsString>);

impl Storage {
    pub fn new() -> Self {
        Storage(HashMap::new())
    }

    fn load_storage(file_path: &str) -> Storage {
        let bytes = match std::fs::read(file_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::info!(error = ?err, "storage file not found, starting empty");
                return Storage::new();
            }
        };

        match rkyv::from_bytes::<Storage, rkyv::rancor::Error>(&bytes) {
            Ok(storage) => storage,
            Err(err) => {
                tracing::error!(error = ?err, "failed to parse
  storage from disk");
                Storage::new()
            }
        }
    }

    fn storage_to_store(self) -> HashMap<StoreKey, ItemsData> {
        self.0
            .into_iter()
            .map(|(key, items)| {
                let items: ItemsData = items
                    .into_iter()
                    .filter_map(|(item_id, data_str)| {
                        serde_json::from_str::<Value>(&data_str)
                            .ok()
                            .map(|value| (item_id, value))
                    })
                    .collect();
                (key, items)
            })
            .collect()
    }

    fn snapshot_to_storage(value: &HashMap<StoreKey, ItemsData>) -> Storage {
        let mut storage = Storage::new();
        for (key, items) in value.iter() {
            let mut items_as_string = HashMap::new();
            for (item_id, data) in items.iter() {
                let data_as_string = serde_json::to_string(data).unwrap_or_default();
                items_as_string.insert(item_id.clone(), data_as_string);
            }
            storage.0.insert(key.clone(), items_as_string);
        }
        storage
    }
}

pub struct KvStore {
    store: Arc<RwLock<HashMap<StoreKey, ItemsData>>>,
    subscribers: RwLock<HashMap<TopicName, Subscribers>>,
    events_tx: tokio::sync::broadcast::Sender<StreamWrapperMessage>,
    #[allow(
        dead_code,
        reason = "Going to be used in the future for graceful shutdown"
    )]
    handler: Option<tokio::task::JoinHandle<()>>,
    dirty: Arc<AtomicBool>,
}

impl KvStore {
    pub fn new(config: Option<Value>) -> Self {
        tracing::debug!("Initializing KvStore with config: {:?}", config);
        let store_method = config
            .clone()
            .and_then(|cfg| {
                cfg.get("store_method")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "in_memory".to_string());

        let channel_size = config
            .clone()
            .and_then(|cfg| cfg.get("channel_size").and_then(|v| v.as_u64()))
            .unwrap_or(256) as usize;

        let file_path = config
            .clone()
            .and_then(|cfg| {
                cfg.get("file_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "kv_store_data.db".to_string());

        let interval = config
            .clone()
            .and_then(|cfg| cfg.get("save_interval_ms").and_then(|v| v.as_u64()))
            .unwrap_or(5000);

        let storage = match store_method.as_str() {
            "in_memory" => Storage::new(),
            "file_based" => Storage::load_storage(&file_path),
            other => {
                tracing::warn!(store_method = %other, "Unknown store_method, defaulting to in_memory");
                Storage::new()
            }
        };
        let store = Arc::new(RwLock::new(storage.storage_to_store()));
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(channel_size);
        let storage_clone = store.clone();

        let dirty = Arc::new(AtomicBool::new(false));
        let dirty_clone = dirty.clone();
        let handler = match store_method.as_str() {
            "file_based" => Some(tokio::spawn(async move {
                Self::save_loop(storage_clone, interval, &file_path, dirty_clone).await;
            })),
            _ => None,
        };
        Self {
            store,
            subscribers: RwLock::new(HashMap::new()),
            events_tx,
            handler,
            dirty,
        }
    }

    async fn save_loop(
        storage: Arc<RwLock<HashMap<StoreKey, ItemsData>>>,
        polling_interval: u64,
        file_path: &str,
        dirty: Arc<AtomicBool>,
    ) {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(polling_interval));
        loop {
            interval.tick().await;
            if !dirty.swap(false, std::sync::atomic::Ordering::AcqRel) {
                continue;
            }
            match Self::save_in_disk(storage.clone(), file_path).await {
                Ok(_) => tracing::info!("Storage saved to disk successfully"),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to save storage to disk");
                    dirty.store(true, std::sync::atomic::Ordering::Release);
                }
            }
        }
    }

    async fn save_in_disk(
        storage: Arc<RwLock<HashMap<StoreKey, ItemsData>>>,
        file_path: &str,
    ) -> Result<(), tokio::task::JoinError> {
        let snapshot = {
            let store = storage.read().await;
            store.clone()
        };
        let file_path = file_path.to_string();
        tokio::task::spawn_blocking(move || {
            let storage = Storage::snapshot_to_storage(&snapshot);
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&storage).unwrap();
            let humanized_size = bytes.len();
            tracing::info!("Saving storage to disk, size {:?}", humanized_size);
            let temp_file_path = format!("{}.tmp", file_path);
            if let Some(parent) = std::path::Path::new(&file_path).parent() {
                std::fs::create_dir_all(parent)
                    .expect("Failed to create parent directories for storage file");
            }
            std::fs::write(&temp_file_path, bytes).expect("Failed to write storage to temp file");
            std::fs::rename(&temp_file_path, file_path)
                .expect("Failed to rename temp file to storage file");
        })
        .await
    }
}

#[async_trait]
impl StreamAdapter for KvStore {
    async fn emit_event(&self, message: StreamWrapperMessage) {
        let _ = self.events_tx.send(message);
    }
    async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        let key = (stream_name.to_string(), group_id.to_string());
        let mut store = self.store.write().await;
        if let Some(topic) = store.get_mut(&key) {
            let existed = topic.contains_key(item_id);
            topic.insert(item_id.to_string(), data.clone());
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
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
            return;
        } else {
            let mut topic = HashMap::new();
            topic.insert(item_id.to_string(), data.clone());
            store.insert(key, topic);
            let message = StreamWrapperMessage {
                timestamp: chrono::Utc::now().timestamp_millis(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: Some(item_id.to_string()),
                event: StreamOutboundMessage::Create { data },
            };
            self.emit_event(message).await;
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
            return;
        }
    }

    async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let store = self.store.read().await;
        let topic = store.get(&key);
        if let Some(group) = topic {
            group.get(item_id).cloned()
        } else {
            None
        }
    }
    async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) {
        let key = (stream_name.to_string(), group_id.to_string());
        let mut store = self.store.write().await;
        if let Some(group) = store.get_mut(&key)
            && let Some(data) = group.remove(item_id)
        {
            let message = StreamWrapperMessage {
                timestamp: chrono::Utc::now().timestamp_millis(),
                stream_name: stream_name.to_string(),
                group_id: group_id.to_string(),
                id: Some(item_id.to_string()),
                event: StreamOutboundMessage::Delete { data: data.clone() },
            };
            self.emit_event(message).await;
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
            return;
        }
        tracing::warn!(stream_name = %stream_name, group_id = %group_id, item_id = %item_id, "Item to delete not found");
    }

    async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let store = self.store.read().await;
        if let Some(topic) = store.get(&key) {
            topic.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        let mut subscribers = self.subscribers.write().await;
        let entry = subscribers.entry(id).or_insert_with(Vec::new);
        entry.push(connection.clone());
    }
    async fn unsubscribe(&self, id: String) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&id);
    }

    async fn watch_events(&self) {
        let mut rx = self.events_tx.subscribe();

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    tracing::debug!("Received stream event: {:?}", msg);
                    let group_id = msg.group_id.clone();
                    let stream_name = msg.stream_name.clone();
                    let subscribers = self.subscribers.read().await;
                    // For simplicity, broadcasting to all subscribers of the stream
                    // In a real implementation, you might want to filter based on group_id or
                    // other criteria
                    for connections in subscribers.values() {
                        for connection in connections.iter() {
                            tracing::debug!(stream_name = %stream_name, group_id = %group_id, "Handling stream message for subscriber");
                            if let Err(e) = connection.handle_stream_message(&msg).await {
                                tracing::error!(error = %e, stream_name = %stream_name, group_id = %group_id, "Failed to handle stream message");
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    tracing::warn!("Lagged in receiving stream events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::error!("Stream events channel closed");
                    break;
                }
            }
        }
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> StreamAdapterFuture {
    Box::pin(async move { Ok(Arc::new(KvStore::new(config)) as Arc<dyn StreamAdapter>) })
}

crate::register_adapter!(<StreamAdapterRegistration> "modules::streams::adapters::KvStore", make_adapter);
