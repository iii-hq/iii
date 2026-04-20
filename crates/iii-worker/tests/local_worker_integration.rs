// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for local worker lifecycle helpers (LOCAL-01 through LOCAL-12).
//!
//! Tests are organized in four groups:
//! 1. Pure function tests (no filesystem, no async)
//! 2. Filesystem tests (sync, tempdir)
//! 3. CWD-dependent async tests (handle_local_add)
//! 4. Platform-gated detect_lan_ip tests

mod common;

use common::fixtures::TestConfigBuilder;
use common::isolation::in_temp_dir_async;
use iii_worker::cli::config_file::{get_worker_path, worker_exists};
use iii_worker::cli::local_worker::{
    build_env_exports, build_libkrun_local_script, build_local_env,
    clean_workspace_preserving_deps, copy_dir_contents, detect_lan_ip, handle_local_add,
    is_local_path, parse_manifest_resources, resolve_worker_name, shell_escape,
};
use iii_worker::cli::project::{ProjectInfo, WORKER_MANIFEST};
use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────────────
// Group 1: Pure function tests (no filesystem, no async)
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn is_local_path_dot_relative() {
    assert!(is_local_path("./myworker"));
}

#[test]
fn is_local_path_absolute() {
    assert!(is_local_path("/home/user/worker"));
}

#[test]
fn is_local_path_tilde() {
    assert!(is_local_path("~/projects/worker"));
}

#[test]
fn is_local_path_registry_name() {
    assert!(!is_local_path("my-worker"));
}

/// LOCAL-10: shell_escape replaces single quotes with the '\'' escape sequence.
#[test]
fn shell_escape_single_quotes() {
    assert_eq!(shell_escape("it's a test"), "it'\\''s a test");
}

#[test]
fn shell_escape_no_special_chars() {
    assert_eq!(shell_escape("normal"), "normal");
}

/// LOCAL-12: build_env_exports excludes III_ENGINE_URL and III_URL keys.
#[test]
fn build_env_exports_excludes_engine_urls() {
    let mut env = HashMap::new();
    env.insert(
        "III_ENGINE_URL".to_string(),
        "ws://localhost:49134".to_string(),
    );
    env.insert("III_URL".to_string(), "ws://localhost:49134".to_string());
    env.insert("VALID_KEY".to_string(), "value".to_string());

    let result = build_env_exports(&env);
    assert!(
        !result.contains("III_ENGINE_URL"),
        "should exclude III_ENGINE_URL"
    );
    assert!(!result.contains("III_URL"), "should exclude III_URL");
    assert!(
        result.contains("export VALID_KEY='value'"),
        "should include VALID_KEY"
    );
}

/// build_env_exports skips keys with invalid characters (spaces, empty).
#[test]
fn build_env_exports_skips_invalid_keys() {
    let mut env = HashMap::new();
    env.insert("invalid key".to_string(), "value".to_string());
    env.insert("".to_string(), "empty-key".to_string());

    let result = build_env_exports(&env);
    assert!(
        !result.contains("invalid key"),
        "should skip keys with spaces"
    );
    assert_eq!(
        result, "true",
        "should return 'true' when no valid keys remain"
    );
}

#[test]
fn build_env_exports_empty_map() {
    let env = HashMap::new();
    let result = build_env_exports(&env);
    assert_eq!(result, "true");
}

/// LOCAL-12 wiring: build_local_env merges engine URL and project env,
/// excluding III_ENGINE_URL/III_URL from project env values.
#[test]
fn build_local_env_merges_and_excludes() {
    let mut project_env = HashMap::new();
    project_env.insert("CUSTOM".to_string(), "val".to_string());
    project_env.insert("III_ENGINE_URL".to_string(), "skip-this".to_string());
    project_env.insert("III_URL".to_string(), "skip-this-too".to_string());

    let result = build_local_env("ws://localhost:49134", &project_env);
    assert_eq!(
        result.get("III_ENGINE_URL").unwrap(),
        "ws://localhost:49134"
    );
    assert_eq!(result.get("III_URL").unwrap(), "ws://localhost:49134");
    assert_eq!(result.get("CUSTOM").unwrap(), "val");
    // Engine URL values come from the function argument, not project_env
    assert_ne!(result.get("III_ENGINE_URL").unwrap(), "skip-this");
    assert_ne!(result.get("III_URL").unwrap(), "skip-this-too");
}

/// LOCAL-11: build_libkrun_local_script includes setup/install when prepared=false.
#[test]
fn build_libkrun_local_script_not_prepared() {
    let project = ProjectInfo {
        name: "test".to_string(),
        language: Some("typescript".to_string()),
        setup_cmd: "apt-get update".to_string(),
        install_cmd: "npm install".to_string(),
        run_cmd: "npm start".to_string(),
        env: HashMap::new(),
    };
    let script = build_libkrun_local_script(&project, false);
    assert!(
        script.contains("apt-get update"),
        "should include setup_cmd"
    );
    assert!(script.contains("npm install"), "should include install_cmd");
    assert!(
        script.contains(".iii-prepared"),
        "should include prepared marker"
    );
    assert!(script.contains("npm start"), "should include run_cmd");
}

/// LOCAL-11: build_libkrun_local_script omits setup/install when prepared=true.
#[test]
fn build_libkrun_local_script_prepared() {
    let project = ProjectInfo {
        name: "test".to_string(),
        language: Some("typescript".to_string()),
        setup_cmd: "apt-get update".to_string(),
        install_cmd: "npm install".to_string(),
        run_cmd: "npm start".to_string(),
        env: HashMap::new(),
    };
    let script = build_libkrun_local_script(&project, true);
    assert!(
        !script.contains("apt-get update"),
        "should omit setup_cmd when prepared"
    );
    assert!(
        !script.contains("npm install"),
        "should omit install_cmd when prepared"
    );
    assert!(script.contains("npm start"), "should still include run_cmd");
}

// ──────────────────────────────────────────────────────────────────────────────
// Group 2: Filesystem tests (sync, tempdir)
// ──────────────────────────────────────────────────────────────────────────────

/// LOCAL-08 partial: resolve_worker_name reads name from manifest.
#[test]
fn resolve_worker_name_from_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = "name: my-custom-worker\nruntime:\n  language: typescript\n";
    std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();
    let name = resolve_worker_name(dir.path());
    assert_eq!(name, "my-custom-worker");
}

/// resolve_worker_name falls back to directory name when no manifest exists.
#[test]
fn resolve_worker_name_fallback_to_dir_name() {
    let dir = tempfile::tempdir().unwrap();
    let name = resolve_worker_name(dir.path());
    let expected = dir.path().file_name().unwrap().to_str().unwrap();
    assert_eq!(name, expected);
}

/// LOCAL-08: parse_manifest_resources returns custom CPU/memory from YAML.
#[test]
fn parse_manifest_resources_custom_values() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join(WORKER_MANIFEST);
    let yaml = "name: resource-test\nresources:\n  cpus: 4\n  memory: 4096\n";
    std::fs::write(&manifest_path, yaml).unwrap();
    let (cpus, memory) = parse_manifest_resources(&manifest_path);
    assert_eq!(cpus, 4);
    assert_eq!(memory, 4096);
}

/// LOCAL-08: parse_manifest_resources returns defaults when path is absent.
#[test]
fn parse_manifest_resources_defaults_on_missing() {
    let dir = tempfile::tempdir().unwrap();
    let nonexistent = dir.path().join("nonexistent.yaml");
    let (cpus, memory) = parse_manifest_resources(&nonexistent);
    assert_eq!(cpus, 2);
    assert_eq!(memory, 2048);
}

/// LOCAL-05, LOCAL-06: copy_dir_contents copies files and skips ignored directories.
#[test]
fn copy_dir_contents_copies_files_skips_ignored() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();

    // Create source files that should be copied
    std::fs::create_dir_all(src.path().join("src")).unwrap();
    std::fs::write(src.path().join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(src.path().join("README.md"), "# README").unwrap();

    // Create directories that should be skipped
    std::fs::create_dir_all(src.path().join("node_modules/pkg")).unwrap();
    std::fs::write(src.path().join("node_modules/pkg/index.js"), "").unwrap();
    std::fs::create_dir_all(src.path().join(".git")).unwrap();
    std::fs::write(src.path().join(".git/config"), "").unwrap();
    std::fs::create_dir_all(src.path().join("target/debug")).unwrap();
    std::fs::write(src.path().join("target/debug/bin"), "").unwrap();
    std::fs::create_dir_all(src.path().join("__pycache__")).unwrap();
    std::fs::write(src.path().join("__pycache__/mod.pyc"), "").unwrap();
    std::fs::create_dir_all(src.path().join(".venv/lib")).unwrap();
    std::fs::write(src.path().join(".venv/lib/site.py"), "").unwrap();
    std::fs::create_dir_all(src.path().join("dist")).unwrap();
    std::fs::write(src.path().join("dist/bundle.js"), "").unwrap();

    copy_dir_contents(src.path(), dst.path()).unwrap();

    // Verify copied files
    assert!(
        dst.path().join("src/main.rs").exists(),
        "src/main.rs should be copied"
    );
    assert!(
        dst.path().join("README.md").exists(),
        "README.md should be copied"
    );

    // Verify skipped directories
    assert!(
        !dst.path().join("node_modules").exists(),
        "node_modules should be skipped"
    );
    assert!(!dst.path().join(".git").exists(), ".git should be skipped");
    assert!(
        !dst.path().join("target").exists(),
        "target should be skipped"
    );
    assert!(
        !dst.path().join("__pycache__").exists(),
        "__pycache__ should be skipped"
    );
    assert!(
        !dst.path().join(".venv").exists(),
        ".venv should be skipped"
    );
    assert!(!dst.path().join("dist").exists(), "dist should be skipped");
}

/// LOCAL-07: clean_workspace_preserving_deps removes source but keeps dependency dirs.
#[test]
fn clean_workspace_preserving_deps_preserves_deps_removes_source() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path();

    // Create dependency directories that should be preserved
    std::fs::create_dir_all(ws.join("node_modules/pkg")).unwrap();
    std::fs::write(ws.join("node_modules/pkg/index.js"), "mod").unwrap();
    std::fs::create_dir_all(ws.join("target/debug")).unwrap();
    std::fs::write(ws.join("target/debug/bin"), "elf").unwrap();
    std::fs::create_dir_all(ws.join(".venv/lib")).unwrap();
    std::fs::write(ws.join(".venv/lib/site.py"), "py").unwrap();
    std::fs::create_dir_all(ws.join("__pycache__")).unwrap();
    std::fs::write(ws.join("__pycache__/mod.pyc"), "pyc").unwrap();

    // Create source files/dirs that should be removed
    std::fs::write(ws.join("main.ts"), "console.log()").unwrap();
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/lib.ts"), "export {}").unwrap();

    clean_workspace_preserving_deps(ws);

    // Dep dirs preserved
    assert!(ws.join("node_modules/pkg/index.js").exists());
    assert!(ws.join("target/debug/bin").exists());
    assert!(ws.join(".venv/lib/site.py").exists());
    assert!(ws.join("__pycache__/mod.pyc").exists());

    // Source files/dirs removed
    assert!(!ws.join("main.ts").exists());
    assert!(!ws.join("src").exists());
}

// ──────────────────────────────────────────────────────────────────────────────
// Group 3: CWD-dependent async tests (handle_local_add)
// ──────────────────────────────────────────────────────────────────────────────

/// LOCAL-01: handle_local_add adds a worker from a valid filesystem path.
/// Note: Without an iii.worker.yaml manifest, resolve_worker_name falls back
/// to the directory name (not the auto-detected project type name).
#[tokio::test]
async fn handle_local_add_valid_path() {
    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        // Create a project directory with package.json (node auto-detect)
        let project_dir = cwd.join("my-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();

        let result =
            handle_local_add(project_dir.to_str().unwrap(), false, false, true, false).await;
        assert_eq!(result, 0, "handle_local_add should return 0 for valid path");

        // Without manifest, resolve_worker_name falls back to directory name
        assert!(
            worker_exists("my-worker"),
            "worker should exist in config.yaml with directory name"
        );
        let stored_path = get_worker_path("my-worker");
        assert!(stored_path.is_some(), "worker path should be stored");
        let stored = stored_path.unwrap();
        // Verify the stored path contains the project directory
        // (canonicalized, so on macOS /tmp -> /private/tmp)
        let canonical_project = std::fs::canonicalize(&project_dir).unwrap();
        assert!(
            stored.contains(canonical_project.to_str().unwrap()),
            "stored path '{}' should contain canonical project dir '{}'",
            stored,
            canonical_project.display()
        );
    })
    .await;
}

/// LOCAL-03: handle_local_add rejects duplicate worker name without --force.
#[tokio::test]
async fn handle_local_add_rejects_duplicate_without_force() {
    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        // Create project with manifest defining worker name
        let project_dir = cwd.join("my-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();
        std::fs::write(
            project_dir.join(WORKER_MANIFEST),
            "name: my-worker\nruntime:\n  language: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
        )
        .unwrap();

        // Pre-populate config.yaml with existing worker
        TestConfigBuilder::new()
            .with_worker("my-worker", None)
            .build(&cwd);

        let result =
            handle_local_add(project_dir.to_str().unwrap(), false, false, true, false).await;
        assert_eq!(
            result, 1,
            "should return 1 when worker exists and force=false"
        );
    })
    .await;
}

/// LOCAL-02: handle_local_add with force=true replaces an existing worker entry.
#[tokio::test]
async fn handle_local_add_force_replaces_existing() {
    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        // Create project with manifest
        let project_dir = cwd.join("my-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();
        std::fs::write(
            project_dir.join(WORKER_MANIFEST),
            "name: my-worker\nruntime:\n  language: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
        )
        .unwrap();

        // Pre-populate config.yaml with old path
        TestConfigBuilder::new()
            .with_worker("my-worker", Some("/old/path"))
            .build(&cwd);

        let result =
            handle_local_add(project_dir.to_str().unwrap(), true, true, true, false).await;
        assert_eq!(result, 0, "should return 0 with force=true");

        let stored_path = get_worker_path("my-worker");
        assert!(stored_path.is_some(), "worker path should be stored");
        let stored = stored_path.unwrap();
        assert!(
            !stored.contains("/old/path"),
            "stored path '{}' should not be the old path",
            stored
        );
        // Verify it contains the new canonical path
        let canonical_project = std::fs::canonicalize(&project_dir).unwrap();
        assert!(
            stored.contains(canonical_project.to_str().unwrap()),
            "stored path '{}' should contain new canonical path '{}'",
            stored,
            canonical_project.display()
        );
    })
    .await;
}

/// LOCAL-04: handle_local_add resolves relative paths to absolute before storing.
#[tokio::test]
async fn handle_local_add_canonicalizes_relative_path() {
    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        // Create project using relative path
        let project_dir = cwd.join("rel-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();

        let result = handle_local_add("./rel-worker", false, false, true, false).await;
        assert_eq!(result, 0, "should succeed with relative path");

        // Without manifest, name falls back to directory name "rel-worker"
        let stored_path = get_worker_path("rel-worker");
        assert!(stored_path.is_some(), "worker path should be stored");
        let stored = stored_path.unwrap();
        assert!(
            stored.starts_with('/'),
            "stored path '{}' should be absolute (start with /)",
            stored
        );
    })
    .await;
}

/// handle_local_add returns 1 for nonexistent path.
#[tokio::test]
async fn handle_local_add_invalid_path_returns_error() {
    in_temp_dir_async(|| async {
        let result =
            handle_local_add("/nonexistent/path/to/worker", false, false, true, false).await;
        assert_eq!(result, 1, "should return 1 for nonexistent path");
    })
    .await;
}

// ──────────────────────────────────────────────────────────────────────────────
// Group 4: Platform-gated detect_lan_ip tests
// ──────────────────────────────────────────────────────────────────────────────

/// LOCAL-09: detect_lan_ip returns Some(IPv4) on macOS.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn detect_lan_ip_macos_returns_some_ipv4() {
    let result = detect_lan_ip().await;
    assert!(result.is_some(), "macOS should return Some(ip)");
    let ip = result.unwrap();
    // Validate IPv4 format without regex dependency
    assert_eq!(ip.split('.').count(), 4, "IP '{}' should have 4 octets", ip);
    assert!(
        ip.split('.').all(|octet| octet.parse::<u8>().is_ok()),
        "IP '{}' octets should all be valid u8",
        ip
    );
}

/// LOCAL-09: detect_lan_ip returns None on Linux (route -n get default is macOS-only).
#[cfg(target_os = "linux")]
#[tokio::test]
async fn detect_lan_ip_linux_returns_none() {
    let result = detect_lan_ip().await;
    assert!(result.is_none(), "Linux should return None");
}
