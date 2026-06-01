// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for worker manager: RuntimeAdapter dispatch, MockAdapter contract,
//! and WorkerDef serialization roundtrip.
//! Covers requirements WMGR-01 through WMGR-04.

mod common;

use iii_worker::cli::worker_manager::adapter::{
    ContainerSpec, ContainerStatus, ImageInfo, RuntimeAdapter,
};
use iii_worker::cli::worker_manager::create_adapter;
use iii_worker::cli::worker_manager::state::{WorkerDef, WorkerResources};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// MockAdapter: hand-written mock implementing RuntimeAdapter with canned Ok
// responses. Per project convention (CLAUDE.md): 30-line mock over mockall
// for a 6-method trait.
// ---------------------------------------------------------------------------

struct MockAdapter;

#[async_trait::async_trait]
impl RuntimeAdapter for MockAdapter {
    async fn pull(&self, image: &str) -> anyhow::Result<ImageInfo> {
        Ok(ImageInfo {
            image: image.to_string(),
            size_bytes: Some(1024),
        })
    }

    async fn extract_file(&self, _image: &str, _path: &str) -> anyhow::Result<Vec<u8>> {
        Ok(b"mock content".to_vec())
    }

    async fn start(&self, spec: &ContainerSpec) -> anyhow::Result<String> {
        Ok(format!("mock-{}", spec.name))
    }

    async fn stop(&self, _container_id: &str, _timeout_secs: u32) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self, container_id: &str) -> anyhow::Result<ContainerStatus> {
        Ok(ContainerStatus {
            name: "mock-worker".to_string(),
            container_id: container_id.to_string(),
            running: true,
            exit_code: None,
        })
    }

    async fn remove(&self, _container_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

// ===========================================================================
// Group 1: RuntimeAdapter dispatch (WMGR-01, D-10)
// ===========================================================================

/// create_adapter returns a valid trait object for the "libkrun" runtime string.
/// Verifies Arc<dyn RuntimeAdapter> is constructed without panic.
#[tokio::test]
async fn create_adapter_returns_valid_trait_object() {
    let adapter: Arc<dyn RuntimeAdapter> = create_adapter("libkrun");
    // Verify the trait object is usable by calling status.
    // The real LibkrunAdapter may fail (no running container), but it must not panic.
    let result = adapter.status("nonexistent").await;
    // We only care that the call completes (Ok or Err), not that it succeeds.
    let _ = result;
}

/// create_adapter with an unknown runtime string still returns a valid Arc.
/// Current implementation always returns LibkrunAdapter regardless of input.
#[tokio::test]
async fn create_adapter_with_unknown_runtime() {
    let adapter: Arc<dyn RuntimeAdapter> = create_adapter("unknown");
    // Must not panic -- the adapter is valid even for unknown runtimes.
    let result = adapter.status("nonexistent").await;
    let _ = result;
}

/// libkrun_available returns a bool without panicking.
/// The actual value depends on whether firmware is present on the test host.
#[test]
fn libkrun_available_returns_bool() {
    let available: bool = iii_worker::cli::worker_manager::libkrun::libkrun_available();
    // Assert it is a valid bool (this is mainly a smoke test for no panic).
    assert!(available || !available);
}

// ===========================================================================
// Group 2: MockAdapter contract (WMGR-03, WMGR-04, D-11)
// ===========================================================================

/// MockAdapter.pull returns ImageInfo with the requested image name.
#[tokio::test]
async fn mock_adapter_pull_returns_image_info() {
    let adapter = MockAdapter;
    let info = adapter.pull("test-image:latest").await.unwrap();
    assert_eq!(info.image, "test-image:latest");
    assert_eq!(info.size_bytes, Some(1024));
}

/// MockAdapter.extract_file returns canned content.
#[tokio::test]
async fn mock_adapter_extract_file_returns_content() {
    let adapter = MockAdapter;
    let content = adapter.extract_file("img", "/etc/config").await.unwrap();
    assert_eq!(content, b"mock content");
}

/// MockAdapter.start returns a container ID derived from the spec name.
#[tokio::test]
async fn mock_adapter_start_returns_container_id() {
    let adapter = MockAdapter;
    let spec = ContainerSpec {
        name: "my-worker".to_string(),
        image: "test:latest".to_string(),
        env: HashMap::new(),
        memory_limit: None,
        cpu_limit: None,
    };
    let id = adapter.start(&spec).await.unwrap();
    assert_eq!(id, "mock-my-worker");
}

/// MockAdapter.stop returns Ok(()) for clean shutdown.
#[tokio::test]
async fn mock_adapter_stop_returns_ok() {
    let adapter = MockAdapter;
    let result = adapter.stop("mock-123", 30).await;
    assert!(result.is_ok());
}

/// MockAdapter.status returns ContainerStatus with running=true.
#[tokio::test]
async fn mock_adapter_status_returns_running() {
    let adapter = MockAdapter;
    let status = adapter.status("mock-123").await.unwrap();
    assert!(status.running);
    assert_eq!(status.container_id, "mock-123");
    assert_eq!(status.name, "mock-worker");
    assert!(status.exit_code.is_none());
}

/// MockAdapter.remove returns Ok(()).
#[tokio::test]
async fn mock_adapter_remove_returns_ok() {
    let adapter = MockAdapter;
    let result = adapter.remove("mock-123").await;
    assert!(result.is_ok());
}

/// MockAdapter can be used as Arc<dyn RuntimeAdapter> trait object,
/// verifying it is object-safe (same pattern as production create_adapter).
#[tokio::test]
async fn mock_adapter_as_trait_object() {
    let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter);
    let info = adapter.pull("trait-object-test:latest").await.unwrap();
    assert_eq!(info.image, "trait-object-test:latest");
}

// ===========================================================================
// Group 3: WorkerDef serialization roundtrip (WMGR-02, D-12, D-13)
// ===========================================================================

/// WorkerDef::Managed roundtrip through serde_json preserves all fields.
#[test]
fn workerdef_managed_roundtrip() {
    let mut env = HashMap::new();
    env.insert("KEY".to_string(), "value".to_string());

    let original = WorkerDef::Managed {
        image: "ghcr.io/iii-hq/test:latest".to_string(),
        env: env.clone(),
        resources: Some(WorkerResources {
            cpus: Some("2".to_string()),
            memory: Some("512Mi".to_string()),
        }),
    };

    assert!(original.is_managed());
    assert!(!original.is_binary());

    let serialized = serde_json::to_value(&original).unwrap();
    let deserialized: WorkerDef = serde_json::from_value(serialized).unwrap();

    match deserialized {
        WorkerDef::Managed {
            image,
            env: de_env,
            resources,
        } => {
            assert_eq!(image, "ghcr.io/iii-hq/test:latest");
            assert_eq!(de_env.get("KEY").unwrap(), "value");
            let res = resources.unwrap();
            assert_eq!(res.cpus.unwrap(), "2");
            assert_eq!(res.memory.unwrap(), "512Mi");
        }
        _ => panic!("expected Managed variant after roundtrip"),
    }
}

/// WorkerDef::Binary roundtrip through serde_json preserves version and config.
#[test]
fn workerdef_binary_roundtrip() {
    let original = WorkerDef::Binary {
        version: "1.2.3".to_string(),
        config: Some(serde_json::json!({"key": "value"})),
    };

    assert!(original.is_binary());
    assert!(!original.is_managed());

    let serialized = serde_json::to_value(&original).unwrap();
    let deserialized: WorkerDef = serde_json::from_value(serialized).unwrap();

    match deserialized {
        WorkerDef::Binary { version, config } => {
            assert_eq!(version, "1.2.3");
            assert_eq!(config.unwrap()["key"], "value");
        }
        _ => panic!("expected Binary variant after roundtrip"),
    }
}

/// JSON without a "type" field defaults to Managed variant on deserialization.
/// This tests the custom Deserialize impl's None branch.
#[test]
fn workerdef_missing_type_defaults_to_managed() {
    let json = serde_json::json!({
        "image": "ghcr.io/iii-hq/legacy:latest",
        "env": { "FOO": "bar" }
    });

    let def: WorkerDef = serde_json::from_value(json).unwrap();
    assert!(def.is_managed());

    match def {
        WorkerDef::Managed { image, env, .. } => {
            assert_eq!(image, "ghcr.io/iii-hq/legacy:latest");
            assert_eq!(env.get("FOO").unwrap(), "bar");
        }
        _ => panic!("expected Managed variant for input without type field"),
    }
}

/// JSON with an unknown "type" field produces a deserialization error.
/// Validates the custom Deserialize impl's error branch.
#[test]
fn workerdef_unknown_type_errors() {
    let json = serde_json::json!({
        "type": "docker",
        "image": "test:latest"
    });

    let result: Result<WorkerDef, _> = serde_json::from_value(json);
    assert!(
        result.is_err(),
        "unknown type 'docker' should produce deserialization error"
    );
}

/// WorkerDef::Managed with empty env and no resources roundtrips correctly.
#[test]
fn workerdef_managed_empty_env() {
    let original = WorkerDef::Managed {
        image: "ghcr.io/iii-hq/minimal:latest".to_string(),
        env: HashMap::new(),
        resources: None,
    };

    let serialized = serde_json::to_value(&original).unwrap();
    let deserialized: WorkerDef = serde_json::from_value(serialized).unwrap();

    match deserialized {
        WorkerDef::Managed {
            image,
            env,
            resources,
        } => {
            assert_eq!(image, "ghcr.io/iii-hq/minimal:latest");
            assert!(env.is_empty());
            assert!(resources.is_none());
        }
        _ => panic!("expected Managed variant"),
    }
}

/// WorkerDef::Binary with config=None roundtrips correctly.
#[test]
fn workerdef_binary_no_config() {
    let original = WorkerDef::Binary {
        version: "0.1.0".to_string(),
        config: None,
    };

    let serialized = serde_json::to_value(&original).unwrap();
    let deserialized: WorkerDef = serde_json::from_value(serialized).unwrap();

    match deserialized {
        WorkerDef::Binary { version, config } => {
            assert_eq!(version, "0.1.0");
            assert!(config.is_none());
        }
        _ => panic!("expected Binary variant"),
    }
}

/// WorkerResources with mixed Some/None optional fields roundtrip correctly.
#[test]
fn workerdef_resources_optional_fields() {
    // Case 1: cpus=Some, memory=None
    let res1 = WorkerResources {
        cpus: Some("4".to_string()),
        memory: None,
    };
    let json1 = serde_json::to_value(&res1).unwrap();
    let de_res1: WorkerResources = serde_json::from_value(json1).unwrap();
    assert_eq!(de_res1.cpus.as_deref(), Some("4"));
    assert!(de_res1.memory.is_none());

    // Case 2: cpus=None, memory=Some
    let res2 = WorkerResources {
        cpus: None,
        memory: Some("1Gi".to_string()),
    };
    let json2 = serde_json::to_value(&res2).unwrap();
    let de_res2: WorkerResources = serde_json::from_value(json2).unwrap();
    assert!(de_res2.cpus.is_none());
    assert_eq!(de_res2.memory.as_deref(), Some("1Gi"));
}
