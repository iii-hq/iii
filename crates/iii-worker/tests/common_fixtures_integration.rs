// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Roundtrip tests validating that TestConfigBuilder generates YAML
//! compatible with config_file.rs public API.

mod common;

use common::fixtures::TestConfigBuilder;
use common::isolation::in_temp_dir;

#[test]
fn roundtrip_no_workers() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new().build(&dir);

        let names = iii_worker::cli::config_file::list_worker_names();
        assert!(names.is_empty(), "expected no workers, got: {:?}", names);
    });
}

#[test]
fn roundtrip_local_worker() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("my-worker", Some("/abs/path"))
            .build(&dir);

        assert!(
            iii_worker::cli::config_file::worker_exists("my-worker"),
            "my-worker should exist"
        );
        assert_eq!(
            iii_worker::cli::config_file::get_worker_path("my-worker"),
            Some("/abs/path".to_string()),
            "worker_path should be /abs/path"
        );
    });
}

#[test]
fn roundtrip_oci_worker() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_oci_worker("pdfkit", "ghcr.io/iii-hq/pdfkit:1.0")
            .build(&dir);

        assert!(
            iii_worker::cli::config_file::worker_exists("pdfkit"),
            "pdfkit should exist"
        );
        assert_eq!(
            iii_worker::cli::config_file::get_worker_image("pdfkit"),
            Some("ghcr.io/iii-hq/pdfkit:1.0".to_string()),
            "image should be ghcr.io/iii-hq/pdfkit:1.0"
        );
    });
}

#[test]
fn roundtrip_mixed_workers() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("local-w", Some("/home/user/proj"))
            .with_oci_worker("oci-w", "ghcr.io/org/img:2.0")
            .with_worker("plain-w", None)
            .build(&dir);

        assert!(
            iii_worker::cli::config_file::worker_exists("local-w"),
            "local-w should exist"
        );
        assert!(
            iii_worker::cli::config_file::worker_exists("oci-w"),
            "oci-w should exist"
        );
        assert!(
            iii_worker::cli::config_file::worker_exists("plain-w"),
            "plain-w should exist"
        );

        assert_eq!(
            iii_worker::cli::config_file::get_worker_path("local-w"),
            Some("/home/user/proj".to_string()),
        );
        assert_eq!(
            iii_worker::cli::config_file::get_worker_image("oci-w"),
            Some("ghcr.io/org/img:2.0".to_string()),
        );
        assert!(
            iii_worker::cli::config_file::get_worker_path("plain-w").is_none(),
            "plain-w should have no path"
        );
    });
}

#[test]
fn roundtrip_worker_no_path() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_worker("plain", None)
            .build(&dir);

        assert!(
            iii_worker::cli::config_file::worker_exists("plain"),
            "plain should exist"
        );
        assert!(
            iii_worker::cli::config_file::get_worker_path("plain").is_none(),
            "plain should have no path"
        );
    });
}
