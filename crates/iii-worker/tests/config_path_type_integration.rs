// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for worker path and type resolution.
//!
//! Covers: append_worker_with_path, get_worker_path, resolve_worker_type.

mod common;

use common::isolation::in_temp_dir;

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
