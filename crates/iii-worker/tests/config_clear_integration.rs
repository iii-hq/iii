// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for handle_managed_clear operations.
//!
//! Covers: clear single worker, clear with invalid name, clear all workers.

mod common;

use common::isolation::in_temp_dir_async;

#[tokio::test]
async fn handle_managed_clear_single_no_artifacts_for_known_worker() {
    in_temp_dir_async(|| async {
        // A worker that's registered (in config.yaml) but has no artifacts
        // yet should succeed silently -- "already clean" is not an error.
        let add_rc =
            iii_worker::cli::managed::handle_managed_add("iii-http", false, false, false, false)
                .await;
        assert_eq!(add_rc, 0, "precondition: add must succeed");
        let exit_code = iii_worker::cli::managed::handle_managed_clear(Some("iii-http"), true);
        assert_eq!(
            exit_code, 0,
            "clear of known-but-clean worker should succeed"
        );
    })
    .await;
}

#[tokio::test]
async fn handle_managed_clear_unknown_worker_exits_error() {
    in_temp_dir_async(|| async {
        // Clearing a worker that is neither in config.yaml nor on disk is
        // almost certainly a typo. Exit 1 so automation catches it instead
        // of silently saying "nothing to clear" and moving on.
        let exit_code =
            iii_worker::cli::managed::handle_managed_clear(Some("not-a-real-worker"), true);
        assert_eq!(exit_code, 1, "clear of unknown worker should exit non-zero");
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
        // Clear all when nothing is installed -- should succeed
        let exit_code = iii_worker::cli::managed::handle_managed_clear(None, true);
        assert_eq!(exit_code, 0);
    })
    .await;
}
