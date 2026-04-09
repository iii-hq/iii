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
// handle_managed_add_many flow tests
// ──────────────────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────────────────
// handle_managed_add flow tests
// ──────────────────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────────────────
// handle_managed_remove_many flow tests
// ──────────────────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────────────────
// handle_managed_add --force tests
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn handle_managed_add_force_builtin_re_adds() {
    in_temp_dir_async(|| async {
        // First add creates config
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, None, false, false)
                .await;
        assert_eq!(exit_code, 0);
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: iii-http"));

        // Force re-add succeeds (builtins have no artifacts to delete)
        let exit_code =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, None, true, false)
                .await;
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
            iii_worker::cli::managed::handle_managed_add("iii-http", false, None, true, true)
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
            iii_worker::cli::managed::handle_managed_add("iii-http", false, None, true, false)
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

// ──────────────────────────────────────────────────────────────────────────────
// handle_managed_clear tests
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn handle_managed_clear_single_no_artifacts() {
    in_temp_dir_async(|| async {
        // Clear a worker that has no artifacts — should succeed silently
        let exit_code = iii_worker::cli::managed::handle_managed_clear(Some("pdfkit"), true);
        assert_eq!(exit_code, 0);
    })
    .await;
}

#[tokio::test]
async fn handle_managed_clear_invalid_name() {
    in_temp_dir_async(|| async {
        // Clear with an invalid name (contains path traversal)
        let exit_code = iii_worker::cli::managed::handle_managed_clear(Some("../etc"), true);
        assert_eq!(exit_code, 1);
    })
    .await;
}

#[tokio::test]
async fn handle_managed_clear_all_no_artifacts() {
    in_temp_dir_async(|| async {
        // Clear all when nothing is installed — should succeed
        let exit_code = iii_worker::cli::managed::handle_managed_clear(None, true);
        assert_eq!(exit_code, 0);
    })
    .await;
}

// ──────────────────────────────────────────────────────────────────────────────
// append_worker_with_path tests
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn append_worker_with_path_creates_entry() {
    in_temp_dir(|| {
        iii_worker::cli::config_file::append_worker_with_path("local-w", "/abs/path", None)
            .unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: local-w"));
        assert!(content.contains("worker_path: /abs/path"));
    });
}

#[test]
fn append_worker_with_path_with_config() {
    in_temp_dir(|| {
        iii_worker::cli::config_file::append_worker_with_path(
            "local-w",
            "/abs/path",
            Some("timeout: 30"),
        )
        .unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(content.contains("- name: local-w"));
        assert!(content.contains("worker_path: /abs/path"));
        assert!(content.contains("timeout: 30"));
    });
}

#[test]
fn append_worker_with_path_merges_existing() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: local-w\n    worker_path: /old\n    config:\n      custom_key: preserved\n",
        )
        .unwrap();
        iii_worker::cli::config_file::append_worker_with_path(
            "local-w",
            "/new/path",
            Some("new_key: added"),
        )
        .unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(
            content.contains("worker_path: /new/path"),
            "path should be updated, got:\n{}",
            content
        );
        assert!(
            content.contains("custom_key"),
            "user config should be preserved, got:\n{}",
            content
        );
        assert!(
            content.contains("new_key"),
            "incoming config should be merged, got:\n{}",
            content
        );
    });
}

#[test]
fn append_worker_with_path_replaces_image_with_path() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: my-worker\n    image: ghcr.io/org/w:1\n",
        )
        .unwrap();
        iii_worker::cli::config_file::append_worker_with_path("my-worker", "/local/path", None)
            .unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();
        assert!(
            content.contains("worker_path: /local/path"),
            "should have worker_path, got:\n{}",
            content
        );
        assert!(
            !content.contains("image:"),
            "image should be removed, got:\n{}",
            content
        );
    });
}

// ──────────────────────────────────────────────────────────────────────────────
// get_worker_path tests
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn get_worker_path_present() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: my-worker\n    worker_path: /home/user/proj\n",
        )
        .unwrap();
        let path = iii_worker::cli::config_file::get_worker_path("my-worker");
        assert_eq!(path, Some("/home/user/proj".to_string()));
    });
}

#[test]
fn get_worker_path_absent() {
    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: oci-worker\n    image: ghcr.io/org/w:1\n",
        )
        .unwrap();
        let path = iii_worker::cli::config_file::get_worker_path("oci-worker");
        assert!(path.is_none());
    });
}

// ──────────────────────────────────────────────────────────────────────────────
// resolve_worker_type tests
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn resolve_worker_type_from_config_file() {
    use iii_worker::cli::config_file::ResolvedWorkerType;

    in_temp_dir(|| {
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: local-w\n    worker_path: /home/user/proj\n  - name: oci-w\n    image: ghcr.io/org/w:1\n  - name: config-w\n",
        )
        .unwrap();

        let local = iii_worker::cli::config_file::resolve_worker_type("local-w");
        assert!(
            matches!(local, ResolvedWorkerType::Local { ref worker_path } if worker_path == "/home/user/proj"),
            "expected Local, got {:?}",
            local
        );

        let oci = iii_worker::cli::config_file::resolve_worker_type("oci-w");
        assert!(
            matches!(oci, ResolvedWorkerType::Oci { ref image, .. } if image == "ghcr.io/org/w:1"),
            "expected Oci, got {:?}",
            oci
        );

        // config-only worker resolves to Config (or Binary if ~/.iii/workers/config-w exists,
        // but that's unlikely in test environments)
        let config = iii_worker::cli::config_file::resolve_worker_type("config-w");
        assert!(
            matches!(
                config,
                ResolvedWorkerType::Config | ResolvedWorkerType::Binary { .. }
            ),
            "expected Config or Binary fallback, got {:?}",
            config
        );
    });
}
