use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde_json::Value;
use tokio::{sync::RwLock, task::JoinHandle};

use super::kv::BuiltinKvStore;

const LISTS_FILE_NAME: &str = "_queue_lists.bin";
const SORTED_SETS_FILE_NAME: &str = "_queue_sorted_sets.bin";

#[derive(Archive, RkyvSerialize, RkyvDeserialize)]
struct KeyStorage(String);

#[derive(Clone, Copy, Debug)]
enum DirtyOp {
    Upsert,
}

fn load_lists_from_dir(dir: &Path) -> HashMap<String, VecDeque<String>> {
    let list_file = dir.join(LISTS_FILE_NAME);
    if !list_file.exists() {
        return HashMap::new();
    }

    match std::fs::read(&list_file) {
        Ok(bytes) => match rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes) {
            Ok(storage) => match serde_json::from_str(&storage.0) {
                Ok(lists) => lists,
                Err(err) => {
                    tracing::warn!(error = ?err, "failed to decode queue lists from disk");
                    HashMap::new()
                }
            },
            Err(err) => {
                tracing::warn!(error = ?err, "failed to parse queue lists file");
                HashMap::new()
            }
        },
        Err(err) => {
            tracing::warn!(error = ?err, "failed to read queue lists file");
            HashMap::new()
        }
    }
}

fn load_sorted_sets_from_dir(dir: &Path) -> HashMap<String, BTreeMap<i64, HashSet<String>>> {
    let sorted_sets_file = dir.join(SORTED_SETS_FILE_NAME);
    if !sorted_sets_file.exists() {
        return HashMap::new();
    }

    match std::fs::read(&sorted_sets_file) {
        Ok(bytes) => match rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes) {
            Ok(storage) => match serde_json::from_str(&storage.0) {
                Ok(sorted_sets) => sorted_sets,
                Err(err) => {
                    tracing::warn!(error = ?err, "failed to decode queue sorted_sets from disk");
                    HashMap::new()
                }
            },
            Err(err) => {
                tracing::warn!(error = ?err, "failed to parse queue sorted_sets file");
                HashMap::new()
            }
        },
        Err(err) => {
            tracing::warn!(error = ?err, "failed to read queue sorted_sets file");
            HashMap::new()
        }
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

    let path = dir.join(LISTS_FILE_NAME);
    let temp_path = dir.join(format!("{}.tmp", LISTS_FILE_NAME));
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

    let path = dir.join(SORTED_SETS_FILE_NAME);
    let temp_path = dir.join(format!("{}.tmp", SORTED_SETS_FILE_NAME));
    let json = serde_json::to_string(sorted_sets)?;
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(json))?;

    tokio::fs::write(&temp_path, bytes).await?;
    tokio::fs::rename(&temp_path, &path).await?;

    Ok(())
}

pub struct QueueKvStore {
    kv: Arc<BuiltinKvStore>,
    lists: Arc<RwLock<HashMap<String, VecDeque<String>>>>,
    sorted_sets: Arc<RwLock<HashMap<String, BTreeMap<i64, HashSet<String>>>>>,
    file_store_dir: Option<PathBuf>,
    dirty: Arc<RwLock<HashMap<String, DirtyOp>>>,
    #[allow(dead_code)]
    handler: Option<JoinHandle<()>>,
}

impl QueueKvStore {
    pub fn new(kv: Arc<BuiltinKvStore>, config: Option<Value>) -> Self {
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

        let lists_from_disk = match &file_store_dir {
            Some(dir) => load_lists_from_dir(dir),
            None => HashMap::new(),
        };
        let sorted_sets_from_disk = match &file_store_dir {
            Some(dir) => load_sorted_sets_from_dir(dir),
            None => HashMap::new(),
        };

        let lists = Arc::new(RwLock::new(lists_from_disk));
        let sorted_sets = Arc::new(RwLock::new(sorted_sets_from_disk));
        let dirty = Arc::new(RwLock::new(HashMap::new()));
        let handler = file_store_dir.clone().map(|dir| {
            let lists = Arc::clone(&lists);
            let sorted_sets = Arc::clone(&sorted_sets);
            let dirty = Arc::clone(&dirty);
            tokio::spawn(async move {
                Self::save_loop(lists, sorted_sets, dirty, interval, dir).await;
            })
        });

        Self {
            kv,
            lists,
            sorted_sets,
            file_store_dir,
            dirty,
            handler,
        }
    }

    async fn save_loop(
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

            for (index, _op) in batch {
                if index == LISTS_FILE_NAME {
                    let lists_snapshot = {
                        let lists = lists.read().await;
                        lists.clone()
                    };
                    if let Err(err) = persist_lists_to_disk(&dir, &lists_snapshot).await {
                        tracing::error!(error = ?err, "failed to persist queue lists");
                        let mut dirty = dirty.write().await;
                        dirty.insert(LISTS_FILE_NAME.to_string(), DirtyOp::Upsert);
                    }
                } else if index == SORTED_SETS_FILE_NAME {
                    let sorted_sets_snapshot = {
                        let sorted_sets = sorted_sets.read().await;
                        sorted_sets.clone()
                    };
                    if let Err(err) = persist_sorted_sets_to_disk(&dir, &sorted_sets_snapshot).await
                    {
                        tracing::error!(error = ?err, "failed to persist queue sorted_sets");
                        let mut dirty = dirty.write().await;
                        dirty.insert(SORTED_SETS_FILE_NAME.to_string(), DirtyOp::Upsert);
                    }
                }
            }
        }
    }

    pub async fn set_job(&self, key: &str, value: Value) {
        self.kv
            .set(key.to_string(), String::new(), value)
            .await;
    }

    pub async fn get_job(&self, key: &str) -> Option<Value> {
        self.kv.get(key.to_string(), String::new()).await
    }

    pub async fn delete_job(&self, key: &str) {
        self.kv.delete(key.to_string(), String::new()).await;
    }

    pub async fn list_job_keys(&self, prefix: &str) -> Vec<String> {
        self.kv.list_keys_with_prefix(prefix.to_string()).await
    }

    pub async fn lpush(&self, key: &str, value: String) {
        let mut lists = self.lists.write().await;
        lists
            .entry(key.to_string())
            .or_insert_with(VecDeque::new)
            .push_front(value);

        if self.file_store_dir.is_some() {
            drop(lists);
            let mut dirty = self.dirty.write().await;
            dirty.insert(LISTS_FILE_NAME.to_string(), DirtyOp::Upsert);
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
            dirty.insert(LISTS_FILE_NAME.to_string(), DirtyOp::Upsert);
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
            dirty.insert(LISTS_FILE_NAME.to_string(), DirtyOp::Upsert);
        }

        removed
    }

    pub async fn llen(&self, key: &str) -> usize {
        let lists = self.lists.read().await;
        lists.get(key).map_or(0, |list| list.len())
    }

    pub async fn zadd(&self, key: &str, score: i64, member: String) {
        let mut sorted_sets = self.sorted_sets.write().await;
        let set = sorted_sets
            .entry(key.to_string())
            .or_insert_with(BTreeMap::new);

        for (_, members) in set.iter_mut() {
            members.remove(&member);
        }

        set.retain(|_, members| !members.is_empty());

        set.entry(score).or_insert_with(HashSet::new).insert(member);

        if self.file_store_dir.is_some() {
            drop(sorted_sets);
            let mut dirty = self.dirty.write().await;
            dirty.insert(SORTED_SETS_FILE_NAME.to_string(), DirtyOp::Upsert);
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
            dirty.insert(SORTED_SETS_FILE_NAME.to_string(), DirtyOp::Upsert);
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

    pub async fn has_queue_state(&self, prefix: &str) -> bool {
        let lists = self.lists.read().await;
        let sorted_sets = self.sorted_sets.read().await;

        let has_lists = lists.keys().any(|k| k.starts_with(prefix));
        let has_sorted_sets = sorted_sets.keys().any(|k| k.starts_with(prefix));

        has_lists || has_sorted_sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("queue_kv_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_queue_kv(config: Option<Value>) -> Arc<QueueKvStore> {
        let base_kv = Arc::new(BuiltinKvStore::new(config.clone()));
        Arc::new(QueueKvStore::new(base_kv, config))
    }

    #[tokio::test]
    async fn test_list_operations() {
        let kv_store = make_queue_kv(None);
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
        let kv_store = make_queue_kv(None);
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
        let kv_store = make_queue_kv(None);
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

        let kv_store = make_queue_kv(Some(config.clone()));

        kv_store
            .lpush("queue:test:waiting", "job1".to_string())
            .await;
        kv_store
            .lpush("queue:test:waiting", "job2".to_string())
            .await;
        kv_store
            .lpush("queue:test:active", "job3".to_string())
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let lists_file = dir.join(LISTS_FILE_NAME);
        assert!(lists_file.exists());

        let new_kv_store = make_queue_kv(Some(config));

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

        let kv_store = make_queue_kv(Some(config.clone()));

        kv_store
            .zadd("queue:test:delayed", 1000, "job1".to_string())
            .await;
        kv_store
            .zadd("queue:test:delayed", 2000, "job2".to_string())
            .await;
        kv_store
            .zadd("queue:test:delayed", 1500, "job3".to_string())
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let sorted_sets_file = dir.join(SORTED_SETS_FILE_NAME);
        assert!(sorted_sets_file.exists());

        let new_kv_store = make_queue_kv(Some(config));

        let range = new_kv_store
            .zrangebyscore("queue:test:delayed", 0, 3000)
            .await;
        assert_eq!(range.len(), 3);
        assert!(range.contains(&"job1".to_string()));
        assert!(range.contains(&"job2".to_string()));
        assert!(range.contains(&"job3".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_lrem_with_count_zero() {
        let kv_store = make_queue_kv(None);
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
        let kv_store = make_queue_kv(None);
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

        assert_eq!(
            items,
            vec![
                "d".to_string(),
                "b".to_string(),
                "c".to_string(),
                "a".to_string()
            ]
        );
    }
}
