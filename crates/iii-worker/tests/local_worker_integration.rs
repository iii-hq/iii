// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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

/// RAII guard overriding `HOME` for tests that exercise `~/.iii` artifact
/// paths (via `dirs::home_dir()`), restoring the prior value on drop.
///
/// SAFETY INVARIANT (load-bearing, not optional): `HOME` is process-global,
/// so the `unsafe` `set_var`/`remove_var` below are only sound while *every*
/// HOME-reading test in this binary holds `CWD_LOCK` via `in_temp_dir_async`,
/// which serializes them. This guard is constructed only inside such closures.
/// A future test that reads `dirs::home_dir()` (directly or transitively)
/// outside `in_temp_dir_async` would race this guard and must not be added
/// without holding the same lock — there is no separate env lock here.
///
/// Unix-only: `dirs::home_dir()` honors `HOME` only on Unix. On Windows it
/// resolves via the Known Folder API (FOLDERID_Profile) and ignores `HOME`,
/// so this guard cannot sandbox `~/.iii`, and a `--force` test would run
/// `remove_dir_all` against the real user home. The `~/.iii` libkrun worker
/// surface this exercises is Unix-only anyway.
#[cfg(unix)]
struct HomeGuard {
    original: Option<std::ffi::OsString>,
}

#[cfg(unix)]
impl HomeGuard {
    fn new(path: &std::path::Path) -> Self {
        let original = std::env::var_os("HOME");
        // SAFETY: test-only; serialized with sibling CWD/HOME tests via CWD_LOCK.
        unsafe { std::env::set_var("HOME", path) };
        Self { original }
    }
}

#[cfg(unix)]
impl Drop for HomeGuard {
    fn drop(&mut self) {
        // SAFETY: test-only; serialized with sibling CWD/HOME tests via CWD_LOCK.
        unsafe {
            match &self.original {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

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
        kind: Some("typescript".to_string()),
        setup_cmd: "apt-get update".to_string(),
        install_cmd: "npm install".to_string(),
        run_cmd: "npm start".to_string(),
        env: HashMap::new(),
        base_image: None,
    };
    let script =
        build_libkrun_local_script(&project, false, /*is_bundle=*/ false, /*overlay=*/ false);
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
        kind: Some("typescript".to_string()),
        setup_cmd: "apt-get update".to_string(),
        install_cmd: "npm install".to_string(),
        run_cmd: "npm start".to_string(),
        env: HashMap::new(),
        base_image: None,
    };
    let script =
        build_libkrun_local_script(&project, true, /*is_bundle=*/ false, /*overlay=*/ false);
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
    let yaml = "name: my-custom-worker\nruntime:\n  kind: typescript\n";
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
            "name: my-worker\nruntime:\n  kind: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
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
            "name: my-worker\nruntime:\n  kind: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
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

/// MOT-3585: `--force` must invalidate the `.iii-prepared` marker so the next
/// boot reruns the in-VM dependency install — even when a co-named binary
/// artifact also exists, which used to trip the `if freed == 0` guard in
/// `delete_worker_artifacts` and strand the managed dir (marker + dep caches).
///
/// Unix-only: relies on `HomeGuard` sandboxing `~/.iii` via `HOME`, which
/// `dirs::home_dir()` honors only on Unix (see `HomeGuard`).
#[cfg(unix)]
#[tokio::test]
async fn handle_local_add_force_removes_prepared_marker() {
    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        // Sandbox HOME so we touch a temp ~/.iii, never the developer's real one.
        let home = cwd.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let _home_guard = HomeGuard::new(&home);

        // Project with a manifest (no `dependencies:` block → no network).
        let project_dir = cwd.join("my-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();
        std::fs::write(
            project_dir.join(WORKER_MANIFEST),
            "name: my-worker\nruntime:\n  kind: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
        )
        .unwrap();

        // Pre-seed config.yaml so the --force replace path runs.
        TestConfigBuilder::new()
            .with_worker("my-worker", Some("/old/path"))
            .build(&cwd);

        // Simulate a previously-prepared worker: a binary artifact (frees > 0,
        // the regression trigger) plus the managed dir holding the marker and a
        // dep cache.
        let binary_dir = home.join(".iii/workers/my-worker");
        std::fs::create_dir_all(&binary_dir).unwrap();
        std::fs::write(binary_dir.join("blob"), "bytes").unwrap();

        let managed_dir = home.join(".iii/managed/my-worker");
        let marker = managed_dir.join("var/.iii-prepared");
        std::fs::create_dir_all(managed_dir.join("bin")).unwrap();
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        std::fs::create_dir_all(managed_dir.join("var/iii/deps/node_modules")).unwrap();

        let result =
            handle_local_add(project_dir.to_str().unwrap(), true, false, true, false).await;
        assert_eq!(result, 0, "force add should succeed");

        // Config entry replaced with the new canonical path.
        let stored = get_worker_path("my-worker").expect("worker path should be stored");
        assert!(
            !stored.contains("/old/path"),
            "stale path should be replaced, got '{}'",
            stored
        );

        // The reinstall trigger: the marker must be gone after --force.
        assert!(
            !marker.exists(),
            "`.iii-prepared` marker must be removed on --force so install reruns"
        );

        // Root-cause coverage: the whole managed dir (marker + the
        // /var/iii/deps caches) must be wiped, not just the marker. Without
        // these, the test passes even if the `delete_worker_artifacts`
        // managed-dir wipe regresses, because the defense-in-depth
        // `remove_file` deletes the marker on its own. A surviving dep cache
        // is half of MOT-3585: a changed lock file would reuse stale deps.
        assert!(
            !managed_dir.exists(),
            "managed dir must be wiped on --force, not stranded by a freed binary artifact"
        );
        assert!(
            !managed_dir.join("var/iii/deps/node_modules").exists(),
            "stale dep cache must be removed on --force so a changed lock file reinstalls"
        );
    })
    .await;
}

/// MOT-3585 part 2 (defense-in-depth): when the managed-dir wipe in
/// `delete_worker_artifacts` fails and strands the dir + marker, the explicit
/// `.iii-prepared` removal must still fire so the next boot reruns install.
/// This is the ONLY test that exercises the `Ok(())` arm of that fallback —
/// the happy-path tests wipe the dir successfully, so their `remove_file`
/// always hits the no-op `NotFound` arm instead.
///
/// We force the wipe to fail deterministically by making the managed dir
/// unreadable (`0o300`: execute+write, no read), so `remove_dir_all` can't
/// enumerate it, while the direct `remove_file(.../var/.iii-prepared)` path
/// still resolves (execute on the dirs, write on `var/`). Unix-only because it
/// relies on POSIX permission semantics.
#[cfg(unix)]
#[tokio::test]
async fn handle_local_add_force_removes_marker_when_dir_wipe_fails() {
    use std::os::unix::fs::PermissionsExt;

    // root bypasses POSIX permission checks, so the 0o300 sabotage below would
    // NOT block remove_dir_all — the wipe would succeed, the dir would vanish,
    // and the `managed_dir.exists()` precondition would fail. Skip honestly
    // (common in container CI that runs as root) rather than fail or, worse,
    // pass for the wrong reason.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!(
            "skipping handle_local_add_force_removes_marker_when_dir_wipe_fails: \
             running as root, the 0o300 permission sabotage is ineffective"
        );
        return;
    }

    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        let home = cwd.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let _home_guard = HomeGuard::new(&home);

        let project_dir = cwd.join("my-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();
        std::fs::write(
            project_dir.join(WORKER_MANIFEST),
            "name: my-worker\nruntime:\n  kind: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
        )
        .unwrap();

        TestConfigBuilder::new()
            .with_worker("my-worker", Some("/old/path"))
            .build(&cwd);

        let managed_dir = home.join(".iii/managed/my-worker");
        let marker = managed_dir.join("var/.iii-prepared");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();

        // Sabotage the wipe: unreadable managed dir => remove_dir_all fails to
        // enumerate, leaving the dir + marker behind for the fallback to clean.
        std::fs::set_permissions(&managed_dir, std::fs::Permissions::from_mode(0o300)).unwrap();

        // Restore perms on scope exit OR unwind, so a panic inside
        // handle_local_add can't leave the 0o300 subtree behind to defeat
        // TempDir cleanup. `exists()` assertions below still work at 0o300
        // (execute bit is set), so restoring after them is unnecessary.
        struct RestorePerms<'a>(&'a std::path::Path);
        impl Drop for RestorePerms<'_> {
            fn drop(&mut self) {
                let _ =
                    std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        let _restore = RestorePerms(&managed_dir);

        let result =
            handle_local_add(project_dir.to_str().unwrap(), true, false, true, false).await;

        assert_eq!(result, 0, "force add should still succeed when the dir wipe fails");
        assert!(
            !marker.exists(),
            "defense-in-depth remove_file must clear the marker even when the dir wipe failed"
        );
        // The dir itself is expected to survive (the wipe failed) — that is the
        // whole point of the marker-only fallback.
        assert!(
            managed_dir.exists(),
            "precondition: the sabotaged dir should still be present, proving the wipe failed"
        );
    })
    .await;
}

/// MOT-3585: when the explicit `.iii-prepared` removal hits a hard error (not
/// NotFound), `--force` must FAIL (exit 1) rather than report success while a
/// stale marker survives — a stale marker makes the next boot skip
/// setup/install and reuse stale deps, which is the bug this PR fixes.
///
/// We make `var/` read-only (`0o500`) so both `delete_worker_artifacts`'
/// recursive wipe and the direct `remove_file` fallback fail with
/// PermissionDenied, leaving the marker in place. Unix-only and skipped under
/// root (root bypasses the permission check).
#[cfg(unix)]
#[tokio::test]
async fn handle_local_add_force_fails_when_marker_removal_errors() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        eprintln!(
            "skipping handle_local_add_force_fails_when_marker_removal_errors: \
             running as root, the read-only var/ sabotage is ineffective"
        );
        return;
    }

    in_temp_dir_async(|| async {
        let cwd = std::env::current_dir().unwrap();

        let home = cwd.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let _home_guard = HomeGuard::new(&home);

        let project_dir = cwd.join("my-worker");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("package.json"), "{}").unwrap();
        std::fs::write(
            project_dir.join(WORKER_MANIFEST),
            "name: my-worker\nruntime:\n  kind: typescript\n  package_manager: npm\n  entry: src/index.ts\n",
        )
        .unwrap();

        TestConfigBuilder::new()
            .with_worker("my-worker", Some("/old/path"))
            .build(&cwd);

        let managed_dir = home.join(".iii/managed/my-worker");
        let var_dir = managed_dir.join("var");
        let marker = var_dir.join(".iii-prepared");
        std::fs::create_dir_all(&var_dir).unwrap();
        std::fs::write(&marker, "").unwrap();

        // Read-only var/ (r-x, no write): the marker can be unlinked by neither
        // the recursive wipe nor the explicit remove_file, so the hard-error
        // arm fires.
        std::fs::set_permissions(&var_dir, std::fs::Permissions::from_mode(0o500)).unwrap();

        // Restore perms on scope exit/unwind so TempDir cleanup isn't defeated.
        struct RestorePerms<'a>(&'a std::path::Path);
        impl Drop for RestorePerms<'_> {
            fn drop(&mut self) {
                let _ =
                    std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        let _restore = RestorePerms(&var_dir);

        let result =
            handle_local_add(project_dir.to_str().unwrap(), true, false, true, false).await;

        assert_eq!(
            result, 1,
            "--force must fail when the prepared marker cannot be removed"
        );
        assert!(
            marker.exists(),
            "precondition: the marker should still be present, proving removal failed"
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
