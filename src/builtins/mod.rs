pub mod filters;
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

#[derive(Clone, Debug, Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub struct Storage(HashMap<String, String>);

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

    fn disk_to_store(self) -> HashMap<String, Value> {
        self.0
            .into_iter()
            .map(|(key, item)| {
                let item = serde_json::from_str::<Value>(&item).unwrap_or_default();
                (key, item)
            })
            .collect::<HashMap<String, Value>>()
    }

    fn store_to_disk(value: &HashMap<String, Value>) -> Storage {
        let mut storage = Storage::new();
        storage.0 = value
            .iter()
            .map(|(key, item)| {
                let item_as_string = serde_json::to_string(item).unwrap_or_default();
                (key.clone(), item_as_string)
            })
            .collect::<HashMap<String, String>>();
        storage
    }
}

pub struct BuiltinKvStore {
    store: Arc<RwLock<HashMap<String, Value>>>,
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
        let data_from_disk = storage.disk_to_store();
        let store = Arc::new(RwLock::new(data_from_disk));
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
        storage: Arc<RwLock<HashMap<String, Value>>>,
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
        storage: Arc<RwLock<HashMap<String, Value>>>,
        file_path: &str,
    ) -> anyhow::Result<()> {
        let snapshot = {
            let store = storage.read().await;
            store.clone()
        };
        let file_path = file_path.to_string();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let storage = Storage::store_to_disk(&snapshot);
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

    pub async fn set(&self, key: String, data: Value) {
        let mut store = self.store.write().await;
        store.insert(key, data);
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }

    pub async fn get(&self, key: String) -> Option<Value> {
        let store = self.store.read().await;
        let results = store.get(&key);
        results.cloned()
    }

    pub async fn delete(&self, key: String) -> Option<Value> {
        let mut store = self.store.write().await;
        let result = store.remove(&key);
        if result.is_some() {
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
        }
        result
    }

    pub async fn list_keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        let store = self.store.read().await;
        store
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    pub async fn update(&self, key: String, ops: Vec<filters::UpdateOp>) -> Option<Value> {
        let mut store = self.store.write().await;
        if let Some(existing_value) = store.get_mut(&key) {
            let mut updated_value = existing_value.clone();
            for op in ops {
                match op {
                    filters::UpdateOp::Set { path, value } => {
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
                    filters::UpdateOp::Merge { path, value } => {
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
                    filters::UpdateOp::Increment { path, by } => {
                        if let Value::Object(ref mut map) = updated_value {
                            if let Some(existing_val) = map.get_mut(&path.0) {
                                if let Some(num) = existing_val.as_i64() {
                                    *existing_val =
                                        Value::Number(serde_json::Number::from(num + by));
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
                    filters::UpdateOp::Decrement { path, by } => {
                        if let Value::Object(ref mut map) = updated_value {
                            if let Some(existing_val) = map.get_mut(&path.0) {
                                if let Some(num) = existing_val.as_i64() {
                                    *existing_val =
                                        Value::Number(serde_json::Number::from(num - by));
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
                    filters::UpdateOp::Remove { path } => {
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
            *existing_value = updated_value.clone();
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
            Some(updated_value)
        } else {
            None
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

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use mockall::mock;

    use super::*;
    use crate::modules::streams::{
        StreamOutboundMessage, adapters::StreamConnection as StreamConnectionTrait,
    };

    mock! {
        pub StreamConnection {}
        #[async_trait]
        impl StreamConnectionTrait for StreamConnection {
            async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()>;
            async fn cleanup(&self);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_default() {
        let storage = Storage::default();
        assert!(storage.0.is_empty(), "Default storage should be empty");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_load_storage() {
        // Test loading from a non-existent file
        let storage = Storage::load_storage("non_existent_file.db");
        assert!(
            storage.0.is_empty(),
            "Storage should be empty for non-existent file"
        );
        // Test with existing file
        let test_storage = Storage::new();
        let test_file_path = "test_storage.db";
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&test_storage).unwrap();
        std::fs::write(test_file_path, bytes).unwrap();
        let loaded_storage = Storage::load_storage(test_file_path);
        assert_eq!(loaded_storage.0.len(), 0);
        std::fs::remove_file(test_file_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_snapshot_to_storage_and_back() {
        let mut original_store: HashMap<String, Value> = HashMap::new();
        let key = "test_stream::test_group::item1".to_string();
        original_store.insert(key.clone(), serde_json::json!({"key": "value"}));

        let storage = Storage::store_to_disk(&original_store);
        let restored_store = storage.disk_to_store();

        assert_eq!(restored_store.len(), original_store.len());
        assert_eq!(
            restored_store.get(&key).unwrap(),
            &serde_json::json!({"key": "value"})
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_save_loop_when_dirty_flag_is_true() {
        let mut store: HashMap<String, Value> = HashMap::new();
        let key = "test_stream::test_group::item1".to_string();
        store.insert(key, serde_json::json!({"key": "value"}));

        let storage = Arc::new(RwLock::new(store));
        let dirty = Arc::new(AtomicBool::new(true));
        let test_file_path = "test_save_loop_storage.db";

        let save_handle = tokio::spawn(async move {
            BuiltinKvStore::save_loop(storage.clone(), 100, test_file_path, dirty.clone()).await;
        });

        // Wait some time to allow the save loop to run
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Check if the file was created
        assert!(std::path::Path::new(test_file_path).exists());

        // Clean up
        save_handle.abort();
        std::fs::remove_file(test_file_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_unsubscribe() {
        let adapter = BuiltInPubSubAdapter::new(None);
        let connection: Arc<dyn StreamConnection> = Arc::new(MockStreamConnection::new());
        let topic_id = "test_topic".to_string();

        adapter
            .subscribe(topic_id.clone(), connection.clone())
            .await;
        {
            let subscribers = adapter.subscribers.read().await;
            assert!(subscribers.contains_key(&topic_id));
            assert_eq!(subscribers.get(&topic_id).unwrap().len(), 1);
        }

        adapter.unsubscribe(topic_id.clone()).await;
        {
            let subscribers = adapter.subscribers.read().await;
            assert!(!subscribers.contains_key(&topic_id));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_save_in_disk_and_load() {
        let mut store: HashMap<String, Value> = HashMap::new();
        let key = "test_stream::test_group::item1".to_string();
        store.insert(key.clone(), serde_json::json!({"key": "value"}));

        let storage = Arc::new(RwLock::new(store));
        let test_file_path = "test_save_storage.db";

        BuiltinKvStore::save_in_disk(storage.clone(), test_file_path)
            .await
            .expect("Failed to save storage to disk");

        let loaded_storage = Storage::load_storage(test_file_path);
        let restored_store = loaded_storage.disk_to_store();
        assert_eq!(restored_store.len(), 1);
        assert_eq!(
            restored_store.get(&key).unwrap(),
            &serde_json::json!({"key": "value"})
        );

        std::fs::remove_file(test_file_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_invalid_store_method() {
        // when this happens it should default to in_memory
        let config = serde_json::json!({
            "store_method": "unknown_method"
        });
        let kv_store = BuiltinKvStore::new(Some(config));
        assert!(kv_store.store.read().await.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_with_config() {
        let config = serde_json::json!({
            "store_method": "file_based",
            "file_path": "test_kv_store.db",
            "save_interval_ms": 1000
        });
        let kv_store = BuiltinKvStore::new(Some(config));
        assert!(kv_store.store.read().await.is_empty());
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_pub_sub() {
        let adapter = BuiltInPubSubAdapter::new(None);

        let stream_name = "test_stream".to_string();
        let group_id = "test_group".to_string();

        let event = StreamOutboundMessage::Create {
            data: serde_json::json!({"key": "value"}),
        };
        let message = StreamWrapperMessage {
            stream_name: stream_name.clone(),
            group_id: group_id.clone(),
            id: Some("item1".to_string()),
            timestamp: chrono::Utc::now().timestamp_millis(),
            event: event.clone(),
        };

        let mut rx = adapter.events_tx.subscribe();
        adapter.send_msg(message.clone());

        let received = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for message")
            .expect("Should receive message");

        assert_eq!(received.stream_name, stream_name);
        assert_eq!(received.group_id, group_id);
        assert_eq!(received.id, Some("item1".to_string()));
        assert_eq!(received.event, event);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_set_get_delete() {
        let kv_store = BuiltinKvStore::new(None);

        let stream_name = "test_stream";
        let group_id = "test_group";
        let item_id = "item1";
        let data = serde_json::json!({"key": "value"});

        let key = format!("{}::{}::{}", stream_name, group_id, item_id);
        // Test set
        kv_store.set(key.clone(), data.clone()).await;

        // Test get
        let retrieved = kv_store.get(key.clone()).await.expect("Item should exist");
        assert_eq!(retrieved, data);

        // Test delete
        let deleted = kv_store
            .delete(key.clone())
            .await
            .expect("Item should exist for deletion");
        assert_eq!(deleted, data);

        // Ensure item is deleted
        let should_be_none = kv_store.get(key).await;
        assert!(should_be_none.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_missing_key() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "missing_key".to_string();

        let result = kv_store
            .update(
                key,
                vec![filters::UpdateOp::Set {
                    path: filters::FieldPath::from(""),
                    value: serde_json::json!({"a": 1}),
                }],
            )
            .await;

        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_set_root() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_set_root".to_string();

        kv_store.set(key.clone(), serde_json::json!({"a": 1})).await;

        let result = kv_store
            .update(
                key.clone(),
                vec![filters::UpdateOp::Set {
                    path: filters::FieldPath::from(""),
                    value: serde_json::json!({"b": 2}),
                }],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"b": 2}));
        assert_eq!(
            kv_store.get(key).await.expect("Value should exist"),
            serde_json::json!({"b": 2})
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_set_field() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_set_field".to_string();

        kv_store
            .set(key.clone(), serde_json::json!({"a": 1, "b": 2}))
            .await;

        let result = kv_store
            .update(
                key.clone(),
                vec![filters::UpdateOp::Set {
                    path: filters::FieldPath::from("c"),
                    value: serde_json::json!(3),
                }],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"a": 1, "b": 2, "c": 3}));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_merge_root() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_merge_root".to_string();

        kv_store.set(key.clone(), serde_json::json!({"a": 1})).await;

        let result = kv_store
            .update(
                key.clone(),
                vec![filters::UpdateOp::Merge {
                    path: None,
                    value: serde_json::json!({"b": 2}),
                }],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"a": 1, "b": 2}));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_merge_non_root_noop() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_merge_non_root".to_string();

        kv_store.set(key.clone(), serde_json::json!({"a": 1})).await;

        let result = kv_store
            .update(
                key.clone(),
                vec![filters::UpdateOp::Merge {
                    path: Some(filters::FieldPath::from("nested")),
                    value: serde_json::json!({"b": 2}),
                }],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"a": 1}));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_increment() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_increment".to_string();

        kv_store.set(key.clone(), serde_json::json!({"a": 1})).await;

        let result = kv_store
            .update(
                key.clone(),
                vec![
                    filters::UpdateOp::Increment {
                        path: filters::FieldPath::from("a"),
                        by: 2,
                    },
                    filters::UpdateOp::Increment {
                        path: filters::FieldPath::from("b"),
                        by: 5,
                    },
                ],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"a": 3, "b": 5}));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_decrement() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_decrement".to_string();

        kv_store
            .set(key.clone(), serde_json::json!({"a": 10}))
            .await;

        let result = kv_store
            .update(
                key.clone(),
                vec![
                    filters::UpdateOp::Decrement {
                        path: filters::FieldPath::from("a"),
                        by: 3,
                    },
                    filters::UpdateOp::Decrement {
                        path: filters::FieldPath::from("b"),
                        by: 2,
                    },
                ],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"a": 7, "b": -2}));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_remove() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "update_remove".to_string();

        kv_store
            .set(key.clone(), serde_json::json!({"a": 1, "b": 2}))
            .await;

        let result = kv_store
            .update(
                key.clone(),
                vec![filters::UpdateOp::Remove {
                    path: filters::FieldPath::from("a"),
                }],
            )
            .await
            .expect("Update should return value");

        assert_eq!(result, serde_json::json!({"b": 2}));
    }
}
