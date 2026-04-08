// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for config_file public API.
//!
//! Each test changes the working directory to a temp dir so that the
//! relative `config.yaml` path used by the public API resolves there.

use std::sync::Mutex;

// Serialize tests that mutate the cwd to prevent races.
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Helper: run an async closure in a temp dir, restoring cwd afterward.
async fn in_temp_dir_async<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let _guard = CWD_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    f().await;
    std::env::set_current_dir(original).unwrap();
}

/// Helper: run a closure in a temp dir, restoring cwd afterward.
fn in_temp_dir<F: FnOnce()>(f: F) {
    let _guard = CWD_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    f();
    std::env::set_current_dir(original).unwrap();
}

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
        std::fs::write("config.yaml", "workers:\n  - name: existing\n").unwrap();
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
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: keep\n  - name: remove-me\n  - name: also-keep\n",
        )
        .unwrap();
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
        std::fs::write("config.yaml", "workers:\n  - name: only\n").unwrap();
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
        std::fs::write("config.yaml", "workers:\n  - name: present\n").unwrap();
        assert!(iii_worker::cli::config_file::worker_exists("present"));
    });
}

#[test]
fn worker_exists_false() {
    in_temp_dir(|| {
        std::fs::write("config.yaml", "workers:\n  - name: other\n").unwrap();
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
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: alpha\n  - name: beta\n  - name: gamma\n",
        )
        .unwrap();
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
        iii_worker::cli::config_file::append_worker("iii-http", Some(default_yaml)).unwrap();

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
        iii_worker::cli::config_file::append_worker("iii-http", Some(default_yaml)).unwrap();

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
            iii_worker::cli::config_file::append_worker(name, Some(default_yaml)).unwrap();

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

// ──────────────────────────────────────────────────────────────────────────────
// handle_managed_add flow tests
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn handle_managed_add_builtin_creates_config() {
    in_temp_dir_async(|| async {
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", "libkrun", "localhost", 49134)
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
            iii_worker::cli::managed::handle_managed_add("iii-http", "libkrun", "localhost", 49134)
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
                iii_worker::cli::managed::handle_managed_add(name, "libkrun", "localhost", 49134)
                    .await;
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
