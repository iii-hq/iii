//! Regression coverage for execution injection without persisting compose overrides.
use super::{
    adapters::{ConfigurationAdapter, ExternalChange, fs::FsAdapter},
    store::ConfigurationStore,
    structs::MigrateAction,
};
use serde_json::{Value, json};
use std::sync::Arc;

async fn fixture() -> (tempfile::TempDir, Arc<FsAdapter>, ConfigurationStore) {
    let dir = tempfile::tempdir().unwrap();
    let adapter = Arc::new(
        FsAdapter::new(Some(json!({"directory":dir.path()})))
            .await
            .unwrap(),
    );
    let store = ConfigurationStore::new(adapter.clone());
    (dir, adapter, store)
}
fn schema() -> Value {
    json!({"type":"object","properties":{"b":{"type":"integer"}},"required":["b"]})
}
async fn ensure(store: &ConfigurationStore, b: i32) {
    store
        .ensure(
            "test".into(),
            "Test".into(),
            "schema refresh".into(),
            schema(),
            Some(json!({"b":b})),
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn injection_before_registration_never_seeds_the_base() {
    let (dir, adapter, store) = fixture().await;
    store.set_memory("test", json!({"b":2})).await.unwrap();
    assert!(store.get("test").await.is_none());
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    ensure(&store, 0).await;
    assert_eq!(
        adapter.get("test").await.unwrap().unwrap().value,
        json!({"b":0})
    );
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
    ensure(&store, 9).await;
    assert_eq!(store.get("test").await.unwrap().value, json!({"b":0}));
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
}

#[tokio::test]
async fn explicit_save_persists_active_values_and_service_restart_loads_saved_base() {
    let (dir, adapter, store) = fixture().await;
    ensure(&store, 1).await;
    let original = std::fs::read(dir.path().join("test.yaml")).unwrap();
    store.set_memory("test", json!({"b":2})).await.unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("test.yaml")).unwrap(),
        original
    );
    let active = store.get_active("test").await.unwrap().value;
    store.set("test", active).await.unwrap();
    assert!(!store.is_injected("test").await);
    assert_eq!(
        adapter.get("test").await.unwrap().unwrap().value,
        json!({"b":2})
    );
    store.set("test", json!({"b":3})).await.unwrap();
    store.set_memory("test", json!({"b":2})).await.unwrap();
    assert_eq!(store.get("test").await.unwrap().value, json!({"b":3}));
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
    // Rebuilding the service loads the saved base, never a prior injection.
    store.set_memory("test", json!({"b":2})).await.unwrap();
    let reloaded = ConfigurationStore::new(Arc::new(
        FsAdapter::new(Some(json!({"directory":dir.path()})))
            .await
            .unwrap(),
    ));
    reloaded.prime_from_adapter().await.unwrap();
    assert_eq!(
        reloaded.get_active("test").await.unwrap().value,
        json!({"b":3})
    );
}

#[tokio::test]
async fn metadata_registration_and_watcher_echo_do_not_persist_or_clear_injection() {
    let (_dir, adapter, store) = fixture().await;
    ensure(&store, 1).await;
    store.set_memory("test", json!({"b":2})).await.unwrap();
    store
        .register(
            "test".into(),
            "New name".into(),
            "metadata".into(),
            schema(),
            None,
            None,
        )
        .await
        .unwrap();
    let base = adapter.get("test").await.unwrap().unwrap();
    store
        .apply_external(&ExternalChange::Updated {
            entry: base,
            old_value: Some(json!({"b":1})),
        })
        .await;
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
    assert_eq!(
        adapter.get("test").await.unwrap().unwrap().value,
        json!({"b":1})
    );
    let edited = adapter.set("test", json!({"b":4})).await.unwrap().entry;
    store
        .apply_external(&ExternalChange::Updated {
            entry: edited,
            old_value: Some(json!({"b":1})),
        })
        .await;
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":4})
    );
}

#[tokio::test]
async fn invalid_injection_and_failed_save_preserve_the_current_execution() {
    let (dir, _, store) = fixture().await;
    ensure(&store, 1).await;
    store.set_memory("test", json!({"b":2})).await.unwrap();
    assert!(store.set_memory("test", json!({"b":"bad"})).await.is_err());
    assert!(store.set("test", json!({"b":"bad"})).await.is_err());
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
    // Force the adapter's atomic rename to fail, without changing the base cache.
    let target = dir.path().join("test.yaml");
    std::fs::remove_file(&target).unwrap();
    std::fs::create_dir(&target).unwrap();
    assert!(store.set("test", json!({"b":3})).await.is_err());
    assert_eq!(store.get("test").await.unwrap().value, json!({"b":1}));
    assert_eq!(
        store.get_active("test").await.unwrap().value,
        json!({"b":2})
    );
}

#[tokio::test]
async fn memory_only_null_is_a_value_not_a_clear_request() {
    let (_dir, _adapter, store) = fixture().await;
    store.set_memory("test", Value::Null).await.unwrap();
    assert!(store.is_injected("test").await);
    assert_eq!(store.get_active("test").await.unwrap().value, Value::Null);
    store.delete("test").await.unwrap();
    assert!(store.get_active("test").await.is_none());
}

async fn seed_migration_entry(store: &ConfigurationStore, id: &str, value: Value) {
    store
        .register(
            id.into(),
            id.into(),
            "Migration fixture".into(),
            json!({}),
            Some(value),
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn migration_moves_active_source_without_persisting_it_or_leaving_old_id_readable() {
    for target_exists in [false, true] {
        for active in [json!({"b": 2}), Value::Null] {
            let (dir, adapter, store) = fixture().await;
            seed_migration_entry(&store, "old", json!({"b": 1})).await;
            if target_exists {
                seed_migration_entry(&store, "new", json!({"b": 90})).await;
            }
            store.set_memory("new", json!({"b": 99})).await.unwrap();
            store.set_memory("old", active.clone()).await.unwrap();
            let original = std::fs::read(dir.path().join("old.yaml")).unwrap();

            let moved = store.migrate("old", "new").await.unwrap();
            assert_eq!(moved.action, MigrateAction::Migrated);
            assert_eq!(moved.entry.unwrap().value, json!({"b": 1}));
            assert!(store.get_active("old").await.is_none());
            assert!(!store.is_injected("old").await);
            assert_eq!(store.get_active("new").await.unwrap().value, active);
            assert!(store.is_injected("new").await);
            assert_eq!(
                adapter.get("new").await.unwrap().unwrap().value,
                json!({"b": 1})
            );
            let disk: Value =
                serde_yaml::from_slice(&std::fs::read(dir.path().join("new.yaml")).unwrap())
                    .unwrap();
            assert_eq!(disk["value"], json!({"b": 1}));
            assert_eq!(
                std::fs::read(dir.path().join("old.yaml.bak")).unwrap(),
                original
            );
            assert!(!dir.path().join("old.yaml").exists());

            // A watcher echo must not undo the moved, unsaved value.
            store
                .apply_external(&ExternalChange::Updated {
                    entry: adapter.get("new").await.unwrap().unwrap(),
                    old_value: Some(json!({"b": 90})),
                })
                .await;
            assert_eq!(store.get_active("new").await.unwrap().value, active);

            // Retrying the migration cannot erase a later update at the new id.
            store.set_memory("new", json!({"b": 3})).await.unwrap();
            assert_eq!(
                store.migrate("old", "new").await.unwrap().action,
                MigrateAction::Preserved
            );
            assert!(store.get_active("old").await.is_none());
            assert_eq!(
                store.get_active("new").await.unwrap().value,
                json!({"b": 3})
            );
        }
    }
}

#[tokio::test]
async fn migration_source_without_override_replaces_stale_destination_memory() {
    let (_dir, adapter, store) = fixture().await;
    seed_migration_entry(&store, "old", json!({"b": 1})).await;
    seed_migration_entry(&store, "new", json!({"b": 90})).await;
    store.set_memory("new", json!({"b": 99})).await.unwrap();
    store.migrate("old", "new").await.unwrap();
    assert!(store.get_active("old").await.is_none());
    assert!(!store.is_injected("new").await);
    assert_eq!(
        store.get_active("new").await.unwrap().value,
        adapter.get("new").await.unwrap().unwrap().value
    );
}

#[tokio::test]
async fn migration_same_id_preserves_active_value_with_or_without_disk_entry() {
    for registered in [false, true] {
        let (dir, _adapter, store) = fixture().await;
        if registered {
            seed_migration_entry(&store, "same", json!({"b": 1})).await;
        }
        store.set_memory("same", json!({"b": 2})).await.unwrap();
        let before = std::fs::read(dir.path().join("same.yaml")).ok();
        let result = store.migrate("same", "same").await.unwrap();
        assert_eq!(
            result.action,
            if registered {
                MigrateAction::Preserved
            } else {
                MigrateAction::Missing
            }
        );
        assert_eq!(
            store.get_active("same").await.unwrap().value,
            json!({"b": 2})
        );
        assert_eq!(std::fs::read(dir.path().join("same.yaml")).ok(), before);
    }
}

#[tokio::test]
async fn migration_missing_source_retires_old_memory_but_preserves_destination() {
    for target_exists in [false, true] {
        let (_dir, _adapter, store) = fixture().await;
        if target_exists {
            seed_migration_entry(&store, "new", json!({"b": 90})).await;
        }
        store.set_memory("old", json!({"b": 2})).await.unwrap();
        store.set_memory("new", json!({"b": 99})).await.unwrap();
        let result = store.migrate("old", "new").await.unwrap();
        assert_eq!(
            result.action,
            if target_exists {
                MigrateAction::Preserved
            } else {
                MigrateAction::Missing
            }
        );
        assert!(store.get_active("old").await.is_none());
        assert_eq!(
            store.get_active("new").await.unwrap().value,
            json!({"b": 99})
        );
    }
}
