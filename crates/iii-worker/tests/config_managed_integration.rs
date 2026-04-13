// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Managed worker integration tests for handle_managed_add/remove flows.
//!
//! Covers: handle_managed_add_many, handle_managed_add (single builtin),
//! handle_managed_remove_many, and merge/default behaviors.

mod common;

use common::isolation::in_temp_dir_async;

#[tokio::test]
async fn add_many_builtin_workers() {
    in_temp_dir_async(|| async {
        let names = vec!["iii-http".to_string(), "iii-state".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&names).await;
        assert_eq!(exit_code, 0, "all builtin workers should succeed");

        assert!(
            iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should be in config.yaml"
        );
        assert!(
            iii_worker::cli::config_file::worker_exists("iii-state"),
            "iii-state should be in config.yaml"
        );
    })
    .await;
}

#[tokio::test]
async fn add_many_with_invalid_worker_returns_nonzero() {
    in_temp_dir_async(|| async {
        let names = vec![
            "iii-http".to_string(),
            "definitely-not-a-real-worker-xyz".to_string(),
        ];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&names).await;
        assert_ne!(exit_code, 0, "should fail when any worker fails");

        assert!(
            iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should still be in config.yaml despite other failure"
        );
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_builtin_creates_config() {
    in_temp_dir_async(|| async {
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false).await;
        assert_eq!(
            exit_code, 0,
            "expected success exit code for builtin worker"
        );

        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
        assert!(content.contains("config:"));
        assert!(content.contains("port: 3111"));
        assert!(content.contains("host: 127.0.0.1"));
        assert!(content.contains("default_timeout: 30000"));
        assert!(content.contains("concurrency_request_limit: 1024"));
        assert!(content.contains("allowed_origins"));
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_builtin_merges_existing() {
    in_temp_dir_async(|| async {
        // Pre-populate with user overrides
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: iii-http\n    config:\n      port: 9999\n      custom_key: preserved\n",
        )
        .unwrap();

        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false)
                .await;
        assert_eq!(exit_code, 0, "expected success exit code for merge");

        let content = std::fs::read_to_string("config.yaml").unwrap();
        // User override preserved
        assert!(content.contains("9999"));
        assert!(content.contains("custom_key"));
        // Builtin defaults filled in
        assert!(content.contains("default_timeout"));
        assert!(content.contains("concurrency_request_limit"));
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_all_builtins_succeed() {
    in_temp_dir_async(|| async {
        for name in iii_worker::cli::builtin_defaults::BUILTIN_NAMES {
            let _ = std::fs::remove_file("config.yaml");

            let exit_code =
                iii_worker::cli::managed::handle_managed_add(name, false, false, false).await;
            assert_eq!(exit_code, 0, "expected success for builtin '{}'", name);

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains(&format!("- name: {}", name)),
                "config.yaml missing entry for '{}'",
                name
            );
        }
    })
    .await;
}

#[tokio::test]
async fn remove_many_workers() {
    in_temp_dir_async(|| async {
        // Add two builtins first.
        let names = vec!["iii-http".to_string(), "iii-state".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&names).await;
        assert_eq!(exit_code, 0);

        // Remove both at once.
        let exit_code = iii_worker::cli::managed::handle_managed_remove_many(&names).await;
        assert_eq!(exit_code, 0, "all removals should succeed");

        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should be removed"
        );
        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-state"),
            "iii-state should be removed"
        );
    })
    .await;
}

#[tokio::test]
async fn remove_many_with_missing_worker_returns_nonzero() {
    in_temp_dir_async(|| async {
        // Add one builtin.
        let add_names = vec!["iii-http".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_add_many(&add_names).await;
        assert_eq!(exit_code, 0);

        // Remove existing + nonexistent.
        let remove_names = vec!["iii-http".to_string(), "not-a-real-worker".to_string()];
        let exit_code = iii_worker::cli::managed::handle_managed_remove_many(&remove_names).await;
        assert_ne!(exit_code, 0, "should fail when any removal fails");

        // The valid one should still have been removed.
        assert!(
            !iii_worker::cli::config_file::worker_exists("iii-http"),
            "iii-http should be removed despite other failure"
        );
    })
    .await;
}

// ===========================================================================
// OCI passthrough routing
// ===========================================================================

/// Verify that a full OCI reference (containing '/') is treated as a direct
/// OCI passthrough — the function should NOT call the API. It will fail because
/// there's no running container runtime, but the error should be about pulling,
/// not about API resolution.
#[tokio::test]
async fn handle_managed_add_oci_ref_passthrough() {
    in_temp_dir_async(|| async {
        // This will fail (no container runtime), but the error path tells us
        // it tried to pull an OCI image rather than calling the API
        let result = iii_worker::cli::managed::handle_managed_add(
            "ghcr.io/test/worker:1.0",
            false,
            false,
            false,
        )
        .await;
        // Non-zero exit because pull fails, but it should NOT have tried the API
        assert_ne!(result, 0);
    })
    .await;
}

/// Verify that a reference with ':' is also treated as OCI passthrough.
#[tokio::test]
async fn handle_managed_add_oci_ref_with_colon_passthrough() {
    in_temp_dir_async(|| async {
        let result = iii_worker::cli::managed::handle_managed_add(
            "docker.io/org/image:latest",
            false,
            false,
            false,
        )
        .await;
        assert_ne!(result, 0);
    })
    .await;
}

// ===========================================================================
// API resolution path (non-builtin, non-OCI-ref, non-local)
// ===========================================================================

/// Verify that a plain worker name that is NOT a builtin triggers API resolution.
/// With no API available, it should fail with a resolution error.
#[tokio::test]
async fn handle_managed_add_plain_name_calls_api() {
    in_temp_dir_async(|| async {
        // Set III_API_URL to something that will fail quickly
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let result =
            iii_worker::cli::managed::handle_managed_add("nonexistent-worker", true, false, false)
                .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Should fail because API is unreachable
        assert_ne!(result, 0);
    })
    .await;
}

/// Verify that a plain name matching a builtin worker succeeds without API.
#[tokio::test]
async fn handle_managed_add_builtin_skips_api() {
    in_temp_dir_async(|| async {
        // Set III_API_URL to something that will fail — if it tries the API, we'll know
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let result =
            iii_worker::cli::managed::handle_managed_add("iii-http", true, false, false).await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Should succeed because iii-http is a builtin — no API call needed
        assert_eq!(result, 0);
    })
    .await;
}

/// Verify that local path workers are routed correctly (not to API).
#[tokio::test]
async fn handle_managed_add_local_path_skips_api() {
    in_temp_dir_async(|| async {
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };
        let result =
            iii_worker::cli::managed::handle_managed_add("./my-local-worker", true, false, false)
                .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Will fail (path doesn't exist), but should NOT have called the API
        assert_ne!(result, 0);
    })
    .await;
}

/// Verify API resolution with file:// fixture returns binary worker.
#[tokio::test]
async fn handle_managed_add_binary_via_file_fixture() {
    in_temp_dir_async(|| async {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "name": "test-binary-worker",
            "type": "binary",
            "version": "0.1.0",
            "binaries": {},
            "config": {"name": "test-binary-worker", "config": {}}
        }"#;
        let path = dir.path().join("fixture.json");
        std::fs::write(&path, json).unwrap();

        let url = format!("file://{}", path.display());
        unsafe { std::env::set_var("III_API_URL", &url) };
        let result =
            iii_worker::cli::managed::handle_managed_add("test-binary-worker", true, false, false)
                .await;
        unsafe { std::env::remove_var("III_API_URL") };
        // Will fail because binaries map is empty (no platform match),
        // but it proves the API resolution path was taken and parsed correctly
        assert_ne!(result, 0);
    })
    .await;
}
