// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Cross-platform config YAML tests.
//!
//! Verifies that config_file.rs produces identical output on Linux and macOS:
//! byte-identical round-trips, comment preservation, merge idempotency, and
//! path resolution through symlinks (macOS /tmp -> /private/tmp).

mod common;

use common::assertions::assert_paths_eq;
use common::isolation::in_temp_dir;
use std::path::Path;

/// CFG-01: Round-trip byte identity.
///
/// Writing the same worker twice via append_worker should produce
/// byte-identical config.yaml content on both runs.
#[test]
fn config_roundtrip_produces_identical_output() {
    in_temp_dir(|| {
        // First write via API
        iii_worker::cli::config_file::append_worker("test-worker", Some("port: 3000")).unwrap();
        let first_content = std::fs::read_to_string("config.yaml").unwrap();

        // Second write of same worker triggers merge path
        iii_worker::cli::config_file::append_worker("test-worker", Some("port: 3000")).unwrap();
        let second_content = std::fs::read_to_string("config.yaml").unwrap();

        assert_eq!(
            first_content, second_content,
            "round-trip should produce byte-identical output.\nFirst:\n{}\nSecond:\n{}",
            first_content, second_content
        );
    });
}

/// CFG-02: Comment preservation after append.
///
/// Comments in multiple positions (file start, before workers, inside worker
/// blocks) must survive an append_worker call for a NEW worker.
#[test]
fn config_comments_preserved_after_append() {
    in_temp_dir(|| {
        // Write config.yaml with comments in multiple positions
        let initial = "# Global comment at file start\nworkers:\n  # Comment before first worker\n  - name: existing-worker\n    # Inline comment in worker block\n    config:\n      port: 3000\n";
        std::fs::write("config.yaml", initial).unwrap();

        // Append a new worker
        iii_worker::cli::config_file::append_worker("new-worker", None).unwrap();
        let content = std::fs::read_to_string("config.yaml").unwrap();

        // All original comments must survive
        assert!(
            content.contains("# Global comment at file start"),
            "global comment lost. Content:\n{}",
            content
        );
        assert!(
            content.contains("# Comment before first worker"),
            "pre-worker comment lost. Content:\n{}",
            content
        );
        assert!(
            content.contains("# Inline comment in worker block"),
            "inline comment lost. Content:\n{}",
            content
        );
        // New worker must also be present
        assert!(
            content.contains("- name: new-worker"),
            "new worker not appended. Content:\n{}",
            content
        );
        // Original worker must be preserved
        assert!(
            content.contains("- name: existing-worker"),
            "existing worker lost. Content:\n{}",
            content
        );
    });
}

/// CFG-03: Merge idempotency.
///
/// Applying the same merge twice (append_worker with identical config on an
/// existing worker) must produce byte-identical output: merge(A,B) twice = merge(A,B) once.
#[test]
fn config_merge_is_idempotent() {
    in_temp_dir(|| {
        // Setup: worker with existing config
        std::fs::write(
            "config.yaml",
            "workers:\n  - name: w\n    config:\n      port: 3000\n      host: custom\n",
        )
        .unwrap();

        // First merge: add new keys, override port
        iii_worker::cli::config_file::append_worker("w", Some("port: 8080\ndebug: true")).unwrap();
        let first_result = std::fs::read_to_string("config.yaml").unwrap();

        // Second merge with identical inputs
        iii_worker::cli::config_file::append_worker("w", Some("port: 8080\ndebug: true")).unwrap();
        let second_result = std::fs::read_to_string("config.yaml").unwrap();

        assert_eq!(
            first_result, second_result,
            "merge(A,B) applied twice should produce byte-identical output.\nFirst:\n{}\nSecond:\n{}",
            first_result, second_result
        );
    });
}

/// CFG-04: Path resolution cross-platform.
///
/// On macOS, /tmp is a symlink to /private/tmp. This test stores a worker
/// path via append_worker_with_path, reads it back, and asserts both the
/// stored path and the original resolve to the same canonical location.
#[test]
fn config_path_resolution_cross_platform() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();

        // Store a path using the tempdir's actual path (which on macOS may be
        // /tmp/... symlinked to /private/tmp/...)
        let worker_path_str = dir.join("my-project").to_string_lossy().to_string();
        std::fs::create_dir_all(dir.join("my-project")).unwrap();

        iii_worker::cli::config_file::append_worker_with_path(
            "local-worker",
            &worker_path_str,
            None,
        )
        .unwrap();

        // Read the stored path back
        let stored_path = iii_worker::cli::config_file::get_worker_path("local-worker")
            .expect("worker path should be stored");

        // The stored path is a literal string (config_file.rs does NOT canonicalize).
        // Verify that canonicalizing both the stored path and the original path
        // yields the same result. This is the key cross-platform check:
        // on macOS /tmp/xxx -> /private/tmp/xxx
        assert_paths_eq(Path::new(&stored_path), &dir.join("my-project"));
    });
}
