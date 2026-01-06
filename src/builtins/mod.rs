use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

use crate::modules::streams::{StreamWrapperMessage, adapters::StreamConnection};

type Subscribers = Vec<Arc<dyn StreamConnection>>;
type TopicName = String;
type GroupId = String;
type ItemId = String;
type ItemsData = HashMap<ItemId, Value>;
type ItemsDataAsString = HashMap<ItemId, String>;
type StoreKey = (TopicName, GroupId);

#[derive(Clone, Debug, Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub struct Storage(HashMap<StoreKey, ItemsDataAsString>);

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}

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

pub struct BuiltinKvStore {
    store: Arc<RwLock<HashMap<StoreKey, ItemsData>>>,
    #[allow(
        dead_code,
        reason = "Going to be used in the future for graceful shutdown"
    )]
    handler: Option<tokio::task::JoinHandle<()>>,
    dirty: Arc<AtomicBool>,
}

impl BuiltinKvStore {
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
                tracing::debug!("No changes detected, skipping save to disk");
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
    ) -> anyhow::Result<()> {
        let snapshot = {
            let store = storage.read().await;
            store.clone()
        };
        let file_path = file_path.to_string();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let storage = Storage::snapshot_to_storage(&snapshot);
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&storage)?;
            let humanized_size = bytes.len();
            tracing::info!("Saving storage to disk, size {:?}", humanized_size);
            let temp_file_path = format!("{}.tmp", file_path);
            if let Some(parent) = std::path::Path::new(&file_path).parent() {
                std::fs::create_dir_all(parent)
                    .expect("Failed to create parent directories for storage file");
            }
            std::fs::write(&temp_file_path, bytes)?;
            std::fs::rename(&temp_file_path, file_path)?;
            Ok(())
        })
        .await;
        match result {
            Ok(inner) => inner,
            Err(e) => Err(anyhow::Error::new(e)),
        }
    }

    pub async fn set(&self, stream_name: &str, group_id: &str, item_id: &str, data: Value) {
        let key = (stream_name.to_string(), group_id.to_string());
        let mut store = self.store.write().await;
        if let Some(topic) = store.get_mut(&key) {
            topic.insert(item_id.to_string(), data.clone());
        } else {
            let mut topic = HashMap::new();
            topic.insert(item_id.to_string(), data.clone());
            store.insert(key, topic);
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }

    pub async fn get(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let store = self.store.read().await;
        let topic = store.get(&key);
        if let Some(group) = topic {
            group.get(item_id).cloned()
        } else {
            None
        }
    }

    pub async fn delete(&self, stream_name: &str, group_id: &str, item_id: &str) -> Option<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let mut store = self.store.write().await;
        if let Some(group) = store.get_mut(&key) {
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
            group.remove(item_id)
        } else {
            None
        }
    }

    pub async fn get_group(&self, stream_name: &str, group_id: &str) -> Vec<Value> {
        let key = (stream_name.to_string(), group_id.to_string());
        let store = self.store.read().await;
        if let Some(topic) = store.get(&key) {
            topic.values().cloned().collect()
        } else {
            Vec::new()
        }
    }
}

pub struct BuiltInPubSubAdapter {
    subscribers: RwLock<HashMap<TopicName, Subscribers>>,
    events_tx: tokio::sync::broadcast::Sender<StreamWrapperMessage>,
}
impl BuiltInPubSubAdapter {
    pub fn new(config: Option<Value>) -> Self {
        let channel_size = config
            .clone()
            .and_then(|cfg| cfg.get("channel_size").and_then(|v| v.as_u64()))
            .unwrap_or(256) as usize;

        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(channel_size);
        Self {
            subscribers: RwLock::new(HashMap::new()),
            events_tx,
        }
    }
    pub async fn subscribe(&self, id: String, connection: Arc<dyn StreamConnection>) {
        let mut subscribers = self.subscribers.write().await;
        let entry = subscribers.entry(id).or_insert_with(Vec::new);
        entry.push(connection.clone());
    }
    pub async fn unsubscribe(&self, id: String) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(&id);
    }

    pub fn send_msg(&self, message: StreamWrapperMessage) {
        let _ = self.events_tx.send(message);
    }

    pub async fn watch_events(&self) {
        let mut rx = self.events_tx.subscribe();

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    tracing::debug!("Received stream event: {:?}", msg);
                    let group_id = msg.group_id.clone();
                    let stream_name = msg.stream_name.clone();
                    let subscribers = self.subscribers.read().await;
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
