// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! CRUD integration tests for config_file public API.
//!
//! Covers: append_worker, remove_worker, worker_exists, list_worker_names,
//! get_worker_image, get_worker_config_as_env, and builtin default operations.

mod common;

use common::fixtures::TestConfigBuilder;
use common::isolation::in_temp_dir;

#[test]
fn append_worker_creates_file_from_scratch() {
    in_temp_dir(|| {
        iii_worker::cli::config_file::append_worker("my-worker", None).unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: my-worker"));
        assert!(content.contains("workers:"));
    });
}

#[test]
fn append_worker_with_config() {
    in_temp_dir(|| {
        iii_worker::cli::config_file::append_worker("my-worker", Some("port: 3000")).unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: my-worker"));
        assert!(content.contains("config:"));
        assert!(content.contains("port: 3000"));
    });
}

#[test]
fn append_worker_appends_to_existing() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("existing", None)
            .build(&dir);
        iii_worker::cli::config_file::append_worker("new-worker", None).unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: existing"));
        assert!(content.contains("- name: new-worker"));
    });
}

#[test]
fn append_worker_with_image() {
    in_temp_dir(|| {
        iii_worker::cli::config_file::append_worker_with_image(
            "pdfkit",
            "ghcr.io/iii-hq/pdfkit:1.0",
            Some("timeout: 30"),
        )
        .unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: pdfkit"));
        assert!(content.contains("image: ghcr.io/iii-hq/pdfkit:1.0"));
        assert!(content.contains("timeout: 30"));
    });
}

#[test]
fn append_worker_idempotent_merge() {
    in_temp_dir(|| {
        iii_worker::cli::config_file::append_worker("w", Some("port: 3000\nhost: custom")).unwrap();
        iii_worker::cli::config_file::append_worker("w", Some("port: 8080\ndebug: true")).unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: w"));
        // User's host should be preserved
        assert!(content.contains("host"));
        // New key from registry should be added
        assert!(content.contains("debug"));
    });
}

#[test]
fn remove_worker_removes_and_preserves_others() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("keep", None)
            .with_worker("remove-me", None)
            .with_worker("also-keep", None)
            .build(&dir);
        iii_worker::cli::config_file::remove_worker("remove-me").unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(!content.contains("remove-me"));
        assert!(content.contains("- name: keep"));
        assert!(content.contains("- name: also-keep"));
    });
}

#[test]
fn remove_worker_not_found_returns_error() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("only", None)
            .build(&dir);
        let result = iii_worker::cli::config_file::remove_worker("ghost");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    });
}

#[test]
fn remove_worker_no_file_returns_error() {
    in_temp_dir(|| {
        let result = iii_worker::cli::config_file::remove_worker("any");
        assert!(result.is_err());
    });
}

#[test]
fn worker_exists_true() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("present", None)
            .build(&dir);
        assert!(iii_worker::cli::config_file::worker_exists("present"));
    });
}

#[test]
fn worker_exists_false() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("other", None)
            .build(&dir);
        assert!(!iii_worker::cli::config_file::worker_exists("absent"));
    });
}

#[test]
fn worker_exists_no_file() {
    in_temp_dir(|| {
        assert!(!iii_worker::cli::config_file::worker_exists("any"));
    });
}

#[test]
fn list_worker_names_empty() {
    in_temp_dir(|| {
        std::fs::write("config.yaml", "workers:\n").unwrap();
        let names = iii_worker::cli::config_file::list_worker_names();
        assert!(names.is_empty());
    });
}

#[test]
fn list_worker_names_multiple() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("alpha", None)
            .with_worker("beta", None)
            .with_worker("gamma", None)
            .build(&dir);
        let names = iii_worker::cli::config_file::list_worker_names();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    });
}

#[test]
fn list_worker_names_no_file() {
    in_temp_dir(|| {
        let names = iii_worker::cli::config_file::list_worker_names();
        assert!(names.is_empty());
    });
}

#[test]
fn get_worker_image_present() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: pdfkit\n    image: ghcr.io/iii-hq/pdfkit:1.0\n",
        )
        .unwrap();
        let image = iii_worker::cli::config_file::get_worker_image("pdfkit");
        assert_eq!(image, Some("ghcr.io/iii-hq/pdfkit:1.0".to_string()));
    });
}

#[test]
fn get_worker_image_absent() {
    in_temp_dir(|| {
        std::fs::write("config.yaml", "workers:\n  - name: binary-worker\n").unwrap();
        let image = iii_worker::cli::config_file::get_worker_image("binary-worker");
        assert!(image.is_none());
    });
}

#[test]
fn get_worker_config_as_env_flat() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: w\n    config:\n      api_key: secret123\n      port: 8080\n",
        )
        .unwrap();
        let env = iii_worker::cli::config_file::get_worker_config_as_env("w");
        assert_eq!(env.get("API_KEY").unwrap(), "secret123");
        assert!(env.contains_key("PORT"));
    });
}

#[test]
fn get_worker_config_as_env_nested() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: w\n    config:\n      database:\n        host: db.local\n        port: 5432\n",
        )
        .unwrap();
        let env = iii_worker::cli::config_file::get_worker_config_as_env("w");
        assert_eq!(env.get("DATABASE_HOST").unwrap(), "db.local");
        assert!(env.contains_key("DATABASE_PORT"));
    });
}

#[test]
fn get_worker_config_as_env_no_config() {
    in_temp_dir(|| {
        std::fs::write("config.yaml", "workers:\n  - name: w\n").unwrap();
        let env = iii_worker::cli::config_file::get_worker_config_as_env("w");
        assert!(env.is_empty());
    });
}

#[test]
fn append_builtin_worker_creates_entry_with_defaults() {
    in_temp_dir(|| {
        let default_yaml =
            iii_worker::cli::builtin_defaults::get_builtin_default("iii-http").unwrap();
        iii_worker::cli::config_file::append_worker("iii-http", Some(default_yaml.as_str()))
            .unwrap();

        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));
        assert!(content.contains("config:"));
        assert!(content.contains("port: 3111"));
        assert!(content.contains("host: 127.0.0.1"));
        assert!(content.contains("default_timeout: 30000"));
        assert!(content.contains("concurrency_request_limit: 1024"));
        assert!(content.contains("allowed_origins"));
    });
}

#[test]
fn append_builtin_worker_merges_with_existing_user_config() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: iii-http\n    config:\n      port: 9999\n      custom_key: preserved\n",
        )
        .unwrap();

        let default_yaml =
            iii_worker::cli::builtin_defaults::get_builtin_default("iii-http").unwrap();
        iii_worker::cli::config_file::append_worker("iii-http", Some(default_yaml.as_str()))
            .unwrap();

        let content = std::fs::read_to_string("config.yaml").unwrap();
        // User's port override is preserved
        assert!(content.contains("9999"));
        // User's custom key is preserved
        assert!(content.contains("custom_key"));
        // Builtin defaults for missing fields are filled in
        assert!(content.contains("default_timeout"));
        assert!(content.contains("concurrency_request_limit"));
    });
}

#[test]
fn all_builtins_produce_valid_config_entries() {
    in_temp_dir(|| {
        for name in iii_worker::cli::builtin_defaults::BUILTIN_NAMES {
            let _ = std::fs::remove_file("config.yaml");

            let default_yaml =
                iii_worker::cli::builtin_defaults::get_builtin_default(name).unwrap();
            iii_worker::cli::config_file::append_worker(name, Some(default_yaml.as_str())).unwrap();

            let content = std::fs::read_to_string("config.yaml").unwrap();
            assert!(
                content.contains(&format!("- name: {}", name)),
                "config.yaml missing entry for '{}'",
                name
            );
            assert!(
                content.contains("config:"),
                "config.yaml missing config block for '{}'",
                name
            );
        }
    });
}
