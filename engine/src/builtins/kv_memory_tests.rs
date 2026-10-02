// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Regression coverage for shared snapshots and cancellation-safe persistence.
use super::*;

fn in_memory_store() -> BuiltinKvStore {
    BuiltinKvStore::new(None)
}
use std::time::Duration;

fn directory() -> PathBuf {
    std::env::temp_dir().join(format!("builtin-kv-memory-{}", uuid::Uuid::new_v4()))
}

// Explicit flush tests own the lifecycle; no immediate background tick races them.
fn manual_store(dir: &Path) -> BuiltinKvStore {
    BuiltinKvStore {
        store: Arc::new(RwLock::new(HashMap::new())),
        file_store_dir: Some(dir.to_path_buf()),
        dirty: Arc::new(RwLock::new(HashMap::new())),
        save_loop_stop: Arc::new(std::sync::Mutex::new(None)),
        flush_lock: Arc::new(tokio::sync::Mutex::new(())),
        default_interval: 60_000,
    }
}

async fn flush(store: &BuiltinKvStore) -> anyhow::Result<()> {
    match &store.file_store_dir {
        Some(dir) => {
            BuiltinKvStore::flush_dirty(&store.store, &store.dirty, &store.flush_lock, dir).await
        }
        None => Ok(()),
    }
}

fn read_legacy(dir: &Path, index: &str) -> IndexMap<String, Value> {
    let bytes = std::fs::read(dir.join(index_file_name(index))).unwrap();
    let value = rkyv::from_bytes::<KeyStorage, rkyv::rancor::Error>(&bytes).unwrap();
    serde_json::from_str(&value.0).unwrap()
}

#[tokio::test]
async fn shared_snapshot_survives_replacement_and_delete() {
    let store = in_memory_store();
    let initial = serde_json::json!({"payload":"x".repeat(65536)});
    store.set("s".into(), "a".into(), initial.clone()).await;
    store.set("s".into(), "b".into(), Value::Bool(true)).await;
    let snapshot = store.store.read().await["s"].clone();
    {
        let live = store.store.read().await;
        assert!(Arc::ptr_eq(&snapshot["a"], &live["s"]["a"]));
    }
    let result = store.set("s".into(), "a".into(), Value::Null).await;
    assert_eq!(result.old_value, Some(initial.clone()));
    assert_eq!(result.new_value, Value::Null);
    store.delete("s".into(), "a".into()).await;
    assert_eq!(snapshot["a"].as_ref(), &initial);
    assert_eq!(store.list("s".into()).await, vec![Value::Bool(true)]);
    let dir = directory();
    persist_index_to_disk(&dir, "snapshot", &snapshot).unwrap();
    assert_eq!(read_legacy(&dir, "snapshot")["a"], initial);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn archive_is_byte_compatible_with_legacy_and_load_validates() {
    let dir = directory();
    let fixtures = [
        IndexMap::<String, Value>::new(),
        IndexMap::from([
            (
                "first".into(),
                serde_json::json!({"s":"中文\n\\\"", "n":18446744073709551615u64}),
            ),
            (
                "second".into(),
                serde_json::json!([null,true,-7,1.25,{"payload":"x".repeat(8192)}]),
            ),
        ]),
    ];
    for (i, data) in fixtures.into_iter().enumerate() {
        let legacy = rkyv::to_bytes::<rkyv::rancor::Error>(&KeyStorage(
            serde_json::to_string(&data).unwrap(),
        ))
        .unwrap();
        let shared: Scope = data
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
            .collect();
        let index = format!("scope:{i}/中文");
        persist_index_to_disk(&dir, &index, &shared).unwrap();
        assert_eq!(
            std::fs::read(dir.join(index_file_name(&index))).unwrap(),
            legacy.as_slice()
        );
        assert_eq!(read_legacy(&dir, &index), data);
        assert_eq!(load_store_from_dir(&dir)[&index], shared);
        std::fs::write(dir.join(index_file_name(&format!("legacy-{i}"))), legacy).unwrap();
        assert_eq!(load_store_from_dir(&dir)[&format!("legacy-{i}")], shared);
    }
    std::fs::write(dir.join("broken.bin"), b"broken").unwrap();
    assert!(!load_store_from_dir(&dir).contains_key("broken"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn moved_array_preserves_json_contract() {
    for items in [
        vec![],
        vec![Value::Null, serde_json::json!({"a":[1,true,"text"]})],
    ] {
        let old = serde_json::to_value(&items).unwrap();
        let moved = Value::Array(items);
        assert_eq!(old, moved);
        assert_eq!(
            serde_json::to_vec(&old).unwrap(),
            serde_json::to_vec(&moved).unwrap()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_flush_keeps_writer_exclusion_until_disk_work_finishes() {
    let dir = directory();
    let store = Arc::new(manual_store(&dir));
    store
        .set(
            "s".into(),
            "key".into(),
            serde_json::json!({"generation":0}),
        )
        .await;
    // Hold data access so the blocking flush is deterministically in flight.
    let mut blocked = store.store.write().await;
    let first = {
        let store = store.clone();
        tokio::spawn(async move { flush(&store).await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while !store.dirty.read().await.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(
        store.flush_lock.try_lock().is_err(),
        "aborted caller must not release writer exclusion"
    );
    let mut second = {
        let store = store.clone();
        tokio::spawn(async move { flush(&store).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut second)
            .await
            .is_err()
    );
    blocked
        .get_mut("s")
        .unwrap()
        .insert("key".into(), Arc::new(serde_json::json!({"generation":1})));
    store
        .dirty
        .write()
        .await
        .insert("s".into(), DirtyOp::Upsert);
    // A hot-reconfigured loop shares the same exclusion, not just explicit flush.
    store.reconfigure(&serde_json::json!({"save_interval_ms":100}));
    drop(blocked);
    tokio::time::timeout(Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    flush(&store).await.unwrap();
    assert_eq!(
        read_legacy(&dir, "s")["key"],
        serde_json::json!({"generation":1})
    );
    if let Some(stop) = store.save_loop_stop.lock().unwrap().take() {
        let _ = stop.send(true);
    }
    let _guard = store.flush_lock.lock().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_writes_reconfigure_and_final_flush_keep_latest_values() {
    let dir = directory();
    let store = Arc::new(manual_store(&dir));
    let mut tasks = tokio::task::JoinSet::new();
    for key in 0..4 {
        let store = store.clone();
        tasks.spawn(async move {
            for generation in 0..30 {
                store
                    .set(
                        "s".into(),
                        key.to_string(),
                        serde_json::json!({"generation":generation}),
                    )
                    .await;
                if generation % 7 == 0 {
                    flush(&store).await.unwrap();
                }
            }
        });
    }
    store.reconfigure(&serde_json::json!({"save_interval_ms":100}));
    while let Some(task) = tasks.join_next().await {
        task.unwrap();
    }
    flush(&store).await.unwrap();
    let values = read_legacy(&dir, "s");
    assert_eq!(values.len(), 4);
    assert!(values.values().all(|v| v["generation"] == 29));
    if let Some(stop) = store.save_loop_stop.lock().unwrap().take() {
        let _ = stop.send(true);
    }
    let _guard = store.flush_lock.lock().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_scope_does_not_prevent_other_snapshots_and_can_retry() {
    let dir = directory();
    let store = manual_store(&dir);
    std::fs::create_dir_all(dir.join(index_file_name("blocked"))).unwrap();
    store
        .set("blocked".into(), "k".into(), Value::Bool(true))
        .await;
    store
        .set("healthy".into(), "k".into(), Value::Bool(false))
        .await;
    assert!(flush(&store).await.is_err());
    assert_eq!(read_legacy(&dir, "healthy")["k"], Value::Bool(false));
    assert!(store.dirty.read().await.contains_key("blocked"));
    std::fs::remove_dir(dir.join(index_file_name("blocked"))).unwrap();
    flush(&store).await.unwrap();
    assert_eq!(read_legacy(&dir, "blocked")["k"], Value::Bool(true));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_snapshot_requeue_preserves_newer_mutation_intent() {
    let dirty = Arc::new(RwLock::new(HashMap::from([(
        "s".to_string(),
        DirtyOp::Delete,
    )])));
    requeue(&dirty, "s".into(), DirtyOp::Upsert);
    assert!(matches!(dirty.blocking_read()["s"], DirtyOp::Delete));
    dirty.blocking_write().insert("s".into(), DirtyOp::Upsert);
    requeue(&dirty, "s".into(), DirtyOp::Delete);
    assert!(matches!(dirty.blocking_read()["s"], DirtyOp::Upsert));
}

#[tokio::test]
async fn replacing_without_snapshot_moves_previous_value_to_result() {
    let store = in_memory_store();
    store
        .set("s".into(), "key".into(), Value::String("a".repeat(65536)))
        .await;
    let pointer = {
        let map = store.store.read().await;
        map["s"]["key"].as_str().unwrap().as_ptr() as usize
    };
    let result = store.set("s".into(), "key".into(), Value::Null).await;
    assert_eq!(
        result.old_value.unwrap().as_str().unwrap().as_ptr() as usize,
        pointer
    );
}

#[tokio::test]
async fn deleted_records_are_released_when_last_snapshot_is_dropped() {
    let store = in_memory_store();
    store
        .set("s".into(), "key".into(), Value::String("x".repeat(65536)))
        .await;
    let snapshot = store.store.read().await["s"].clone();
    let weak = Arc::downgrade(&snapshot["key"]);
    drop(store.delete("s".into(), "key".into()).await);
    assert_eq!(weak.strong_count(), 1);
    assert_eq!(snapshot["key"].as_str().unwrap().len(), 65536);
    drop(snapshot);
    assert!(weak.upgrade().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_and_concurrent_writes_preserve_membership_order_and_record_consistency() {
    let store = Arc::new(in_memory_store());
    for id in 0..64 {
        store
            .set(
                "s".into(),
                format!("k{id:03}"),
                serde_json::json!({"id":id,"generation":0,"payload":"0".repeat(4096)}),
            )
            .await;
    }
    let gate = Arc::new(tokio::sync::Barrier::new(5));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let store = store.clone();
        let gate = gate.clone();
        tasks.spawn(async move {
            gate.wait().await;
            for _ in 0..20 {
                let values = store.list("s".into()).await;
                assert_eq!(values.len(), 64);
                for (id, value) in values.iter().enumerate() {
                    assert_eq!(value["id"], id);
                    let generation = value["generation"].as_u64().unwrap() as u8;
                    assert!(
                        value["payload"]
                            .as_str()
                            .unwrap()
                            .bytes()
                            .all(|b| b == b'0' + generation)
                    );
                }
                tokio::task::yield_now().await;
            }
        });
    }
    gate.wait().await;
    for generation in 1..=3 {
        for id in 0..64 {
            store
                .set(
                    "s".into(),
                    format!("k{id:03}"),
                    serde_json::json!({"id":id,"generation":generation,
                "payload":((b'0'+generation as u8) as char).to_string().repeat(4096)}),
                )
                .await;
            tokio::task::yield_now().await;
        }
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    assert!(
        store
            .list("s".into())
            .await
            .iter()
            .all(|v| v["generation"] == 3)
    );
}

#[tokio::test]
async fn update_moves_previous_tree_without_snapshot_and_preserves_shared_snapshot() {
    let store = in_memory_store();
    let initial = serde_json::json!({"payload":"x".repeat(65536),"count":0});
    store.set("s".into(), "key".into(), initial.clone()).await;
    let pointer = {
        let map = store.store.read().await;
        map["s"]["key"]["payload"].as_str().unwrap().as_ptr() as usize
    };
    let result = store
        .update(
            "s".into(),
            "key".into(),
            vec![UpdateOp::increment("count", 1)],
        )
        .await;
    assert!(result.errors.is_empty());
    assert_eq!(
        result.old_value.as_ref().unwrap()["payload"]
            .as_str()
            .unwrap()
            .as_ptr() as usize,
        pointer
    );
    assert_eq!(result.old_value.unwrap(), initial);
    assert_eq!(result.new_value["count"], 1);
    let snapshot = store.store.read().await["s"].clone();
    let result = store
        .update(
            "s".into(),
            "key".into(),
            vec![UpdateOp::increment("count", 1)],
        )
        .await;
    assert_eq!(snapshot["key"]["count"], 1);
    assert_eq!(result.old_value.unwrap()["count"], 1);
    assert_eq!(result.new_value["count"], 2);
}

#[tokio::test]
async fn stale_delete_marker_persists_repopulated_scope() {
    let dir = directory();
    let store = manual_store(&dir);
    store
        .set("scope".into(), "old".into(), Value::Bool(false))
        .await;
    flush(&store).await.unwrap();
    store
        .set("scope".into(), "live".into(), Value::Bool(true))
        .await;
    store
        .dirty
        .write()
        .await
        .insert("scope".into(), DirtyOp::Delete);

    flush(&store).await.unwrap();

    let persisted = read_legacy(&dir, "scope");
    assert_eq!(persisted["old"], Value::Bool(false));
    assert_eq!(persisted["live"], Value::Bool(true));
    let live: IndexMap<String, Value> = store.store.read().await["scope"]
        .iter()
        .map(|(key, value)| (key.clone(), value.as_ref().clone()))
        .collect();
    assert_eq!(persisted, live);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn stale_upsert_marker_removes_empty_or_absent_scope() {
    let dir = directory();
    let store = manual_store(&dir);
    store
        .set("empty".into(), "key".into(), Value::Bool(true))
        .await;
    flush(&store).await.unwrap();
    store.delete("empty".into(), "key".into()).await;
    store
        .dirty
        .write()
        .await
        .insert("empty".into(), DirtyOp::Upsert);
    flush(&store).await.unwrap();
    assert!(!dir.join(index_file_name("empty")).exists());

    store
        .set("absent".into(), "key".into(), Value::Bool(true))
        .await;
    flush(&store).await.unwrap();
    store.store.write().await.remove("absent");
    store
        .dirty
        .write()
        .await
        .insert("absent".into(), DirtyOp::Upsert);
    flush(&store).await.unwrap();
    assert!(!dir.join(index_file_name("absent")).exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn stale_release_delete_marker_preserves_live_lock_scope() {
    let dir = directory();
    let store = manual_store(&dir);
    assert!(
        store
            .try_acquire_lock("locks", "old", "owner-1", 600_000)
            .await
    );
    flush(&store).await.unwrap();
    assert!(store.release_lock("locks", "old", "owner-1").await);
    assert!(
        store
            .try_acquire_lock("locks", "live", "owner-2", 600_000)
            .await
    );
    store
        .dirty
        .write()
        .await
        .insert("locks".into(), DirtyOp::Delete);

    flush(&store).await.unwrap();

    let persisted = read_legacy(&dir, "locks");
    assert!(persisted.contains_key("live"));
    let live: IndexMap<String, Value> = store.store.read().await["locks"]
        .iter()
        .map(|(key, value)| (key.clone(), value.as_ref().clone()))
        .collect();
    assert_eq!(persisted, live);
    std::fs::remove_dir_all(dir).unwrap();
}
