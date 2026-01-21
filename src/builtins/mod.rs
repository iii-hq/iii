use std::{
    collections::HashMap,
    ffi::OsStr,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

use crate::modules::{
    kv_server::structs::{UpdateOp, UpdateResult},
    streams::{StreamWrapperMessage, adapters::StreamConnection},
};

type Subscribers = Vec<Arc<dyn StreamConnection>>;
type TopicName = String;

const SCALAR_SENTINEL_KEY: &str = "__kv_store_scalar__";
const KEY_FILE_EXTENSION: &str = "bin";

fn is_scalar_map(map: &HashMap<String, Value>) -> bool {
    map.len() == 1 && map.contains_key(SCALAR_SENTINEL_KEY)
}

fn map_to_value(map: &HashMap<String, Value>) -> Value {
    if let Some(value) = map.get(SCALAR_SENTINEL_KEY)
        && is_scalar_map(map)
    {
        return value.clone();
    }

    Value::Object(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

fn value_to_map(value: Value) -> HashMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        other => {
            let mut map = HashMap::new();
            map.insert(SCALAR_SENTINEL_KEY.to_string(), other);
            map
        }
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize)]
struct KeyStorage(String);

#[derive(Clone, Copy, Debug)]
enum DirtyOp {
    Upsert,
    Delete,
}

fn encode_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for byte in key.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn decode_key(encoded: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut iter = encoded.as_bytes().iter().copied();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let high = iter.next()?;
            let low = iter.next()?;
            let high = (high as char).to_digit(16)? as u8;
            let low = (low as char).to_digit(16)? as u8;
            bytes.push((high << 4) | low);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn key_file_name(key: &str) -> String {
    format!("{}.{}", encode_key(key), KEY_FILE_EXTENSION)
}

fn key_from_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let file_name = file_name.strip_suffix(&format!(".{}", KEY_FILE_EXTENSION))?;
    decode_key(file_name)
}

fn load_store_from_dir(dir: &Path) -> HashMap<String, HashMap<String, Value>> {
    let mut store = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::info!(error = ?err, "storage directory not found, starting empty");
            return store;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension() != Some(OsStr::new(KEY_FILE_EXTENSION)) {
            continue;
        }
        let key = match key_from_path(&path) {
            Some(key) => key,
            None => {
                tracing::warn!(path = %path.display(), "invalid key filename, skipping");
                continue;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to read key file");
                continue;
            }
        };
        let storage = match rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes) {
            Ok(storage) => storage,
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to parse key file");
                continue;
            }
        };
        let value = match serde_json::from_str::<Value>(&storage.0) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to decode key value");
                continue;
            }
        };
        store.insert(key, value_to_map(value));
    }

    store
}

async fn persist_key_to_disk(
    dir: &Path,
    key: &str,
    value: &HashMap<String, Value>,
) -> anyhow::Result<()> {
    if let Err(err) = tokio::fs::create_dir_all(dir).await {
        tracing::error!(error = ?err, path = %dir.display(), "failed to create storage directory");
        return Err(err.into());
    }

    let file_name = key_file_name(key);
    let path = dir.join(&file_name);
    let temp_path = dir.join(format!("{}.tmp", file_name));
    let value = map_to_value(value);
    let json = serde_json::to_string(&value)?;
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(json))?;

    tokio::fs::write(&temp_path, bytes).await?;
    tokio::fs::rename(&temp_path, &path).await?;

    Ok(())
}

async fn delete_key_from_disk(dir: &Path, key: &str) -> anyhow::Result<()> {
    let path = dir.join(key_file_name(key));
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetResult {
    pub old_value: Option<Value>,
    pub new_value: Value,
}

pub struct BuiltinKvStore {
    store: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
    file_store_dir: Option<PathBuf>,
    dirty: Arc<RwLock<HashMap<String, DirtyOp>>>,
    #[allow(
        dead_code,
        reason = "Going to be used in the future for graceful shutdown"
    )]
    handler: Option<tokio::task::JoinHandle<()>>,
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

        let file_store_dir = match store_method.as_str() {
            "file_based" => {
                let dir = PathBuf::from(&file_path);
                if let Err(err) = std::fs::create_dir_all(&dir) {
                    tracing::error!(error = ?err, path = %dir.display(), "failed to create storage directory");
                }
                Some(dir)
            }
            "in_memory" => None,
            other => {
                tracing::warn!(store_method = %other, "Unknown store_method, defaulting to in_memory");
                None
            }
        };

        let data_from_disk = match &file_store_dir {
            Some(dir) => load_store_from_dir(dir),
            None => HashMap::new(),
        };
        let store = Arc::new(RwLock::new(data_from_disk));
        let dirty = Arc::new(RwLock::new(HashMap::new()));
        let handler = file_store_dir.clone().map(|dir| {
            let store = Arc::clone(&store);
            let dirty = Arc::clone(&dirty);
            tokio::spawn(async move {
                Self::save_loop(store, dirty, interval, dir).await;
            })
        });

        Self {
            store,
            file_store_dir,
            dirty,
            handler,
        }
    }

    async fn save_loop(
        store: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
        dirty: Arc<RwLock<HashMap<String, DirtyOp>>>,
        polling_interval: u64,
        dir: PathBuf,
    ) {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(polling_interval));
        loop {
            interval.tick().await;
            let batch = {
                let mut dirty = dirty.write().await;
                if dirty.is_empty() {
                    continue;
                }
                dirty.drain().collect::<Vec<_>>()
            };

            for (key, op) in batch {
                match op {
                    DirtyOp::Upsert => {
                        let value = {
                            let store = store.read().await;
                            store.get(&key).cloned()
                        };
                        if let Some(value) = value
                            && let Err(err) = persist_key_to_disk(&dir, &key, &value).await
                        {
                            tracing::error!(error = ?err, key = %key, "failed to persist key");
                            let mut dirty = dirty.write().await;
                            dirty.insert(key, DirtyOp::Upsert);
                        }
                    }
                    DirtyOp::Delete => {
                        if let Err(err) = delete_key_from_disk(&dir, &key).await {
                            tracing::error!(error = ?err, key = %key, "failed to delete key");
                            let mut dirty = dirty.write().await;
                            dirty.insert(key, DirtyOp::Delete);
                        }
                    }
                }
            }
        }
    }

    pub async fn set(&self, key: String, data: Value) -> SetResult {
        let mut store = self.store.write().await;
        let old_value = store.get(&key).map(map_to_value);
        store.insert(key.clone(), value_to_map(data.clone()));
        drop(store);

        if self.file_store_dir.is_some() {
            let mut dirty = self.dirty.write().await;
            dirty.insert(key.clone(), DirtyOp::Upsert);
        }

        SetResult {
            old_value,
            new_value: data,
        }
    }

    pub async fn get(&self, key: String) -> Option<Value> {
        let store = self.store.read().await;
        store.get(&key).map(map_to_value)
    }

    pub async fn delete(&self, key: String) -> Option<Value> {
        let mut store = self.store.write().await;
        let result = store.remove(&key).map(|value| map_to_value(&value));
        drop(store);
        if result.is_some() && self.file_store_dir.is_some() {
            let mut dirty = self.dirty.write().await;
            dirty.insert(key.clone(), DirtyOp::Delete);
        }
        result
    }

    pub async fn list_keys_with_prefix(&self, prefix: String) -> Vec<String> {
        let store = self.store.read().await;
        store
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect()
    }

    pub async fn update(&self, key: String, ops: Vec<UpdateOp>) -> Option<UpdateResult> {
        let mut store = self.store.write().await;
        if let Some(existing_value) = store.get_mut(&key) {
            let old_value = map_to_value(existing_value);
            let mut updated_value = map_to_value(existing_value);

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
                    UpdateOp::Decrement { path, by } => {
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

            *existing_value = value_to_map(updated_value.clone());
            drop(store);

            if self.file_store_dir.is_some() {
                let mut dirty = self.dirty.write().await;
                dirty.insert(key.clone(), DirtyOp::Upsert);
            }

            Some(UpdateResult {
                old_value: Some(old_value),
                new_value: updated_value,
            })
        } else {
            None
        }
    }

    pub async fn list(&self, key: String) -> Vec<Value> {
        let store = self.store.read().await;
        store.get(&key).map_or(vec![], |topic| {
            if is_scalar_map(topic) {
                return vec![];
            }
            topic.values().cloned().collect()
        })
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

    fn temp_store_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kv_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_file_based_load_set_delete() {
        let dir = temp_store_dir();
        let key = "test_stream::test_group::item1";
        let data = serde_json::json!({"key": "value"});
        let file_path = dir.join(key_file_name(key));

        let json = serde_json::to_string(&data).unwrap();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(json)).unwrap();
        std::fs::write(&file_path, bytes).unwrap();

        let config = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });
        let kv_store = BuiltinKvStore::new(Some(config));

        let loaded = kv_store.get(key.to_string()).await;
        assert_eq!(loaded, Some(data.clone()));

        let updated = serde_json::json!({"key": "updated"});
        kv_store.set(key.to_string(), updated.clone()).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let bytes = std::fs::read(&file_path).unwrap();
        let storage = rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes).unwrap();
        let on_disk = serde_json::from_str::<Value>(&storage.0).unwrap();
        assert_eq!(on_disk, updated);

        kv_store.delete(key.to_string()).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!file_path.exists());

        std::fs::remove_dir_all(&dir).unwrap();
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
        let dir = temp_store_dir();
        let config = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });
        let kv_store = BuiltinKvStore::new(Some(config));
        assert!(kv_store.store.read().await.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
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
    async fn test_builtin_kv_store_update_basic_operations() {
        use crate::modules::kv_server::structs::{FieldPath, UpdateOp};

        let kv_store = BuiltinKvStore::new(None);
        let key = "test:update:basic".to_string();

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store.set(key.clone(), initial.clone()).await;

        // Test Set operation
        let result = kv_store
            .update(
                key.clone(),
                vec![UpdateOp::Set {
                    path: FieldPath("name".to_string()),
                    value: Value::String("B".to_string()),
                }],
            )
            .await
            .expect("Update should succeed");

        assert_eq!(result.old_value, Some(initial));
        assert_eq!(result.new_value["name"], "B");
        assert_eq!(result.new_value["counter"], 0);

        // Test Increment operation
        let result = kv_store
            .update(
                key.clone(),
                vec![UpdateOp::Increment {
                    path: FieldPath("counter".to_string()),
                    by: 5,
                }],
            )
            .await
            .expect("Update should succeed");

        assert_eq!(result.new_value["counter"], 5);

        // Test Decrement operation
        let result = kv_store
            .update(
                key.clone(),
                vec![UpdateOp::Decrement {
                    path: FieldPath("counter".to_string()),
                    by: 2,
                }],
            )
            .await
            .expect("Update should succeed");

        assert_eq!(result.new_value["counter"], 3);

        // Test Remove operation
        let result = kv_store
            .update(
                key.clone(),
                vec![UpdateOp::Remove {
                    path: FieldPath("name".to_string()),
                }],
            )
            .await
            .expect("Update should succeed");

        assert!(result.new_value.get("name").is_none());
        assert_eq!(result.new_value["counter"], 3);

        // Test Merge operation
        let result = kv_store
            .update(
                key.clone(),
                vec![UpdateOp::Merge {
                    path: None,
                    value: serde_json::json!({"name": "C", "extra": "field"}),
                }],
            )
            .await
            .expect("Update should succeed");

        assert_eq!(result.new_value["name"], "C");
        assert_eq!(result.new_value["counter"], 3);
        assert_eq!(result.new_value["extra"], "field");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_multiple_ops_in_single_call() {
        use crate::modules::kv_server::structs::{FieldPath, UpdateOp};

        let kv_store = BuiltinKvStore::new(None);
        let key = "test:update:multi_ops".to_string();

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store.set(key.clone(), initial).await;

        // Apply multiple operations in a single update call
        let result = kv_store
            .update(
                key.clone(),
                vec![
                    UpdateOp::Set {
                        path: FieldPath("name".to_string()),
                        value: Value::String("Z".to_string()),
                    },
                    UpdateOp::Increment {
                        path: FieldPath("counter".to_string()),
                        by: 10,
                    },
                    UpdateOp::Set {
                        path: FieldPath("status".to_string()),
                        value: Value::String("active".to_string()),
                    },
                ],
            )
            .await
            .expect("Update should succeed");

        assert_eq!(result.new_value["name"], "Z");
        assert_eq!(result.new_value["counter"], 10);
        assert_eq!(result.new_value["status"], "active");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_two_threads_counter_and_name() {
        use crate::modules::kv_server::structs::{FieldPath, UpdateOp};
        use futures::Future;
        use std::pin::Pin;

        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let key = "test:update:two_threads".to_string();

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store.set(key.clone(), initial).await;

        let names = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

        let mut all_tasks: Vec<Pin<Box<dyn Future<Output = Option<UpdateResult>> + Send>>> = vec![];

        // Add counter update tasks (500 increments of 2 each)
        for _ in 0..500 {
            let kv = Arc::clone(&kv_store);
            let k = key.clone();
            let task = Box::pin(async move {
                kv.update(
                    k,
                    vec![UpdateOp::Increment {
                        path: FieldPath("counter".to_string()),
                        by: 2,
                    }],
                )
                .await
            });
            all_tasks.push(task);
        }

        // Add name update tasks (500 sets cycling through A-J)
        for i in 0..500 {
            let kv = Arc::clone(&kv_store);
            let k = key.clone();
            let name_idx = i % 10;
            let name_value = names[name_idx].to_string();
            let task = Box::pin(async move {
                kv.update(
                    k,
                    vec![UpdateOp::Set {
                        path: FieldPath("name".to_string()),
                        value: Value::String(name_value),
                    }],
                )
                .await
            });
            all_tasks.push(task);
        }

        let results = futures::future::join_all(all_tasks).await;

        let mut success_count = 0;
        let mut error_count = 0;
        for result in &results {
            match result {
                Some(_) => success_count += 1,
                None => error_count += 1,
            }
        }

        assert_eq!(
            error_count, 0,
            "All updates should succeed, but {} failed",
            error_count
        );
        assert_eq!(
            success_count, 1000,
            "Expected 1000 successful updates (500 counter + 500 name)"
        );

        let final_value = kv_store.get(key).await.expect("Value should exist");

        assert_eq!(
            final_value["counter"], 1000,
            "Counter should be 1000 after 500 increments of 2"
        );

        let name = final_value["name"].as_str().unwrap_or("");
        assert_eq!("I", name.to_string());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_concurrent_500_calls() {
        use crate::modules::kv_server::structs::{FieldPath, UpdateOp};

        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let key = "test:update:concurrent_500".to_string();

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store.set(key.clone(), initial).await;

        let names = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

        // Create 500 concurrent update futures
        let mut tasks = vec![];
        for i in 0..500 {
            let kv_store = Arc::clone(&kv_store);
            let key = key.clone();
            let name = names[i % 10].to_string();

            let task = tokio::spawn(async move {
                kv_store
                    .update(
                        key,
                        vec![
                            UpdateOp::Increment {
                                path: FieldPath("counter".to_string()),
                                by: 2,
                            },
                            UpdateOp::Set {
                                path: FieldPath("name".to_string()),
                                value: Value::String(name),
                            },
                        ],
                    )
                    .await
            });

            tasks.push(task);
        }

        // Wait for all tasks to complete
        for task in tasks {
            let _ = task.await;
        }

        // Verify final result
        let final_value = kv_store.get(key).await.expect("Value should exist");

        let counter = final_value["counter"].as_i64().unwrap_or(0);

        println!(
            "BuiltinKvStore concurrent test result: counter = {} (expected 1000)",
            counter
        );
        println!("Final name: {}", final_value["name"]);

        // Counter should be exactly 1000 because RwLock serializes the updates
        assert_eq!(
            counter, 1000,
            "Counter should be 1000 after 500 concurrent increments of 2"
        );

        // Name should be one of A-J
        let name = final_value["name"].as_str().unwrap_or("");
        assert!(
            names.contains(&name),
            "Name should be one of A-J, got '{}'",
            name
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_concurrent_500_using_join_all() {
        use crate::modules::kv_server::structs::{FieldPath, UpdateOp};

        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let key = "test:update:concurrent_join_all".to_string();

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store.set(key.clone(), initial).await;

        let names = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

        // Create 500 update futures without spawning
        let mut futures = vec![];
        for i in 0..500 {
            let kv_store = Arc::clone(&kv_store);
            let key = key.clone();
            let name = names[i % 10].to_string();

            let future = async move {
                kv_store
                    .update(
                        key,
                        vec![
                            UpdateOp::Increment {
                                path: FieldPath("counter".to_string()),
                                by: 2,
                            },
                            UpdateOp::Set {
                                path: FieldPath("name".to_string()),
                                value: Value::String(name),
                            },
                        ],
                    )
                    .await
            };

            futures.push(future);
        }

        // Execute all futures concurrently using join_all
        futures::future::join_all(futures).await;

        // Verify final result
        let final_value = kv_store.get(key).await.expect("Value should exist");

        let counter = final_value["counter"].as_i64().unwrap_or(0);

        println!(
            "BuiltinKvStore join_all test result: counter = {} (expected 1000)",
            counter
        );
        println!("Final name: {}", final_value["name"]);

        // Counter should be exactly 1000 because RwLock serializes the updates
        assert_eq!(
            counter, 1000,
            "Counter should be 1000 after 500 concurrent increments of 2"
        );

        // Name should be one of A-J
        let name = final_value["name"].as_str().unwrap_or("");
        assert!(
            names.contains(&name),
            "Name should be one of A-J, got '{}'",
            name
        );
    }
}
