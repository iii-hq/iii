use std::{
    collections::HashMap,
    ffi::OsStr,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde_json::Value;
use tokio::sync::RwLock;

const KEY_FILE_EXTENSION: &str = "bin";

#[derive(Archive, RkyvSerialize, RkyvDeserialize)]
struct KeyStorage(String);

#[derive(Clone, Copy, Debug)]
enum DirtyOp {
    Upsert,
    Delete,
}

fn encode_index(index: &str) -> String {
    let mut out = String::with_capacity(index.len());
    for byte in index.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn decode_index(encoded: &str) -> Option<String> {
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

fn index_file_name(index: &str) -> String {
    format!("{}.{}", encode_index(index), KEY_FILE_EXTENSION)
}

fn index_from_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let file_name = file_name.strip_suffix(&format!(".{}", KEY_FILE_EXTENSION))?;
    decode_index(file_name)
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
        let index = match index_from_path(&path) {
            Some(index) => index,
            None => {
                tracing::warn!(path = %path.display(), "invalid index filename, skipping");
                continue;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to read index file");
                continue;
            }
        };
        let storage = match rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes) {
            Ok(storage) => storage,
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to parse index file");
                continue;
            }
        };
        let value = match serde_json::from_str::<HashMap<String, Value>>(&storage.0) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to decode index value");
                continue;
            }
        };
        store.insert(index, value);
    }

    store
}

async fn persist_index_to_disk(
    dir: &Path,
    index: &str,
    value: &HashMap<String, Value>,
) -> anyhow::Result<()> {
    if let Err(err) = tokio::fs::create_dir_all(dir).await {
        tracing::error!(error = ?err, path = %dir.display(), "failed to create storage directory");
        return Err(err.into());
    }

    let file_name = index_file_name(index);
    let path = dir.join(&file_name);
    let temp_path = dir.join(format!("{}.tmp", file_name));
    let json = serde_json::to_string(value)?;
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(json))?;

    tokio::fs::write(&temp_path, bytes).await?;
    tokio::fs::rename(&temp_path, &path).await?;

    Ok(())
}

async fn delete_index_from_disk(dir: &Path, index: &str) -> anyhow::Result<()> {
    let path = dir.join(index_file_name(index));
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
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

            for (index, op) in batch {
                match op {
                    DirtyOp::Upsert => {
                        let value = {
                            let store = store.read().await;
                            store.get(&index).cloned()
                        };
                        if let Some(value) = value
                            && let Err(err) = persist_index_to_disk(&dir, &index, &value).await
                        {
                            tracing::error!(error = ?err, index = %index, "failed to persist index");
                            let mut dirty = dirty.write().await;
                            dirty.insert(index, DirtyOp::Upsert);
                        }
                    }
                    DirtyOp::Delete => {
                        if let Err(err) = delete_index_from_disk(&dir, &index).await {
                            tracing::error!(error = ?err, index = %index, "failed to delete index");
                            let mut dirty = dirty.write().await;
                            dirty.insert(index, DirtyOp::Delete);
                        }
                    }
                }
            }
        }
    }

    pub async fn set(&self, index: String, key: String, data: Value) -> SetResult {
        let result = {
            let mut store = self.store.write().await;
            let index_map = store.get_mut(&index);

            if let Some(index_map) = index_map {
                let old_value = index_map.get(&key).cloned();
                index_map.insert(key.clone(), data.clone());

                SetResult {
                    old_value,
                    new_value: data.clone(),
                }
            } else {
                let mut index_map = HashMap::new();
                index_map.insert(key, data.clone());
                store.insert(index.clone(), index_map);

                SetResult {
                    old_value: None,
                    new_value: data.clone(),
                }
            }
        };

        if self.file_store_dir.is_some() {
            self.dirty.write().await.insert(index, DirtyOp::Upsert);
        }

        result
    }

    pub async fn get(&self, index: String, key: String) -> Option<Value> {
        let store = self.store.read().await;
        let index = store.get(&index);

        if let Some(index) = index {
            return index.get(&key).cloned();
        }

        None
    }

    pub async fn delete(&self, index: String, key: String) -> Option<Value> {
        let (removed, dirty_op) = {
            let mut store = self.store.write().await;
            let index_map = store.get_mut(&index);

            if let Some(index_map) = index_map {
                let removed = index_map.remove(&key);
                let dirty_op = if removed.is_some() {
                    if index_map.is_empty() {
                        Some(DirtyOp::Delete)
                    } else {
                        Some(DirtyOp::Upsert)
                    }
                } else {
                    None
                };
                (removed, dirty_op)
            } else {
                (None, None)
            }
        };

        if removed.is_some()
            && self.file_store_dir.is_some()
            && let Some(dirty_op) = dirty_op
        {
            self.dirty.write().await.insert(index, dirty_op);
        }

        removed
    }

    pub async fn update(&self, index: String, key: String, ops: Vec<UpdateOp>) -> UpdateResult {
        let mut store = self.store.write().await;

        // Automatically create index_map if it doesn't exist
        let index_map = store.entry(index.clone()).or_insert_with(HashMap::new);

        let old_value = index_map.get(&key).cloned();
        // Automatically create key entry with empty object if it doesn't exist
        let existing_value = index_map
            .entry(key.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));

        let mut updated_value = existing_value.clone();

        for op in ops {
            match op {
                UpdateOp::Set { path, value } => {
                    if path.0.is_empty()
                        && let Some(value) = value
                    {
                        updated_value = value;
                    } else if let Value::Object(ref mut map) = updated_value {
                        map.insert(path.0, value.unwrap_or(Value::Null));
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
                                *existing_val = Value::Number(serde_json::Number::from(num + by));
                            } else {
                                *existing_val = Value::Number(serde_json::Number::from(by));
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
                                *existing_val = Value::Number(serde_json::Number::from(num - by));
                            } else {
                                *existing_val = Value::Number(serde_json::Number::from(0));
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

        // Write the updated value back to the store
        *existing_value = updated_value.clone();

        drop(store);

        if self.file_store_dir.is_some() {
            self.dirty
                .write()
                .await
                .insert(index.clone(), DirtyOp::Upsert);
        }

        UpdateResult {
            old_value,
            new_value: updated_value,
        }
    }

    pub async fn list_keys_with_prefix(&self, prefix: String) -> Vec<String> {
        let store = self.store.read().await;
        store
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect()
    }

    pub async fn list(&self, index: String) -> Vec<Value> {
        let store = self.store.read().await;
        store
            .get(&index)
            .map_or(vec![], |topic| topic.values().cloned().collect())
    }
}

#[cfg(test)]
mod test {
    use iii_sdk::{FieldPath, UpdateOp};

    use super::*;

    fn temp_store_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kv_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_file_based_load_set_delete() {
        let dir = temp_store_dir();
        let index = "test";
        let key = "test_group::item1";
        let data = serde_json::json!({"key": "value"});
        let index_data = HashMap::from([(key.to_string(), data.clone())]);
        let file_path = dir.join(index_file_name(index));

        persist_index_to_disk(&dir, index, &index_data.clone())
            .await
            .unwrap();

        let config = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });
        let kv_store = BuiltinKvStore::new(Some(config));

        let loaded = kv_store.get(index.to_string(), key.to_string()).await;
        assert_eq!(loaded, Some(data.clone()));

        let updated = serde_json::json!({"key": "updated"});
        kv_store
            .set(index.to_string(), key.to_string(), updated.clone())
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let bytes = std::fs::read(&file_path).unwrap();
        let storage = rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes).unwrap();
        let on_disk_index_map: HashMap<String, Value> = serde_json::from_str(&storage.0).unwrap();
        let on_disk_value = on_disk_index_map.get(key).unwrap();
        assert_eq!(on_disk_value, &updated);

        kv_store.delete(index.to_string(), key.to_string()).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!file_path.exists());

        std::fs::remove_dir_all(&dir).unwrap();
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
    async fn test_builtin_kv_store_set_get_delete() {
        let kv_store = BuiltinKvStore::new(None);
        let data = serde_json::json!({"key": "value"});
        let index = "test";
        let key = "test_key";
        // Test set
        kv_store
            .set(index.to_string(), key.to_string(), data.clone())
            .await;

        // Test get
        let retrieved = kv_store
            .get(index.to_string(), key.to_string())
            .await
            .expect("Item should exist");
        assert_eq!(retrieved, data);

        // Test delete
        let deleted = kv_store
            .delete(index.to_string(), key.to_string())
            .await
            .expect("Item should exist for deletion");
        assert_eq!(deleted, data);

        // Ensure item is deleted
        let should_be_none = kv_store.get(index.to_string(), key.to_string()).await;
        assert!(should_be_none.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_basic_operations() {
        let kv_store = BuiltinKvStore::new(None);
        let index = "test";
        let key = "update:basic";

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store
            .set(index.to_string(), key.to_string(), initial.clone())
            .await;

        // Test Set operation
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![UpdateOp::Set {
                    path: FieldPath("name".to_string()),
                    value: Some(serde_json::json!("B")),
                }],
            )
            .await;

        assert_eq!(result.old_value, Some(initial));
        assert_eq!(result.new_value["name"], "B");
        assert_eq!(result.new_value["counter"], 0);

        // Test Increment operation
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![UpdateOp::Increment {
                    path: FieldPath("counter".to_string()),
                    by: 5,
                }],
            )
            .await;

        assert_eq!(result.new_value["counter"], 5);

        // Test Decrement operation
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![UpdateOp::Decrement {
                    path: FieldPath("counter".to_string()),
                    by: 2,
                }],
            )
            .await;

        assert_eq!(result.new_value["counter"], 3);

        // Test Remove operation
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![UpdateOp::Remove {
                    path: FieldPath("name".to_string()),
                }],
            )
            .await;

        assert!(result.new_value.get("name").is_none());
        assert_eq!(result.new_value["counter"], 3);

        // Test Merge operation
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![UpdateOp::Merge {
                    path: None,
                    value: serde_json::json!({"name": "C", "extra": "field"}),
                }],
            )
            .await;

        assert_eq!(result.new_value["name"], "C");
        assert_eq!(result.new_value["counter"], 3);
        assert_eq!(result.new_value["extra"], "field");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_multiple_ops_in_single_call() {
        let kv_store = BuiltinKvStore::new(None);
        let index = "test";
        let key = "update:multi_ops";

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store
            .set(index.to_string(), key.to_string(), initial.clone())
            .await;

        // Apply multiple operations in a single update call
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![
                    UpdateOp::Set {
                        path: FieldPath("name".to_string()),
                        value: Some(Value::String("Z".to_string())),
                    },
                    UpdateOp::Increment {
                        path: FieldPath("counter".to_string()),
                        by: 10,
                    },
                    UpdateOp::Set {
                        path: FieldPath("status".to_string()),
                        value: Some(Value::String("active".to_string())),
                    },
                ],
            )
            .await;

        assert_eq!(result.new_value["name"], "Z");
        assert_eq!(result.new_value["counter"], 10);
        assert_eq!(result.new_value["status"], "active");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_update_new_object_with_increment() {
        let kv_store = BuiltinKvStore::new(None);
        let index = "test";
        let key = "update:new_object_with_increment";
        let result = kv_store
            .update(
                index.to_string(),
                key.to_string(),
                vec![UpdateOp::Increment {
                    path: FieldPath("counter".to_string()),
                    by: 1,
                }],
            )
            .await;
        assert_eq!(result.new_value["counter"], 1);
        assert_eq!(result.old_value, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_two_threads_counter_and_name() {
        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let index = "test";
        let key = "update:two_threads";

        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store
            .set(index.to_string(), key.to_string(), initial.clone())
            .await;

        let names: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();

        let kv_counter = Arc::clone(&kv_store);
        let counter_handle = tokio::spawn(async move {
            let mut tasks = vec![];
            for _ in 0..500 {
                let task = kv_counter.update(
                    index.to_string(),
                    key.to_string(),
                    vec![UpdateOp::Increment {
                        path: FieldPath("counter".to_string()),
                        by: 2,
                    }],
                );
                tasks.push(task);
            }
            futures::future::join_all(tasks).await;
        });

        let kv_name = Arc::clone(&kv_store);
        let name_handle = tokio::spawn(async move {
            for name in names {
                kv_name
                    .update(
                        index.to_string(),
                        key.to_string(),
                        vec![UpdateOp::Set {
                            path: FieldPath("name".to_string()),
                            value: Some(Value::String(name)),
                        }],
                    )
                    .await;
            }
        });

        // Wait for both spawns to complete
        let (counter_result, name_result) = tokio::join!(counter_handle, name_handle);
        counter_result.expect("Counter spawn failed");
        name_result.expect("Name spawn failed");

        let final_value = kv_store
            .get(index.to_string(), key.to_string())
            .await
            .expect("Value should exist");

        assert_eq!(
            final_value["counter"], 1000,
            "Counter should be 1000 after 500 increments of 2"
        );

        let name = final_value["name"].as_str().unwrap_or("");
        assert_eq!("Z", name);
    }

    #[tokio::test()]
    async fn test_get_set() {
        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let index = "test";
        let key = "test_key";
        let data = serde_json::json!({"key": "value"});
        kv_store
            .set(index.to_string(), key.to_string(), data.clone())
            .await;
        let retrieved = kv_store.get(index.to_string(), key.to_string()).await;
        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_builtin_kv_store_update_concurrent_500_calls() {
        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let index = "test";
        let key = "update:concurrent_500";

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store
            .set(index.to_string(), key.to_string(), initial.clone())
            .await;

        let names = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

        // Create 500 concurrent update futures
        let mut tasks = vec![];
        for i in 0..500 {
            let kv_store = Arc::clone(&kv_store);
            let name = names[i % 10].to_string();

            let task = tokio::spawn(async move {
                kv_store
                    .update(
                        index.to_string(),
                        key.to_string(),
                        vec![
                            UpdateOp::Increment {
                                path: FieldPath("counter".to_string()),
                                by: 2,
                            },
                            UpdateOp::Set {
                                path: FieldPath("name".to_string()),
                                value: Some(Value::String(name)),
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
        let final_value = kv_store
            .get(index.to_string(), key.to_string())
            .await
            .expect("Value should exist");

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
        let kv_store = Arc::new(BuiltinKvStore::new(None));
        let index = "test";
        let key = "update:concurrent_join_all";

        // Set initial value
        let initial = serde_json::json!({"name": "A", "counter": 0});
        kv_store
            .set(index.to_string(), key.to_string(), initial.clone())
            .await;

        let names = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

        // Create 500 update futures without spawning
        let mut futures = vec![];
        for i in 0..500 {
            let kv_store = Arc::clone(&kv_store);
            let name = names[i % 10].to_string();

            let future = async move {
                kv_store
                    .update(
                        index.to_string(),
                        key.to_string(),
                        vec![
                            UpdateOp::Increment {
                                path: FieldPath("counter".to_string()),
                                by: 2,
                            },
                            UpdateOp::Set {
                                path: FieldPath("name".to_string()),
                                value: Some(Value::String(name)),
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
        let final_value = kv_store
            .get(index.to_string(), key.to_string())
            .await
            .expect("Value should exist");

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
