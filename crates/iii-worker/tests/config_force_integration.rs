// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Force/reset-config integration tests for handle_managed_add --force.
//!
//! Covers: force re-add, force with reset_config (clears overrides),
//! and force without reset_config (preserves overrides).

mod common;

use common::isolation::in_temp_dir_async;

#[tokio::test]
async fn handle_managed_add_force_builtin_re_adds() {
    in_temp_dir_async(|| async {
        // First add creates config
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false).await;
        assert_eq!(exit_code, 0);
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));

        // Force re-add succeeds (builtins have no artifacts to delete)
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, true, false).await;
        assert_eq!(exit_code, 0);
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_force_reset_config_clears_overrides() {
    in_temp_dir_async(|| async {
        // Pre-populate with user overrides
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: iii-http\n    config:\n      port: 9999\n      custom_key: preserved\n",
        )
        .unwrap();

        // Force with reset_config should clear user overrides and re-apply defaults
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, true, true)
                .await;
        assert_eq!(exit_code, 0);

        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
        // Builtin defaults should be present
        assert!(content.contains("default_timeout"));
        // User override should NOT be preserved (reset_config wipes it)
        assert!(!content.contains("custom_key"));
    })
    .await;
}

#[tokio::test]
async fn handle_managed_add_force_without_reset_preserves_config() {
    in_temp_dir_async(|| async {
        // Pre-populate with user overrides
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: iii-http\n    config:\n      port: 9999\n      custom_key: preserved\n",
        )
        .unwrap();

        // Force WITHOUT reset_config should preserve user overrides
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, true, false)
                .await;
        assert_eq!(exit_code, 0);

        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
        // User override preserved via merge
        assert!(content.contains("9999"));
        assert!(content.contains("custom_key"));
    })
    .await;
}
