// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end coverage for `iii worker add <local-path>` manifest key
//! validation. Mirrors the harness in `local_add_dependencies_integration.rs`:
//! a CWD + `III_API_URL` mutex, a temp project dir, and a real
//! `handle_managed_add` call that asserts disk state afterwards. The manifests
//! here declare no `dependencies`, so the deps stage is a no-op and never hits
//! the network.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Serializes tests in this file that mutate CWD and `III_API_URL`.
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

fn write_manifest(dir: &Path, body: &str) {
    std::fs::write(dir.join("iii.worker.yaml"), body).unwrap();
}

async fn add_local(worker_dir: &Path) -> i32 {
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
    rc
}

/// An unknown manifest key (typo / unsupported) must FAIL the add before any
/// state is written — proving the validation gate runs ahead of the
/// config.yaml/iii.lock mutations.
#[tokio::test]
async fn local_add_unknown_key_fails_and_writes_nothing() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("bad-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        // `runtimee` is an unknown top-level key; `scripts.start` keeps the
        // manifest otherwise loadable so we reach the key validator.
        write_manifest(
            &worker_dir,
            "name: bad-worker\nscripts:\n  start: \"node index.js\"\nruntimee:\n  foo: bar\n",
        );

        let rc = add_local(&worker_dir).await;

        assert_ne!(rc, 0, "unknown manifest key must fail the add");
        let cwd = std::env::current_dir().unwrap();
        assert!(
            !cwd.join("config.yaml").exists(),
            "config.yaml must not exist — validation fails before append"
        );
        assert!(
            !cwd.join("iii.lock").exists(),
            "iii.lock must not exist — validation fails before install"
        );
    })
    .await;
}

/// Deprecated keys (`runtime.kind`, `config`, legacy top-level `language`/
/// `entry`) must WARN but still allow the add to succeed and write the worker
/// to config.yaml — so existing/scaffolded workers keep installing.
#[tokio::test]
async fn local_add_deprecated_keys_succeed_and_add_worker() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("dep-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        write_manifest(
            &worker_dir,
            "name: dep-worker\n\
             language: typescript\n\
             entry: src/index.ts\n\
             runtime:\n  kind: bun\n\
             config:\n  port: 3000\n",
        );

        let rc = add_local(&worker_dir).await;

        assert_eq!(rc, 0, "deprecated-only manifest must still add (rc 0)");
        let config = std::fs::read_to_string(std::env::current_dir().unwrap().join("config.yaml"))
            .unwrap_or_default();
        assert!(
            config.contains("dep-worker"),
            "config.yaml must contain the added worker; got:\n{config}"
        );
    })
    .await;
}

/// `description` is a supported field (read by managed.rs, documented in
/// workers.mdx); a manifest using it must add cleanly, not hard-fail.
#[tokio::test]
async fn local_add_description_field_succeeds() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("desc-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        write_manifest(
            &worker_dir,
            "name: desc-worker\n\
             description: Evaluate things over iii functions.\n\
             scripts:\n  start: \"node index.js\"\n",
        );

        let rc = add_local(&worker_dir).await;

        assert_eq!(rc, 0, "manifest with `description` must add (rc 0)");
        assert!(
            std::fs::read_to_string(std::env::current_dir().unwrap().join("config.yaml"))
                .unwrap_or_default()
                .contains("desc-worker"),
            "config.yaml must contain the added worker"
        );
    })
    .await;
}

/// A manifest that exists but lacks `name` must fail at the validation gate
/// with the required-field error — NOT fall through to auto-detection and the
/// false "No project manifest detected" diagnosis (the file plainly exists).
/// Nothing may be written.
#[tokio::test]
async fn local_add_nameless_manifest_fails_and_writes_nothing() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("nameless-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        // Even with a package.json present (the old auto-detect fallback), an
        // existing iii.worker.yaml is authoritative and must carry `name`.
        write_manifest(&worker_dir, "scripts:\n  start: \"node index.js\"\n");
        std::fs::write(
            worker_dir.join("package.json"),
            "{ \"name\": \"nameless-worker\" }",
        )
        .unwrap();

        let rc = add_local(&worker_dir).await;

        assert_ne!(rc, 0, "nameless manifest must fail the add");
        let cwd = std::env::current_dir().unwrap();
        assert!(
            !cwd.join("config.yaml").exists(),
            "config.yaml must not exist — name gate fails before append"
        );
    })
    .await;
}

/// A whitespace-padded `name` must add cleanly under the trimmed name —
/// `worker::validate` reports the trimmed name as valid, so the add path must
/// agree instead of failing on the embedded space.
#[tokio::test]
async fn local_add_padded_name_trims_and_succeeds() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("padded-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        write_manifest(
            &worker_dir,
            "name: \" padded-worker \"\nscripts:\n  start: \"node index.js\"\n",
        );

        let rc = add_local(&worker_dir).await;

        assert_eq!(rc, 0, "padded name must trim and add (rc 0)");
        let config = std::fs::read_to_string(std::env::current_dir().unwrap().join("config.yaml"))
            .unwrap_or_default();
        assert!(
            config.contains("padded-worker"),
            "config.yaml must contain the trimmed worker name; got:\n{config}"
        );
    })
    .await;
}

/// An oversize manifest is rejected (memory-DoS guard) before any state is
/// written, with nothing persisted.
#[tokio::test]
async fn local_add_oversize_manifest_fails_and_writes_nothing() {
    in_temp_dir(|| async {
        let worker_dir: PathBuf = std::env::current_dir().unwrap().join("big-worker");
        std::fs::create_dir(&worker_dir).unwrap();
        // Valid YAML, but past the 64 KiB cap.
        write_manifest(
            &worker_dir,
            &format!(
                "name: big-worker\nscripts:\n  start: x\nbloat: \"{}\"\n",
                "a".repeat(70 * 1024)
            ),
        );

        let rc = add_local(&worker_dir).await;

        assert_ne!(rc, 0, "oversize manifest must fail the add");
        let cwd = std::env::current_dir().unwrap();
        assert!(
            !cwd.join("config.yaml").exists(),
            "config.yaml must not exist — oversize manifest rejected before append"
        );
    })
    .await;
}
