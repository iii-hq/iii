// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

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
        assert!(names.is_empty(), "expected no workers, got: {names:?}");
    });
}

#[test]
fn roundtrip_oci_worker() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_oci_worker("pdfkit", "ghcr.io/iii-hq/pdfkit:1.0")
            .build(&dir);

        assert!(iii_worker::cli::config_file::worker_exists("pdfkit"));
        assert_eq!(
            iii_worker::cli::config_file::get_worker_image("pdfkit"),
            Some("ghcr.io/iii-hq/pdfkit:1.0".to_string())
        );
    });
}

#[test]
fn roundtrip_mixed_workers() {
    in_temp_dir(|| {
        let dir = std::env::current_dir().unwrap();
        TestConfigBuilder::new()
            .with_oci_worker("oci-w", "ghcr.io/org/img:2.0")
            .with_worker("plain-w")
            .build(&dir);

        assert!(iii_worker::cli::config_file::worker_exists("oci-w"));
        assert!(iii_worker::cli::config_file::worker_exists("plain-w"));
        assert_eq!(
            iii_worker::cli::config_file::get_worker_image("oci-w"),
            Some("ghcr.io/org/img:2.0".to_string())
        );
    });
}
