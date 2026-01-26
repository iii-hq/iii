use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
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

fn load_lists_from_dir(dir: &Path) -> HashMap<String, VecDeque<String>> {
    let list_file = dir.join("_lists.bin");
    if !list_file.exists() {
        return HashMap::new();
    }

    match std::fs::read(&list_file) {
        Ok(bytes) => {
            match rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes) {
                Ok(storage) => {
                    match serde_json::from_str(&storage.0) {
                        Ok(lists) => lists,
                        Err(err) => {
                            tracing::warn!(error = ?err, "failed to decode lists from disk");
                            HashMap::new()
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = ?err, "failed to parse lists file");
                    HashMap::new()
                }
            }
        }
        Err(err) => {
            tracing::warn!(error = ?err, "failed to read lists file");
            HashMap::new()
        }
    }
}

fn load_sorted_sets_from_dir(dir: &Path) -> HashMap<String, BTreeMap<i64, HashSet<String>>> {
    let sorted_sets_file = dir.join("_sorted_sets.bin");
    if !sorted_sets_file.exists() {
        return HashMap::new();
    }

    match std::fs::read(&sorted_sets_file) {
        Ok(bytes) => {
            match rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes) {
                Ok(storage) => {
                    match serde_json::from_str(&storage.0) {
                        Ok(sorted_sets) => sorted_sets,
                        Err(err) => {
                            tracing::warn!(error = ?err, "failed to decode sorted_sets from disk");
                            HashMap::new()
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = ?err, "failed to parse sorted_sets file");
                    HashMap::new()
                }
            }
        }
        Err(err) => {
            tracing::warn!(error = ?err, "failed to read sorted_sets file");
            HashMap::new()
        }
    }
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

async fn persist_lists_to_disk(
    dir: &Path,
    lists: &HashMap<String, VecDeque<String>>,
) -> anyhow::Result<()> {
    if let Err(err) = tokio::fs::create_dir_all(dir).await {
        tracing::error!(error = ?err, path = %dir.display(), "failed to create storage directory");
        return Err(err.into());
    }

    let file_name = "_lists.bin";
    let path = dir.join(file_name);
    let temp_path = dir.join(format!("{}.tmp", file_name));
    let json = serde_json::to_string(lists)?;
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(json))?;

    tokio::fs::write(&temp_path, bytes).await?;
    tokio::fs::rename(&temp_path, &path).await?;

    Ok(())
}

async fn persist_sorted_sets_to_disk(
    dir: &Path,
    sorted_sets: &HashMap<String, BTreeMap<i64, HashSet<String>>>,
) -> anyhow::Result<()> {
    if let Err(err) = tokio::fs::create_dir_all(dir).await {
        tracing::error!(error = ?err, path = %dir.display(), "failed to create storage directory");
        return Err(err.into());
    }

    let file_name = "_sorted_sets.bin";
    let path = dir.join(file_name);
    let temp_path = dir.join(format!("{}.tmp", file_name));
    let json = serde_json::to_string(sorted_sets)?;
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(json))?;

    tokio::fs::write(&temp_path, bytes).await?;
    tokio::fs::rename(&temp_path, &path).await?;

    Ok(())
}

pub struct BuiltinKvStore {
    store: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
    lists: Arc<RwLock<HashMap<String, VecDeque<String>>>>,
    sorted_sets: Arc<RwLock<HashMap<String, BTreeMap<i64, HashSet<String>>>>>,
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
        let lists_from_disk = match &file_store_dir {
            Some(dir) => load_lists_from_dir(dir),
            None => HashMap::new(),
        };
        let sorted_sets_from_disk = match &file_store_dir {
            Some(dir) => load_sorted_sets_from_dir(dir),
            None => HashMap::new(),
        };

        let store = Arc::new(RwLock::new(data_from_disk));
        let lists = Arc::new(RwLock::new(lists_from_disk));
        let sorted_sets = Arc::new(RwLock::new(sorted_sets_from_disk));
        let dirty = Arc::new(RwLock::new(HashMap::new()));
        let handler = file_store_dir.clone().map(|dir| {
            let store = Arc::clone(&store);
            let lists = Arc::clone(&lists);
            let sorted_sets = Arc::clone(&sorted_sets);
            let dirty = Arc::clone(&dirty);
            tokio::spawn(async move {
                Self::save_loop(store, lists, sorted_sets, dirty, interval, dir).await;
            })
        });

        Self {
            store,
            lists,
            sorted_sets,
            file_store_dir,
            dirty,
            handler,
        }
    }

    async fn save_loop(
        store: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
        lists: Arc<RwLock<HashMap<String, VecDeque<String>>>>,
        sorted_sets: Arc<RwLock<HashMap<String, BTreeMap<i64, HashSet<String>>>>>,
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
                if index == "_lists" {
                    let lists_snapshot = {
                        let lists = lists.read().await;
                        lists.clone()
                    };
                    if let Err(err) = persist_lists_to_disk(&dir, &lists_snapshot).await {
                        tracing::error!(error = ?err, "failed to persist lists");
                        let mut dirty = dirty.write().await;
                        dirty.insert("_lists".to_string(), DirtyOp::Upsert);
                    }
                } else if index == "_sorted_sets" {
                    let sorted_sets_snapshot = {
                        let sorted_sets = sorted_sets.read().await;
                        sorted_sets.clone()
                    };
                    if let Err(err) = persist_sorted_sets_to_disk(&dir, &sorted_sets_snapshot).await {
                        tracing::error!(error = ?err, "failed to persist sorted_sets");
                        let mut dirty = dirty.write().await;
                        dirty.insert("_sorted_sets".to_string(), DirtyOp::Upsert);
                    }
                } else {
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
    }

    pub async fn set(&self, index: String, key: String, data: Value) -> SetResult {
        let mut store = self.store.write().await;
        let index_map = store.get_mut(&index);

        if let Some(index_map) = index_map {
            let old_value = index_map.get(&key).cloned();
            index_map.insert(key.clone(), data.clone());

            if self.file_store_dir.is_some() {
                let mut dirty = self.dirty.write().await;
                dirty.insert(index.clone(), DirtyOp::Upsert);
            }

            return SetResult {
                old_value,
                new_value: data,
            };
        }

        let mut index_map = HashMap::new();
        index_map.insert(key, data.clone());
        store.insert(index.clone(), index_map);

        if self.file_store_dir.is_some() {
            let mut dirty = self.dirty.write().await;
            dirty.insert(index, DirtyOp::Upsert);
        }

        SetResult {
            old_value: None,
            new_value: data,
        }
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
        let mut store = self.store.write().await;
        let index_map = store.get_mut(&index);

        if let Some(index_map) = index_map {
            let removed = index_map.remove(&key);

            if removed.is_some() && self.file_store_dir.is_some() {
                let mut dirty = self.dirty.write().await;
                dirty.insert(index, DirtyOp::Delete);
            }

            return if removed.is_some() { removed } else { None };
        }

        None
    }

    pub async fn update(
        &self,
        index: String,
        key: String,
        ops: Vec<UpdateOp>,
    ) -> Option<UpdateResult> {
        let mut store = self.store.write().await;
        let index_map = store.get_mut(&index)?;

        if let Some(existing_value) = index_map.get_mut(&key) {
            let old_value = existing_value.clone();
            let mut updated_value = existing_value.clone();

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

            // Write the updated value back to the store
            *existing_value = updated_value.clone();

            if self.file_store_dir.is_some() {
                let mut dirty = self.dirty.write().await;
                dirty.insert(index.clone(), DirtyOp::Upsert);
            }

            drop(store);

            Some(UpdateResult {
                old_value: Some(old_value),
                new_value: updated_value,
            })
        } else {
            None
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

    pub async fn lpush(&self, key: &str, value: String) {
        let mut lists = self.lists.write().await;
        lists.entry(key.to_string())
            .or_insert_with(VecDeque::new)
            .push_front(value);

        if self.file_store_dir.is_some() {
            drop(lists);
            let mut dirty = self.dirty.write().await;
            dirty.insert("_lists".to_string(), DirtyOp::Upsert);
        }
    }

    pub async fn rpop(&self, key: &str) -> Option<String> {
        let mut lists = self.lists.write().await;
        let list = lists.get_mut(key)?;
        let result = list.pop_back();
        let should_remove = list.is_empty();
        if should_remove {
            lists.remove(key);
        }

        if self.file_store_dir.is_some() {
            drop(lists);
            let mut dirty = self.dirty.write().await;
            dirty.insert("_lists".to_string(), DirtyOp::Upsert);
        }

        result
    }

    pub async fn lrem(&self, key: &str, count: i32, value: &str) -> usize {
        let mut lists = self.lists.write().await;
        let Some(list) = lists.get_mut(key) else {
            return 0;
        };

        let mut removed = 0;
        if count > 0 {
            let count = count as usize;
            list.retain(|item| {
                if removed < count && item == value {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        } else if count < 0 {
            let count = count.unsigned_abs() as usize;
            let original_len = list.len();
            let mut indices_to_remove = Vec::new();
            
            for i in (0..original_len).rev() {
                if removed >= count {
                    break;
                }
                if list[i] == value {
                    indices_to_remove.push(i);
                    removed += 1;
                }
            }
            
            indices_to_remove.sort_unstable();
            for &idx in indices_to_remove.iter().rev() {
                list.remove(idx);
            }
        } else {
            let original_len = list.len();
            list.retain(|item| item != value);
            removed = original_len - list.len();
        }

        if list.is_empty() {
            lists.remove(key);
        }

        if self.file_store_dir.is_some() {
            drop(lists);
            let mut dirty = self.dirty.write().await;
            dirty.insert("_lists".to_string(), DirtyOp::Upsert);
        }

        removed
    }

    pub async fn llen(&self, key: &str) -> usize {
        let lists = self.lists.read().await;
        lists.get(key).map_or(0, |list| list.len())
    }

    pub async fn zadd(&self, key: &str, score: i64, member: String) {
        let mut sorted_sets = self.sorted_sets.write().await;
        let set = sorted_sets.entry(key.to_string())
            .or_insert_with(BTreeMap::new);
        
        for (_, members) in set.iter_mut() {
            members.remove(&member);
        }
        
        set.retain(|_, members| !members.is_empty());
        
        set.entry(score)
            .or_insert_with(HashSet::new)
            .insert(member);

        if self.file_store_dir.is_some() {
            drop(sorted_sets);
            let mut dirty = self.dirty.write().await;
            dirty.insert("_sorted_sets".to_string(), DirtyOp::Upsert);
        }
    }

    pub async fn zrem(&self, key: &str, member: &str) -> bool {
        let mut sorted_sets = self.sorted_sets.write().await;
        let Some(set) = sorted_sets.get_mut(key) else {
            return false;
        };

        let mut found = false;
        set.retain(|_, members| {
            if members.remove(member) {
                found = true;
            }
            !members.is_empty()
        });

        if set.is_empty() {
            sorted_sets.remove(key);
        }

        if found && self.file_store_dir.is_some() {
            drop(sorted_sets);
            let mut dirty = self.dirty.write().await;
            dirty.insert("_sorted_sets".to_string(), DirtyOp::Upsert);
        }

        found
    }

    pub async fn zrangebyscore(&self, key: &str, min: i64, max: i64) -> Vec<String> {
        let sorted_sets = self.sorted_sets.read().await;
        let Some(set) = sorted_sets.get(key) else {
            return vec![];
        };

        let mut result = Vec::new();
        for (_score, members) in set.range(min..=max) {
            for member in members {
                result.push(member.clone());
            }
        }
        result
    }

    pub async fn has_queue_state_with_prefix(&self, prefix: &str) -> bool {
        let lists = self.lists.read().await;
        let sorted_sets = self.sorted_sets.read().await;
        
        let has_lists = lists.keys().any(|k| k.starts_with(prefix));
        let has_sorted_sets = sorted_sets.keys().any(|k| k.starts_with(prefix));
        
        has_lists || has_sorted_sets
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
                index.to_string(),
                key.to_string(),
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
                index.to_string(),
                key.to_string(),
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
                index.to_string(),
                key.to_string(),
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
                index.to_string(),
                key.to_string(),
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
                            value: Value::String(name),
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

    #[tokio::test]
    async fn test_list_operations() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "test_list";

        kv_store.lpush(key, "value1".to_string()).await;
        kv_store.lpush(key, "value2".to_string()).await;
        kv_store.lpush(key, "value3".to_string()).await;

        assert_eq!(kv_store.llen(key).await, 3);

        let popped = kv_store.rpop(key).await;
        assert_eq!(popped, Some("value1".to_string()));
        assert_eq!(kv_store.llen(key).await, 2);

        let removed = kv_store.lrem(key, 1, "value2").await;
        assert_eq!(removed, 1);
        assert_eq!(kv_store.llen(key).await, 1);

        let last = kv_store.rpop(key).await;
        assert_eq!(last, Some("value3".to_string()));
        assert_eq!(kv_store.llen(key).await, 0);

        let empty = kv_store.rpop(key).await;
        assert_eq!(empty, None);
    }

    #[tokio::test]
    async fn test_sorted_set_operations() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "test_sorted_set";

        kv_store.zadd(key, 100, "member1".to_string()).await;
        kv_store.zadd(key, 200, "member2".to_string()).await;
        kv_store.zadd(key, 300, "member3".to_string()).await;
        kv_store.zadd(key, 150, "member4".to_string()).await;

        let range = kv_store.zrangebyscore(key, 100, 200).await;
        assert_eq!(range.len(), 3);
        assert!(range.contains(&"member1".to_string()));
        assert!(range.contains(&"member2".to_string()));
        assert!(range.contains(&"member4".to_string()));

        let removed = kv_store.zrem(key, "member2").await;
        assert!(removed);

        let range_after = kv_store.zrangebyscore(key, 100, 300).await;
        assert_eq!(range_after.len(), 3);
        assert!(!range_after.contains(&"member2".to_string()));

        kv_store.zadd(key, 250, "member1".to_string()).await;
        let updated_range = kv_store.zrangebyscore(key, 100, 200).await;
        assert!(!updated_range.contains(&"member1".to_string()));

        let high_range = kv_store.zrangebyscore(key, 200, 300).await;
        assert!(high_range.contains(&"member1".to_string()));
    }

    #[tokio::test]
    async fn test_zadd_prunes_empty_buckets() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "test_zadd_prune";

        kv_store.zadd(key, 100, "member1".to_string()).await;
        kv_store.zadd(key, 200, "member2".to_string()).await;
        kv_store.zadd(key, 300, "member3".to_string()).await;

        kv_store.zadd(key, 400, "member1".to_string()).await;

        let sorted_sets = kv_store.sorted_sets.read().await;
        let set = sorted_sets.get(key).unwrap();
        
        assert!(set.get(&100).is_none());
        assert!(set.get(&400).is_some());
        assert_eq!(set.get(&400).unwrap().len(), 1);
        assert!(set.get(&400).unwrap().contains("member1"));
        
        assert_eq!(set.len(), 3);
    }

    #[tokio::test]
    async fn test_lists_persistence() {
        let dir = temp_store_dir();
        let config = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });

        let kv_store = BuiltinKvStore::new(Some(config.clone()));
        
        kv_store.lpush("queue:test:waiting", "job1".to_string()).await;
        kv_store.lpush("queue:test:waiting", "job2".to_string()).await;
        kv_store.lpush("queue:test:active", "job3".to_string()).await;

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let lists_file = dir.join("_lists.bin");
        assert!(lists_file.exists(), "Lists file should be persisted");

        let new_kv_store = BuiltinKvStore::new(Some(config));

        assert_eq!(new_kv_store.llen("queue:test:waiting").await, 2);
        assert_eq!(new_kv_store.llen("queue:test:active").await, 1);

        let popped = new_kv_store.rpop("queue:test:waiting").await;
        assert_eq!(popped, Some("job1".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_sorted_sets_persistence() {
        let dir = temp_store_dir();
        let config = serde_json::json!({
            "store_method": "file_based",
            "file_path": dir.to_string_lossy(),
            "save_interval_ms": 5
        });

        let kv_store = BuiltinKvStore::new(Some(config.clone()));
        
        kv_store.zadd("queue:test:delayed", 1000, "job1".to_string()).await;
        kv_store.zadd("queue:test:delayed", 2000, "job2".to_string()).await;
        kv_store.zadd("queue:test:delayed", 1500, "job3".to_string()).await;

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let sorted_sets_file = dir.join("_sorted_sets.bin");
        assert!(sorted_sets_file.exists(), "Sorted sets file should be persisted");

        let new_kv_store = BuiltinKvStore::new(Some(config));

        let range = new_kv_store.zrangebyscore("queue:test:delayed", 0, 3000).await;
        assert_eq!(range.len(), 3);
        assert!(range.contains(&"job1".to_string()));
        assert!(range.contains(&"job2".to_string()));
        assert!(range.contains(&"job3".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_lrem_with_count_zero() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "test_lrem_zero";

        kv_store.lpush(key, "a".to_string()).await;
        kv_store.lpush(key, "b".to_string()).await;
        kv_store.lpush(key, "a".to_string()).await;
        kv_store.lpush(key, "c".to_string()).await;
        kv_store.lpush(key, "a".to_string()).await;

        let removed = kv_store.lrem(key, 0, "a").await;
        assert_eq!(removed, 3);
        assert_eq!(kv_store.llen(key).await, 2);

        let next = kv_store.rpop(key).await;
        assert_eq!(next, Some("b".to_string()));
        let next = kv_store.rpop(key).await;
        assert_eq!(next, Some("c".to_string()));
    }

    #[tokio::test]
    async fn test_lrem_with_negative_count() {
        let kv_store = BuiltinKvStore::new(None);
        let key = "test_lrem_negative";

        kv_store.lpush(key, "d".to_string()).await;
        kv_store.lpush(key, "a".to_string()).await;
        kv_store.lpush(key, "b".to_string()).await;
        kv_store.lpush(key, "a".to_string()).await;
        kv_store.lpush(key, "c".to_string()).await;
        kv_store.lpush(key, "a".to_string()).await;

        let removed = kv_store.lrem(key, -2, "a").await;
        assert_eq!(removed, 2);
        assert_eq!(kv_store.llen(key).await, 4);

        let mut items: Vec<String> = Vec::new();
        while let Some(item) = kv_store.rpop(key).await {
            items.push(item);
        }

        assert_eq!(items, vec!["d".to_string(), "b".to_string(), "c".to_string(), "a".to_string()]);
    }
}
