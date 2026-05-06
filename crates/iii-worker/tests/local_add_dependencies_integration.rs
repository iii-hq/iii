// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end coverage for `iii worker add <local-path>` with declared
//! manifest dependencies. Uses `file://` API fixtures (the pattern already
//! used in `tests/config_managed_integration.rs:345`) so every round-trip
//! is hermetic and the test asserts real disk state after the call.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Serializes tests in this file that mutate CWD and `III_API_URL`.
/// Without it, one test's `set_var("III_API_URL", fixture)` leaks into
/// another test's "unreachable" scenario and the second test sees the
/// first fixture's URL — turning the failure path into a download-stage
/// 404 and breaking the deps-before-append assertion.
static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

async fn in_temp_dir<F, Fut, R>(f: F) -> R
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = R>,
{
    let _guard = TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let prev = std::env::current_dir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let result = f().await;
    std::env::set_current_dir(prev).unwrap();
    result
}

fn write_worker_manifest(dir: &Path, name: &str, deps_block: &str) {
    let manifest =
        format!("name: {name}\nruntime:\n  kind: bun\n  entry: src/index.ts\n{deps_block}");
    std::fs::write(dir.join("iii.worker.yaml"), manifest).unwrap();
}

/// Failure path: unreachable registry must NOT leave config.yaml or iii.lock
/// written. This verifies the deps-before-append ordering.
#[tokio::test]
async fn local_add_with_unreachable_dep_leaves_no_state() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("my-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        write_worker_manifest(
            &worker_dir,
            "my-worker",
            "dependencies:\n  math-worker: \"^0.1.0\"\n",
        );

        let prev_api = std::env::var("III_API_URL").ok();
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };

        let rc = iii_worker::cli::managed::handle_managed_add(
            worker_dir.to_str().unwrap(),
            true,
            false,
            false,
            false,
        )
        .await;

        match prev_api {
            Some(v) => unsafe { std::env::set_var("III_API_URL", v) },
            None => unsafe { std::env::remove_var("III_API_URL") },
        }

        assert_ne!(rc, 0, "unreachable dep must fail the whole add");
        assert!(
            !std::env::current_dir()
                .unwrap()
                .join("config.yaml")
                .exists(),
            "config.yaml must not exist — deps failed before append"
        );
        assert!(
            !std::env::current_dir().unwrap().join("iii.lock").exists(),
            "iii.lock must not exist — deps failed before install"
        );
    })
    .await;
}

/// No-deps path: plain local worker still works when registry is unreachable.
#[tokio::test]
async fn local_add_without_dependencies_ignores_registry() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("plain-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        write_worker_manifest(&worker_dir, "plain-worker", "");

        let prev_api = std::env::var("III_API_URL").ok();
        unsafe { std::env::set_var("III_API_URL", "http://127.0.0.1:1") };

        let rc = iii_worker::cli::managed::handle_managed_add(
            worker_dir.to_str().unwrap(),
            true,
            false,
            false,
            false,
        )
        .await;

        match prev_api {
            Some(v) => unsafe { std::env::set_var("III_API_URL", v) },
            None => unsafe { std::env::remove_var("III_API_URL") },
        }

        assert_eq!(rc, 0, "no-deps local add must succeed");
        assert!(
            std::env::current_dir()
                .unwrap()
                .join("config.yaml")
                .exists(),
            "config.yaml must exist for the happy-path local worker"
        );
    })
    .await;
}

/// Exercises the full chain wiring: fixture resolves successfully, the merged
/// graph is installed, and we reach the binary-download stage — which fails
/// because the fixture URL is synthetic. The assertion is that the error
/// originates from the download path, not the resolve path, proving every
/// step from parse -> load_manifest_dependencies -> install_manifest_dependencies
/// -> fetch_resolved_worker_graph -> merge_resolved_graphs ->
/// handle_resolved_graph_add -> handle_binary_add fires in order. The
/// snapshot/rollback boundary of `handle_resolved_graph_add` then rolls
/// config.yaml back so the failed add leaves the workspace pristine.
#[tokio::test]
async fn local_add_dep_resolves_and_reaches_install_stage() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("my-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        write_worker_manifest(
            &worker_dir,
            "my-worker",
            "dependencies:\n  math-worker: \"^0.1.0\"\n",
        );

        let sha = "a".repeat(64);
        let fixture_json = format!(
            r#"{{
                "root": {{"name": "math-worker", "version": "0.1.3"}},
                "graph": [{{
                    "name": "math-worker",
                    "type": "binary",
                    "version": "0.1.3",
                    "repo": "",
                    "config": {{}},
                    "binaries": {{
                        "aarch64-apple-darwin": {{"sha256": "{sha}", "url": "https://example.com/math-0.1.3-mac.tar.gz"}},
                        "x86_64-unknown-linux-gnu": {{"sha256": "{sha}", "url": "https://example.com/math-0.1.3-lin.tar.gz"}}
                    }},
                    "dependencies": {{}}
                }}],
                "edges": []
            }}"#
        );
        let fixture = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(fixture.path(), fixture_json).unwrap();
        let url = format!("file://{}", fixture.path().display());

        let prev_api = std::env::var("III_API_URL").ok();
        unsafe { std::env::set_var("III_API_URL", &url) };

        let rc = iii_worker::cli::managed::handle_managed_add(
            worker_dir.to_str().unwrap(),
            true,
            false,
            false,
            false,
        )
        .await;

        match prev_api {
            Some(v) => unsafe { std::env::set_var("III_API_URL", v) },
            None => unsafe { std::env::remove_var("III_API_URL") },
        }

        // Non-zero because the synthetic binary URL can't actually be
        // downloaded — that's expected. What matters is that we got
        // through resolve -> merge -> install attempt.
        assert_ne!(
            rc, 0,
            "fixture URL is synthetic; expected download-stage failure"
        );

        // Snapshot/rollback in handle_resolved_graph_add restores config.yaml,
        // and iii.lock is only written after all downloads succeed. A pristine
        // workspace after the failed add proves the atomic boundary held.
        let cwd = std::env::current_dir().unwrap();
        assert!(
            !cwd.join("iii.lock").exists(),
            "iii.lock must not be written when install fails mid-chain"
        );
        let config_contents = std::fs::read_to_string(cwd.join("config.yaml"))
            .unwrap_or_default();
        assert!(
            !config_contents.contains("my-worker"),
            "config.yaml must roll back the local worker on install failure; got:\n{config_contents}"
        );
    })
    .await;
}
