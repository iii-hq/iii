// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for handle_managed_clear operations.
//!
//! Covers: clear single worker, clear with invalid name, clear all workers.

mod common;

use common::isolation::in_temp_dir_async;

#[tokio::test]
async fn handle_managed_clear_single_no_artifacts() {
    in_temp_dir_async(|| async {
        // Clear a worker that has no artifacts -- should succeed silently
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
        // Clear all when nothing is installed -- should succeed
        let exit_code = iii_worker::cli::managed::handle_managed_clear(None, true);
        assert_eq!(exit_code, 0);
    })
    .await;
}
