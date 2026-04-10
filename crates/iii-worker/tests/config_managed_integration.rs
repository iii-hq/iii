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
            iii_worker::cli::managed::handle_managed_add("iii-http", false, None, false, false)
                .await;
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
            iii_worker::cli::managed::handle_managed_add("iii-http", false, None, false, false)
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
                iii_worker::cli::managed::handle_managed_add(name, false, None, false, false).await;
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
